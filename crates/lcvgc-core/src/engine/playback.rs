//! 本番再生ドライバ
//!
//! `Evaluator` の `active_scene` を tick 毎に借用し、各 tick のイベントを
//! `MidiSink` に送出する。`Stop` 評価で蓄積された AllNotesOff も吸い上げる。
//!
//! state の single source of truth は Evaluator 側に集約され、driver は
//! 「読むだけ（+ AllNotesOff 取り出し）」の薄いレイヤとして振る舞う。
//!
//! Playback driver for production. Borrows `Evaluator::active_scene` tick
//! by tick and dispatches events to a `MidiSink`, draining queued
//! AllNotesOff messages from Stop evaluation. The evaluator remains the
//! single source of truth; the driver is a thin read-only adapter.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time;
use tracing::{error, info, warn};

use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::evaluator::{Evaluator, SceneTransitionOutcome};
use crate::engine::midi_sink::MidiSink;
use crate::midi::message::MidiMessage;

/// 1 つの論理 device に紐付く MIDI sink エントリ
///
/// Issue #49 で追加された型エイリアス。`PlaybackDriver` は device 論理名を
/// キーにこの sink を選び、MIDI イベントを送出する。
///
/// Boxed `MidiSink` entry keyed by logical device name (Issue #49).
pub type BoxedSink = Box<dyn MidiSink>;

/// tick 駆動の再生ドライバ
///
/// Issue #49: device ごとに `MidiSink` を保持する HashMap 形式に拡張。
/// `MidiEvent.device` をキーに対応する sink へ振り分ける。未登録 device
/// 宛のイベントは warn ログを出してドロップする。
///
/// Tick-driven playback driver. As of Issue #49 the driver owns a map of
/// `(device name -> MidiSink)` and routes every event to the sink matching
/// `MidiEvent.device`; events addressed to unknown devices are logged at
/// `warn` level and dropped.
pub struct PlaybackDriver {
    /// 共有 Evaluator / Shared evaluator
    evaluator: Arc<Mutex<Evaluator>>,
    /// device 論理名 -> MIDI sink のマップ
    /// Logical device name -> MIDI sink map
    sinks: HashMap<String, BoxedSink>,
    /// 現在の tick 位置 / Current tick position
    current_tick: u64,
    /// 前回 step 時に active_scene が Some だったか（None→Some の遷移で tick リセット）
    /// Whether the last step observed an active scene (used to reset current_tick on
    /// None→Some transition).
    was_active: bool,
}

