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
    /// 単音ノートイベントとアーティキュレーション、および任意の velocity 上書き値。
    /// `Option<u8>` が `Some(v)` のとき Note On の velocity は `v` を採用、
    /// `None` のときコンパイラ既定値 (= 100) を採用する。
    ///
    /// A single note event with articulation and an optional velocity override.
    /// `Some(v)` overrides the Note On velocity with `v`; `None` falls back to
    /// the compiler default (`100`).
    Note(NoteEvent, Articulation, Option<u8>),
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
        /// velocity 上書き（オプション）。`Some(v)` のとき chord 内の全 Note On に
        /// `v` を適用する。`None` の場合コンパイラ既定値 (= 100) を採用。
        /// Optional velocity override. When `Some(v)`, all Note On events
        /// emitted from this chord use `v`. `None` falls back to the
        /// compiler default (`100`).
        velocity: Option<u8>,
    },
    /// リピート記号
    /// Repetition marker
    Repetition(Repetition),
    /// 小節ジャンプ
    /// Bar jump marker
    BarJump(BarJump),
    /// `|` 拍境界スナップ。
    /// コンパイル時に「直近 `|`/行頭以降の累積 tick が `ticks_per_beat` 未満なら
    /// 次拍境界まで埋め (休符)、超過なら直前拍境界まで戻す (= 末尾の音を削る)」を実行する。
    /// drum 行の `|` と意味を揃えるためのマーカー。
    ///
    /// Beat-boundary snap. At compile time:
    ///   - if elapsed ticks since the last `|`/row start are <= one beat,
    ///     pad forward (with rest) to the next beat boundary.
    ///   - if elapsed ticks exceed one beat, truncate back to the previous
    ///     beat boundary, dropping the trailing notes that overran.
    PipeSnap,
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
    /// 並列レイヤーの開始行か。
    /// `true` のとき、このラインから新しいレイヤー (carry-over リセット、
    /// `current_tick` を 0 にリセット) として扱われる。
    /// `false` のとき、直前の同 instrument ラインからの連結として扱われる。
    /// 並列レイヤーが切り替わるトリガー:
    ///   - 別 instrument のライン
    ///   - `---` (3 文字の独立行) セパレータ
    ///   - クリップ本体の最初のライン
    ///
    /// Whether this line starts a new parallel layer.
    /// When `true`, this line begins a fresh layer (carry-over reset,
    /// `current_tick` reset to 0). When `false`, the line is merged onto the
    /// preceding same-instrument line, inheriting its carry-over state.
    /// New layers are started by:
    ///   - a different instrument
    ///   - a `---` divider line (exactly three hyphens, on its own line)
    ///   - the first line in the clip body
    pub is_layer_start: bool,
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
