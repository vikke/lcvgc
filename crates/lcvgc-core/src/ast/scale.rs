use crate::ast::common::NoteName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleType {
    Major,
    Minor,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleDef {
    pub root: NoteName,
    pub scale_type: ScaleType,
}
