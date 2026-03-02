use crate::ast::common::NoteName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordSuffix {
    Maj,
    Maj7,
    Min,
    Min7,
    Dom7,
    Dim,
    Dim7,
    Aug,
    Min7b5,
    MinMaj7,
    Sus4,
    Sus2,
    Sixth,
    Min6,
    Ninth,
    Min9,
    Add9,
    Thirteenth,
    Min13,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NoteEvent {
    Single {
        name: NoteName,
        octave: Option<u8>,
        duration: Option<u16>,
        dotted: bool,
    },
    ChordName {
        root: NoteName,
        suffix: ChordSuffix,
        octave: Option<u8>,
        duration: Option<u16>,
        dotted: bool,
    },
    Rest {
        duration: Option<u16>,
        dotted: bool,
    },
}
