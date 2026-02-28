use crate::ast::common::NoteName;

/// NoteName + octave -> MIDI note number (0-127)
/// C4 = 60 basis. Formula: (octave + 1) * 12 + semitone
/// octave uses MIDI standard -1 offset (octave=0 -> MIDI C-1 = 0)
pub fn note_number(name: NoteName, octave: u8) -> u8 {
    let semitone: u8 = match name {
        NoteName::C => 0,
        NoteName::Cs | NoteName::Db => 1,
        NoteName::D => 2,
        NoteName::Ds | NoteName::Eb => 3,
        NoteName::E => 4,
        NoteName::F => 5,
        NoteName::Fs | NoteName::Gb => 6,
        NoteName::G => 7,
        NoteName::Gs | NoteName::Ab => 8,
        NoteName::A => 9,
        NoteName::As | NoteName::Bb => 10,
        NoteName::B => 11,
    };
    (octave + 1) * 12 + semitone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c4_is_60() {
        assert_eq!(note_number(NoteName::C, 4), 60);
    }

    #[test]
    fn a4_is_69() {
        assert_eq!(note_number(NoteName::A, 4), 69);
    }

    #[test]
    fn c0_is_12() {
        assert_eq!(note_number(NoteName::C, 0), 12);
    }

    #[test]
    fn g9_is_127() {
        assert_eq!(note_number(NoteName::G, 9), 127);
    }

    #[test]
    fn enharmonic_cs_db() {
        assert_eq!(
            note_number(NoteName::Cs, 4),
            note_number(NoteName::Db, 4)
        );
    }

    #[test]
    fn enharmonic_ds_eb() {
        assert_eq!(
            note_number(NoteName::Ds, 4),
            note_number(NoteName::Eb, 4)
        );
    }

    #[test]
    fn enharmonic_fs_gb() {
        assert_eq!(
            note_number(NoteName::Fs, 4),
            note_number(NoteName::Gb, 4)
        );
    }

    #[test]
    fn enharmonic_gs_ab() {
        assert_eq!(
            note_number(NoteName::Gs, 4),
            note_number(NoteName::Ab, 4)
        );
    }

    #[test]
    fn enharmonic_as_bb() {
        assert_eq!(
            note_number(NoteName::As, 4),
            note_number(NoteName::Bb, 4)
        );
    }

    #[test]
    fn boundary_b9() {
        assert_eq!(note_number(NoteName::B, 9), 131);
    }

    #[test]
    fn middle_c_neighbors() {
        assert_eq!(note_number(NoteName::B, 3), 59);
        assert_eq!(note_number(NoteName::C, 4), 60);
        assert_eq!(note_number(NoteName::Cs, 4), 61);
    }
}
