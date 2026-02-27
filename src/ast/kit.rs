use crate::ast::common::NoteName;

#[derive(Debug, Clone, PartialEq)]
pub struct KitInstrumentNote {
    pub name: NoteName,
    pub octave: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KitInstrument {
    pub name: String,
    pub channel: u8,
    pub note: KitInstrumentNote,
    pub gate_normal: Option<u8>,
    pub gate_staccato: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KitDef {
    pub name: String,
    pub device: String,
    pub instruments: Vec<KitInstrument>,
}
