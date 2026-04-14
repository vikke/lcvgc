use std::thread;
use std::time::Duration;

use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::midi_sink::MidiSink;
use crate::engine::player::ScenePlayer;

/// ScenePlayer を tick 単位で駆動し、各 tick のイベントを MidiSink に送出するループ
///
/// Phase 2 では Evaluator 統合前のプリミティブとして同期 API (`step`) と
/// 実時刻駆動 API (`run_for`) を提供する。
/// MIDI sink への送出順は `ScenePlayer::events_at` が返す順に従う。
///
/// Drives a `ScenePlayer` tick by tick, dispatching each tick's events to a
/// `MidiSink`. Phase 2 primitive: synchronous `step` plus wall-clock `run_for`.
pub struct TickLoop<S: MidiSink> {
    /// 駆動対象のシーンプレイヤー / Scene player to drive
    scene: ScenePlayer,
    /// テンポ・PPQ を提供するクロック / Clock providing tempo and PPQ
    clock: Clock,
    /// MIDI 送出先 / MIDI sink destination
    sink: S,
    /// 現在の tick 位置 / Current tick position
    current_tick: u64,
}

impl<S: MidiSink> TickLoop<S> {
    /// 新しい TickLoop を生成する
    ///
    /// # 引数 / Arguments
    /// * `scene` - 駆動する ScenePlayer / ScenePlayer to drive
    /// * `clock` - テンポ・PPQ を提供する Clock / Clock providing tempo and PPQ
    /// * `sink` - MIDI 送出先 / MIDI sink
    pub fn new(scene: ScenePlayer, clock: Clock, sink: S) -> Self {
        Self {
            scene,
            clock,
            sink,
            current_tick: 0,
        }
    }

    /// 現在の tick 位置を返す
    /// Returns the current tick position.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// 内部 ScenePlayer への不変参照
    pub fn scene(&self) -> &ScenePlayer {
        &self.scene
    }

    /// 内部 ScenePlayer への可変参照（動的差し替え・ミュート操作などに使用）
    pub fn scene_mut(&mut self) -> &mut ScenePlayer {
        &mut self.scene
    }

    /// 内部 Sink への不変参照（テスト用途）
    pub fn sink(&self) -> &S {
        &self.sink
    }

    /// 内部 Sink への可変参照
    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// 内部 Clock への不変参照
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// 1 tick 分進める: 現在 tick のイベントを送出してから `advance_all(1)`
    ///
    /// 送出途中でエラーが起きた場合は即座に返す（残イベントは未送出、tick も未進行）。
    ///
    /// Advances by one tick: dispatches events at the current tick, then advances
    /// the scene by one tick. Returns early on the first sink error.
    pub fn step(&mut self) -> Result<(), EngineError> {
        let messages: Vec<_> = self
            .scene
            .events_at(self.current_tick)
            .into_iter()
            .map(|ev| ev.message.clone())
            .collect();
        for msg in &messages {
            self.sink.send(msg)?;
        }
        self.scene.advance_all(1);
        self.current_tick += 1;
        Ok(())
    }

