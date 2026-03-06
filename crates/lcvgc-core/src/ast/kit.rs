use crate::ast::common::NoteName;

/// キット内インストゥルメントのノート指定
/// Note specification for an instrument within a kit
#[derive(Debug, Clone, PartialEq)]
pub struct KitInstrumentNote {
    /// 音名
    /// Note name
    pub name: NoteName,
    /// オクターブ
    /// Octave
    pub octave: u8,
}

/// キット内のインストゥルメント定義
/// Instrument definition within a kit
#[derive(Debug, Clone, PartialEq)]
pub struct KitInstrument {
    /// インストゥルメント名
    /// Instrument name
    pub name: String,
    /// MIDIチャンネル (1-16)
    /// MIDI channel (1-16)
    pub channel: u8,
    /// MIDIノート指定
    /// MIDI note specification
    pub note: KitInstrumentNote,
    /// 通常ゲート値（オプション、0-127）
    /// Normal gate value (optional, 0-127)
    pub gate_normal: Option<u8>,
    /// スタッカートゲート値（オプション、0-127）
    /// Staccato gate value (optional, 0-127)
    pub gate_staccato: Option<u8>,
}

/// キット定義（ドラムキット等の複数インストゥルメントのグループ）
/// Kit definition (a group of multiple instruments such as a drum kit)
#[derive(Debug, Clone, PartialEq)]
pub struct KitDef {
    /// キット名
    /// Kit name
    pub name: String,
    /// 割り当てデバイス名
    /// Assigned device name
    pub device: String,
    /// キット内インストゥルメントのリスト
    /// List of instruments within the kit
    pub instruments: Vec<KitInstrument>,
}
