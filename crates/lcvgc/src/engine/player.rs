use crate::engine::compiler::{CompiledClip, MidiEvent};
use crate::midi::probability::should_trigger;
use rand::Rng;
use std::collections::{BTreeMap, HashSet};

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
    /// 当該ループ周期で確率抽選 / random-choice 抽選により mute されている events index 集合。
    /// ループ境界をまたぐ毎に再抽選で更新される。
    ///
    /// Set of event indices that lost the probability roll (drum)
    /// or were not selected by random-choice arpeggio for the current loop
    /// iteration. Refreshed every loop boundary so each loop yields a
    /// fresh variation.
    masked_events: HashSet<usize>,
    /// `tick → そのtickで発火する event の clip.events 内 index 列` の索引。
    /// `events_at` を線形走査 (O(N)) から O(log N + k) に下げるための前計算結果。
    /// clip swap 時 (`replace_clip` → `advance` 内で take) に `rebuild_events_index`
    /// で再構築する。mask の有無は反映しない（mask は `events_at` 側で filter する）。
    ///
    /// Precomputed `tick → indices into clip.events` index that lets
    /// `events_at` finish in O(log N + k) instead of O(N). Rebuilt whenever
    /// the underlying clip is swapped (`replace_clip` followed by `advance`).
    /// The mask state is intentionally not folded in — masks are applied at
    /// query time inside `events_at`.
    events_by_tick: BTreeMap<u64, Vec<usize>>,
}

impl ClipPlayer {
    /// 新しいClipPlayerを生成する
    ///
    /// 生成時に最初のループ周期分の確率抽選も同時に実行する。
    /// ドラム発音率行が無い場合は抽選自体が空となり、副作用は無い。
    ///
    /// Creates a new ClipPlayer and immediately rolls the drum probability
    /// mask for the first loop iteration. Clips without probability rows
    /// roll an empty mask, which is a no-op.
    pub fn new(clip: CompiledClip, looping: bool) -> Self {
        let events_by_tick = build_events_by_tick(&clip);
        let mut player = Self {
            clip,
            pending_clip: None,
            current_tick: 0,
            looping,
            muted: false,
            paused: false,
            masked_events: HashSet::new(),
            events_by_tick,
        };
        player.reroll_masks(&mut rand::thread_rng());
        player
    }

    /// テスト用: 任意の RNG を渡して ClipPlayer を生成する
    /// Test helper: build a ClipPlayer with a caller-supplied RNG.
    #[cfg(test)]
    pub fn new_with_rng<R: Rng>(clip: CompiledClip, looping: bool, rng: &mut R) -> Self {
        let events_by_tick = build_events_by_tick(&clip);
        let mut player = Self {
            clip,
            pending_clip: None,
            current_tick: 0,
            looping,
            muted: false,
            paused: false,
            masked_events: HashSet::new(),
            events_by_tick,
        };
        player.reroll_masks(rng);
        player
    }

    /// 現在の確率抽選マスク（テストおよび内省用）
    /// Snapshot of currently masked event indices (for tests / introspection).
    pub fn masked_event_indices(&self) -> &HashSet<usize> {
        &self.masked_events
    }

