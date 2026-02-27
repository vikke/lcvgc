use crate::ast::common::NoteName;

#[derive(Debug, Clone, PartialEq)]
pub struct CcMapping {
    pub alias: String,
    pub cc_number: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentNote {
    pub name: NoteName,
    pub octave: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentDef {
    pub name: String,
    pub device: String,
    pub channel: u8,
    pub note: Option<InstrumentNote>,
    pub gate_normal: Option<u8>,
    pub gate_staccato: Option<u8>,
    pub cc_mappings: Vec<CcMapping>,
}
