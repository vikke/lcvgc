//! 本番再生ドライバ
//!
//! `Evaluator` の `active_scene` を tick 毎に借用し、各 tick のイベントを
//! `MidiSink` に送出する。`Stop` 評価で蓄積された AllNotesOff も吸い上げる。
//!
//! state の single source of truth は Evaluator 側に集約され、driver は
//! 「読むだけ（+ AllNotesOff 取り出し）」の薄いレイヤとして振る舞う。
//!
//! Issue #54: sink マップを `Arc<Mutex<HashMap>>` 共有にして、driver と
//! バイナリ側 (main.rs) の receiver タスクで同じハンドルを持てるようにした。
//! これにより device ブロックを LSP 経由で動的に評価したタイミングで
//! sink を追加・差し替えしても driver は走り続けられる。
//!
//! Playback driver for production. Borrows `Evaluator::active_scene` tick
//! by tick and dispatches events to a `MidiSink`, draining queued
//! AllNotesOff messages from Stop evaluation. The evaluator remains the
//! single source of truth; the driver is a thin read-only adapter.
//!
//! Issue #54: the sink map became an `Arc<Mutex<HashMap>>` shared between
//! the driver and the binary's receiver task so that newly evaluated
//! `device` blocks (e.g. dispatched via LSP) can swap sinks live without
//! tearing the driver down.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::runtime::Handle;
use tokio::sync::{Mutex, MutexGuard, Notify};
use tracing::{debug, error, info, warn};

use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::evaluator::{Evaluator, SceneTransitionOutcome};
use crate::engine::midi_sink::MidiSink;
use crate::midi::message::MidiMessage;

/// scene の clip 差し替えを適用する量子化単位（小節数）。
///
/// 再生中の scene 内 clip を上書き（replace_clip で pending 化）しても即座には
/// 切り替えず、transport 起点からこの小節数グリッドの頭に達した時点で全 clip を
/// 一斉に commit する。これにより「長い clip だと切替までが遠い」問題を避けつつ、
/// 全 clip が同じ小節頭で揃って展開する。
///
/// 現状は 4/4 専用前提の固定値。変拍子対応や scene 単位の可変化は将来の課題。
///
/// Quantization grid (in bars) at which staged clip replacements are applied.
/// Overwriting a clip used by a playing scene stages a pending swap; the swap
/// is committed for every clip together when the transport reaches a multiple
/// of this many bars from the start. This bounds the swap latency for long
/// clips and keeps all clips switching on the same downbeat. Fixed value for
/// now (assumes 4/4); odd meters / per-scene configurability are future work.
const SWAP_QUANTIZE_BARS: u64 = 4;

/// 1 つの論理 device に紐付く MIDI sink エントリ
///
/// Issue #49 で追加された型エイリアス。`PlaybackDriver` は device 論理名を
/// キーにこの sink を選び、MIDI イベントを送出する。
///
/// Boxed `MidiSink` entry keyed by logical device name (Issue #49).
pub type BoxedSink = Box<dyn MidiSink>;

/// 動的に変更可能な sink マップの共有ハンドル
///
/// `lcvgc` バイナリ側で device ブロックの動的評価を受けて sink を
/// 追加・差し替えするため、`PlaybackDriver` と外部 (main.rs の receiver
/// タスク) の双方で同じ Arc を持つ。
///
/// Shared handle to a mutable sink map. Both the driver and the binary
/// receiver task hold the same `Arc` so device events can swap sinks live.
pub type SharedSinks = Arc<Mutex<HashMap<String, BoxedSink>>>;

/// 新規 sink 追加時に driver を起こすための notify
///
/// `run_driver_with_shared` は sinks が空のときに `notified().await` で寝ており、
/// 受信側は sink 追加・差し替え後に `notify_one()` を呼ぶ。
///
/// Notifier used to wake `run_driver_with_shared` after a sink is added or
/// swapped. The driver parks on `notified()` while sinks are empty.
pub type SinksNotify = Arc<Notify>;

/// tick 駆動の再生ドライバ
///
/// Issue #49: device ごとに `MidiSink` を保持する HashMap 形式に拡張。
/// `MidiEvent.device` をキーに対応する sink へ振り分ける。未登録 device
/// 宛のイベントは warn ログを出してドロップする。
/// Issue #54: sink マップを `SharedSinks` (`Arc<Mutex<HashMap>>`) で保持し、
/// 外部から動的に追加・差し替えできるようにした。
///
/// Tick-driven playback driver. As of Issue #49 the driver routes events
/// to the sink matching `MidiEvent.device`; events addressed to unknown
/// devices are logged at `warn` level and dropped. Issue #54 made the
/// sink map an `Arc<Mutex<HashMap>>` so it can be swapped live by
/// receivers outside of the driver.
pub struct PlaybackDriver {
    /// 共有 Evaluator / Shared evaluator
    evaluator: Arc<Mutex<Evaluator>>,
    /// device 論理名 -> MIDI sink の共有マップ（Issue #54）
    /// Shared logical device name -> MIDI sink map (Issue #54)
    sinks: SharedSinks,
    /// 現在の tick 位置 / Current tick position
    current_tick: u64,
    /// 前回 step 時に active_scene が Some だったか（None→Some の遷移で tick リセット）
    /// Whether the last step observed an active scene (used to reset current_tick on
    /// None→Some transition).
    was_active: bool,
}

impl PlaybackDriver {
    /// 共有 sink マップ (`SharedSinks`) を受け取って `PlaybackDriver` を生成する
    ///
    /// Issue #54 で導入。main.rs 側で device 動的評価を受けて差し替える
    /// receiver タスクと同じ Arc を共有する想定。
    ///
    /// # Arguments
    /// * `evaluator` - Arc<Mutex<Evaluator>> 共有参照
    /// * `sinks`     - `SharedSinks` 共有 sink マップ
    ///
    /// # Returns
    /// 構築済みの `PlaybackDriver`
    ///
    /// Construct a driver that shares its sink map with external code.
    pub fn with_shared_sinks(evaluator: Arc<Mutex<Evaluator>>, sinks: SharedSinks) -> Self {
        Self {
            evaluator,
            sinks,
            current_tick: 0,
            was_active: false,
        }
    }

    /// sink マップを明示指定して `PlaybackDriver` を生成する（互換 API）
    ///
    /// Issue #54: 内部で `Arc<Mutex<_>>` に詰め直して `with_shared_sinks` に
    /// 委譲する。後方互換のため引数は所有値の `HashMap` を受け取る。
    ///
    /// # Arguments
    /// * `evaluator` - Arc<Mutex<Evaluator>> 共有参照
    /// * `sinks` - device 論理名 -> MidiSink ボックスのマップ
    ///
    /// Backward-compatible constructor; wraps the supplied map in a fresh
    /// `Arc<Mutex<_>>` and forwards to `with_shared_sinks`.
    pub fn with_sinks(evaluator: Arc<Mutex<Evaluator>>, sinks: HashMap<String, BoxedSink>) -> Self {
        Self::with_shared_sinks(evaluator, Arc::new(Mutex::new(sinks)))
    }

