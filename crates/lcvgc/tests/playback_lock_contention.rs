//! 再生もたつき(stuttering)の原因1「ホットリロード/LSP の重い eval が再生スレッドと
//! 同じ `Evaluator` ロックを奪い合う」ことを実測で確定させる計測テスト。
//!
//! 実ドライバ (`run_driver_with_shared`) をループ clip で走らせ、NoteOn の配送時刻を
//! タイムスタンプ付きで記録する。平常時(baseline)と、並行タスクが共有 `Evaluator`
//! ロックを確定時間保持する競合時(contention)とで、NoteOn 配送間隔(ジッタ)を比較する。
//! 競合時にだけ間隔が跳ね上がれば、もたつきの主因がロック競合であることが直接示される。
//! ロック保持時間は eval 速度(マシン性能)に依存しない固定値で模擬するため、高速環境でも
//! 決定論的に再現できる。
//!
//! Measurement test that confirms playback stuttering cause #1: anything holding
//! the same `Evaluator` lock the playback driver takes every tick (hot reload /
//! LSP) stalls NoteOn delivery. We run the real driver on a looping clip and
//! timestamp every NoteOn delivery, then compare inter-NoteOn gaps between a quiet
//! baseline window and a contention window in which a concurrent task holds the
//! shared `Evaluator` lock for a fixed duration. Holding for a fixed time (rather
//! than for a real parse+compile) makes the test independent of eval speed, so it
//! reproduces deterministically even on fast machines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Notify};
use tokio::time::sleep;

use lcvgc::engine::error::EngineError;
use lcvgc::engine::evaluator::Evaluator;
use lcvgc::engine::midi_sink::MidiSink;
use lcvgc::engine::playback::{run_driver_with_shared, BoxedSink, SharedSinks, SinksNotify};
use lcvgc::midi::message::MidiMessage;

/// 配送時刻付きで MIDI メッセージを記録する計測用 sink。
/// `SharedMockSink` と同じく内部を `Arc` 共有し、driver 外から `snapshot()` できる。
///
/// A timestamp-recording sink: captures `(Instant, MidiMessage)` for every send so
/// the test can measure delivery jitter from outside the driver.
#[derive(Clone)]
struct TimedSink {
    events: Arc<StdMutex<Vec<(Instant, MidiMessage)>>>,
}

impl TimedSink {
    fn new() -> Self {
        Self {
            events: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    /// 記録済みイベントのコピーを返す。
    /// Returns a copy of all recorded events.
    fn snapshot(&self) -> Vec<(Instant, MidiMessage)> {
        self.events.lock().expect("TimedSink poisoned").clone()
    }
}

impl MidiSink for TimedSink {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), EngineError> {
        self.events
            .lock()
            .expect("TimedSink poisoned")
            .push((Instant::now(), *msg));
        Ok(())
    }
}

/// 1 bar あたり 32 個の NoteOn(32 分音符)を出すループ clip。tempo_realtime.rs と同じ。
/// BPM=240 では NoteOn 周期 ≈ 31.25ms。
///
/// Looping clip emitting 32 NoteOns per bar (32nd notes); ~31.25ms apart at BPM=240.
fn playback_src() -> &'static str {
    "device dev { port test }\n\
     instrument inst { device dev\n channel 1 }\n\
     clip c1 [bars 1] { inst c:3:32 c c c c c c c c c c c c c c c c c c c c c c c c c c c c c c c }\n\
     scene s1 { c1 }\n"
}

/// 連続するイベント時刻列から、隣接間隔(ms)の列を作る。
/// Computes inter-event gaps (ms) from a sequence of timestamps.
fn gaps_ms(times: &[Instant]) -> Vec<f64> {
    times
        .windows(2)
        .map(|w| w[1].duration_since(w[0]).as_secs_f64() * 1000.0)
        .collect()
}

/// 統計 (件数, 平均, p95, 最大) を返す。
/// Returns (count, mean, p95, max) of the gap samples.
fn stats(gaps: &[f64]) -> (usize, f64, f64, f64) {
    if gaps.is_empty() {
        return (0, 0.0, 0.0, 0.0);
    }
    let mut sorted = gaps.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let p95_idx = ((sorted.len() as f64) * 0.95).ceil() as usize;
    let p95 = sorted[p95_idx.min(sorted.len() - 1)];
    let max = *sorted.last().unwrap();
    (sorted.len(), mean, p95, max)
}

