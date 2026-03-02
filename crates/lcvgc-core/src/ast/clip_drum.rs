/// Hit symbol in a drum step sequencer pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitSymbol {
    /// `x` — normal hit, velocity 100
    Normal,
    /// `X` — accent hit, velocity 127
    Accent,
    /// `o` — ghost note, velocity 40
    Ghost,
    /// `.` — rest (silence)
    Rest,
}

impl HitSymbol {
    /// Returns the MIDI velocity for this hit, or `None` for a rest.
    pub fn velocity(self) -> Option<u8> {
        match self {
            HitSymbol::Normal => Some(100),
            HitSymbol::Accent => Some(127),
            HitSymbol::Ghost => Some(40),
            HitSymbol::Rest => None,
        }
    }
}

/// A single instrument row in a drum clip.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumRow {
    pub instrument: String,
    pub hits: Vec<HitSymbol>,
    /// Per-step firing probability 0-100. `None` means all steps are 100%.
    pub probability: Option<Vec<u8>>,
}