    /// 単一 device (`"default"`) の sink を持つ `PlaybackDriver` を生成する
    ///
    /// 後方互換用の簡便コンストラクタ。MidiEvent.device が `""` または
    /// `"default"` のいずれも `"default"` sink にルーティングされる。
    ///
    /// Convenience constructor wiring a single sink under the `"default"`
    /// device name (for callers that still operate in single-device mode).
    pub fn new<S: MidiSink + 'static>(evaluator: Arc<Mutex<Evaluator>>, sink: S) -> Self {
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("default".to_string(), Box::new(sink));
        Self::with_sinks(evaluator, sinks)
    }

    /// 現在の tick 位置を返す
    /// Returns the current tick position.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// 共有 sink マップへのハンドル clone を返す
    ///
    /// Issue #54: テストや外部の receiver タスクから driver と同じ
    /// `SharedSinks` を握って差し替えるためのアクセサ。
    ///
    /// Returns a clone of the shared sink handle so external code (tests,
    /// receiver tasks) can mutate the same map the driver uses.
    pub fn shared_sinks(&self) -> SharedSinks {
        Arc::clone(&self.sinks)
    }

    /// 1 tick 進める
    ///
    /// 1. `take_pending_all_notes_off` の結果を CC#123 value=0 として各 channel へ送出
    /// 2. `active_scene_mut` が Some なら `events_at(current_tick)` を送出し `advance_all(1)`、
    ///    `scene_tick_length` 境界に到達したら `on_scene_loop_complete` を呼ぶ
    /// 3. `active_scene` が None なら current_tick を 0 にリセット
    ///
    /// ロック取得順序: **Evaluator → Sinks**（デッドロック防止のため固定）。
    /// Lock order is fixed at **Evaluator → Sinks** to avoid deadlocks.
    ///
    /// Advances by one tick; dispatches queued AllNotesOff, then events
    /// at the current tick while `active_scene` is Some, resetting the
    /// tick counter when it goes back to None.
    pub async fn step_once(&mut self) -> Result<(), EngineError> {
        // ---------------- 1. Evaluator ロック内で送出すべき情報を集める ----------------
        // ロック順序「Evaluator → Sinks」を守るため、まず Evaluator から必要な情報を
        // 取り出して drop し、その後で sinks をロックする。
        let mut ev = self.evaluator.lock().await;

        // Evaluator ロック中に AllNotesOff / Transport キューを吸い上げる（借用を短く保つ）
        // Issue #50: System Real-Time Start/Stop は Play/Stop 評価で積まれる。
        let pending_all_notes_off = ev.take_pending_all_notes_off();
        let mut pending_transport = ev.take_pending_transport();

        // PR #88: 再生中の MIDI Timing Clock (0xF8) を 24 PPQN で送出。
        // active_scene が Some のときだけ、`Clock::is_clock_tick(current_tick)`
        // が真の tick で `transport = true` な device 全てに 0xF8 を積む。
        // Start (0xFA) と同じ step で送出されるため、Play 直後の最初の step は
        // Start + Clock(tick=0) が同時に流れる (MIDI 仕様: 最初の Clock が beat 0)。
        //
        // PR #88: emit MIDI Timing Clock (0xF8) at 24 PPQN while playing.
        // When `active_scene` is Some and `Clock::is_clock_tick(current_tick)`
        // is true, we queue a Clock for every transport-enabled device. The
        // first step after Play therefore emits Start and Clock together,
        // satisfying the MIDI spec that the first Clock marks beat 0.
        if ev.active_scene().is_some() {
            let clock_snapshot = ev.clock_snapshot();
            if clock_snapshot.is_clock_tick(self.current_tick) {
                for device in ev.transport_enabled_devices() {
                    pending_transport.push((device, MidiMessage::Clock));
                }
            }
        }

        // clip 差し替えを適用する 4 小節グリッド長（tick）。ppq・拍子から算出。
        // BPM 変更では ticks_per_bar は不変なので、テンポチェンジ中でもグリッドは安定。
        // The 4-bar grid length (ticks) at which staged clip swaps are applied.
        let swap_grid = ev
            .clock_snapshot()
            .ticks_per_bar()
            .saturating_mul(SWAP_QUANTIZE_BARS);

        let (routed, scene_len, has_active_scene) = match ev.active_scene_mut() {
            Some(scene) => {
                // None→Some 遷移を検出したら tick を 0 から始める
                if !self.was_active {
                    self.current_tick = 0;
                    self.was_active = true;
                }

                // 4 小節グリッドの頭に達したら、待機中の clip 差し替えを一斉適用する。
                // events_at を読む前に commit することで、グリッド頭の downbeat から
                // 新 clip の小節頭が鳴り始める。pending の無い clip は触らない。
                // Commit staged clip swaps on the 4-bar grid downbeat, before
                // reading events, so the new clips sound from their bar head.
                if swap_grid > 0 && self.current_tick.is_multiple_of(swap_grid) {
                    scene.commit_pending_clips();
                }

                // Issue #49: (device, message) ペアで送出先を確定させる
                let routed: Vec<(String, MidiMessage)> = scene
                    .events_at(self.current_tick)
                    .into_iter()
                    .map(|e| (e.device.clone(), e.message))
                    .collect();
                scene.advance_all(1);
                let scene_len = scene.scene_tick_length();
                (routed, scene_len, true)
            }
            None => {
                // 再生停止中: tick をリセットして次の play に備える
                // 残っている AllNotesOff / Transport は送出して stop 側面をカバーする
                self.current_tick = 0;
                self.was_active = false;
                (Vec::new(), 0, false)
            }
        };
        drop(ev);

        // ---------------- 2. Sinks ロック内で実際の送出を行う ----------------
        {
            let mut sinks = self.sinks.lock().await;

            // Issue #50: まず Transport (Start/Stop) を送出する。Start は tick イベントより
            // 前に外部機材に届ける必要があり、Stop も AllNotesOff と並んで早めに送るのが自然。
            // Issue #50: emit Transport (Start/Stop) first so external gear sees Start
            // before any note tick. Stop dovetails with AllNotesOff as a stop-side cleanup.
            Self::dispatch_transport(&mut sinks, &pending_transport)?;

            // 先に AllNotesOff を送出（scene 境界や mute で積まれた分）
            Self::dispatch_all_notes_off(&mut sinks, &pending_all_notes_off)?;

            // 続いて本来の tick イベントを送出
            for (device, msg) in &routed {
                match Self::resolve_sink(&mut sinks, device) {
                    Some(sink) => sink.send(msg)?,
                    None => warn!(
                        "イベント送出先 sink が未登録: device={} msg={:?}",
                        device, msg
                    ),
                }
            }
        }

        if !has_active_scene {
            return Ok(());
        }

        self.current_tick += 1;

        // scene 境界に到達したらループ完了通知
        if scene_len > 0 && self.current_tick.is_multiple_of(scene_len) {
            let mut ev = self.evaluator.lock().await;
            match ev.on_scene_loop_complete()? {
                SceneTransitionOutcome::Continue => {}
                SceneTransitionOutcome::NextScene { .. } => {
                    // 新 scene は Evaluator 側で差し替え済み、次 step で tick=0 から再開
                    self.current_tick = 0;
                }
                SceneTransitionOutcome::SceneComplete | SceneTransitionOutcome::SessionComplete => {
                    // active_scene は None に戻っているので次 step で was_active=false へ
                }
            }
        }

        Ok(())
    }

    /// `MidiEvent.device` に対応する sink を `MutexGuard` 経由で解決する
    ///
    /// 空文字列 (= compile 時に device 未指定だった MidiEvent) は
    /// `"default"` sink にフォールバックする。該当 sink が無ければ `None`。
    ///
    /// # Arguments
    /// * `sinks` - sink マップへの可変参照（lock 後の MutexGuard 由来）
    /// * `event_device` - MidiEvent から取り出した device 名
    ///
    /// # Returns
    /// 該当 sink への可変参照、未登録なら `None`
    ///
    /// Resolve the sink for `event_device` against an already-locked sink
    /// map. Empty string falls back to the `"default"` sink.
    fn resolve_sink<'a>(
        sinks: &'a mut MutexGuard<'_, HashMap<String, BoxedSink>>,
        event_device: &str,
    ) -> Option<&'a mut BoxedSink> {
        let key: &str = if event_device.is_empty() {
            "default"
        } else {
            event_device
        };
        sinks.get_mut(key)
    }

    /// 蓄積された `(device, channel)` ごとに AllNotesOff (CC#123 value=0) を
    /// 該当 sink へ送出する。未登録 device は warn ログを出してスキップする。
    ///
    /// # Arguments
    /// * `sinks` - lock 済み sink マップへの可変参照
    /// * `pairs` - `(device, channel)` ペアのスライス
    ///
    /// Dispatches `(device, channel)` AllNotesOff pairs queued by the
    /// evaluator to the matching sink, warning and skipping unknown devices.
    fn dispatch_all_notes_off(
        sinks: &mut MutexGuard<'_, HashMap<String, BoxedSink>>,
        pairs: &[(String, crate::midi::channel::MidiChannel)],
    ) -> Result<(), EngineError> {
        for (device, ch) in pairs {
            let msg = MidiMessage::ControlChange {
                channel: *ch,
                cc: 123,
                value: 0,
            };
            match Self::resolve_sink(sinks, device) {
                Some(sink) => sink.send(&msg)?,
                None => warn!(
                    "AllNotesOff の送出先 sink が未登録: device={} channel={}",
                    device,
                    ch.as_one_based()
                ),
            }
        }
        Ok(())
    }

    /// Issue #50: Evaluator が蓄積した `(device, MidiMessage)`（Start / Stop /
    /// Continue）を該当 sink に送出する。未登録 device は warn + drop。
    ///
    /// # Arguments
    /// * `sinks` - lock 済み sink マップへの可変参照
    /// * `pairs` - `(device, MidiMessage)` ペアのスライス
    ///
    /// Issue #50: dispatch `(device, MidiMessage)` System Real-Time pairs queued
    /// by the Evaluator. Unknown devices are logged and dropped.
    fn dispatch_transport(
        sinks: &mut MutexGuard<'_, HashMap<String, BoxedSink>>,
        pairs: &[(String, MidiMessage)],
    ) -> Result<(), EngineError> {
        for (device, msg) in pairs {
            match Self::resolve_sink(sinks, device) {
                Some(sink) => sink.send(msg)?,
                None => warn!(
                    "Transport メッセージ送出先 sink が未登録: device={} msg={:?}",
                    device, msg
                ),
            }
        }
        Ok(())
    }
}

