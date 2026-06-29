use crate::ast::unresolved::UnresolvedVarRefs;
use crate::ast::var::VarDef;
use crate::domain::pitch::NoteName;
use crate::midi::channel::MidiChannel;

/// CCパラメータのエイリアスマッピング
/// CC parameter alias mapping
#[derive(Debug, Clone, PartialEq)]
pub struct CcMapping {
    /// エイリアス名
    /// Alias name
    pub alias: String,
    /// CCナンバー (0-127)
    /// CC number (0-127)
    pub cc_number: u8,
    /// CCナンバーの変数参照（未解決時に使用）
    /// Variable reference for CC number (used when unresolved)
    pub cc_number_ref: Option<String>,
}

/// インストゥルメントのデフォルトノート指定
/// Default note specification for an instrument
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentNote {
    /// 音名
    /// Note name
    pub name: NoteName,
    /// オクターブ
    /// Octave
    pub octave: u8,
}

/// インストゥルメント定義
/// Instrument definition
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentDef {
    /// インストゥルメント名
    /// Instrument name
    pub name: String,
    /// 割り当てデバイス名
    /// Assigned device name
    pub device: String,
    /// MIDIチャンネル
    /// MIDI channel
    pub channel: MidiChannel,
    /// デフォルトノート（オプション）
    /// Default note (optional)
    pub note: Option<InstrumentNote>,
    /// 通常ゲート値（オプション、0-127）
    /// Normal gate value (optional, 0-127)
    pub gate_normal: Option<u8>,
    /// スタッカートゲート値（オプション、0-127）
    /// Staccato gate value (optional, 0-127)
    pub gate_staccato: Option<u8>,
    /// 通常ベロシティ（オプション、0-127）。音程楽器では `vN` 未指定ノートの既定値、
    /// ドラムでは `x`（Normal）の既定値として使われる。
    /// Normal velocity (optional, 0-127). Used as the default for pitched notes
    /// without a `vN` suffix, and for the `x` (Normal) drum hit.
    pub velocity_normal: Option<u8>,
    /// アクセントベロシティ（オプション、0-127）。ドラムの `X`（Accent）の既定値を上書きする。
    /// Accent velocity (optional, 0-127). Overrides the default for the `X` (Accent) drum hit.
    pub velocity_accent: Option<u8>,
    /// ゴーストベロシティ（オプション、0-127）。ドラムの `o`（Ghost）の既定値を上書きする。
    /// Ghost velocity (optional, 0-127). Overrides the default for the `o` (Ghost) drum hit.
    pub velocity_ghost: Option<u8>,
    /// CCマッピングのリスト
    /// List of CC mappings
    pub cc_mappings: Vec<CcMapping>,
    /// ブロック内ローカル変数定義（§6.1 ブロックスコープ）
    /// Local variable definitions within the block (§6.1 block scope)
    pub local_vars: Vec<VarDef>,
    /// 未解決変数参照（§6 変数展開）
    /// Unresolved variable references (§6 variable expansion)
    pub unresolved: UnresolvedVarRefs,
}
