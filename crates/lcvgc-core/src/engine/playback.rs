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

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time;
use tracing::{error, info};

use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::evaluator::{Evaluator, SceneTransitionOutcome};
use crate::engine::midi_sink::MidiSink;
use crate::midi::message::MidiMessage;

/// tick 駆動の再生ドライバ
///
/// Tick-driven playback driver.
pub struct PlaybackDriver<S: MidiSink> {
    /// 共有 Evaluator / Shared evaluator
    evaluator: Arc<Mutex<Evaluator>>,
    /// MIDI 送出先 / MIDI sink
    sink: S,
    /// 現在の tick 位置 / Current tick position
    current_tick: u64,
    /// 前回 step 時に active_scene が Some だったか（None→Some の遷移で tick リセット）
    /// Whether the last step observed an active scene (used to reset current_tick on
    /// None→Some transition).
    was_active: bool,
}

impl<S: MidiSink> PlaybackDriver<S> {
    /// 新しい PlaybackDriver を生成する
    ///
    /// # Arguments
    /// * `evaluator` - Arc<Mutex<Evaluator>> 共有参照
    /// * `sink` - MIDI 出力先
    pub fn new(evaluator: Arc<Mutex<Evaluator>>, sink: S) -> Self {
        Self {
            evaluator,
            sink,
            current_tick: 0,
            was_active: false,
        }
    }

    /// 現在の tick 位置を返す
    /// Returns the current tick position.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// 内部 sink への不変参照（テスト用途）
    /// Immutable reference to the sink (for tests).
    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// 内部 sink への可変参照（テスト用途）
    /// Mutable reference to the sink (for tests).
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
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

        // AllNotesOff (CC#123 value=0) を各 channel に送出
        for ch in ev.take_pending_all_notes_off() {
            let msg = MidiMessage::ControlChange {
                channel: ch,
                cc: 123,
                value: 0,
            };
            self.sink.send(&msg)?;
        }

        let Some(scene) = ev.active_scene_mut() else {
            // 再生停止中: tick をリセットして次の play に備える
            self.current_tick = 0;
            self.was_active = false;
            return Ok(());
        };

        // None→Some 遷移を検出したら tick を 0 から始める
        if !self.was_active {
            self.current_tick = 0;
            self.was_active = true;
        }

        let messages: Vec<MidiMessage> = scene
            .events_at(self.current_tick)
            .into_iter()
            .map(|ev| ev.message.clone())
            .collect();
        scene.advance_all(1);
        let scene_len = scene.scene_tick_length();
        drop(ev);

        for msg in &messages {
            self.sink.send(msg)?;
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
}

/// tokio タスクで PlaybackDriver を tick 間隔で駆動する
///
/// Clock の `tick_duration_us()` を参照して sleep する単純なループ。
/// `EngineError` は error ログに出力してループ継続する（将来的にはリカバリ戦略を拡張）。
///
/// Runs `PlaybackDriver::step_once` on a tokio interval derived from the
/// clock's tick duration. Errors are logged; the loop continues.
pub async fn run_driver<S: MidiSink>(evaluator: Arc<Mutex<Evaluator>>, sink: S, clock: Clock) {
    let mut driver = PlaybackDriver::new(evaluator, sink);
    let dur_us = clock.tick_duration_us().max(1);
    let mut interval = time::interval(Duration::from_micros(dur_us));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    info!(
        "再生ドライバ起動: tick duration = {} us (BPM={}, PPQ={})",
        dur_us,
        clock.bpm(),
        clock.ppq()
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
    use crate::engine::midi_sink::MockSink;

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

    /// 空 Evaluator に対する step_once は sink に何も出さず tick もリセット状態を保つ
    #[tokio::test]
    async fn step_once_on_empty_evaluator_is_noop() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let mut driver = PlaybackDriver::new(evaluator, MockSink::default());

        driver.step_once().await.unwrap();
        assert!(driver.sink().sent.is_empty());
        assert_eq!(driver.current_tick(), 0);
    }

    /// play 直後に step_once を数回実行すると clip のイベントが MockSink に落ちる
    #[tokio::test]
    async fn play_then_step_sends_note_events() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;

        let mut driver = PlaybackDriver::new(evaluator.clone(), MockSink::default());

        // 最初の step で tick=0 のイベント (NoteOn) が送出される
        driver.step_once().await.unwrap();
        assert!(
            driver
                .sink()
                .sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "NoteOn が送出されていない: {:?}",
            driver.sink().sent
        );
    }

    /// stop 評価で蓄積された AllNotesOff (CC#123) が次 step で送出される
    #[tokio::test]
    async fn stop_emits_all_notes_off_on_next_step() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        // まず 1 step 進めて NoteOn を出す
        let mut driver = PlaybackDriver::new(evaluator.clone(), MockSink::default());
        driver.step_once().await.unwrap();
        driver.sink_mut().sent.clear();

        // stop 評価 → active_scene=None + pending_all_notes_off に ch0 が積まれる
        eval(&evaluator, "stop\n").await;

        driver.step_once().await.unwrap();

        // CC#123 value=0 on channel 0 が送出されていること
        let found_all_notes_off = driver.sink().sent.iter().any(|m| {
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
            driver.sink().sent
        );
    }

    /// mute <clip> 後はそのチャンネルの NoteOn が送出されなくなる
    #[tokio::test]
    async fn mute_clip_silences_its_channel() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let mut driver = PlaybackDriver::new(evaluator.clone(), MockSink::default());

        // mute を入れてから step
        eval(&evaluator, "mute c1\n").await;

        // AllNotesOff の吸い上げ後、scene 先頭から start（但し clip は muted なので NoteOn 無し）
        for _ in 0..10 {
            driver.step_once().await.unwrap();
        }

        let note_on_count = driver
            .sink()
            .sent
            .iter()
            .filter(|m| matches!(m, MidiMessage::NoteOn { .. }))
            .count();
        assert_eq!(
            note_on_count,
            0,
            "mute 後に NoteOn が送出された: {:?}",
            driver.sink().sent
        );
    }

    /// play → stop → play で tick カウンタが 0 にリセットされ、
    /// 新しい scene の先頭から NoteOn が送出される
    #[tokio::test]
    async fn replay_resets_tick_counter() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        eval(&evaluator, setup_src()).await;
        eval(&evaluator, "play s1\n").await;
        let mut driver = PlaybackDriver::new(evaluator.clone(), MockSink::default());

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
        driver.sink_mut().sent.clear();
        eval(&evaluator, "play s1\n").await;
        driver.step_once().await.unwrap();
        assert!(
            driver
                .sink()
                .sent
                .iter()
                .any(|m| matches!(m, MidiMessage::NoteOn { .. })),
            "再 play 後に NoteOn が出ていない: {:?}",
            driver.sink().sent
        );
    }
}