/// tokio タスクで PlaybackDriver を tick 間隔で駆動する（互換 API）
///
/// Clock の `tick_duration_us()` を参照して sleep する単純なループ。
/// `EngineError` は error ログに出力してループ継続する（将来的にはリカバリ戦略を拡張）。
/// Issue #49: sink マップで複数 device 宛を受け取り、`MidiEvent.device` で
/// 振り分ける。
/// Issue #54: 内部で `SharedSinks` に詰め直して `run_driver_with_shared` に
/// 委譲する。`Notify` は新しく作るので外部から起こすことはできない。動的
/// 差し替えが必要なら `run_driver_with_shared` を直接呼ぶこと。
///
/// Runs `PlaybackDriver::step_once` on a tokio interval derived from the
/// clock's tick duration. Errors are logged; the loop continues. Backward
/// compatible wrapper around `run_driver_with_shared` that allocates a
/// fresh `SharedSinks` / `SinksNotify`.
pub async fn run_driver(
    evaluator: Arc<Mutex<Evaluator>>,
    sinks: HashMap<String, BoxedSink>,
    clock: Arc<RwLock<Clock>>,
) {
    let shared: SharedSinks = Arc::new(Mutex::new(sinks));
    let notify: SinksNotify = Arc::new(Notify::new());
    run_driver_with_shared(evaluator, shared, notify, clock).await;
}

/// busy wait に切り替えるしきい値
///
/// 残り時間がこれ以下になったら `std::thread::sleep` を諦めて `spin_loop` で
/// 詰める。Windows の OS timer resolution が 15.6ms に丸められても、最後の
/// 2ms 以下を spin で詰めれば想定 deadline ±数十us に収まる。値を大きくする
/// ほど精度が上がるが CPU 消費も増える。
///
/// Threshold below which we switch from `thread::sleep` to busy spinning.
/// Larger values give tighter deadline accuracy but burn more CPU.
pub const SPIN_THRESHOLD: Duration = Duration::from_millis(2);

/// 次 tick までの待ち方を表す純粋な戦略
///
/// `next_wait_strategy(now, deadline)` の返り値で、driver loop はこれを見て
/// `thread::sleep` するか `spin_loop` で busy wait するかを切り替える。
/// 単純な enum なので副作用なし、テストで網羅可能。
///
/// Pure decision returned by `next_wait_strategy`. The driver loop dispatches
/// `thread::sleep` vs `spin_loop` based on this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitStrategy {
    /// この時間 `thread::sleep` で粗く待つ
    /// Coarse-grained `thread::sleep` for the given duration
    Sleep(Duration),
    /// この時刻まで `spin_loop` で busy wait する
    /// Spin-loop until the given instant
    SpinUntil(Instant),
    /// もう deadline を過ぎている (即 step すべき)
    /// Deadline already passed; fire immediately
    NoWait,
}

/// `now` から `deadline` までの待ち方を決める純粋関数
///
/// - `now >= deadline`: 既に過ぎているので `NoWait`
/// - 残り `<= SPIN_THRESHOLD`: `SpinUntil(deadline)` で busy wait
/// - 残り `> SPIN_THRESHOLD`: `Sleep(残り - SPIN_THRESHOLD)` で粗く待ち、
///   次回 iteration で spin 領域に入る想定
///
/// # Arguments
/// * `now` - 現在時刻
/// * `deadline` - 次 tick の目標時刻
///
/// # Returns
/// `WaitStrategy` enum
///
/// Pure function deciding how to wait. Sleeps coarsely up to
/// `deadline - SPIN_THRESHOLD`, then spins for the remaining ≤ 2ms.
fn next_wait_strategy(now: Instant, deadline: Instant) -> WaitStrategy {
    if now >= deadline {
        return WaitStrategy::NoWait;
    }
    let remaining = deadline - now;
    if remaining <= SPIN_THRESHOLD {
        WaitStrategy::SpinUntil(deadline)
    } else {
        WaitStrategy::Sleep(remaining - SPIN_THRESHOLD)
    }
}

