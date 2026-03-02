#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteName {
    C,
    Cs,
    Db,
    D,
    Ds,
    Eb,
    E,
    F,
    Fs,
    Gb,
    G,
    Gs,
    Ab,
    A,
    As,
    Bb,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Octave(pub u8);

impl Octave {
    pub fn new(value: u8) -> Option<Self> {
        if value <= 9 {
            Some(Octave(value))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    Dotted(DottedInner),
}

/// Inner duration for dotted notes (cannot itself be dotted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DottedInner {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSpec {
    pub kind: GateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    Normal,
    Staccato,
    Direct(u8),
}

impl Default for GateSpec {
    fn default() -> Self {
        GateSpec {
            kind: GateKind::Normal,
        }
    }
}
