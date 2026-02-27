/// CarryOverState manages octave/duration carry-over for shorthand note notation.
/// Default: octave=4, duration=4 (quarter note).

#[derive(Debug, Clone, PartialEq)]
pub struct CarryOverState {
    pub octave: u8,
    pub duration: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedNote {
    pub octave: u8,
    pub duration: u16,
    pub dotted: bool,
}

impl CarryOverState {
    pub fn new() -> Self {
        CarryOverState { octave: 4, duration: 4 }
    }

    /// Resolve a note with optional octave/duration, updating internal state.
    pub fn resolve(&mut self, octave: Option<u8>, duration: Option<u16>, dotted: bool) -> ResolvedNote {
        if let Some(oct) = octave {
            self.octave = oct;
        }
        if let Some(dur) = duration {
            self.duration = dur;
        }
        ResolvedNote {
            octave: self.octave,
            duration: self.duration,
            dotted,
        }
    }
}

impl Default for CarryOverState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let state = CarryOverState::new();
        assert_eq!(state.octave, 4);
        assert_eq!(state.duration, 4);
    }

    #[test]
    fn full_specify() {
        let mut state = CarryOverState::new();
        let note = state.resolve(Some(3), Some(8), false);
        assert_eq!(note, ResolvedNote { octave: 3, duration: 8, dotted: false });
        assert_eq!(state.octave, 3);
        assert_eq!(state.duration, 8);
    }

    #[test]
    fn both_omitted() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(None, None, false);
        assert_eq!(note, ResolvedNote { octave: 3, duration: 8, dotted: false });
    }

    #[test]
    fn octave_omitted_duration_changed() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(None, Some(4), false);
        assert_eq!(note, ResolvedNote { octave: 3, duration: 4, dotted: false });
    }

    #[test]
    fn octave_changed_duration_omitted() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(Some(5), None, false);
        assert_eq!(note, ResolvedNote { octave: 5, duration: 8, dotted: false });
    }

    #[test]
    fn sequential_resolve() {
        let mut state = CarryOverState::new();
        // c:3:8
        let n1 = state.resolve(Some(3), Some(8), false);
        assert_eq!(n1, ResolvedNote { octave: 3, duration: 8, dotted: false });
        // c (both omitted)
        let n2 = state.resolve(None, None, false);
        assert_eq!(n2, ResolvedNote { octave: 3, duration: 8, dotted: false });
        // f::4 (octave omitted, duration changed)
        let n3 = state.resolve(None, Some(4), false);
        assert_eq!(n3, ResolvedNote { octave: 3, duration: 4, dotted: false });
    }

    #[test]
    fn dotted_note() {
        let mut state = CarryOverState::new();
        let note = state.resolve(Some(3), Some(4), true);
        assert_eq!(note, ResolvedNote { octave: 3, duration: 4, dotted: true });
    }
}