#[cfg_attr(
    windows,
    ignore = "Windows のデフォルト timer resolution (15.6ms) では tick=520us が丸められて測定不能。\
              lcvgc バイナリ側の win_timer::HighResolutionTimer で実機解決済み"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_reload_lock_contention_causes_noteon_stutter() {
    const BPM: f64 = 240.0;
    // 旧来のロック内 eval が共有 Evaluator ロックを握り続ける時間を、マシン性能
    // (eval 速度) に依存しない確定値で模擬する。NoteOn 周期 (~31ms @ BPM240) に対し
    // 十分大きく取り、ロック競合が NoteOn 配送遅延を起こすことを決定論的に再現する。
    const LOCK_HOLD_MS: u64 = 20;

    // 1) ドライバを起動してループ再生。
    let evaluator = Arc::new(Mutex::new(Evaluator::new(BPM)));
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source(playback_src()).expect("setup eval");
        ev.eval_source("play s1 [loop]\n").expect("play eval");
    }

    let sink = TimedSink::new();
    let mut map: HashMap<String, BoxedSink> = HashMap::new();
    map.insert("dev".to_string(), Box::new(sink.clone()));
    let shared: SharedSinks = Arc::new(Mutex::new(map));
    let notify: SinksNotify = Arc::new(Notify::new());
    let clock = evaluator.lock().await.clock_handle();

    let driver_handle = {
        let ev = Arc::clone(&evaluator);
        let sinks = Arc::clone(&shared);
        let notify = Arc::clone(&notify);
        let clk = Arc::clone(&clock);
        tokio::spawn(async move {
            run_driver_with_shared(ev, sinks, notify, clk).await;
        })
    };

    // 3) ベースライン窓: 干渉なしで 1500ms 収集。
    sleep(Duration::from_millis(1500)).await;
    let contention_start = Instant::now();

    // 4) 競合窓: 並行タスクが共有 Evaluator ロックを確定時間保持する。
    //    ホットリロード(watcher.rs の lock 内 eval_file)など「重い処理がロックを長時間
    //    握る」状況を、eval 速度に依存しない固定時間で忠実に模擬する。guard を保持した
    //    まま sleep することで、その間 再生ドライバ (step_once の毎 tick lock) を確実に
    //    待たせる。
    let interference = {
        let ev = Arc::clone(&evaluator);
        tokio::spawn(async move {
            for _ in 0..6 {
                sleep(Duration::from_millis(250)).await;
                // 再生ドライバが毎 tick 奪い合うのと同じ共有ロックを確定時間保持する。
                let _guard = ev.lock().await;
                sleep(Duration::from_millis(LOCK_HOLD_MS)).await;
                // _guard drop でロック解放
            }
        })
    };
    sleep(Duration::from_millis(1500)).await;
    let contention_end = Instant::now();
    let _ = interference.await;

    driver_handle.abort();

    // 5) NoteOn の配送時刻を窓ごとに分類して間隔統計を出す。
    let events = sink.snapshot();
    let note_on_times: Vec<Instant> = events
        .iter()
        .filter(|(_, m)| matches!(m, MidiMessage::NoteOn { .. }))
        .map(|(t, _)| *t)
        .collect();

    let baseline_times: Vec<Instant> = note_on_times
        .iter()
        .copied()
        .filter(|t| *t < contention_start)
        .collect();
    let contention_times: Vec<Instant> = note_on_times
        .iter()
        .copied()
        .filter(|t| *t >= contention_start && *t <= contention_end)
        .collect();

    let base_gaps = gaps_ms(&baseline_times);
    let cont_gaps = gaps_ms(&contention_times);
    let (bn, bmean, bp95, bmax) = stats(&base_gaps);
    let (cn, cmean, cp95, cmax) = stats(&cont_gaps);

    let expected_gap_ms = 60.0 * (60_000_000.0 / (BPM * 480.0)) / 1000.0;

    println!("=== playback lock-contention measurement (BPM={BPM}) ===");
    println!("理論 NoteOn 周期 (32 分音符) ≈ {expected_gap_ms:.2} ms");
    println!("競合タスクのロック保持時間 = {LOCK_HOLD_MS} ms / 回 (固定)");
    println!("baseline   gaps: n={bn} mean={bmean:.2} p95={bp95:.2} max={bmax:.2} ms");
    println!("contention gaps: n={cn} mean={cmean:.2} p95={cp95:.2} max={cmax:.2} ms");
    println!(
        "→ max gap 悪化: baseline {bmax:.2} ms → contention {cmax:.2} ms (+{:.2} ms)",
        cmax - bmax
    );
    println!("========================================================");

    // 計測が成立していること (両窓に十分なサンプル)。
    assert!(
        bn >= 10 && cn >= 10,
        "サンプル不足: baseline={bn}, contention={cn} (driver/sink が機能していない可能性)"
    );

    // 主張: 競合窓では最大 NoteOn 間隔が、ロック保持時間の相当分まで悪化する。
    // = ロック競合がもたつき(配送遅延)を引き起こしている直接証拠。
    // しきい値はジッタ余裕を見てロック保持時間の 50% を採用。
    let degradation = cmax - bmax;
    assert!(
        degradation >= LOCK_HOLD_MS as f64 * 0.5,
        "競合による max gap 悪化 {degradation:.2} ms がロック保持 {LOCK_HOLD_MS} ms に対し小さすぎる。\
         ロック競合がもたつきの主因という仮説と不整合"
    );
}

