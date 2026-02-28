use crate::engine::compiler::{CompiledClip, MidiEvent};

/// 単一クリップの再生状態を管理するプレイヤー
#[derive(Debug, Clone)]
pub struct ClipPlayer {
    /// 再生対象のコンパイル済みクリップ
    clip: CompiledClip,
    /// 現在の再生tick位置
    current_tick: u64,
    /// ループ再生するかどうか
    looping: bool,
}

impl ClipPlayer {
    /// 新しいClipPlayerを生成する
    pub fn new(clip: CompiledClip, looping: bool) -> Self {
        Self {
            clip,
            current_tick: 0,
            looping,
        }
    }

    /// 指定tickにあるイベントを返す
    ///
    /// ループ時はtotal_ticksでmodした実効tickで検索する。
    /// 非ループ時はtotal_ticksを超えたら空を返す。
    pub fn events_at(&self, tick: u64) -> Vec<&MidiEvent> {
        if !self.looping && tick >= self.clip.total_ticks {
            return Vec::new();
        }
        let effective = self.effective_tick(tick);
        self.clip
            .events
            .iter()
            .filter(|e| e.tick == effective)
            .collect()
    }

    /// 現在の再生tick位置を取得
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// tickを進める
    pub fn advance(&mut self, ticks: u64) {
        self.current_tick += ticks;
    }

    /// ループ完了判定（looping=falseの場合のみtrue）
    pub fn is_done(&self) -> bool {
        if self.looping {
            false
        } else {
            self.current_tick >= self.clip.total_ticks
        }
    }

    /// 再生位置をリセット
    pub fn reset(&mut self) {
        self.current_tick = 0;
    }

    /// ループ内の実効tick（total_ticksでmod）
    fn effective_tick(&self, tick: u64) -> u64 {
        if self.clip.total_ticks == 0 {
            return 0;
        }
        tick % self.clip.total_ticks
    }
}

/// 複数クリップを並行管理するシーンプレイヤー
///
/// ポリリズム対応：各クリップは独自のtotal_ticksを持つ
#[derive(Debug)]
pub struct ScenePlayer {
    /// (クリップ名, プレイヤー) のリスト
    players: Vec<(String, ClipPlayer)>,
}

impl ScenePlayer {
    /// 空のScenePlayerを生成する
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
        }
    }

    /// クリップを追加
    pub fn add_clip(&mut self, name: String, clip: CompiledClip, looping: bool) {
        self.players.push((name, ClipPlayer::new(clip, looping)));
    }

    /// 指定tickの全クリップのイベントを収集
    pub fn events_at(&self, tick: u64) -> Vec<&MidiEvent> {
        self.players
            .iter()
            .flat_map(|(_, player)| player.events_at(tick))
            .collect()
    }

    /// 全クリップのtickを進める
    pub fn advance_all(&mut self, ticks: u64) {
        for (_, player) in &mut self.players {
            player.advance(ticks);
        }
    }

    /// 全クリップが完了したか（looping=trueのクリップは常にfalse）
    pub fn all_done(&self) -> bool {
        self.players
            .iter()
            .filter(|(_, p)| !p.looping)
            .all(|(_, p)| p.is_done())
    }

    /// 全クリップをリセット
    pub fn reset_all(&mut self) {
        for (_, player) in &mut self.players {
            player.reset();
        }
    }

    /// クリップ数
    pub fn clip_count(&self) -> usize {
        self.players.len()
    }
}