/// `SharedSinks` + `SinksNotify` 版の再生ドライバランナ（Issue #54）
///
/// sinks が空の間は `notify.notified().await` でブロックし、receiver タスクが
/// `notify_one()` を呼んで sink を追加した時点で目を覚まし、tick ループに入る。
/// device の動的登録（LSP 経由など）に追従して driver を起動するためのエントリ。
///
/// PR #60: tick の wait に `tokio::time::interval` を使うのをやめ、`spawn_blocking`
/// で確保した OS スレッド上で `thread::sleep` + `spin_loop` のハイブリッドに
/// 書き換えた。Windows 11 24H2 (build 26100+) では `timeBeginPeriod(1)` を呼んでも
/// tokio の time driver の wait が 15.6ms 粒度に丸められるケースがあるため、
/// OS timer resolution に依存しない実装にする。step_once は async のため
/// `Handle::block_on` で同期化して呼ぶ。
///
/// # Arguments
/// * `evaluator` - 共有 Evaluator
/// * `sinks` - 共有 sink マップ。外部から `lock().await.insert(...)` で差し替え可能
/// * `notify` - sink 投入時に外部が `notify_one()` を呼ぶ Notify
/// * `clock` - tick 間隔を決める Clock
///
/// # Errors
/// この関数自体は無限ループで `Result` を返さない。`step_once` のエラーは
/// `error!` ログに記録した上でループを継続する。
///
/// Run the playback driver with a shared sink map. Parks on `notify` while
/// sinks are empty. PR #60: switched the tick wait from `tokio::time::interval`
/// to a `spawn_blocking` thread that mixes `thread::sleep` and `spin_loop`,
/// which removes dependence on the OS timer resolution. `step_once` is async
/// and is called via `Handle::block_on` on the blocking thread.
pub async fn run_driver_with_shared(
    evaluator: Arc<Mutex<Evaluator>>,
    sinks: SharedSinks,
    notify: SinksNotify,
    clock: Arc<RwLock<Clock>>,
) {
    // 1. sinks が空の間はブロック。spurious wake 対策で while ループにする。
    //    `notified()` は呼び出し時点で「未消化通知が無ければ Future を返す」
    //    ので、map.is_empty() を確認してから await する。
    loop {
        {
            let map = sinks.lock().await;
            if !map.is_empty() {
                break;
            }
        }
        info!("再生ドライバ待機中: sinks 空のため device 動的登録を待機します");
        notify.notified().await;
    }

    {
        let map = sinks.lock().await;
        let (bpm, ppq, dur_us) = {
            let c = clock.read().expect("clock RwLock poisoned");
            (c.bpm(), c.ppq(), c.tick_duration_us().max(1))
        };
        info!(
            "再生ドライバ起動: tick duration = {} us (BPM={}, PPQ={}, devices={:?})",
            dur_us,
            bpm,
            ppq,
            map.keys().collect::<Vec<_>>()
        );
    }

    // tokio runtime ハンドルを spawn_blocking 先のスレッドに渡し、async な
    // step_once を block_on で呼ぶ。spawn_blocking はこの async 関数を呼んだ
    // tokio runtime のブロッキング pool を使うため、Handle::current() でその
    // runtime ハンドルを取得しておく。
    //
    // Capture the current runtime handle so the blocking thread can drive
    // the async `step_once` via `block_on`.
    let handle = Handle::current();
    let evaluator_for_thread = Arc::clone(&evaluator);
    let sinks_for_thread = Arc::clone(&sinks);
    let clock_for_thread = Arc::clone(&clock);
    // 協調的シャットダウンフラグ。本 async 関数が drop / abort された場合に
    // ガード経由で true がセットされ、blocking loop が次 iteration で抜ける。
    // spawn_blocking で起こした OS スレッドは tokio から abort できないため、
    // この明示的シグナルでスレッドリークを防ぐ。
    //
    // Cooperative shutdown flag. The guard sets this to true on drop, allowing
    // the OS thread spawned via `spawn_blocking` to exit (since `spawn_blocking`
    // tasks cannot be aborted by tokio).
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_for_thread = Arc::clone(&shutdown);

    let join = tokio::task::spawn_blocking(move || {
        driver_blocking_loop(
            handle,
            evaluator_for_thread,
            sinks_for_thread,
            clock_for_thread,
            shutdown_for_thread,
        );
    });

    // この async 関数のスコープを抜ける時 (drop / abort 含む) に必ず
    // shutdown を立てる小さなガード。spawn_blocking 側はこれを見てループを
    // 抜ける。
    // RAII guard that sets the shutdown flag on drop, ensuring the blocking
    // loop terminates when the parent async task is cancelled.
    struct ShutdownGuard(Arc<AtomicBool>);
    impl Drop for ShutdownGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let _guard = ShutdownGuard(Arc::clone(&shutdown));

    // 通常 driver_blocking_loop は shutdown が立つまで return しない。
    // spawn_blocking が panic した場合のみエラーログを出す。
    // Normally the blocking loop runs until `shutdown` is set; this returns
    // early only on panic.
    if let Err(e) = join.await {
        error!("driver blocking task が異常終了: {:?}", e);
    }
}

/// blocking thread 上で回る driver の本体
///
/// `spawn_blocking` で確保した OS スレッド上で動作し、`Instant` ベースの累積
/// deadline を維持しつつ `thread::sleep` + `spin_loop` のハイブリッドで wait
/// する。step_once は async なので `Handle::block_on` で同期化して呼ぶ。
/// tempo 変更は毎 tick 後にチェックし、変化があれば次回 deadline を新 tick
/// duration で組み直す。
///
/// # Arguments
/// * `handle` - 親 tokio runtime のハンドル (block_on 用)
/// * `evaluator` - 共有 Evaluator
/// * `sinks` - 共有 sink マップ
/// * `clock` - 共有 Clock
///
/// Tick loop running on the blocking thread. Uses an `Instant`-based deadline
/// schedule with `thread::sleep` + `spin_loop` for sub-millisecond accuracy
/// independent of OS timer resolution. Calls async `step_once` via
/// `handle.block_on`.
fn driver_blocking_loop(
    handle: Handle,
    evaluator: Arc<Mutex<Evaluator>>,
    sinks: SharedSinks,
    clock: Arc<RwLock<Clock>>,
    shutdown: Arc<AtomicBool>,
) {
    let mut driver = PlaybackDriver::with_shared_sinks(evaluator, sinks);

    let mut last_dur_us = read_tick_duration_us(&clock);
    // 累積 deadline。最初の tick は「即 fire」したいので now を起点にする。
    // Cumulative deadline. Start from now so the first tick fires immediately.
    let mut next_deadline = Instant::now();

    loop {
        // 協調的シャットダウン: 親 async 関数が drop / abort された場合に
        // フラグが立つので、ループの先頭で必ずチェックする。
        // Cooperative shutdown: bail out at the top of each iteration if the
        // parent async task signaled cancellation.
        if shutdown.load(Ordering::SeqCst) {
            debug!("driver blocking loop: shutdown 検知につき終了");
            return;
        }

        // tempo 変更検知 (deadline 計算前に行う)
        // Detect tempo change before computing the next deadline.
        let cur_dur_us = read_tick_duration_us(&clock);
        if cur_dur_us != last_dur_us {
            let new_bpm = clock.read().expect("clock RwLock poisoned").bpm();
            info!(
                "tempo 変更検知: tick duration {} us → {} us (BPM={})",
                last_dur_us, cur_dur_us, new_bpm
            );
            last_dur_us = cur_dur_us;
            // tempo 変更後は次 deadline を「今から新 tick duration」で振り直す。
            // 既に予約済みの古い deadline が新 tempo に対して妥当かは保証できない。
            // After tempo change, recompute the next deadline from now.
            next_deadline = Instant::now();
        }

        // 現在 deadline まで wait
        // Wait until the current deadline.
        let before_wait = Instant::now();
        wait_until(next_deadline, &shutdown);
        let waited_us = before_wait.elapsed().as_micros();
        // wait_until 中に shutdown が立ったら、step を実行せずに上に戻って
        // ループ先頭の早期 return に拾わせる。
        // If shutdown was triggered during wait, skip step and exit on next
        // iteration via the early return at the loop top.
        if shutdown.load(Ordering::SeqCst) {
            continue;
        }

        // step_once (async) を block_on で同期実行
        // Drive the async step on this blocking thread.
        let before_step = Instant::now();
        let step_result = handle.block_on(driver.step_once());
        if let Err(e) = step_result {
            error!("再生ドライバエラー: {}", e);
        }
        let step_us = before_step.elapsed().as_micros();
        debug!(
            "tick: waited={}us step={}us (target_interval={}us)",
            waited_us, step_us, last_dur_us
        );

        // 次 tick の deadline を加算。step_once 中の経過時間も含めて累積し、
        // 全体としての BPM 精度を保つ (interval が遅れた分は次 tick で取り戻す)。
        // Advance the deadline by one tick. Drift from a slow step_once is
        // absorbed by `wait_until`'s `NoWait` path on the next iteration.
        next_deadline += Duration::from_micros(last_dur_us);

        // 診断 warn: 実 wait が target の 5 倍を超えていたら異常。
        // PR #60 の busy/sleep 化後はほぼ出ないはずだが、回帰検知用に残す。
        // Diagnostic warn (kept as a regression check after PR #60).
        let target_us_u128 = u128::from(last_dur_us.max(1));
        if waited_us > target_us_u128.saturating_mul(5) {
            warn!(
                "tick wait 異常: target={}us 実測={}us (≧5x). \
                 OS timer resolution が要求粒度に追いついていない可能性あり",
                last_dur_us, waited_us
            );
        }
    }
}

/// `wait_until` 内の `thread::sleep` 1 回あたりの上限
///
/// shutdown 検知のため sleep を細切れにする。長すぎると shutdown 反応が遅れ、
/// 短すぎるとオーバヘッドが増える。50ms にしておくと、driver 停止時の遅延は
/// 最大 50ms 程度。
///
/// Maximum duration of a single `thread::sleep` slice in `wait_until`. Keeps
/// shutdown latency bounded.
const SLEEP_SLICE_CAP: Duration = Duration::from_millis(50);