impl PlaybackDriver {
    /// sink マップを明示指定して `PlaybackDriver` を生成する
    ///
    /// # Arguments
    /// * `evaluator` - Arc<Mutex<Evaluator>> 共有参照
    /// * `sinks` - device 論理名 -> MidiSink ボックスのマップ
    pub fn with_sinks(evaluator: Arc<Mutex<Evaluator>>, sinks: HashMap<String, BoxedSink>) -> Self {
        Self {
            evaluator,
            sinks,
            current_tick: 0,
            was_active: false,
        }
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

    /// 指定 device の sink への不変参照（テスト用途）
    /// Immutable reference to the sink for `device` (for tests).
    pub fn sink(&self, device: &str) -> Option<&BoxedSink> {
        self.sinks.get(device)
    }

    /// 指定 device の sink への可変参照（テスト用途）
    /// Mutable reference to the sink for `device` (for tests).
    pub fn sink_mut(&mut self, device: &str) -> Option<&mut BoxedSink> {
        self.sinks.get_mut(device)
    }

    /// `MidiEvent.device` に対応する sink を解決する
    ///
    /// 空文字列 (= compile 時に device 未指定だった MidiEvent) は
    /// `"default"` sink にフォールバックする。該当 sink が無ければ `None`。
    ///
    /// Resolve the sink for `event_device`; empty string falls back to the
    /// `"default"` sink. Returns `None` when neither key is registered.
    fn resolve_sink(&mut self, event_device: &str) -> Option<&mut BoxedSink> {
        let key: &str = if event_device.is_empty() {
            "default"
        } else {
            event_device
        };
        self.sinks.get_mut(key)
    }

    /// 1 tick 進める
    ///
    /// 1. `take_pending_all_notes_off` の結果を CC#123 value=0 として各 channel へ送出
    /// 2. `active_scene_mut` が Some なら `events_at(current_tick)` を送出し `advance_all(1)`、
    ///    `scene_tick_length` 境界に到達したら `on_scene_loop_complete` を呼ぶ
    /// 3. `active_scene` が None なら current_tick を 0 にリセット
    ///
    /// Advances by one tick; dispatches queued AllNotesOff, then events
    /// at the current tick while `active_scene` is Some, resetting the
    /// tick counter when it goes back to None.
    pub async fn step_once(&mut self) -> Result<(), EngineError> {
        let mut ev = self.evaluator.lock().await;

        // Evaluator ロック中に AllNotesOff / Transport キューを吸い上げる（借用を短く保つ）
        // Issue #50: System Real-Time Start/Stop は Play/Stop 評価で積まれる。
        let pending_all_notes_off = ev.take_pending_all_notes_off();
        let pending_transport = ev.take_pending_transport();

        let Some(scene) = ev.active_scene_mut() else {
            // 再生停止中: tick をリセットして次の play に備える
            // 残っている AllNotesOff / Transport は送出して stop 側面をカバーする
            self.current_tick = 0;
            self.was_active = false;
            drop(ev);
            self.dispatch_transport(&pending_transport)?;
            self.dispatch_all_notes_off(&pending_all_notes_off)?;
            return Ok(());
        };

        // None→Some 遷移を検出したら tick を 0 から始める
        if !self.was_active {
            self.current_tick = 0;
            self.was_active = true;
        }

        // Issue #49: (device, message) ペアで送出先を確定させる
        let routed: Vec<(String, MidiMessage)> = scene
            .events_at(self.current_tick)
            .into_iter()
            .map(|ev| (ev.device.clone(), ev.message.clone()))
            .collect();
        scene.advance_all(1);
        let scene_len = scene.scene_tick_length();
        drop(ev);

        // Issue #50: まず Transport (Start/Stop) を送出する。Start は tick イベントより
        // 前に外部機材に届ける必要があり、Stop も AllNotesOff と並んで早めに送るのが自然。
        // Issue #50: emit Transport (Start/Stop) first so external gear sees Start
        // before any note tick. Stop dovetails with AllNotesOff as a stop-side cleanup.
        self.dispatch_transport(&pending_transport)?;

        // 先に AllNotesOff を送出（scene 境界や mute で積まれた分）
        self.dispatch_all_notes_off(&pending_all_notes_off)?;

        // 続いて本来の tick イベントを送出
        for (device, msg) in &routed {
            match self.resolve_sink(device) {
                Some(sink) => sink.send(msg)?,
                None => warn!(
                    "イベント送出先 sink が未登録: device={} msg={:?}",
                    device, msg
                ),
            }
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

    /// 蓄積された `(device, channel)` ごとに AllNotesOff (CC#123 value=0) を
    /// 該当 sink へ送出する。未登録 device は warn ログを出してスキップする。
    ///
    /// Dispatches `(device, channel)` AllNotesOff pairs queued by the
    /// evaluator to the matching sink, warning and skipping unknown devices.
    fn dispatch_all_notes_off(&mut self, pairs: &[(String, u8)]) -> Result<(), EngineError> {
        for (device, ch) in pairs {
            let msg = MidiMessage::ControlChange {
                channel: *ch,
                cc: 123,
                value: 0,
            };
            match self.resolve_sink(device) {
                Some(sink) => sink.send(&msg)?,
                None => warn!(
                    "AllNotesOff の送出先 sink が未登録: device={} channel={}",
                    device, ch
                ),
            }
        }
        Ok(())
    }

    /// Issue #50: Evaluator が蓄積した `(device, MidiMessage)`（Start / Stop /
    /// Continue）を該当 sink に送出する。未登録 device は warn + drop。
    ///
    /// Issue #50: dispatch `(device, MidiMessage)` System Real-Time pairs queued
    /// by the Evaluator. Unknown devices are logged and dropped.
    fn dispatch_transport(&mut self, pairs: &[(String, MidiMessage)]) -> Result<(), EngineError> {
        for (device, msg) in pairs {
            match self.resolve_sink(device) {
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

/// tokio タスクで PlaybackDriver を tick 間隔で駆動する
///
/// Clock の `tick_duration_us()` を参照して sleep する単純なループ。
/// `EngineError` は error ログに出力してループ継続する（将来的にはリカバリ戦略を拡張）。
/// Issue #49: sink マップで複数 device 宛を受け取り、`MidiEvent.device` で
/// 振り分ける。
///
/// Runs `PlaybackDriver::step_once` on a tokio interval derived from the
/// clock's tick duration. Errors are logged; the loop continues. As of
/// Issue #49 the caller supplies a `device name -> MidiSink` map so that
/// events generated from clips bound to different devices are dispatched
/// to the corresponding sink.
pub async fn run_driver(
    evaluator: Arc<Mutex<Evaluator>>,
    sinks: HashMap<String, BoxedSink>,
    clock: Clock,
) {
    let mut driver = PlaybackDriver::with_sinks(evaluator, sinks);
    let dur_us = clock.tick_duration_us().max(1);
    let mut interval = time::interval(Duration::from_micros(dur_us));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    info!(
        "再生ドライバ起動: tick duration = {} us (BPM={}, PPQ={}, devices={:?})",
        dur_us,
        clock.bpm(),
        clock.ppq(),
        driver.sinks.keys().collect::<Vec<_>>()
    );

    loop {
        interval.tick().await;
        if let Err(e) = driver.step_once().await {
            error!("再生ドライバエラー: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::midi_sink::SharedMockSink;

    /// eval_source で DSL を評価する小ヘルパ
    async fn eval(evaluator: &Arc<Mutex<Evaluator>>, source: &str) {
        let mut ev = evaluator.lock().await;
        ev.eval_source(source).expect("eval ok");
    }

    /// device + instrument + clip + scene を一通り登録する DSL
    /// channel 0 の clip `c1` を scene `s1` に登録、その後呼び出し側で `play s1` を発行する
    fn setup_src() -> &'static str {
        "device dev { port test }\n\
         instrument inst { device dev\n channel 0 }\n\
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

        // CC#123 value=0 on channel 0 が送出されていること
        let sent = handle.snapshot();
        let found_all_notes_off = sent.iter().any(|m| {
            matches!(
                m,
                MidiMessage::ControlChange {
                    channel: 0,
                    cc: 123,
                    value: 0,
                }
            )
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

        // synth_a には channel=1 の NoteOn のみ
        assert!(
            a_sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { channel: 1, .. })),
            "synth_a に channel=1 の NoteOn が来ていない: {:?}",
            a_sent
        );
        assert!(
            !a_sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { channel: 2, .. })),
            "synth_a に channel=2 の NoteOn が漏れた: {:?}",
            a_sent
        );

        // synth_b には channel=2 の NoteOn のみ
        assert!(
            b_sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { channel: 2, .. })),
            "synth_b に channel=2 の NoteOn が来ていない: {:?}",
            b_sent
        );
        assert!(
            !b_sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { channel: 1, .. })),
            "synth_b に channel=1 の NoteOn が漏れた: {:?}",
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

        let found_anof = |msgs: &[MidiMessage], ch: u8| {
            msgs.iter().any(|m| {
                matches!(
                    m,
                    MidiMessage::ControlChange { channel, cc: 123, value: 0 } if *channel == ch
                )
            })
        };

        assert!(
            found_anof(&a_sent, 1),
            "synth_a に AllNotesOff (ch=1) が来ていない: {:?}",
            a_sent
        );
        assert!(
            !found_anof(&b_sent, 2) && !found_anof(&b_sent, 1),
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
            instrument inst { device ghost\n  channel 0\n}\n\
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
}