/// in-use clip を `[bars]` 引数で重く再定義する DSL を作る。
/// `playback_src` の `inst` を使い、`bars*32` 個の音符で compile コストを支配的にする。
/// 32 音符/小節 (=32 分音符) を維持するので、再定義後も NoteOn の周期は変わらない。
///
/// Builds a heavy redefinition of the in-use clip `c1` using `playback_src`'s `inst`,
/// with `bars*32` notes so compile cost dominates. It keeps 32 notes/bar (32nd notes)
/// so the NoteOn cadence is unchanged after the swap.
fn heavy_clip_redef(bars: usize) -> String {
    let notes: String = "c ".repeat(bars * 32);
    format!("clip c1 [bars {bars}] {{ inst {notes}}}\n")
}

/// 再生中 (active_scene が c1 を使用) に in-use clip を再定義する compile コストを、
/// ロック内 (従来経路 = `eval_source` 即時 compile) で実測する。
///
/// Measures the in-lock cost of redefining the in-use clip `c1` while a scene using it
/// is active — the cost the legacy path would hold the shared lock for.
fn measure_in_lock_redef_ms(redef: &str, iters: usize) -> f64 {
    let mut total = 0.0;
    for _ in 0..iters {
        let mut ev = Evaluator::new(240.0);
        ev.eval_source(playback_src()).expect("setup eval");
        ev.eval_source("play s1 [loop]\n").expect("play eval");
        let t = Instant::now();
        // in_use clip の再定義 → その場で compile_clip が走る (ロック内 compile)。
        ev.eval_source(redef).expect("in-lock redef");
        total += t.elapsed().as_secs_f64() * 1000.0;
    }
    total / iters as f64
}