    /// 指定 tick 数を実時刻駆動で進める
    ///
    /// 各 tick の間に `clock.tick_duration_us()` マイクロ秒 sleep する。
    /// テストでは `Clock` を高 BPM/低 PPQ にすれば実行時間を短縮できる。
    ///
    /// Runs for `ticks` ticks in wall-clock time, sleeping `tick_duration_us`
    /// microseconds between ticks. For tests prefer `step` or a fast clock.
    pub fn run_for(&mut self, ticks: u64) -> Result<(), EngineError> {
        let dur = Duration::from_micros(self.clock.tick_duration_us());
        for _ in 0..ticks {
            self.step()?;
            if dur.as_micros() > 0 {
                thread::sleep(dur);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::compiler::{CompiledClip, MidiEvent};
    use crate::engine::midi_sink::MockSink;
    use crate::midi::message::MidiMessage;

    fn note_on(note: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel: 0,
            note,
            velocity: 100,
        }
    }

    fn make_clip(events: Vec<(u64, MidiMessage)>, total_ticks: u64) -> CompiledClip {
        CompiledClip {
            events: events
                .into_iter()
                .map(|(tick, message)| MidiEvent { tick, message })
                .collect(),
            total_ticks,
            warnings: vec![],
        }
    }

    /// 空 scene に対する step は no-op（sink に何も送出されず、tick のみ進む）
    #[test]
    fn step_on_empty_scene_is_noop_but_advances_tick() {
        let scene = ScenePlayer::new();
        let clock = Clock::new(120.0);
        let sink = MockSink::default();
        let mut tl = TickLoop::new(scene, clock, sink);

        tl.step().unwrap();
        assert_eq!(tl.current_tick(), 1);
        assert!(tl.sink().sent.is_empty());
    }

    /// tick 0 にあるイベントは step() 1 回で送出される
    #[test]
    fn step_dispatches_events_at_current_tick() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60)), (0, note_on(64))], 480),
            true,
        );
        let mut tl = TickLoop::new(scene, Clock::new(120.0), MockSink::default());

        tl.step().unwrap();
        assert_eq!(tl.sink().sent.len(), 2);
        assert_eq!(tl.current_tick(), 1);
    }

    /// tick 0 と tick 240 のイベントが、それぞれの step で送出される順を検証
    #[test]
    fn step_sequence_preserves_tick_order() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60)), (2, note_on(64))], 480),
            true,
        );
        let mut tl = TickLoop::new(scene, Clock::new(120.0), MockSink::default());

        tl.step().unwrap(); // tick 0
        tl.step().unwrap(); // tick 1
        tl.step().unwrap(); // tick 2

        assert_eq!(tl.sink().sent.len(), 2);
        assert!(matches!(
            tl.sink().sent[0],
            MidiMessage::NoteOn { note: 60, .. }
        ));
        assert!(matches!(
            tl.sink().sent[1],
            MidiMessage::NoteOn { note: 64, .. }
        ));
        assert_eq!(tl.current_tick(), 3);
    }

    /// ミュート中のクリップのイベントは送出されない（Phase 1 API との連携）
    #[test]
    fn muted_clip_events_are_not_dispatched() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.mute_clip("a");
        let mut tl = TickLoop::new(scene, Clock::new(120.0), MockSink::default());

        tl.step().unwrap();
        assert!(tl.sink().sent.is_empty());
        assert_eq!(tl.current_tick(), 1);
    }

    /// run_for(n) は n step と等価（sink 送出件数で検証）
    #[test]
    fn run_for_advances_exact_tick_count() {
        let mut scene = ScenePlayer::new();
        // tick 0, 1, 2 にそれぞれ1イベント
        scene.add_clip(
            "a".to_string(),
            make_clip(
                vec![(0, note_on(60)), (1, note_on(61)), (2, note_on(62))],
                480,
            ),
            true,
        );
        // 高速クロック（BPM=60000, PPQ=1 → tick_duration_us=1）で run_for を即時化
        let clock = Clock::with_ppq(60000.0, 1);
        let mut tl = TickLoop::new(scene, clock, MockSink::default());

        tl.run_for(3).unwrap();
        assert_eq!(tl.current_tick(), 3);
        assert_eq!(tl.sink().sent.len(), 3);
    }

    /// scene_mut 経由のミュート操作が次 step から反映される
    #[test]
    fn scene_mut_mute_takes_effect_next_step() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60)), (1, note_on(61))], 480),
            true,
        );
        let mut tl = TickLoop::new(scene, Clock::new(120.0), MockSink::default());

        tl.step().unwrap(); // tick 0: 送出される
        assert_eq!(tl.sink().sent.len(), 1);

        tl.scene_mut().mute_clip("a");
        tl.step().unwrap(); // tick 1: ミュート済みで送出されない
        assert_eq!(tl.sink().sent.len(), 1);
    }
}