impl Default for ScenePlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::message::MidiMessage;

    /// テスト用のCompiledClipを生成するヘルパー
    fn make_clip(events: Vec<(u64, MidiMessage)>, total_ticks: u64) -> CompiledClip {
        CompiledClip {
            events: events
                .into_iter()
                .map(|(tick, message)| MidiEvent { tick, message })
                .collect(),
            total_ticks,
        }
    }

    fn note_on(note: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel: 0,
            note,
            velocity: 100,
        }
    }

    fn note_off(note: u8) -> MidiMessage {
        MidiMessage::NoteOff {
            channel: 0,
            note,
            velocity: 0,
        }
    }

    #[test]
    fn clip_player_new_initializes_correctly() {
        let clip = make_clip(vec![], 480);
        let player = ClipPlayer::new(clip, true);
        assert_eq!(player.current_tick(), 0);
        assert!(!player.is_done());
    }

    #[test]
    fn events_at_returns_matching_events() {
        let clip = make_clip(vec![(0, note_on(60)), (0, note_on(64)), (240, note_on(67))], 480);
        let player = ClipPlayer::new(clip, false);
        let events = player.events_at(0);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn events_at_no_match_returns_empty() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let player = ClipPlayer::new(clip, false);
        let events = player.events_at(100);
        assert!(events.is_empty());
    }

    #[test]
    fn advance_increments_tick() {
        let clip = make_clip(vec![], 480);
        let mut player = ClipPlayer::new(clip, false);
        player.advance(10);
        assert_eq!(player.current_tick(), 10);
        player.advance(5);
        assert_eq!(player.current_tick(), 15);
    }

    #[test]
    fn is_done_when_not_looping_past_total() {
        let clip = make_clip(vec![], 480);
        let mut player = ClipPlayer::new(clip, false);
        assert!(!player.is_done());
        player.advance(480);
        assert!(player.is_done());
    }

    #[test]
    fn is_done_false_when_looping() {
        let clip = make_clip(vec![], 480);
        let mut player = ClipPlayer::new(clip, true);
        player.advance(9999);
        assert!(!player.is_done());
    }

    #[test]
    fn reset_sets_tick_to_zero() {
        let clip = make_clip(vec![], 480);
        let mut player = ClipPlayer::new(clip, false);
        player.advance(100);
        player.reset();
        assert_eq!(player.current_tick(), 0);
    }

    #[test]
    fn looping_wraps_via_modulo() {
        let clip = make_clip(vec![(0, note_on(60)), (240, note_on(64))], 480);
        let player = ClipPlayer::new(clip, true);
        // tick 480 は tick 0 に相当
        let events = player.events_at(480);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tick, 0);
        // tick 720 は tick 240 に相当
        let events = player.events_at(720);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tick, 240);
    }

    #[test]
    fn non_looping_past_total_returns_empty() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let player = ClipPlayer::new(clip, false);
        let events = player.events_at(480);
        assert!(events.is_empty());
    }

    #[test]
    fn scene_player_new_empty() {
        let scene = ScenePlayer::new();
        assert_eq!(scene.clip_count(), 0);
    }

    #[test]
    fn scene_player_add_clip() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("kick".to_string(), make_clip(vec![], 480), true);
        scene.add_clip("snare".to_string(), make_clip(vec![], 960), true);
        assert_eq!(scene.clip_count(), 2);
    }

    #[test]
    fn scene_player_events_at_aggregates() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            false,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            false,
        );
        let events = scene.events_at(0);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn scene_player_all_done() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("a".to_string(), make_clip(vec![], 480), false);
        scene.add_clip("b".to_string(), make_clip(vec![], 480), true);
        assert!(!scene.all_done());
        scene.advance_all(480);
        // "a" is done, "b" is looping (ignored) → all_done = true
        assert!(scene.all_done());
    }

    #[test]
    fn polyrhythm_different_total_ticks() {
        let mut scene = ScenePlayer::new();
        // 3拍子クリップ（tick 0にイベント、total=360）
        scene.add_clip(
            "three".to_string(),
            make_clip(vec![(0, note_on(60))], 360),
            true,
        );
        // 4拍子クリップ（tick 0にイベント、total=480）
        scene.add_clip(
            "four".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );
        // tick 0: 両方ヒット
        assert_eq!(scene.events_at(0).len(), 2);
        // tick 360: threeのみ（360%360=0）、fourは360%480=360で不一致
        assert_eq!(scene.events_at(360).len(), 1);
        // tick 480: fourのみ（480%480=0）、threeは480%360=120で不一致
        assert_eq!(scene.events_at(480).len(), 1);
        // tick 720: 両方（720%360=0, 720%480=240→不一致）…threeのみ
        // 実は720%480=240なのでfourはヒットしない
        assert_eq!(scene.events_at(720).len(), 1);
        // LCM(360,480)=1440で再び同時
        assert_eq!(scene.events_at(1440).len(), 2);
    }

    #[test]
    fn scene_player_reset_all() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("a".to_string(), make_clip(vec![], 480), false);
        scene.advance_all(100);
        scene.reset_all();
        // all_doneはfalseに戻る（tick=0 < 480）
        assert!(!scene.all_done());
    }
}
