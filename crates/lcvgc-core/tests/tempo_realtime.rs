//! PR #57: `run_driver_with_shared` の interval 動作を実時間で計測する一回限りのテスト。
//!
//! BPM=60 と BPM=240 で driver を spawn し、同じ clip を loop 再生したときに
//! SharedMockSink に NoteOn が 30 件届くまでの wallclock を測定する。
//! 期待は両者の比が tempo の比 (4:1) に概ね一致すること。比例していれば
//! driver が tempo に応じて interval を切り替えていることが裏付けられる。
//!
//! Realtime measurement of `run_driver_with_shared` interval behavior. Spawns
//! the driver at BPM=60 and BPM=240 respectively, plays the same looping clip,
//! and measures the wallclock time until SharedMockSink receives 30 NoteOn
//! messages. Expectation: the ratio matches the tempo ratio (4:1).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Notify};
use tokio::time::sleep;

use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::midi_sink::SharedMockSink;
use lcvgc_core::engine::playback::{run_driver_with_shared, BoxedSink, SharedSinks, SinksNotify};
use lcvgc_core::midi::message::MidiMessage;

/// 1 bar あたり 32 個の NoteOn (32 分音符) を出すループ clip を仕込む DSL。
/// PPQ=480 / 4 拍 / 8 ticks per 32nd note なので、note 間隔 = 60 ticks。
/// BPM=60 では 1 NoteOn ≈ 125ms、BPM=240 では 1 NoteOn ≈ 31.25ms 周期になる想定。
fn setup_src() -> &'static str {
    "device dev { port test }\n\
     instrument inst { device dev\n channel 1 }\n\
     clip c1 [bars 1] { inst c:3:32 c c c c c c c c c c c c c c c c c c c c c c c c c c c c c c c }\n\
     scene s1 { c1 }\n"
}

/// 与えた BPM で driver を spawn し、SharedMockSink に NoteOn が target_count 件
/// 届くまでの wallclock を返す。timeout_ms 経過時点で件数不足なら None。
async fn measure_30_noteon(
    bpm: f64,
    target_count: usize,
    timeout_ms: u64,
) -> (Option<Duration>, usize) {
    let evaluator = Arc::new(Mutex::new(Evaluator::new(bpm)));
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source(setup_src()).expect("setup eval");
        ev.eval_source("play s1 [loop]\n").expect("play eval");
    }

    let handle = SharedMockSink::new();
    let mut map: HashMap<String, BoxedSink> = HashMap::new();
    map.insert("dev".to_string(), Box::new(handle.clone()));
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

    let start = Instant::now();
    let deadline = start + Duration::from_millis(timeout_ms);
    let mut elapsed_at_target: Option<Duration> = None;

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let count = handle
            .snapshot()
            .iter()
            .filter(|m| matches!(m, MidiMessage::NoteOn { .. }))
            .count();
        if count >= target_count {
            elapsed_at_target = Some(now.duration_since(start));
            break;
        }
        // ポーリング間隔は計測精度を犠牲にしすぎない範囲で。
        sleep(Duration::from_millis(2)).await;
    }

    let final_count = handle
        .snapshot()
        .iter()
        .filter(|m| matches!(m, MidiMessage::NoteOn { .. }))
        .count();

    driver_handle.abort();
    (elapsed_at_target, final_count)
}

/// 期待値計算: PPQ=480, 32 分音符 = 60 tick, 1 NoteOn 周期 = 60 * tick_us。
/// 30 NoteOn 完了時刻 ≈ (30 - 1) * 60 * tick_us (最初の 1 件目が tick 0 で出る想定)。
fn expected_us_for_n_noteon(bpm: f64, n: usize) -> u64 {
    let tick_us = 60_000_000.0 / (bpm * 480.0);
    // 1 件目は tick 0 で即発火、以降 60 tick ごと。
    let intervals = (n.saturating_sub(1)) as f64;
    (intervals * 60.0 * tick_us) as u64
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn driver_interval_scales_with_tempo() {
    const TARGET: usize = 30;

    // BPM=60: 30 NoteOn 完了見込み ≈ 29 * 60 * 2083us ≈ 3624 ms。timeout は十分余裕。
    // BPM=240: 30 NoteOn 完了見込み ≈ 29 * 60 * 520us ≈ 905 ms。
    let (elapsed_60, count_60) = measure_30_noteon(60.0, TARGET, 8000).await;
    let (elapsed_240, count_240) = measure_30_noteon(240.0, TARGET, 8000).await;

    let exp_60_us = expected_us_for_n_noteon(60.0, TARGET);
    let exp_240_us = expected_us_for_n_noteon(240.0, TARGET);

    println!("=== driver interval realtime measurement ===");
    println!(
        "BPM=60  : measured = {:?} (count_at_end={}), expected ≈ {} us ({} ms)",
        elapsed_60,
        count_60,
        exp_60_us,
        exp_60_us / 1000
    );
    println!(
        "BPM=240 : measured = {:?} (count_at_end={}), expected ≈ {} us ({} ms)",
        elapsed_240,
        count_240,
        exp_240_us,
        exp_240_us / 1000
    );

    let m60 = elapsed_60.expect("BPM=60 で 30 NoteOn が timeout 内に届かなかった");
    let m240 = elapsed_240.expect("BPM=240 で 30 NoteOn が timeout 内に届かなかった");

    let ratio_measured = m60.as_secs_f64() / m240.as_secs_f64();
    println!(
        "ratio (BPM60 / BPM240) measured = {:.3}, expected = 4.000",
        ratio_measured
    );
    println!("===========================================");

    // 4倍比 ±35% の範囲内なら driver は tempo に応じて間隔を変えていると判定。
    // OS のタイマ分解能/ポーリング誤差を考慮した緩めの許容値。
    assert!(
        ratio_measured > 2.6 && ratio_measured < 5.4,
        "tempo 比 4:1 から大きく外れた: ratio={:.3} (BPM60={:?}, BPM240={:?})",
        ratio_measured,
        m60,
        m240
    );
}