/// 回帰テスト: ロック外 eval (prepare/apply 分離) なら、重い再評価を行っても
/// NoteOn 配送のもたつきが実 eval 時間に追従しないこと。
///
/// 上の `hot_reload_lock_contention_causes_noteon_stutter` が「ロック内 compile は
/// もたつく」ことを示すのに対し、本テストは**実際の3経路と同じ off-lock フロー**
/// (`snapshot_for_prepare` → ロック外 `prepare_source` → ロック内 `apply_prepared`)
/// で in-use clip を再定義する。重い compile は prepare (ロック外) で行われ、apply は
/// キャッシュ済み成果物の差し替えだけなので、競合窓の max gap が実 eval 時間に追従しない。
///
/// Regression test: with off-lock eval (the prepare/apply split), heavy re-evaluation
/// does not translate into NoteOn stutter. While the test above shows in-lock compile
/// stutters, this one redefines an in-use clip via the real off-lock flow
/// (`snapshot_for_prepare` → off-lock `prepare_source` → in-lock `apply_prepared`). The
/// heavy compile happens during prepare (off-lock); apply only swaps the cached
/// artifact, so the contention-window max gap does not track real eval time.
#[cfg_attr(
    windows,
    ignore = "Windows のデフォルト timer resolution (15.6ms) では tick=520us が丸められて測定不能。\
              lcvgc バイナリ側の win_timer::HighResolutionTimer で実機解決済み"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn off_lock_eval_does_not_cause_noteon_stutter() {
    const BPM: f64 = 240.0;
    // compile を支配的にするための重い in-use clip 再定義 (512 小節 = 16384 音符)。
    let heavy_a = heavy_clip_redef(512);
    let heavy_b = heavy_clip_redef(513);

    // 1) ロック内 compile (従来経路) の所要時間を実測 (off-lock が回避すべきコスト)。
    let real_eval_ms = measure_in_lock_redef_ms(&heavy_a, 3);

    // 2) ドライバを起動してループ再生。
    let evaluator = Arc::new(Mutex::new(Evaluator::new(BPM)));
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source(playback_src()).expect("setup eval");
        ev.eval_source("play s1 [loop]\n").expect("play eval");
    }

    let sink = TimedSink::new();
    let mut map: HashMap<String, BoxedSink> = HashMap::new();
    map.insert("dev".to_string(), Box::new(sink.clone()));
    let shared: SharedSinks = Arc::new(Mutex::new(map));
    let notify: SinksNotify = Arc::new(Notify::new());
    let clock = evaluator.lock().await.clock_handle();

    let driver_handle = {
        let ev = Arc::clone(&evaluator);
        let sinks = Arc::clone(&shared);
        let notify = Arc::clone(&notify);
        let clk = Arc::clone(&clock);
        tokio::spawn(async move {
            run_driver_with_shared(ev, sinks, notify, clk).await;
        })
    };

    // 3) ベースライン窓: 干渉なしで 1500ms 収集。
    sleep(Duration::from_millis(1500)).await;
    let contention_start = Instant::now();

    // 4) 競合窓: ホットリロード等と同じ off-lock フローで in-use clip を再定義する。
    //    snapshot は短時間ロック、重い prepare(parse+compile) はロック外、apply は
    //    キャッシュ差し替えのみで短時間ロック。再生ドライバが奪われる時間は apply の
    //    僅かな間だけになる。
    let interference = {
        let ev = Arc::clone(&evaluator);
        tokio::spawn(async move {
            for k in 0..6 {
                sleep(Duration::from_millis(250)).await;
                // (1) snapshot: 短時間ロック。
                let snapshot = { ev.lock().await.snapshot_for_prepare() };
                // (2) prepare: ロック外で重い parse+compile。
                let redef = if k % 2 == 0 { &heavy_a } else { &heavy_b };
                let prepared = snapshot.prepare_source(redef).expect("off-lock prepare");
                // (3) apply: 短時間ロックで差し替えのみ (compile はキャッシュ再利用)。
                {
                    ev.lock().await.apply_prepared(prepared).expect("apply");
                }
            }
        })
    };
    sleep(Duration::from_millis(1500)).await;
    let contention_end = Instant::now();
    let _ = interference.await;

    driver_handle.abort();

    // 5) NoteOn の配送時刻を窓ごとに分類して間隔統計を出す。
    let events = sink.snapshot();
    let note_on_times: Vec<Instant> = events
        .iter()
        .filter(|(_, m)| matches!(m, MidiMessage::NoteOn { .. }))
        .map(|(t, _)| *t)
        .collect();

    let baseline_times: Vec<Instant> = note_on_times
        .iter()
        .copied()
        .filter(|t| *t < contention_start)
        .collect();
    let contention_times: Vec<Instant> = note_on_times
        .iter()
        .copied()
        .filter(|t| *t >= contention_start && *t <= contention_end)
        .collect();

    let base_gaps = gaps_ms(&baseline_times);
    let cont_gaps = gaps_ms(&contention_times);
    let (bn, bmean, bp95, bmax) = stats(&base_gaps);
    let (cn, cmean, cp95, cmax) = stats(&cont_gaps);

    let expected_gap_ms = 60.0 * (60_000_000.0 / (BPM * 480.0)) / 1000.0;

    println!("=== off-lock eval regression (BPM={BPM}) ===");
    println!("理論 NoteOn 周期 (32 分音符) ≈ {expected_gap_ms:.2} ms");
    println!("ロック内 compile (従来経路, 512 小節 clip 再定義) = {real_eval_ms:.2} ms / 回");
    println!("baseline   gaps: n={bn} mean={bmean:.2} p95={bp95:.2} max={bmax:.2} ms");
    println!("contention gaps: n={cn} mean={cmean:.2} p95={cp95:.2} max={cmax:.2} ms");
    println!(
        "→ max gap 変化: baseline {bmax:.2} ms → contention {cmax:.2} ms ({:+.2} ms)",
        cmax - bmax
    );
    println!("============================================");

    // 計測が成立していること (両窓に十分なサンプル)。
    assert!(
        bn >= 10 && cn >= 10,
        "サンプル不足: baseline={bn}, contention={cn} (driver/sink が機能していない可能性)"
    );

    // 主張: off-lock eval なら、競合窓の max gap 悪化が実 eval 時間に追従しない。
    // ロック内 compile なら悪化は real_eval_ms 相当だが、prepare/apply 分離では
    // ロック保持が apply の僅かな時間だけなので、悪化は real_eval_ms の半分未満に収まる。
    let degradation = cmax - bmax;
    assert!(
        degradation < real_eval_ms * 0.5,
        "off-lock eval なのに max gap 悪化 {degradation:.2} ms が実 eval {real_eval_ms:.2} ms に追従している。\
         ロック保持時間が compile コストに依存してしまっている疑い"
    );
}
