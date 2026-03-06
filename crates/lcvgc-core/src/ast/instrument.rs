use crate::ast::common::NoteName;

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
    /// MIDIチャンネル (1-16)
    /// MIDI channel (1-16)
    pub channel: u8,
    /// デフォルトノート（オプション）
    /// Default note (optional)
    pub note: Option<InstrumentNote>,
    /// 通常ゲート値（オプション、0-127）
    /// Normal gate value (optional, 0-127)
    pub gate_normal: Option<u8>,
    /// スタッカートゲート値（オプション、0-127）
    /// Staccato gate value (optional, 0-127)
    pub gate_staccato: Option<u8>,
    /// CCマッピングのリスト
    /// List of CC mappings
    pub cc_mappings: Vec<CcMapping>,
}
