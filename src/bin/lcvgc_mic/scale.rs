/// Scale filter for snapping MIDI notes to the nearest scale degree.

pub struct ScaleFilter {
    root: u8,
    intervals: Vec<u8>,
}

impl ScaleFilter {
    /// Create a new ScaleFilter from a root MIDI note and scale name.
    /// Supported scales: major, minor, dorian, mixolydian, pentatonic.
    pub fn new(root: u8, scale_name: &str) -> Option<Self> {
        let intervals = match scale_name.to_lowercase().as_str() {
            "major" => vec![0, 2, 4, 5, 7, 9, 11],
            "minor" => vec![0, 2, 3, 5, 7, 8, 10],
            "dorian" => vec![0, 2, 3, 5, 7, 9, 10],
            "mixolydian" => vec![0, 2, 4, 5, 7, 9, 10],
            "pentatonic" => vec![0, 2, 4, 7, 9],
            _ => return None,
        };
        Some(Self { root, intervals })
    }

    /// Snap a MIDI note to the nearest degree in this scale.
    pub fn snap_to_scale(&self, midi_note: u8) -> u8 {
        let root_class = self.root % 12;

        let mut best_note = midi_note;
        let mut best_distance = i32::MAX;

        // Check candidates within +/- 6 semitones
        for &interval in &self.intervals {
            let scale_class = (root_class + interval) % 12;

            // Find the nearest octave instance of this scale degree
            for octave_offset in -1i32..=1 {
                let candidate =
                    midi_note as i32 - ((midi_note as i32 % 12) - scale_class as i32) + octave_offset * 12;
                if candidate < 0 || candidate > 127 {
                    continue;
                }
                let distance = (candidate - midi_note as i32).abs();
                if distance < best_distance {
                    best_distance = distance;
                    best_note = candidate as u8;
                }
            }
        }

        best_note
    }

    /// Parse a scale string like "C major" or "D# minor" into a ScaleFilter.
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.trim().splitn(2, ' ').collect();
        if parts.len() != 2 {
            return None;
        }

        let root = note_name_to_midi_class(parts[0])?;
        Self::new(root, parts[1])
    }
}

/// Parse a note name like "C", "D#", "Bb" to MIDI pitch class (0-11).
fn note_name_to_midi_class(name: &str) -> Option<u8> {
    let mut chars = name.chars();
    let base = match chars.next()? {
        'C' | 'c' => 0,
        'D' | 'd' => 2,
        'E' | 'e' => 4,
        'F' | 'f' => 5,
        'G' | 'g' => 7,
        'A' | 'a' => 9,
        'B' | 'b' => 11,
        _ => return None,
    };

    let modifier = match chars.next() {
        Some('#') => 1i8,
        Some('b') => -1,
        None => 0,
        _ => return None,
    };

    Some(((base as i8 + modifier).rem_euclid(12)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snap_c_major_in_scale() {
        let filter = ScaleFilter::from_str("C major").unwrap();
        // C4 (60) is in C major -> stays
        assert_eq!(filter.snap_to_scale(60), 60);
        // E4 (64) is in C major -> stays
        assert_eq!(filter.snap_to_scale(64), 64);
    }

    #[test]
    fn test_snap_c_major_out_of_scale() {
        let filter = ScaleFilter::from_str("C major").unwrap();
        // C#4 (61) snaps to C4 (60) or D4 (62)
        let snapped = filter.snap_to_scale(61);
        assert!(snapped == 60 || snapped == 62, "Got {snapped}");
    }

    #[test]
    fn test_snap_c_major_f_sharp() {
        let filter = ScaleFilter::from_str("C major").unwrap();
        // F#4 (66) snaps to F4 (65) or G4 (67)
        let snapped = filter.snap_to_scale(66);
        assert!(snapped == 65 || snapped == 67, "Got {snapped}");
    }

    #[test]
    fn test_snap_a_minor() {
        let filter = ScaleFilter::from_str("A minor").unwrap();
        // A4 (69) in A minor -> stays
        assert_eq!(filter.snap_to_scale(69), 69);
        // G#4 (68) not in A minor, snap to G4 (67) or A4 (69)
        let snapped = filter.snap_to_scale(68);
        assert!(snapped == 67 || snapped == 69, "Got {snapped}");
    }

    #[test]
    fn test_snap_pentatonic() {
        let filter = ScaleFilter::from_str("C pentatonic").unwrap();
        // C pentatonic: C D E G A
        assert_eq!(filter.snap_to_scale(60), 60); // C
        assert_eq!(filter.snap_to_scale(64), 64); // E
        assert_eq!(filter.snap_to_scale(67), 67); // G
    }

    #[test]
    fn test_dorian() {
        let filter = ScaleFilter::from_str("D dorian").unwrap();
        assert!(filter.snap_to_scale(62) == 62); // D
    }

    #[test]
    fn test_mixolydian() {
        let filter = ScaleFilter::from_str("G mixolydian").unwrap();
        assert!(filter.snap_to_scale(67) == 67); // G
    }

    #[test]
    fn test_invalid_scale() {
        assert!(ScaleFilter::from_str("C lydian").is_none());
    }

    #[test]
    fn test_note_name_parsing() {
        assert_eq!(note_name_to_midi_class("C"), Some(0));
        assert_eq!(note_name_to_midi_class("D#"), Some(3));
        assert_eq!(note_name_to_midi_class("Bb"), Some(10));
    }
}
