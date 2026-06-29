use crate::domain::chord::ChordSuffix;
use crate::domain::pitch::NoteName;
use crate::parser::clip_arpeggio::Arpeggio;

/// ノートイベント（単音、コード名、休符）
/// Note event (single note, chord name, or rest)
#[derive(Debug, Clone, PartialEq)]
pub enum NoteEvent {
    /// 単音ノート
    /// Single note
    Single {
        /// 音名
        /// Note name
        name: NoteName,
        /// オクターブ（オプション）
        /// Octave (optional)
        octave: Option<u8>,
        /// 音価（ティック数、オプション）
        /// Duration in ticks (optional)
        duration: Option<u16>,
        /// 付点の有無
        /// Whether the note is dotted
        dotted: bool,
    },
    /// コード名による指定
    /// Chord specified by name
    ChordName {
        /// ルート音名
        /// Root note name
        root: NoteName,
        /// コードサフィックス
        /// Chord suffix
        suffix: ChordSuffix,
        /// オクターブ（オプション）
        /// Octave (optional)
        octave: Option<u8>,
        /// 音価（ティック数、オプション）
        /// Duration in ticks (optional)
        duration: Option<u16>,
        /// 付点の有無
        /// Whether the note is dotted
        dotted: bool,
        /// アルペジオ指定（オプション）。`Some` のときコード構成音をシーケンスとして発音する。
        /// Optional arpeggio specification. When `Some`, chord tones are sequenced one at a time.
        arpeggio: Option<Arpeggio>,
    },
    /// 休符
    /// Rest
    Rest {
        /// 音価（ティック数、オプション）
        /// Duration in ticks (optional)
        duration: Option<u16>,
        /// 付点の有無
        /// Whether the note is dotted
        dotted: bool,
    },
}
