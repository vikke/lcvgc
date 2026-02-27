use crate::ast::clip_cc::CcAutomation;
use crate::ast::clip_drum::DrumRow;
use crate::ast::clip_note::NoteEvent;
use crate::ast::common::NoteName;
use crate::parser::clip_arpeggio::Arpeggio;
use crate::parser::clip_articulation::Articulation;
use crate::parser::clip_bar_jump::BarJump;
use crate::parser::clip_options::ClipOptions;
use crate::parser::clip_repetition::Repetition;

/// A single element in a pitched instrument line.
#[derive(Debug, Clone, PartialEq)]
pub enum PitchedElement {
    Note(NoteEvent, Articulation),
    ChordBracket {
        notes: Vec<(NoteName, Option<u8>)>,
        duration: Option<u16>,
        dotted: bool,
        articulation: Articulation,
        arpeggio: Option<Arpeggio>,
    },
    Repetition(Repetition),
    BarJump(BarJump),
}

/// A line of pitched instrument notation.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchedLine {
    pub instrument: String,
    pub elements: Vec<PitchedElement>,
}

/// The body of a drum clip.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumClipBody {
    pub kit: String,
    pub resolution: u16,
    pub rows: Vec<DrumRow>,
    pub cc_automations: Vec<CcAutomation>,
}

/// The body of a pitched clip.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchedClipBody {
    pub lines: Vec<PitchedLine>,
    pub cc_automations: Vec<CcAutomation>,
}

/// Clip body: either pitched or drum.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipBody {
    Pitched(PitchedClipBody),
    Drum(DrumClipBody),
}

/// A complete clip definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipDef {
    pub name: String,
    pub options: ClipOptions,
    pub body: ClipBody,
}