    /// ドラム発音率行と random-choice 抽選を再実行し、両者の結果を `masked_events` に
    /// 反映する。
    ///
    /// - `drum_mask_groups`: 各 group につき `should_trigger` を 1 回引き、外れた
    ///   group の event indices を全て mask する。
    /// - `random_choice_groups`: 各 group の候補から 1 つを選び、それ以外の候補に
    ///   属する index を全て mask する。
    ///
    /// どちらも空ならば HashSet 1 つの clear のみで終了する。
    ///
    /// Rerolls drum probability masks and random-choice arpeggio selection,
    /// merging both results into `masked_events`. For each drum group rolls
    /// once and masks losing groups; for each random-choice group picks one
    /// candidate and masks all others.
    pub fn reroll_masks<R: Rng>(&mut self, rng: &mut R) {
        self.masked_events.clear();
        for group in &self.clip.drum_mask_groups {
            if !should_trigger(Some(group.probability), rng) {
                for idx in &group.event_indices {
                    self.masked_events.insert(*idx);
                }
            }
        }
        for group in &self.clip.random_choice_groups {
            if group.candidates.is_empty() {
                continue;
            }
            let chosen_idx = rng.gen_range(0..group.candidates.len());
            for (i, cand) in group.candidates.iter().enumerate() {
                if i == chosen_idx {
                    continue;
                }
                for idx in cand {
                    self.masked_events.insert(*idx);
                }
            }
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
    /// 確率抽選で当該ループに mask されている event index は除外する。
    ///
    /// Returns events at the given tick. Skips events whose index lost the
    /// probability roll for the current loop iteration.
    pub fn events_at(&self, tick: u64) -> Vec<&MidiEvent> {
        if self.muted || self.paused {
            return Vec::new();
        }
        if !self.looping && tick >= self.clip.total_ticks {
            return Vec::new();
        }
        let effective = self.effective_tick(tick);
        // 索引 (`events_by_tick`) を引いて該当 tick の event index 列だけを舐める。
        // mask は元の実装と同様に query 時に適用する。
        // Look up the per-tick index and walk only the matching event indices;
        // masking is applied at query time, matching the original semantics.
        match self.events_by_tick.get(&effective) {
            Some(indices) => indices
                .iter()
                .filter(|idx| !self.masked_events.contains(idx))
                .map(|idx| &self.clip.events[*idx])
                .collect(),
            None => Vec::new(),
        }
    }

    /// 現在の再生tick位置を取得
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// tickを進める。ループ頭到達時にpending_clipがあれば差し替える。
    /// paused 状態では tick を進めない（位相凍結、§10.4）。
    ///
    /// ループ境界をまたいだ場合、ドラム発音率行に基づく確率抽選を再実行し
    /// 次ループ周期の mask を更新する。`pending_clip` が swap された場合も、
    /// swap 後の clip の確率行に対して即時抽選する。
    ///
    /// Advance tick. If `pending_clip` exists and the loop boundary is
    /// crossed, swap it in. Whenever the loop boundary is crossed (with or
    /// without a pending swap), reroll the drum probability mask so each
    /// loop produces a fresh variation.
    pub fn advance(&mut self, ticks: u64) {
        if self.paused {
            return;
        }
        let old_tick = self.current_tick;
        self.current_tick += ticks;

        if self.looping && self.clip.total_ticks > 0 {
            let old_loop = old_tick / self.clip.total_ticks;
            let new_loop = self.current_tick / self.clip.total_ticks;
            if new_loop > old_loop {
                if self.pending_clip.is_some() {
                    self.clip = self.pending_clip.take().unwrap();
                    // clip を差し替えたので、events_at が引く tick→indices 索引も
                    // 新 clip のもので再構築する。
                    // Rebuild the tick→indices map for the swapped-in clip.
                    self.events_by_tick = build_events_by_tick(&self.clip);
                    // ループ頭からの相対位置を維持
                    // Maintain relative position from loop start
                    self.current_tick %= self.clip.total_ticks;
                }
                // 新しいループ周期に入ったので確率抽選を再実行する
                // Entered a new loop iteration → reroll the probability mask
                self.reroll_masks(&mut rand::thread_rng());
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

/// `CompiledClip.events` を走査して `tick → events 内 index 列` の索引を構築する。
///
/// `ClipPlayer::events_at` の毎 tick 線形走査を避けるための前計算。並び順は元の
/// `events` 配列順を保つ（既存呼び出し側が依存する送出順を維持するため）。
/// 同じ tick に複数 event がある場合は `Vec<usize>` で保持する。
///
/// Builds the `tick → indices into clip.events` map used by `events_at` to
/// avoid a per-tick linear scan. Preserves the original `events` ordering so
/// existing call sites observe the same dispatch order as before.
fn build_events_by_tick(clip: &CompiledClip) -> BTreeMap<u64, Vec<usize>> {
    let mut index: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (i, e) in clip.events.iter().enumerate() {
        index.entry(e.tick).or_default().push(i);
    }
    index
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
    pub fn channels_in_use(&self) -> Vec<(String, crate::midi::channel::MidiChannel)> {
        let mut pairs: Vec<(String, crate::midi::channel::MidiChannel)> = Vec::new();
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
    pub fn channels_of_clip(&self, name: &str) -> Vec<(String, crate::midi::channel::MidiChannel)> {
        let mut pairs: Vec<(String, crate::midi::channel::MidiChannel)> = Vec::new();
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
fn channel_of(msg: &crate::midi::message::MidiMessage) -> crate::midi::channel::MidiChannel {
    use crate::midi::message::MidiMessage;
    match msg {
        MidiMessage::NoteOn { channel, .. }
        | MidiMessage::NoteOff { channel, .. }
        | MidiMessage::ControlChange { channel, .. }
        | MidiMessage::ProgramChange { channel, .. } => *channel,
        MidiMessage::Start | MidiMessage::Stop | MidiMessage::Continue | MidiMessage::Clock => {
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
            drum_mask_groups: vec![],
            random_choice_groups: vec![],
        }
    }

    fn note_on(note: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel: crate::midi::channel::MidiChannel::from_zero_based(0).unwrap(),
            note,
            velocity: 100,
        }
    }

    #[allow(dead_code)]
    fn note_off(note: u8) -> MidiMessage {
        MidiMessage::NoteOff {
            channel: crate::midi::channel::MidiChannel::from_zero_based(0).unwrap(),
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

    /// `build_events_by_tick` が tick 0 の連続 event を元 Vec 順で並べることを検証。
    /// `events_at` の dispatch 順互換を担保するための直接テスト。
    #[test]
    fn build_events_by_tick_preserves_insertion_order_for_same_tick() {
        let clip = make_clip(
            vec![(0, note_on(60)), (0, note_on(64)), (0, note_on(67))],
            480,
        );
        let index = build_events_by_tick(&clip);
        let at_zero = index.get(&0).expect("tick 0 should exist");
        // 索引内の event index は 0, 1, 2 の順で詰まっている必要がある
        assert_eq!(at_zero, &vec![0, 1, 2]);
        // 結果として events_at(0) も note 60, 64, 67 の順
        let player = ClipPlayer::new(clip, false);
        let events = player.events_at(0);
        let notes: Vec<u8> = events
            .iter()
            .filter_map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => Some(note),
                _ => None,
            })
            .collect();
        assert_eq!(notes, vec![60, 64, 67]);
    }

    /// 索引が引かれない tick (= event 0 件の空 tick) では `events_at` が空 Vec を返し、
    /// かつ BTreeMap への lookup が一度で済む (＝旧来の全走査が走らない) ことを
    /// 間接的に担保するテスト。
    #[test]
    fn events_at_empty_tick_returns_empty() {
        let clip = make_clip(vec![(0, note_on(60)), (240, note_on(64))], 480);
        let player = ClipPlayer::new(clip, true);
        assert!(player.events_at(1).is_empty());
        assert!(player.events_at(239).is_empty());
        assert!(player.events_at(479).is_empty());
    }

    /// `replace_clip` で swap 後、`events_by_tick` 索引も新 clip 由来に
    /// 切り替わっていることを `events_at` 経由で確認する (旧 clip の event
    /// (note=60) が新 clip swap 後の tick 0 で引かれないこと)。
    #[test]
    fn replace_clip_rebuilds_events_index() {
        // 旧: tick 0 に note 60, 新: tick 0 に空, tick 120 に note 72
        let clip_a = make_clip(vec![(0, note_on(60))], 480);
        let clip_b = make_clip(vec![(120, note_on(72))], 480);
        let mut player = ClipPlayer::new(clip_a, true);

        player.replace_clip(clip_b);
        player.advance(480); // ループ境界越え → swap 発火

        // 新 clip の tick 0 には event が無い (= 旧 clip の note=60 が
        // 索引に残っているなら誤って返ってしまう)
        assert!(player.events_at(player.current_tick()).is_empty());

        // 120 tick 進めれば新 clip の note=72 が出てくる
        player.advance(120);
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

    // --- ドラム発音率 (probability) テスト ---

    use crate::engine::compiler::DrumProbabilityGroup;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    /// テスト用: probability group 付きの CompiledClip を作る
    /// Test helper: build a CompiledClip with drum probability groups attached.
    fn make_clip_with_groups(
        events: Vec<(u64, MidiMessage)>,
        total_ticks: u64,
        groups: Vec<DrumProbabilityGroup>,
    ) -> CompiledClip {
        CompiledClip {
            events: events
                .into_iter()
                .map(|(tick, message)| MidiEvent::new(tick, message, ""))
                .collect(),
            total_ticks,
            warnings: vec![],
            drum_mask_groups: groups,
            random_choice_groups: vec![],
        }
    }

    /// probability 0 の group は最初のループから常に mask される
    /// A group with probability=0 must be masked from the very first loop.
    #[test]
    fn drum_mask_zero_probability_always_muted() {
        let clip = make_clip_with_groups(
            vec![(0, note_on(60)), (60, note_off(60))],
            480,
            vec![DrumProbabilityGroup {
                event_indices: vec![0, 1],
                probability: 0,
            }],
        );
        let mut rng = StdRng::seed_from_u64(1);
        let player = ClipPlayer::new_with_rng(clip, true, &mut rng);
        assert!(player.events_at(0).is_empty());
        assert!(player.events_at(60).is_empty());
    }

    /// probability 100 を group に含めるのは仕様上は無いが、保険として常に発音
    /// Even if a 100% group sneaks in, it must always trigger.
    #[test]
    fn drum_mask_full_probability_always_triggers() {
        let clip = make_clip_with_groups(
            vec![(0, note_on(60))],
            480,
            vec![DrumProbabilityGroup {
                event_indices: vec![0],
                probability: 100,
            }],
        );
        let mut rng = StdRng::seed_from_u64(1);
        let player = ClipPlayer::new_with_rng(clip, true, &mut rng);
        assert_eq!(player.events_at(0).len(), 1);
    }

    /// ループ境界をまたぐ毎に mask が再抽選されることを検証
    /// Mask must be rerolled every time the player crosses a loop boundary.
    #[test]
    fn drum_mask_reroll_changes_per_loop() {
        // probability=50 の group を 64 個並べ、二回のループで mask 集合が変化する
        // 確率は天文学的に低い (2^-64)。複数 group ならば rerolled の証拠となる。
        // 64 groups at 50% — observing the *same* mask twice in a row is
        // 2^-64. So if the two loops produce different masks, we know the
        // reroll path actually fired.
        let mut events = Vec::new();
        let mut groups = Vec::new();
        for i in 0..64u64 {
            let tick = i * 4;
            events.push((tick, note_on(60)));
            groups.push(DrumProbabilityGroup {
                event_indices: vec![i as usize],
                probability: 50,
            });
        }
        let clip = make_clip_with_groups(events, 480, groups);
        let mut rng = StdRng::seed_from_u64(7);
        let mut player = ClipPlayer::new_with_rng(clip, true, &mut rng);

        let first_mask = player.masked_event_indices().clone();
        // ループ境界を跨ぐ
        player.advance(480);
        let second_mask = player.masked_event_indices().clone();

        assert_ne!(
            first_mask, second_mask,
            "mask must be rerolled at loop boundary"
        );
    }

    /// drum_mask_groups が空ならループ越境しても masked_events は常に空
    /// With no probability groups, masked_events stays empty across loops.
    #[test]
    fn drum_mask_no_groups_no_op() {
        let clip = make_clip(vec![(0, note_on(60))], 480);
        let mut rng = StdRng::seed_from_u64(7);
        let mut player = ClipPlayer::new_with_rng(clip, true, &mut rng);
        assert!(player.masked_event_indices().is_empty());
        player.advance(480);
        assert!(player.masked_event_indices().is_empty());
    }

    // --- random_choice_groups テスト ---

    use crate::engine::compiler::RandomChoiceGroup;

    /// random_choice_groups 付き clip を生成するテストヘルパー
    fn make_clip_with_random_groups(
        events: Vec<(u64, MidiMessage)>,
        total_ticks: u64,
        random_choice_groups: Vec<RandomChoiceGroup>,
    ) -> CompiledClip {
        CompiledClip {
            events: events
                .into_iter()
                .map(|(tick, message)| MidiEvent::new(tick, message, ""))
                .collect(),
            total_ticks,
            warnings: vec![],
            drum_mask_groups: vec![],
            random_choice_groups,
        }
    }

    /// 候補2件の random_choice_group では、毎ループ必ず1候補だけが残り、
    /// 残りは masked_events に積まれる。
    /// In a 2-candidate group, exactly one candidate survives per loop and
    /// the other is masked.
    #[test]
    fn random_choice_keeps_exactly_one_candidate_per_loop() {
        // 同一 tick=0 に 2 候補 (NoteOn(60), NoteOn(64)) を重ね、片方だけ生き残ることを確認
        let clip = make_clip_with_random_groups(
            vec![(0, note_on(60)), (0, note_on(64))],
            480,
            vec![RandomChoiceGroup {
                candidates: vec![vec![0], vec![1]],
            }],
        );
        let mut rng = StdRng::seed_from_u64(1);
        let player = ClipPlayer::new_with_rng(clip, true, &mut rng);

        // tick=0 で events_at が返すのはちょうど 1 件
        let events = player.events_at(0);
        assert_eq!(events.len(), 1, "ちょうど 1 候補だけ生き残るべき");
        // その他は masked
        assert_eq!(player.masked_event_indices().len(), 1);
    }

    /// ループ境界をまたぐ毎に random-choice の選択が（少なくとも）変わり得ること。
    /// 多数の独立 group で、二回連続で同じ選択になる確率が天文学的に低い構成を組み、
    /// 異なるマスクが得られることを示す。
    /// Across a loop boundary, random selections must reroll. Build many
    /// independent groups so two consecutive identical selections are
    /// astronomically unlikely.
    #[test]
    fn random_choice_reroll_changes_per_loop() {
        // 64 group × 各 2 候補 → 同じ選択集合を 2 回連続で引く確率は 2^-64
        let mut events = Vec::new();
        let mut groups: Vec<RandomChoiceGroup> = Vec::new();
        for i in 0..64u64 {
            let tick = i * 4;
            let on_idx_a = events.len();
            events.push((tick, note_on(60)));
            let on_idx_b = events.len();
            events.push((tick, note_on(72)));
            groups.push(RandomChoiceGroup {
                candidates: vec![vec![on_idx_a], vec![on_idx_b]],
            });
        }
        let clip = make_clip_with_random_groups(events, 480, groups);
        let mut rng = StdRng::seed_from_u64(7);
        let mut player = ClipPlayer::new_with_rng(clip, true, &mut rng);

        let first_mask = player.masked_event_indices().clone();
        player.advance(480);
        let second_mask = player.masked_event_indices().clone();

        assert_ne!(
            first_mask, second_mask,
            "random_choice mask must be rerolled at loop boundary"
        );
    }

    /// random_choice_groups と drum_mask_groups は両立し、両方の mask が合算される。
    /// random_choice_groups and drum_mask_groups coexist; masks merge.
    #[test]
    fn random_choice_and_drum_mask_coexist() {
        // event 0: drum prob=0 (確実に mask)
        // event 1, 2: random choice 候補 2 件 (どちらか 1 つだけ残る)
        let clip = CompiledClip {
            events: vec![
                MidiEvent::new(0, note_on(36), ""),
                MidiEvent::new(60, note_on(60), ""),
                MidiEvent::new(60, note_on(64), ""),
            ],
            total_ticks: 480,
            warnings: vec![],
            drum_mask_groups: vec![DrumProbabilityGroup {
                event_indices: vec![0],
                probability: 0,
            }],
            random_choice_groups: vec![RandomChoiceGroup {
                candidates: vec![vec![1], vec![2]],
            }],
        };
        let mut rng = StdRng::seed_from_u64(1);
        let player = ClipPlayer::new_with_rng(clip, true, &mut rng);

        // event 0 は必ず mask、random は 1 件だけ残るので mask は計 2 件
        assert_eq!(player.masked_event_indices().len(), 2);
        assert!(player.masked_event_indices().contains(&0));
    }
}
