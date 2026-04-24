use crate::engine::compiler::{CompiledClip, MidiEvent};

/// 2つの u64 の最大公約数
/// Greatest common divisor of two u64 values.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// 2つの u64 の最小公倍数（0 の場合は 0 を返す）
/// Least common multiple of two u64 values (returns 0 if either is 0).
fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        0
    } else {
        a / gcd(a, b) * b
    }
}

/// 単一クリップの再生状態を管理するプレイヤー
/// Player managing playback state for a single clip
#[derive(Debug, Clone)]
pub struct ClipPlayer {
    /// 再生対象のコンパイル済みクリップ
    clip: CompiledClip,
    /// 次ループ頭で差し替える待機クリップ（§7: 動的上書き対応）
    /// Pending clip to swap in at the next loop boundary (§7: dynamic replacement)
    pending_clip: Option<CompiledClip>,
    /// 現在の再生tick位置
    current_tick: u64,
    /// ループ再生するかどうか
    looping: bool,
    /// ミュート状態（§10.3 `stop <clip>` によるclip単位ミュート対応）
    /// Mute state (for §10.3 clip-level mute via `stop <clip>`)
    muted: bool,
    /// ポーズ状態（§10.4 `pause <clip>` による clip 単位の tick 凍結対応）
    /// Pause state (for §10.4 clip-level tick freeze via `pause <clip>`)
    ///
    /// muted と独立したフラグ。pause 中は advance() で tick が進まず、
    /// events_at() は空 Vec を返す。muted と異なり位相が凍結される。
    /// Independent flag from `muted`. While paused, `advance()` keeps
    /// `current_tick` unchanged and `events_at()` returns an empty Vec.
    /// Unlike mute, the phase (position within the loop) is frozen.
    paused: bool,
}

impl ClipPlayer {
    /// 新しいClipPlayerを生成する
    pub fn new(clip: CompiledClip, looping: bool) -> Self {
        Self {
            clip,
            pending_clip: None,
            current_tick: 0,
            looping,
            muted: false,
            paused: false,
        }
    }

    /// このクリップをミュートする（`events_at` が空Vecを返すようになる）
    /// Mute this clip — `events_at` will return an empty Vec while muted.
    pub fn mute(&mut self) {
        self.muted = true;
    }

    /// ミュートを解除する
    /// Unmute this clip.
    pub fn unmute(&mut self) {
        self.muted = false;
    }

