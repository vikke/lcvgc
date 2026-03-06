use crate::ast::common::NoteName;

/// コードサフィックス（和音の種類）
/// Chord suffix (chord quality)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordSuffix {
    /// メジャー
    /// Major
    Maj,
    /// メジャーセブンス
    /// Major seventh
    Maj7,
    /// マイナー
    /// Minor
    Min,
    /// マイナーセブンス
    /// Minor seventh
    Min7,
    /// ドミナントセブンス
    /// Dominant seventh
    Dom7,
    /// ディミニッシュ
    /// Diminished
    Dim,
    /// ディミニッシュセブンス
    /// Diminished seventh
    Dim7,
    /// オーギュメント
    /// Augmented
    Aug,
    /// マイナーセブンフラットファイブ（ハーフディミニッシュ）
    /// Minor seventh flat five (half-diminished)
    Min7b5,
    /// マイナーメジャーセブンス
    /// Minor-major seventh
    MinMaj7,
    /// サスフォー
    /// Suspended fourth
    Sus4,
    /// サスツー
    /// Suspended second
    Sus2,
    /// シックスス
    /// Sixth
    Sixth,
    /// マイナーシックスス
    /// Minor sixth
    Min6,
    /// ナインス
    /// Ninth
    Ninth,
    /// マイナーナインス
    /// Minor ninth
    Min9,
    /// アドナインス
    /// Add ninth
    Add9,
    /// サーティーンス
    /// Thirteenth
    Thirteenth,
    /// マイナーサーティーンス
    /// Minor thirteenth
    Min13,
}

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
