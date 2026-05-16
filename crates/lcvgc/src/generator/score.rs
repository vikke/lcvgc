//! ジェネレーターの共通中間表現 (Score IR)。
//!
//! reader (SMF / MDX / ...) はバイナリを `Score` に変換し、emitter は `Score`
//! のみを入力として lcvgc DSL 文字列を生成する。フォーマット固有の事情
//! (loop コマンド、Mercury チャンネル等) は reader 内に閉じる。
//!
//! Common intermediate representation. Readers convert binaries into `Score`;
//! the emitter consumes only `Score`. Format-specific concerns are isolated
//! inside each reader.

/// 入力フォーマット全体を表す Score。
///
/// `ppq` (pulses per quarter note) を基準時間とし、すべての `tick` 値はこの
/// PPQ に揃えた整数として保持する。reader は自身の時間粒度をここで PPQ に
/// 揃え、emitter は PPQ から lcvgc Duration への量子化を行う。
///
/// `ppq` is the time base shared by every tick value in the score.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    /// 1 四分音符あたりの tick 数 (PPQ)
    /// Pulses per quarter note
    pub ppq: u32,
    /// 楽曲の初期テンポ (BPM)。途中変更は emitter ではコメントとして処理する。
    /// Initial tempo in BPM
    pub initial_bpm: f32,
    /// 拍子 (分子, 分母)。Default 4/4
    /// Time signature (numerator, denominator)
    pub time_signature: TimeSignature,
    /// 楽曲タイトル（reader が抽出できた場合）
    /// Song title if the reader extracted it
    pub title: Option<String>,
    /// パート (Track) の一覧
    /// Tracks
    pub tracks: Vec<Track>,
}

impl Default for Score {
    fn default() -> Self {
        Score {
            ppq: 480,
            initial_bpm: 120.0,
            time_signature: TimeSignature::default(),
            title: None,
            tracks: Vec::new(),
        }
    }
}

/// 拍子。
/// Time signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    /// 分子 (1 小節中の拍数)
    /// Numerator
    pub numerator: u8,
    /// 分母 (1 拍の音価。2=半音符, 4=四分, 8=八分, 16=十六分)
    /// Denominator
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        TimeSignature {
            numerator: 4,
            denominator: 4,
        }
    }
}

/// パート (Track)。
///
/// `kind` が `Drum` なら kit / drum 記法、`Melodic` なら音程楽器記法で
/// emitter が出力する。
///
/// A musical part. Kind selects between melodic and drum emit modes.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// 識別用の論理名 (例: `fm_a`, `ch_1`)。lcvgc instrument 名にも使われる。
    /// Logical name (used as the lcvgc instrument name)
    pub name: String,
    /// MIDI チャンネル (1-16)。MDX FM ch も MIDI ch に正規化したものを入れる。
    /// MIDI channel (1-16)
    pub midi_channel: u8,
    /// パート種別
    /// Part kind
    pub kind: TrackKind,
    /// イベント列。`start_tick` 昇順でなくても良いが、emitter で整列される。
    /// Event list (sorted by emitter)
    pub events: Vec<Event>,
}

/// パートの種類。
/// Track kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    /// 音程楽器 (instrument + clip)
    /// Pitched instrument
    Melodic,
    /// ドラム (kit + drum step 記法)
    /// Drum kit
    Drum,
}

/// パート内のイベント。
///
/// `LoopBlock` は MDX のループ区間を表す。emitter は内部イベントを `(...)*N`
/// として展開する。
///
/// A musical event inside a track. `LoopBlock` represents a repeated section.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// ノート (Note On 〜 Note Off の区間)
    /// A note with on/off ticks
    Note {
        /// Note On の tick
        /// On tick
        start_tick: u64,
        /// Note Off の tick (= start_tick + duration_ticks)
        /// Off tick
        end_tick: u64,
        /// MIDI ノート番号 (0-127)
        /// MIDI note number
        midi_note: u8,
        /// ベロシティ (1-127)
        /// Velocity
        velocity: u8,
    },
    /// ループブロック (MDX 由来)。emitter で `(...)*count` 展開する。
    ///
    /// 内部イベントの tick は **絶対 tick** で保持する (展開後の position は
    /// emitter が再計算する)。
    ///
    /// Loop block originating from MDX. Inner events use absolute ticks.
    LoopBlock {
        /// ループ開始 tick (内部 tick 基準でもブロック先頭)
        /// Loop start tick
        start_tick: u64,
        /// ループ 1 回分のイベント
        /// Events of a single iteration
        events: Vec<Event>,
        /// 繰り返し回数 (MDX → 仕様で 2 固定)
        /// Repeat count
        count: u32,
    },
}

impl Event {
    /// イベントの開始 tick を返す。
    /// Start tick of this event.
    pub fn start_tick(&self) -> u64 {
        match self {
            Event::Note { start_tick, .. } => *start_tick,
            Event::LoopBlock { start_tick, .. } => *start_tick,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_score_is_4_4_at_120_bpm_with_480_ppq() {
        let score = Score::default();
        assert_eq!(score.ppq, 480);
        assert!((score.initial_bpm - 120.0).abs() < f32::EPSILON);
        assert_eq!(score.time_signature.numerator, 4);
        assert_eq!(score.time_signature.denominator, 4);
        assert!(score.tracks.is_empty());
        assert!(score.title.is_none());
    }

    #[test]
    fn event_start_tick_handles_both_variants() {
        let note = Event::Note {
            start_tick: 240,
            end_tick: 480,
            midi_note: 60,
            velocity: 100,
        };
        assert_eq!(note.start_tick(), 240);

        let loop_block = Event::LoopBlock {
            start_tick: 1920,
            events: vec![note.clone()],
            count: 2,
        };
        assert_eq!(loop_block.start_tick(), 1920);
    }

    #[test]
    fn track_holds_events_and_kind() {
        let track = Track {
            name: "ch_1".to_string(),
            midi_channel: 1,
            kind: TrackKind::Melodic,
            events: vec![Event::Note {
                start_tick: 0,
                end_tick: 480,
                midi_note: 60,
                velocity: 100,
            }],
        };
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.kind, TrackKind::Melodic);
    }
}