    /// ミュート中か
    /// Whether this clip is currently muted.
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// このクリップをポーズする（§10.4 `pause <clip>`）
    ///
    /// ポーズ中は `advance()` で tick が進まず位相が凍結される。
    /// `events_at()` は空 Vec を返す。muted と独立したフラグ。
    ///
    /// Pauses this clip (§10.4 `pause <clip>`). While paused, `advance()`
    /// does not advance `current_tick` and `events_at()` returns an empty
    /// Vec. Independent from the mute flag.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// ポーズを解除する
    /// Resumes this clip.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// ポーズ中か
    /// Whether this clip is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// 指定tickにあるイベントを返す
    ///
    /// ループ時はtotal_ticksでmodした実効tickで検索する。
    /// 非ループ時はtotal_ticksを超えたら空を返す。
    /// muted または paused の場合は空 Vec を返す。
    pub fn events_at(&self, tick: u64) -> Vec<&MidiEvent> {
        if self.muted || self.paused {
            return Vec::new();
        }
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

    /// tickを進める。ループ頭到達時にpending_clipがあれば差し替える。
    /// paused 状態では tick を進めない（位相凍結、§10.4）。
    ///
    /// Advance tick. If pending_clip exists and loop boundary is reached, swap it in.
    /// While paused, the tick is frozen (phase preservation, §10.4).
    pub fn advance(&mut self, ticks: u64) {
        if self.paused {
            return;
        }
        let old_tick = self.current_tick;
        self.current_tick += ticks;

        // ループ頭検出: pending_clipがあり、ループ境界をまたいだら差し替え
        // Detect loop boundary: swap pending_clip when crossing loop boundary
        if self.looping && self.pending_clip.is_some() && self.clip.total_ticks > 0 {
            let old_loop = old_tick / self.clip.total_ticks;
            let new_loop = self.current_tick / self.clip.total_ticks;
            if new_loop > old_loop {
                self.clip = self.pending_clip.take().unwrap();
                // ループ頭からの相対位置を維持
                // Maintain relative position from loop start
                self.current_tick %= self.clip.total_ticks;
            }
        }
    }

    /// 次ループ頭で差し替えるクリップをセットする（§7: 動的上書き）
    /// Set a clip to replace the current one at the next loop boundary (§7: dynamic replacement)
    pub fn replace_clip(&mut self, clip: CompiledClip) {
        self.pending_clip = Some(clip);
    }

    /// 待機中のクリップがあるかどうか
    /// Whether a pending clip is waiting for replacement
    pub fn has_pending(&self) -> bool {
        self.pending_clip.is_some()
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

    /// このクリップの total_ticks を返す
    /// Returns this clip's total_ticks.
    pub fn total_ticks(&self) -> u64 {
        self.clip.total_ticks
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

    /// 名前指定でクリップを動的差し替え（次ループ頭で切り替え）
    /// Replace a clip by name (swapped at the next loop boundary)
    pub fn replace_clip(&mut self, name: &str, clip: CompiledClip) {
        if let Some((_, player)) = self.players.iter_mut().find(|(n, _)| n == name) {
            player.replace_clip(clip);
        }
    }

    /// クリップ数
    pub fn clip_count(&self) -> usize {
        self.players.len()
    }

    /// シーン1ループ分の tick 長（内包クリップの total_ticks の LCM）
    ///
    /// ポリリズム時に全クリップが同時に頭に戻るまでの tick 数を返す。
    /// クリップが空、または total_ticks=0 のクリップがある場合は 0 を返す。
    ///
    /// Returns the tick length of one scene loop — the LCM of every contained
    /// clip's `total_ticks`. Returns 0 when the scene is empty or any contained
    /// clip has total_ticks=0.
    pub fn scene_tick_length(&self) -> u64 {
        if self.players.is_empty() {
            return 0;
        }
        let mut acc: u64 = 1;
        for (_, p) in &self.players {
            let t = p.total_ticks();
            if t == 0 {
                return 0;
            }
            acc = lcm(acc, t);
        }
        acc
    }

    /// 指定名のクリップをミュートする（未知名は no-op）
    /// Mute the clip with the given name (no-op if not found).
    pub fn mute_clip(&mut self, name: &str) {
        if let Some((_, player)) = self.players.iter_mut().find(|(n, _)| n == name) {
            player.mute();
        }
    }

    /// 指定名のクリップのミュートを解除（未知名は no-op）
    /// Unmute the clip with the given name (no-op if not found).
    pub fn unmute_clip(&mut self, name: &str) {
        if let Some((_, player)) = self.players.iter_mut().find(|(n, _)| n == name) {
            player.unmute();
        }
    }

    /// 指定名のクリップがミュート中か（未知名は false）
    /// Whether the named clip is muted (false if not found).
    pub fn is_muted(&self, name: &str) -> bool {
        self.players
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.is_muted())
            .unwrap_or(false)
    }

    /// 全クリップのミュートを解除
    /// Unmute all clips.
    pub fn unmute_all(&mut self) {
        for (_, player) in &mut self.players {
            player.unmute();
        }
    }

    /// 指定名のクリップを pause する（未知名は no-op、§10.4）
    /// Pause the clip with the given name (no-op if not found).
    pub fn pause_clip(&mut self, name: &str) {
        if let Some((_, player)) = self.players.iter_mut().find(|(n, _)| n == name) {
            player.pause();
        }
    }

    /// 指定名のクリップの pause を解除（未知名は no-op、§10.4）
    /// Resume the clip with the given name (no-op if not found).
    pub fn resume_clip(&mut self, name: &str) {
        if let Some((_, player)) = self.players.iter_mut().find(|(n, _)| n == name) {
            player.resume();
        }
    }

    /// 指定名のクリップが pause 中か（未知名は false、§10.4）
    /// Whether the named clip is paused (false if not found).
    pub fn is_clip_paused(&self, name: &str) -> bool {
        self.players
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.is_paused())
            .unwrap_or(false)
    }

    /// 全クリップを pause する（§10.4 全体 pause 用）
    /// Pause every clip (used for §10.4 global pause).
    pub fn pause_all_clips(&mut self) {
        for (_, player) in &mut self.players {
            player.pause();
        }
    }

    /// 全クリップの pause を解除する（§10.4 全体 resume 用）
    /// Resume every clip (used for §10.4 global resume).
    pub fn resume_all_clips(&mut self) {
        for (_, player) in &mut self.players {
            player.resume();
        }
    }

    /// 内包する全 clip の全イベントから使用中の (device, channel) を集める
    ///
    /// Stop 時の AllNotesOff 送信先を決定するために使う。ミュート状態は
    /// 無視して、元の clip 定義が対象とする (device, channel) を返す。
    /// Issue #49: device ごとに AllNotesOff を振り分けるため、device 名を
    /// 同時に返す API に変更。
    ///
    /// Collects every (device, MIDI channel) pair used by any event in any
    /// contained clip, ignoring mute state. Used to determine per-device
    /// AllNotesOff destinations on stop (Issue #49).
    pub fn channels_in_use(&self) -> Vec<(String, u8)> {
        let mut pairs: Vec<(String, u8)> = Vec::new();
        for (_, p) in &self.players {
            for ev in &p.clip.events {
                let pair = (ev.device.clone(), channel_of(&ev.message));
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        pairs
    }

    /// 指定名の clip が使用する (device, channel) 一覧
    ///
    /// 該当 clip が見つからない、または全イベントを持たない場合は空 Vec。
    /// Issue #49: mute <clip> で該当 device のみに AllNotesOff を飛ばす
    /// ために device 名もセットで返す。
    ///
    /// Returns the (device, channel) pairs used by the clip with the given
    /// name. Empty when the clip is not found or has no events.
    pub fn channels_of_clip(&self, name: &str) -> Vec<(String, u8)> {
        let mut pairs: Vec<(String, u8)> = Vec::new();
        if let Some((_, p)) = self.players.iter().find(|(n, _)| n == name) {
            for ev in &p.clip.events {
                let pair = (ev.device.clone(), channel_of(&ev.message));
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        pairs
    }

    /// 指定名の clip が登録されているか
    /// Whether a clip with the given name exists in this scene.
    pub fn has_clip(&self, name: &str) -> bool {
        self.players.iter().any(|(n, _)| n == name)
    }
}

/// MidiMessage からチャンネル番号を取り出す
/// System Real-Time (Start/Stop/Continue) は channel を持たず compiled clip にも
/// 含まれないため到達しない。
/// Extracts the channel number from a MidiMessage.
/// System Real-Time messages do not carry a channel and never appear in compiled clip events.
fn channel_of(msg: &crate::midi::message::MidiMessage) -> u8 {
    use crate::midi::message::MidiMessage;
    match msg {
        MidiMessage::NoteOn { channel, .. }
        | MidiMessage::NoteOff { channel, .. }
        | MidiMessage::ControlChange { channel, .. }
        | MidiMessage::ProgramChange { channel, .. } => *channel,
        MidiMessage::Start | MidiMessage::Stop | MidiMessage::Continue => {
            unreachable!("System Real-Time messages are not part of compiled clip events")
        }
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
                .map(|(tick, message)| MidiEvent::new(tick, message, ""))
                .collect(),
            total_ticks,
            warnings: vec![],
        }
    }

    fn note_on(note: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel: 0,
            note,
            velocity: 100,
        }
    }

    #[allow(dead_code)]
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
        let clip = make_clip(
            vec![(0, note_on(60)), (0, note_on(64)), (240, note_on(67))],
            480,
        );
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

    // --- 動的クリップ差し替えテスト ---

    /// replace_clip後、次のループ頭で新クリップに切り替わることを検証
    /// Verify that after replace_clip, the new clip takes effect at the next loop boundary
    #[test]
    fn clip_player_replace_at_loop_boundary() {
        let clip_a = make_clip(vec![(0, note_on(60))], 480);
        let clip_b = make_clip(vec![(0, note_on(72))], 480);
        let mut player = ClipPlayer::new(clip_a, true);

        // ループ中盤で差し替えをセット
        player.advance(240);
        player.replace_clip(clip_b);
        assert!(player.has_pending());

        // まだ切り替わっていない（tick=240, clip_aのイベント）
        let events = player.events_at(240);
        assert!(events.is_empty()); // tick 240にイベントなし

        // ループ頭を超える
        player.advance(240); // tick = 480 → ループ頭到達

        // 切り替わった後はclip_bのイベント（note=72）
        assert!(!player.has_pending());
        let events = player.events_at(player.current_tick());
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].message,
            MidiMessage::NoteOn { note: 72, .. }
        ));
    }