/// 指定 deadline まで sleep + spin で待つ
///
/// `next_wait_strategy` の戦略を見て、`Sleep` ならその時間 `thread::sleep`、
/// `SpinUntil` なら `spin_loop` で busy wait、`NoWait` なら即 return。
/// sleep 直後に「目覚めが早すぎた / 遅すぎた」場合に備え、while ループで
/// 残時間を再評価する。
///
/// shutdown フラグが渡されており、sleep / spin の合間に true になったら
/// 即 return する (driver_blocking_loop から協調的に止められる)。
///
/// # Arguments
/// * `deadline` - この時刻まで待つ
/// * `shutdown` - 協調的シャットダウンフラグ。true なら即 return
///
/// Wait until `deadline` using the sleep/spin hybrid. Sleeps are sliced by
/// `SLEEP_SLICE_CAP` so that the shutdown flag is checked frequently.
fn wait_until(deadline: Instant, shutdown: &AtomicBool) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        match next_wait_strategy(now, deadline) {
            WaitStrategy::NoWait => return,
            WaitStrategy::Sleep(d) => {
                let slice = d.min(SLEEP_SLICE_CAP);
                std::thread::sleep(slice);
            }
            WaitStrategy::SpinUntil(target) => {
                while Instant::now() < target {
                    if shutdown.load(Ordering::SeqCst) {
                        return;
                    }
                    std::hint::spin_loop();
                }
                return;
            }
        }
    }
}

