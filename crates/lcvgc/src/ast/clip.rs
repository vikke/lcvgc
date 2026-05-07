use crate::ast::clip_cc::CcAutomation;
use crate::ast::clip_drum::DrumRow;
use crate::ast::clip_note::NoteEvent;
use crate::ast::common::NoteName;
use crate::parser::clip_arpeggio::Arpeggio;
use crate::parser::clip_articulation::Articulation;
use crate::parser::clip_bar_jump::BarJump;
use crate::parser::clip_options::ClipOptions;
use crate::parser::clip_repetition::Repetition;

/// 音程付きインストゥルメントラインの単一要素
/// A single element in a pitched instrument line.
#[derive(Debug, Clone, PartialEq)]
pub enum PitchedElement {
    /// 単音ノートイベントとアーティキュレーション
    /// A single note event with articulation
    Note(NoteEvent, Articulation),
    /// コードブラケット（複数音の同時発音）
    /// A chord bracket (simultaneous sounding of multiple notes)
    ChordBracket {
        /// コード構成音のリスト（音名とオプションのオクターブ）
        /// List of chord tones (note name and optional octave)
        notes: Vec<(NoteName, Option<u8>)>,
        /// 音価（ティック数）
        /// Duration in ticks
        duration: Option<u16>,
        /// 付点の有無
        /// Whether the note is dotted
        dotted: bool,
        /// アーティキュレーション指定
        /// Articulation specification
        articulation: Articulation,
        /// アルペジオ指定（オプション）
        /// Arpeggio specification (optional)
        arpeggio: Option<Arpeggio>,
    },
    /// リピート記号
    /// Repetition marker
    Repetition(Repetition),
    /// 小節ジャンプ
    /// Bar jump marker
    BarJump(BarJump),
}

/// 音程付きインストゥルメントの記譜ライン
/// A line of pitched instrument notation.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchedLine {
    /// インストゥルメント名
    /// Instrument name
    pub instrument: String,
    /// ライン内の要素リスト
    /// List of elements in the line
    pub elements: Vec<PitchedElement>,
}

/// ドラムクリップの本体
/// The body of a drum clip.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumClipBody {
    /// 使用するキット名
    /// Kit name to use
    pub kit: String,
    /// ステップ解像度（ティック数）
    /// Step resolution in ticks
    pub resolution: u16,
    /// ドラム行のリスト
    /// List of drum rows
    pub rows: Vec<DrumRow>,
    /// CCオートメーションのリスト
    /// List of CC automations
    pub cc_automations: Vec<CcAutomation>,
}

/// 音程付きクリップの本体
/// The body of a pitched clip.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchedClipBody {
    /// 音程付きラインのリスト
    /// List of pitched lines
    pub lines: Vec<PitchedLine>,
    /// CCオートメーションのリスト
    /// List of CC automations
    pub cc_automations: Vec<CcAutomation>,
}

/// クリップ本体: 音程付きまたはドラム
/// Clip body: either pitched or drum.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipBody {
    /// 音程付きクリップ
    /// Pitched clip
    Pitched(PitchedClipBody),
    /// ドラムクリップ
    /// Drum clip
    Drum(DrumClipBody),
}

/// クリップ定義の全体
/// A complete clip definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipDef {
    /// クリップ名
    /// Clip name
    pub name: String,
    /// クリップオプション
    /// Clip options
    pub options: ClipOptions,
    /// クリップ本体
    /// Clip body
    pub body: ClipBody,
}