    /// replace_clipなしでは従来通り動作することを検証
    /// Verify that without replace_clip, behavior is unchanged
    #[test]
    fn clip_player_no_replace_normal_loop() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let mut player = ClipPlayer::new(clip, true);

        assert!(!player.has_pending());
        player.advance(480);
        // ループしてtick 0 に戻る（ただしcurrent_tickは480）
        let events = player.events_at(480);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].message,
            MidiMessage::NoteOn { note: 60, .. }
        ));
    }

    // --- scene_tick_length テスト (#37 Phase 4) ---

    /// 空 scene では 0 を返す
    #[test]
    fn scene_tick_length_empty_is_zero() {
        let scene = ScenePlayer::new();
        assert_eq!(scene.scene_tick_length(), 0);
    }

    /// 単一クリップでは そのクリップの total_ticks を返す
    #[test]
    fn scene_tick_length_single_clip() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("a".to_string(), make_clip(vec![], 480), true);
        assert_eq!(scene.scene_tick_length(), 480);
    }

    /// ポリリズム: 360 と 480 の LCM = 1440
    #[test]
    fn scene_tick_length_polyrhythm_lcm() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("three".to_string(), make_clip(vec![], 360), true);
        scene.add_clip("four".to_string(), make_clip(vec![], 480), true);
        assert_eq!(scene.scene_tick_length(), 1440);
    }

    // --- ミュートAPIテスト (#37 Phase 1) ---

    /// ClipPlayerの初期状態はミュート解除
    #[test]
    fn clip_player_not_muted_by_default() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let player = ClipPlayer::new(clip, true);
        assert!(!player.is_muted());
    }

    /// mute後はevents_atが空を返し、unmute後は再びイベントを返す
    #[test]
    fn clip_player_mute_suppresses_events() {
        let clip = make_clip(vec![(0, note_on(60)), (240, note_on(64))], 480);
        let mut player = ClipPlayer::new(clip, true);

        assert_eq!(player.events_at(0).len(), 1);

        player.mute();
        assert!(player.is_muted());
        assert!(player.events_at(0).is_empty());
        assert!(player.events_at(240).is_empty());

        player.unmute();
        assert!(!player.is_muted());
        assert_eq!(player.events_at(0).len(), 1);
        assert_eq!(player.events_at(240).len(), 1);
    }

    /// ミュート中もtickは進む（unmute後に現在位置から再開）
    #[test]
    fn clip_player_mute_does_not_stop_tick_advance() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let mut player = ClipPlayer::new(clip, true);
        player.mute();
        player.advance(240);
        assert_eq!(player.current_tick(), 240);
    }

    /// ScenePlayer::mute_clip で該当クリップのみミュート、他は影響なし
    #[test]
    fn scene_player_mute_clip_targets_single_clip() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );

        assert_eq!(scene.events_at(0).len(), 2);

        scene.mute_clip("a");
        assert!(scene.is_muted("a"));
        assert!(!scene.is_muted("b"));

        let events = scene.events_at(0);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].message,
            MidiMessage::NoteOn { note: 72, .. }
        ));

        scene.unmute_clip("a");
        assert!(!scene.is_muted("a"));
        assert_eq!(scene.events_at(0).len(), 2);
    }

    /// 存在しないクリップ名への操作は no-op
    #[test]
    fn scene_player_mute_unknown_clip_is_noop() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("a".to_string(), make_clip(vec![], 480), true);
        scene.mute_clip("unknown");
        assert!(!scene.is_muted("unknown"));
        assert!(!scene.is_muted("a"));
    }

    /// unmute_all は全クリップのミュートを解除する
    #[test]
    fn scene_player_unmute_all_clears_all_mutes() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );

        scene.mute_clip("a");
        scene.mute_clip("b");
        assert!(scene.is_muted("a") && scene.is_muted("b"));

        scene.unmute_all();
        assert!(!scene.is_muted("a") && !scene.is_muted("b"));
        assert_eq!(scene.events_at(0).len(), 2);
    }

    /// ScenePlayer経由での動的クリップ差し替え
    /// Dynamic clip replacement via ScenePlayer
    #[test]
    fn scene_player_replace_clip() {
        let mut scene = ScenePlayer::new();
        let clip_a = make_clip(vec![(0, note_on(60))], 480);
        let clip_b = make_clip(vec![(0, note_on(72))], 480);

        scene.add_clip("bass".to_string(), clip_a, true);

        // tick=240で差し替え予約
        scene.advance_all(240);
        scene.replace_clip("bass", clip_b);

        // ループ頭を超える
        scene.advance_all(240);

        // 切り替わっている
        let events = scene.events_at(0);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].message,
            MidiMessage::NoteOn { note: 72, .. }
        ));
    }

    // --- ポーズAPIテスト (#44 Phase 1) ---

    /// ClipPlayer の初期状態はポーズ解除
    /// ClipPlayer is not paused by default.
    #[test]
    fn clip_player_not_paused_by_default() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let player = ClipPlayer::new(clip, true);
        assert!(!player.is_paused());
    }

    /// pause 後は events_at が空を返し、resume 後は再びイベントを返す
    /// After pause, events_at returns empty; after resume, events come back.
    #[test]
    fn clip_player_pause_suppresses_events() {
        let clip = make_clip(vec![(0, note_on(60)), (240, note_on(64))], 480);
        let mut player = ClipPlayer::new(clip, true);

        assert_eq!(player.events_at(0).len(), 1);

        player.pause();
        assert!(player.is_paused());
        assert!(player.events_at(0).is_empty());
        assert!(player.events_at(240).is_empty());

        player.resume();
        assert!(!player.is_paused());
        assert_eq!(player.events_at(0).len(), 1);
        assert_eq!(player.events_at(240).len(), 1);
    }

    /// pause 中は tick が進まない（位相凍結）
    /// Tick does not advance while paused (phase frozen).
    #[test]
    fn clip_player_pause_freezes_tick() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let mut player = ClipPlayer::new(clip, true);

        player.advance(120);
        assert_eq!(player.current_tick(), 120);

        player.pause();
        player.advance(240);
        // pause 中は tick が進まない
        // Tick is frozen while paused
        assert_eq!(player.current_tick(), 120);

        player.resume();
        player.advance(60);
        assert_eq!(player.current_tick(), 180);
    }

    /// paused と muted は独立したフラグ
    /// paused and muted are independent flags.
    #[test]
    fn clip_player_paused_and_muted_are_independent() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let mut player = ClipPlayer::new(clip, true);

        player.mute();
        assert!(player.is_muted());
        assert!(!player.is_paused());

        player.pause();
        assert!(player.is_muted());
        assert!(player.is_paused());

        player.unmute();
        assert!(!player.is_muted());
        assert!(player.is_paused());
        // muted は解除されたが paused なので events は空
        // muted is cleared but paused keeps events empty
        assert!(player.events_at(0).is_empty());

        player.resume();
        assert!(!player.is_paused());
        assert_eq!(player.events_at(0).len(), 1);
    }

    /// ScenePlayer::pause_clip で該当クリップのみ pause、他は影響なし
    /// ScenePlayer::pause_clip pauses only the targeted clip.
    #[test]
    fn scene_player_pause_clip_targets_single_clip() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );

        assert_eq!(scene.events_at(0).len(), 2);

        scene.pause_clip("a");
        assert!(scene.is_clip_paused("a"));
        assert!(!scene.is_clip_paused("b"));

        let events = scene.events_at(0);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].message,
            MidiMessage::NoteOn { note: 72, .. }
        ));

        scene.resume_clip("a");
        assert!(!scene.is_clip_paused("a"));
        assert_eq!(scene.events_at(0).len(), 2);
    }

    /// 存在しないクリップ名への pause/resume は no-op
    /// pause/resume on an unknown clip name is a no-op.
    #[test]
    fn scene_player_pause_unknown_clip_is_noop() {
        let mut scene = ScenePlayer::new();
        scene.add_clip("a".to_string(), make_clip(vec![], 480), true);
        scene.pause_clip("unknown");
        assert!(!scene.is_clip_paused("unknown"));
        assert!(!scene.is_clip_paused("a"));
    }

    /// resume_all_clips は全クリップのポーズを解除する
    /// resume_all_clips clears paused state for every clip.
    #[test]
    fn scene_player_resume_all_clips_clears_all_pauses() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );

        scene.pause_clip("a");
        scene.pause_clip("b");
        assert!(scene.is_clip_paused("a") && scene.is_clip_paused("b"));

        scene.resume_all_clips();
        assert!(!scene.is_clip_paused("a") && !scene.is_clip_paused("b"));
        assert_eq!(scene.events_at(0).len(), 2);
    }

    /// pause_all_clips は全クリップを pause する
    /// pause_all_clips pauses every clip in the scene.
    #[test]
    fn scene_player_pause_all_clips_pauses_all() {
        let mut scene = ScenePlayer::new();
        scene.add_clip(
            "a".to_string(),
            make_clip(vec![(0, note_on(60))], 480),
            true,
        );
        scene.add_clip(
            "b".to_string(),
            make_clip(vec![(0, note_on(72))], 480),
            true,
        );

        scene.pause_all_clips();
        assert!(scene.is_clip_paused("a") && scene.is_clip_paused("b"));
        assert!(scene.events_at(0).is_empty());

        // advance_all を呼んでも位相は進まない
        // advance_all does not advance phase while paused
        scene.advance_all(240);
        scene.resume_all_clips();
        // resume 後は tick 0 のままイベントが取れる
        // After resume, events at tick 0 still apply
        assert_eq!(scene.events_at(0).len(), 2);
    }
}