/// 共有 Clock から `tick_duration_us` を読み出す小さなヘルパ
///
/// 0 を返さないように `max(1)` をかける。`run_driver_with_shared` の起動時と
/// 毎 tick の比較で同じ前処理を行うため共通化した。
///
/// Reads `tick_duration_us` from the shared clock, clamping to 1us minimum
/// so `Duration::from_micros(0)` cannot reach `time::interval`.
fn read_tick_duration_us(clock: &Arc<RwLock<Clock>>) -> u64 {
    clock
        .read()
        .expect("clock RwLock poisoned")
        .tick_duration_us()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::midi_sink::SharedMockSink;
    use crate::midi::channel::MidiChannel;

    /// eval_source で DSL を評価する小ヘルパ
    async fn eval(evaluator: &Arc<Mutex<Evaluator>>, source: &str) {
        let mut ev = evaluator.lock().await;
        ev.eval_source(source).expect("eval ok");
    }

    /// device + instrument + clip + scene を一通り登録する DSL
    /// DSL channel 1 (wire 0-based 0) の clip `c1` を scene `s1` に登録、その後呼び出し側で `play s1` を発行する
    fn setup_src() -> &'static str {
        "device dev { port test }\n\
         instrument inst { device dev\n channel 1 }\n\
         clip c1 [bars 1] { inst c }\n\
         scene s1 { c1 }\n"
    }

    /// "dev" 1 つだけの sink マップを作るヘルパ。返り値の handle から
    /// driver 内部 sink の送出履歴を観測できる。
    fn single_dev_sinks() -> (HashMap<String, BoxedSink>, SharedMockSink) {
        let handle = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("dev".to_string(), Box::new(handle.clone()));
        (sinks, handle)
    }

    /// 空 Evaluator に対する step_once は sink に何も出さず tick もリセット状態を保つ
    #[tokio::test]
    async fn step_once_on_empty_evaluator_is_noop() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator, sinks);

        driver.step_once().await.unwrap();
        assert!(handle.snapshot().is_empty());
        assert_eq!(driver.current_tick(), 0);
    }

    /// play 直後に step_once を数回実行すると clip のイベントが MockSink に落ちる
    #[tokio::test]
    async fn play_then_step_sends_note_events() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // 最初の step で tick=0 のイベント (NoteOn) が送出される
        driver.step_once().await.unwrap();
        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "NoteOn が送出されていない: {:?}",
            sent
        );
    }

    /// stop 評価で蓄積された AllNotesOff (CC#123) が次 step で送出される
    #[tokio::test]
    async fn stop_emits_all_notes_off_on_next_step() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        // まず 1 step 進めて NoteOn を出す
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);
        driver.step_once().await.unwrap();
        handle.clear();

        // stop 評価 → active_scene=None + pending_all_notes_off に ch0 が積まれる
        eval(&evaluator, "stop\n").await;

        driver.step_once().await.unwrap();

        // CC#123 value=0 on wire channel 0 (DSL channel 1) が送出されていること
        let sent = handle.snapshot();
        let expected_ch = MidiChannel::from_zero_based(0).unwrap();
        let found_all_notes_off = sent.iter().any(|m| {
            if let MidiMessage::ControlChange {
                channel,
                cc: 123,
                value: 0,
            } = m
            {
                *channel == expected_ch
            } else {
                false
            }
        });
        assert!(
            found_all_notes_off,
            "AllNotesOff (CC#123) が送出されていない: {:?}",
            sent
        );
    }

    /// mute <clip> 後はそのチャンネルの NoteOn が送出されなくなる
    #[tokio::test]
    async fn mute_clip_silences_its_channel() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // mute を入れてから step
        eval(&evaluator, "mute c1\n").await;

        // AllNotesOff の吸い上げ後、scene 先頭から start（但し clip は muted なので NoteOn 無し）
        for _ in 0..10 {
            driver.step_once().await.unwrap();
        }

        let sent = handle.snapshot();
        let note_on_count = sent
            .iter()
            .filter(|m| matches!(m, MidiMessage::NoteOn { .. }))
            .count();
        assert_eq!(
            note_on_count, 0,
            "mute 後に NoteOn が送出された: {:?}",
            sent
        );
    }

    /// play → stop → play で tick カウンタが 0 にリセットされ、
    /// 新しい scene の先頭から NoteOn が送出される
    #[tokio::test]
    async fn replay_resets_tick_counter() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // 5 tick 進める
        for _ in 0..5 {
            driver.step_once().await.unwrap();
        }
        assert_eq!(driver.current_tick(), 5);

        // stop → tick が 0 に戻る
        eval(&evaluator, "stop\n").await;
        driver.step_once().await.unwrap();
        assert_eq!(driver.current_tick(), 0);

        // 再 play → 新 scene 先頭からの NoteOn が出る
        handle.clear();
        eval(&evaluator, "play s1\n").await;
        driver.step_once().await.unwrap();
        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "再 play 後に NoteOn が出ていない: {:?}",
            sent
        );
    }

    // ---------------------------------------------------------------------
    // Issue #49: 複数 device ルーティングの検証
    // ---------------------------------------------------------------------

    /// 2 つの異なる device を持つ scene を play すると、各 clip の
    /// MIDI イベントが対応する sink に**のみ**届き、相手方には流れない。
    ///
    /// Issue #49: On a scene wiring two devices, events bound to one device
    /// must be delivered only to that device's sink and not to the other.
    #[tokio::test]
    async fn multi_device_routes_events_to_correct_sinks() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "device synth_a { port port_a }\n\
                   device synth_b { port port_b }\n\
                   instrument lead {\n  device synth_a\n  channel 1\n}\n\
                   instrument pad {\n  device synth_b\n  channel 2\n}\n\
                   clip a [bars 1] {\n  lead c\n}\n\
                   clip b [bars 1] {\n  pad c\n}\n\
                   scene s { a b }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        let handle_a = SharedMockSink::new();
        let handle_b = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("synth_a".to_string(), Box::new(handle_a.clone()));
        sinks.insert("synth_b".to_string(), Box::new(handle_b.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // tick=0 の NoteOn が両 device に 1 つずつ流れる想定
        driver.step_once().await.unwrap();

        let a_sent = handle_a.snapshot();
        let b_sent = handle_b.snapshot();

        // DSL channel 1 → wire 0 (lead/synth_a)、DSL channel 2 → wire 1 (pad/synth_b)
        let lead_ch = MidiChannel::from_one_based(1).unwrap();
        let pad_ch = MidiChannel::from_one_based(2).unwrap();
        let on_with = |msgs: &[MidiMessage], ch: MidiChannel| {
            msgs.iter().any(|m| {
                if let MidiMessage::NoteOn { channel, .. } = m {
                    *channel == ch
                } else {
                    false
                }
            })
        };

        // synth_a には lead の channel の NoteOn のみ
        assert!(
            on_with(&a_sent, lead_ch),
            "synth_a に lead channel の NoteOn が来ていない: {:?}",
            a_sent
        );
        assert!(
            !on_with(&a_sent, pad_ch),
            "synth_a に pad channel の NoteOn が漏れた: {:?}",
            a_sent
        );

        // synth_b には pad の channel の NoteOn のみ
        assert!(
            on_with(&b_sent, pad_ch),
            "synth_b に pad channel の NoteOn が来ていない: {:?}",
            b_sent
        );
        assert!(
            !on_with(&b_sent, lead_ch),
            "synth_b に lead channel の NoteOn が漏れた: {:?}",
            b_sent
        );
    }

    /// 複数 device 下で `mute <clip>` すると、該当 device にのみ AllNotesOff
    /// (CC#123) が送出され、他 device には届かない。
    ///
    /// Issue #49: `mute <clip>` on a multi-device scene should send
    /// AllNotesOff only to the sink of the clip's device.
    #[tokio::test]
    async fn multi_device_mute_emits_all_notes_off_only_on_target() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "device synth_a { port port_a }\n\
                   device synth_b { port port_b }\n\
                   instrument lead {\n  device synth_a\n  channel 1\n}\n\
                   instrument pad {\n  device synth_b\n  channel 2\n}\n\
                   clip a [bars 1] {\n  lead c\n}\n\
                   clip b [bars 1] {\n  pad c\n}\n\
                   scene s { a b }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        let handle_a = SharedMockSink::new();
        let handle_b = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("synth_a".to_string(), Box::new(handle_a.clone()));
        sinks.insert("synth_b".to_string(), Box::new(handle_b.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // 1 step 進めてから全履歴クリア、続いて clip "a" を mute
        driver.step_once().await.unwrap();
        handle_a.clear();
        handle_b.clear();
        eval(&evaluator, "mute a\n").await;
        driver.step_once().await.unwrap();

        let a_sent = handle_a.snapshot();
        let b_sent = handle_b.snapshot();

        let found_anof = |msgs: &[MidiMessage], ch: MidiChannel| {
            msgs.iter().any(|m| {
                matches!(
                    m,
                    MidiMessage::ControlChange { channel, cc: 123, value: 0 } if *channel == ch
                )
            })
        };

        let lead_ch = MidiChannel::from_one_based(1).unwrap();
        let pad_ch = MidiChannel::from_one_based(2).unwrap();
        assert!(
            found_anof(&a_sent, lead_ch),
            "synth_a に AllNotesOff (lead channel) が来ていない: {:?}",
            a_sent
        );
        assert!(
            !found_anof(&b_sent, pad_ch) && !found_anof(&b_sent, lead_ch),
            "synth_b に AllNotesOff が漏れた: {:?}",
            b_sent
        );
    }

    /// 未登録 device 宛のイベントは warn してドロップするだけで、step_once が
    /// エラーにならないこと。
    ///
    /// Issue #49: Events addressed to an unknown device must be dropped with
    /// a warning; `step_once` should not propagate an error.
    #[tokio::test]
    async fn events_to_unknown_device_are_dropped_without_error() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "device unknown { port pX }\n\
                   instrument lead {\n  device unknown\n  channel 1\n}\n\
                   clip a [bars 1] {\n  lead c\n}\n\
                   scene s { a }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        // sink マップには "unknown" を登録しない
        let other = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("other".to_string(), Box::new(other.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // step_once は Ok で抜ける
        driver.step_once().await.unwrap();

        // "other" sink には何も届かない
        assert!(other.snapshot().is_empty());
    }

    // =========================================================================
    // Issue #50: MIDI System Real-Time (Start / Stop) 送出ルーティング
    // Issue #50: MIDI System Real-Time transport dispatch tests
    // =========================================================================

    /// Issue #50: play 後の step_once で transport=true device に Start が届く
    #[tokio::test]
    async fn play_dispatches_midi_start_to_transport_device() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        driver.step_once().await.unwrap();
        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::Start)),
            "Start が送出されていない: {:?}",
            sent
        );
        // Start は NoteOn より前に送られる
        let start_idx = sent.iter().position(|m| matches!(m, MidiMessage::Start));
        let note_idx = sent
            .iter()
            .position(|m| matches!(m, MidiMessage::NoteOn { .. }));
        if let (Some(s), Some(n)) = (start_idx, note_idx) {
            assert!(s < n, "Start は NoteOn より前に送出されるべき");
        }
    }

    /// Issue #50: stop 後の step_once で transport=true device に Stop が届く
    #[tokio::test]
    async fn stop_dispatches_midi_stop_to_transport_device() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);
        driver.step_once().await.unwrap();
        handle.clear();

        eval(&evaluator, "stop\n").await;
        driver.step_once().await.unwrap();

        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::Stop)),
            "Stop が送出されていない: {:?}",
            sent
        );
    }

    /// Issue #50: transport=false の device には Start/Stop が届かない
    #[tokio::test]
    async fn transport_false_device_does_not_receive_transport() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "\
            device a { port pa\n  transport true\n}\n\
            device b { port pb\n  transport false\n}\n\
            instrument inst_a { device a\n  channel 1\n}\n\
            clip c [bars 1] { inst_a c }\n\
            scene s { c }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        let handle_a = SharedMockSink::new();
        let handle_b = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("a".to_string(), Box::new(handle_a.clone()));
        sinks.insert("b".to_string(), Box::new(handle_b.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        driver.step_once().await.unwrap();

        assert!(
            handle_a
                .snapshot()
                .iter()
                .any(|m| matches!(m, MidiMessage::Start)),
            "transport=true の device a に Start が届くべき"
        );
        assert!(
            !handle_b
                .snapshot()
                .iter()
                .any(|m| matches!(m, MidiMessage::Start)),
            "transport=false の device b には Start が届くべきでない"
        );

        // stop も同様
        handle_a.clear();
        handle_b.clear();
        eval(&evaluator, "stop\n").await;
        driver.step_once().await.unwrap();

        assert!(handle_a
            .snapshot()
            .iter()
            .any(|m| matches!(m, MidiMessage::Stop)));
        assert!(!handle_b
            .snapshot()
            .iter()
            .any(|m| matches!(m, MidiMessage::Stop)));
    }

    /// Issue #50: 未登録 device への transport メッセージは warn + drop で panic しない
    #[tokio::test]
    async fn transport_to_unknown_device_is_dropped() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "\
            device ghost { port pg }\n\
            instrument inst { device ghost\n  channel 1\n}\n\
            clip c [bars 1] { inst c }\n\
            scene s { c }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        // ghost device を sinks に入れない
        let other = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("other".to_string(), Box::new(other.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        driver.step_once().await.unwrap();
        assert!(other.snapshot().is_empty());
    }

    // =========================================================================
    // PR #88: MIDI Timing Clock (0xF8) 24 PPQN 周期送出
    // PR #88: MIDI Timing Clock (0xF8) emission at 24 PPQN while playing
    // =========================================================================

    /// PR #88: play 後の最初の step_once (tick=0) で transport=true device に
    /// MIDI Timing Clock (0xF8) が届く。Start (0xFA) と同じ step で送出され、
    /// MIDI 仕様の「Start 直後の最初の Clock が beat 0」要件を満たす。
    #[tokio::test]
    async fn first_step_after_play_emits_clock_alongside_start() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        driver.step_once().await.unwrap();
        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::Start)),
            "Start が送出されていない: {:?}",
            sent
        );
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::Clock)),
            "Start と同時に最初の Clock (0xF8) が送出されていない: {:?}",
            sent
        );
    }

    /// PR #88: PPQ=480 + BPM=120 で 1 拍 (480 ticks) 進めると Clock が 24 個流れる。
    /// 24 PPQN の固定レート要件を検証する。
    #[tokio::test]
    async fn one_beat_emits_24_clocks() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // tick 0 .. 479 = 480 ticks 進める (1 拍)。Clock は tick 0, 20, ... 460 の
        // 24 箇所で送出されるはず。
        for _ in 0..480 {
            driver.step_once().await.unwrap();
        }
        let sent = handle.snapshot();
        let clock_count = sent
            .iter()
            .filter(|m| matches!(m, MidiMessage::Clock))
            .count();
        assert_eq!(
            clock_count, 24,
            "PPQ=480 で 1 拍 (480 ticks) 進めたとき Clock は 24 個でなければならない: {} 個 / 全送出 {:?}",
            clock_count, sent
        );
    }

    /// PR #88: stop 後の step_once では Clock が送出されない。
    /// active_scene が None になった時点で 24 PPQN の周期送出は止まる。
    #[tokio::test]
    async fn stop_halts_clock_emission() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // 数 tick 進めて Clock が出ていることを確認
        for _ in 0..40 {
            driver.step_once().await.unwrap();
        }
        let before_stop = handle.snapshot();
        assert!(
            before_stop.iter().any(|m| matches!(m, MidiMessage::Clock)),
            "stop 前に Clock が出ていない: {:?}",
            before_stop
        );

        // stop してから handle をクリアし、さらに step を進める
        eval(&evaluator, "stop\n").await;
        driver.step_once().await.unwrap(); // Stop 送出される step
        handle.clear();
        for _ in 0..50 {
            driver.step_once().await.unwrap();
        }
        let after_stop = handle.snapshot();
        assert!(
            !after_stop.iter().any(|m| matches!(m, MidiMessage::Clock)),
            "stop 後に Clock が送出されている: {:?}",
            after_stop
        );
    }

    /// PR #88: transport=false の device には Clock が届かない (Start/Stop と同様)
    #[tokio::test]
    async fn transport_false_device_does_not_receive_clock() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let src = "\
            device a { port pa\n  transport true\n}\n\
            device b { port pb\n  transport false\n}\n\
            instrument inst_a { device a\n  channel 1\n}\n\
            clip c [bars 1] { inst_a c }\n\
            scene s { c }\n";
        eval(&evaluator, src).await;
        eval(&evaluator, "play s\n").await;

        let handle_a = SharedMockSink::new();
        let handle_b = SharedMockSink::new();
        let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
        sinks.insert("a".to_string(), Box::new(handle_a.clone()));
        sinks.insert("b".to_string(), Box::new(handle_b.clone()));
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // tick 0 と tick 20 の 2 つの Clock 境界を踏む
        for _ in 0..40 {
            driver.step_once().await.unwrap();
        }

        assert!(
            handle_a
                .snapshot()
                .iter()
                .any(|m| matches!(m, MidiMessage::Clock)),
            "transport=true の device a に Clock が届くべき"
        );
        assert!(
            !handle_b
                .snapshot()
                .iter()
                .any(|m| matches!(m, MidiMessage::Clock)),
            "transport=false の device b には Clock が届くべきでない"
        );
    }

    // =========================================================================
    // Issue #54: SharedSinks + Notify 待機モデルの検証
    // Issue #54: SharedSinks + Notify wake-up model coverage
    // =========================================================================

    /// Issue #54: `with_shared_sinks` で構築した driver でも play → step_once で
    /// NoteOn が SharedMockSink に届くこと。`with_sinks` 互換層と同じ挙動を保つ。
    ///
    /// A driver built via `with_shared_sinks` must still dispatch NoteOn to
    /// the shared map's sink, on par with the legacy `with_sinks` path.
    #[tokio::test]
    async fn with_shared_sinks_runs_step_once() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let handle = SharedMockSink::new();
        let shared: SharedSinks = Arc::new(Mutex::new(HashMap::new()));
        shared
            .lock()
            .await
            .insert("dev".to_string(), Box::new(handle.clone()));
        let mut driver = PlaybackDriver::with_shared_sinks(evaluator.clone(), shared);

        driver.step_once().await.unwrap();
        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "NoteOn が SharedSinks 経由で送出されていない: {:?}",
            sent
        );
    }

    /// Issue #54: 空の `SharedSinks` で driver を構築し、play 評価後に
    /// 外部から `lock().await.insert(...)` で sink を後付け投入しても、
    /// 次の step_once で NoteOn が届く。これは LSP 経由で device を後から
    /// eval した場合の経路と同じ。
    ///
    /// Inserting a sink late into the shared map (mimicking dynamic device
    /// evaluation) must be picked up by the next `step_once` call.
    #[tokio::test]
    async fn shared_sinks_late_insert_dispatches_events() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        // 空の SharedSinks で driver を作る（main.rs の起動直後を模倣）。
        let shared: SharedSinks = Arc::new(Mutex::new(HashMap::new()));
        let mut driver = PlaybackDriver::with_shared_sinks(evaluator.clone(), Arc::clone(&shared));

        // 後付けで sink を投入（外部 receiver タスクの代理）。
        // tick=0 のイベントは play 直後 + 最初の step_once で消化されるため、
        // step_once を呼ぶ前に投入することで NoteOn を取りこぼさない。
        let handle = SharedMockSink::new();
        shared
            .lock()
            .await
            .insert("dev".to_string(), Box::new(handle.clone()));

        // 投入後に step_once すると、その tick のイベントが新 sink に流れる。
        driver.step_once().await.unwrap();

        let sent = handle.snapshot();
        assert!(
            sent.iter().any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "後付け sink 投入後に NoteOn が届いていない: {:?}",
            sent
        );
    }

    /// Issue #54: `run_driver_with_shared` は sinks 空のとき `notify` を待つ
    /// だけで `interval.tick()` まで到達しない。一定時間内にループから抜け
    /// ないことを `tokio::time::timeout` の Elapsed で確認する。
    ///
    /// While sinks are empty, `run_driver_with_shared` must park on
    /// `notify.notified()` and never enter the tick loop. Verified by
    /// observing a `timeout` Elapsed without any side-effect on the
    /// shared map.
    #[tokio::test]
    async fn run_driver_with_shared_blocks_until_sink_inserted() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let shared: SharedSinks = Arc::new(Mutex::new(HashMap::new()));
        let notify: SinksNotify = Arc::new(Notify::new());
        let clock = Arc::new(RwLock::new(Clock::new(120.0)));

        // 50ms 経過しても run_driver_with_shared が抜けない（= まだ tick ループに入って
        // いない）ことを timeout の Elapsed で確認する。タイムアウト前に await が解除
        // されたら fail。
        let result = tokio::time::timeout(
            Duration::from_millis(50),
            run_driver_with_shared(
                Arc::clone(&evaluator),
                Arc::clone(&shared),
                Arc::clone(&notify),
                clock,
            ),
        )
        .await;
        assert!(
            result.is_err(),
            "sinks 空状態で run_driver_with_shared が早期に return した"
        );

        // この間 sinks マップは空のままで誰も触っていない
        assert!(shared.lock().await.is_empty());
    }

    /// PR #57: `read_tick_duration_us` は共有 Clock の現在値を反映し、
    /// 0 を返さないようにクランプする。
    ///
    /// PR #57: `read_tick_duration_us` reads the current clock value and
    /// clamps to 1us minimum.
    #[test]
    fn read_tick_duration_us_reflects_clock_updates() {
        use crate::ast::tempo::Tempo;
        let clock = Arc::new(RwLock::new(Clock::new(120.0)));
        let initial = read_tick_duration_us(&clock);
        assert!(initial >= 1);

        // BPM を倍にすると tick duration はおおよそ半分になる
        clock.write().unwrap().apply_tempo(&Tempo::Absolute(240));
        let after = read_tick_duration_us(&clock);
        assert!(
            after < initial,
            "tempo 倍速化で tick duration が短くなっていない: initial={}, after={}",
            initial,
            after
        );
    }

    /// PR #57: `run_driver_with_shared` 起動後に共有 Clock の BPM を変えると、
    /// driver は新 BPM で sink にイベントを送出し続ける。回帰防止の最小確認:
    /// tempo を変えた後も NoteOn が届くこと、および BPM 変更前後で driver が
    /// パニックせずに動き続けること。
    ///
    /// PR #57: After `run_driver_with_shared` is running, mutating the shared
    /// clock's BPM keeps the driver alive and still dispatching NoteOn.
    /// Minimal regression guard.
    #[tokio::test]
    async fn run_driver_with_shared_survives_tempo_change_and_keeps_dispatching() {
        use crate::ast::tempo::Tempo;

        let evaluator = Arc::new(Mutex::new(Evaluator::new(60000.0)));
        // 高速 BPM で setup → play まで進める。clock も同じ高速値で揃える。
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let handle = SharedMockSink::new();
        let shared: SharedSinks = Arc::new(Mutex::new(HashMap::new()));
        shared
            .lock()
            .await
            .insert("dev".to_string(), Box::new(handle.clone()));
        let notify: SinksNotify = Arc::new(Notify::new());
        let clock = evaluator.lock().await.clock_handle();

        // driver を spawn (高速 BPM のまま回す)
        let driver_handle = {
            let ev = evaluator.clone();
            let sinks = Arc::clone(&shared);
            let notify = Arc::clone(&notify);
            let clk = Arc::clone(&clock);
            tokio::spawn(async move {
                run_driver_with_shared(ev, sinks, notify, clk).await;
            })
        };

        // 少し回して NoteOn が届く窓を作る
        tokio::time::sleep(Duration::from_millis(20)).await;
        let before = handle.snapshot().len();

        // BPM を 120 (1000 倍遅) に変える。driver は次 tick 比較で新 interval に
        // 切り替わるはず。tempo 変更時に driver が panic しないことが本テストの
        // メイン関心事。
        clock.write().unwrap().apply_tempo(&Tempo::Absolute(120));

        // tempo 変更後も driver が生きていることを確認するため、もう少し回す
        tokio::time::sleep(Duration::from_millis(20)).await;

        // driver はまだ生きている (= JoinHandle が完了していない)
        assert!(!driver_handle.is_finished(), "tempo 変更で driver が落ちた");

        // 全体として NoteOn が 1 件以上届いている (= sink dispatch が機能した)
        let total = handle.snapshot();
        assert!(
            total
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "tempo 変更を含む再生で NoteOn が一度も届いていない (before={}, total={})",
            before,
            total.len()
        );

        driver_handle.abort();
    }

    // =========================================================================
    // PR #60: WaitStrategy / next_wait_strategy の単体テスト
    // PR #60: Unit tests for WaitStrategy / next_wait_strategy
    // =========================================================================

    /// `now` が `deadline` を既に過ぎていれば `NoWait` を返す。
    /// 同時刻ちょうども `NoWait` 扱いとする (この瞬間に step すべき)。
    ///
    /// When `now >= deadline`, the strategy is `NoWait`: it is already
    /// time (or past time) to fire the next tick.
    #[test]
    fn next_wait_strategy_no_wait_when_deadline_passed() {
        let now = Instant::now();
        // deadline ≦ now の場合は全て NoWait
        let past = now - Duration::from_micros(100);
        assert!(matches!(
            next_wait_strategy(now, past),
            WaitStrategy::NoWait
        ));
        assert!(matches!(next_wait_strategy(now, now), WaitStrategy::NoWait));
    }

    /// 残り時間が `SPIN_THRESHOLD` (= 2ms) 以下なら `SpinUntil(deadline)` を返す。
    /// 上限ぎりぎり (2ms ちょうど) も spin 側に倒し、busy wait 範囲を広めに取る。
    ///
    /// When the remaining time is at most `SPIN_THRESHOLD`, busy-wait via
    /// `SpinUntil(deadline)` to maximize precision.
    #[test]
    fn next_wait_strategy_spins_when_within_threshold() {
        let now = Instant::now();
        // 残り 1us → spin
        let near = now + Duration::from_micros(1);
        match next_wait_strategy(now, near) {
            WaitStrategy::SpinUntil(t) => assert_eq!(t, near),
            other => panic!("expected SpinUntil, got {:?}", other),
        }
        // 残り 2ms ちょうど → spin (境界は spin 側)
        let edge = now + SPIN_THRESHOLD;
        match next_wait_strategy(now, edge) {
            WaitStrategy::SpinUntil(t) => assert_eq!(t, edge),
            other => panic!("expected SpinUntil at threshold, got {:?}", other),
        }
    }

    /// 残り時間が `SPIN_THRESHOLD` を超えるなら `Sleep(残り - SPIN_THRESHOLD)` を返す。
    /// sleep 後に SPIN_THRESHOLD 以下の領域に入る想定で、その後 spin に切り替わる。
    ///
    /// Beyond the threshold, sleep for `remaining - SPIN_THRESHOLD` so the
    /// next iteration will be inside the spin region.
    #[test]
    fn next_wait_strategy_sleeps_minus_threshold_when_over() {
        let now = Instant::now();
        // 残り 10ms → Sleep(10ms - 2ms) = Sleep(8ms)
        let far = now + Duration::from_millis(10);
        match next_wait_strategy(now, far) {
            WaitStrategy::Sleep(d) => {
                assert_eq!(d, Duration::from_millis(10) - SPIN_THRESHOLD);
            }
            other => panic!("expected Sleep, got {:?}", other),
        }
        // 残り 2ms + 1us → 1us だけ sleep
        let just_over = now + SPIN_THRESHOLD + Duration::from_micros(1);
        match next_wait_strategy(now, just_over) {
            WaitStrategy::Sleep(d) => assert_eq!(d, Duration::from_micros(1)),
            other => panic!("expected Sleep just over threshold, got {:?}", other),
        }
    }

    /// 送出済みメッセージ列から最初の NoteOn のノート番号を取り出す
    /// Extract the first NoteOn note number from a list of sent messages.
    fn first_note_on_sent(msgs: &[MidiMessage]) -> Option<u8> {
        msgs.iter().find_map(|m| match m {
            MidiMessage::NoteOn { note, .. } => Some(*note),
            _ => None,
        })
    }

    /// 再生中の clip 上書きは「1 小節境界」では切り替わらず、transport 起点の
    /// 「4 小節グリッド」で初めて新 clip へ一斉 commit される。
    ///
    /// ppq を 24 に下げて 1 小節 = 96 tick / 4 小節グリッド = 384 tick とし、
    /// step 数を現実的に抑えて検証する。
    ///
    /// A clip overwrite during playback does NOT swap on a 1-bar boundary; it
    /// commits to the new clip only on the transport's 4-bar grid. PPQ is
    /// lowered to 24 (1 bar = 96 ticks, grid = 384 ticks) to keep the step
    /// count small.
    #[tokio::test]
    async fn clip_swap_applies_on_four_bar_grid_not_each_bar() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        // grid を縮めるため play 前に ppq を下げる（compile と grid 双方に効く）
        {
            let ev = evaluator.lock().await;
            ev.clock_handle().write().unwrap().set_ppq(24);
        }
        // 4 小節グリッドをまたぐので [loop] 再生にする（既定は Once）
        eval(&evaluator, "play s1 [loop]\n").await;

        let (sinks, handle) = single_dev_sinks();
        let mut driver = PlaybackDriver::with_sinks(evaluator.clone(), sinks);

        // tick 0 の NoteOn を基準ノートとして取得（初期 clip = inst c）
        driver.step_once().await.unwrap();
        let base = first_note_on_sent(&handle.snapshot()).expect("initial NoteOn at tick 0");

        // 再生中に c1 を inst e (+4 半音) で上書き → pending stage
        eval(&evaluator, "clip c1 [bars 1] {\n  inst e\n}\n").await;

        // 1 小節境界 (tick 96) では 4 小節グリッドでないので未切替（旧ノートのまま）
        while driver.current_tick() < 96 {
            driver.step_once().await.unwrap();
        }
        handle.clear();
        driver.step_once().await.unwrap(); // tick 96 のイベント送出
        assert_eq!(
            first_note_on_sent(&handle.snapshot()),
            Some(base),
            "1 小節境界では切り替わってはいけない"
        );

        // 4 小節グリッド (tick 384) で commit → 新ノート (+4) に切り替わる
        while driver.current_tick() < 384 {
            driver.step_once().await.unwrap();
        }
        handle.clear();
        driver.step_once().await.unwrap(); // tick 384: commit 後に events_at
        assert_eq!(
            first_note_on_sent(&handle.snapshot()),
            Some(base + 4),
            "4 小節グリッドで新 clip へ切り替わるべき"
        );
    }
}
