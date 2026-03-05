use crate::ast::common::NoteName;
use crate::ast::scale::ScaleType;

#[derive(Debug, Clone, PartialEq)]
pub struct DiatonicChord {
    pub degree: u8,
    pub root: NoteName,
    pub quality: &'static str,
    pub label: String,
    pub detail: String,
}

pub fn scale_intervals(scale_type: ScaleType) -> &'static [u8] {
    match scale_type {
        ScaleType::Major => &[0, 2, 4, 5, 7, 9, 11],
        ScaleType::Minor => &[0, 2, 3, 5, 7, 8, 10],
        ScaleType::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
        ScaleType::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
        ScaleType::Dorian => &[0, 2, 3, 5, 7, 9, 10],
        ScaleType::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
        ScaleType::Lydian => &[0, 2, 4, 6, 7, 9, 11],
        ScaleType::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
        ScaleType::Locrian => &[0, 1, 3, 5, 6, 8, 10],
    }
}

pub fn note_to_semitone(note: NoteName) -> u8 {
    match note {
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
    }
}

pub fn semitone_to_note(semitone: u8) -> NoteName {
    match semitone % 12 {
        0 => NoteName::C,
        1 => NoteName::Cs,
        2 => NoteName::D,
        3 => NoteName::Ds,
        4 => NoteName::E,
        5 => NoteName::F,
        6 => NoteName::Fs,
        7 => NoteName::G,
        8 => NoteName::Gs,
        9 => NoteName::A,
        10 => NoteName::As,
        11 => NoteName::B,
        _ => unreachable!(),
    }
}

fn note_name_display(note: NoteName) -> &'static str {
    match note {
        NoteName::C => "C",
        NoteName::Cs | NoteName::Db => "C#",
        NoteName::D => "D",
        NoteName::Ds | NoteName::Eb => "D#",
        NoteName::E => "E",
        NoteName::F => "F",
        NoteName::Fs | NoteName::Gb => "F#",
        NoteName::G => "G",
        NoteName::Gs | NoteName::Ab => "G#",
        NoteName::A => "A",
        NoteName::As | NoteName::Bb => "A#",
        NoteName::B => "B",
    }
}

fn quality_name(quality: &str) -> &str {
    match quality {
        "" => "major",
        "m" => "minor",
        "dim" => "diminished",
        "aug" => "augmented",
        _ => "unknown",
    }
}

const DEGREE_LABELS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];

/// スケールの各音の絶対セミトーン値（0-11）を返す
///
/// # Arguments
/// * `root` - スケールのルート音
/// * `scale_type` - スケールタイプ
///
/// # Returns
/// スケール構成音の絶対セミトーン値（7個）
pub fn scale_note_semitones(root: NoteName, scale_type: ScaleType) -> Vec<u8> {
    let intervals = scale_intervals(scale_type);
    let root_semi = note_to_semitone(root);
    intervals.iter().map(|&i| (root_semi + i) % 12).collect()
}

/// 指定セミトーン値がスケール内の何度かを返す（0-indexed）
///
/// # Returns
/// スケール内であれば Some(degree_index) (0=I, 1=II, ...)、スケール外なら None
pub fn scale_degree_of(semitone: u8, root: NoteName, scale_type: ScaleType) -> Option<usize> {
    let semitones = scale_note_semitones(root, scale_type);
    semitones.iter().position(|&s| s == semitone % 12)
}

pub fn diatonic_chords(root: NoteName, scale_type: ScaleType) -> Vec<DiatonicChord> {
    let intervals = scale_intervals(scale_type);
    let root_semi = note_to_semitone(root);

    (0..7)
        .map(|i| {
            let first = intervals[i];
            let third = intervals[(i + 2) % 7];
            let fifth = intervals[(i + 4) % 7];

            let interval_1_3 = (third + 12 - first) % 12;
            let interval_3_5 = (fifth + 12 - third) % 12;

            let quality: &'static str = match (interval_1_3, interval_3_5) {
                (4, 3) => "",
                (3, 4) => "m",
                (3, 3) => "dim",
                (4, 4) => "aug",
                _ => "",
            };

            let chord_root = semitone_to_note((root_semi + first) % 12);
            let display = note_name_display(chord_root);

            DiatonicChord {
                degree: (i + 1) as u8,
                root: chord_root,
                quality,
                label: format!("{}{}", display, quality),
                detail: format!("{} - {}", DEGREE_LABELS[i], quality_name(quality)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_intervals_major() {
        assert_eq!(scale_intervals(ScaleType::Major), &[0, 2, 4, 5, 7, 9, 11]);
    }

    #[test]
    fn test_scale_intervals_minor() {
        assert_eq!(scale_intervals(ScaleType::Minor), &[0, 2, 3, 5, 7, 8, 10]);
    }

    #[test]
    fn test_note_to_semitone_c() {
        assert_eq!(note_to_semitone(NoteName::C), 0);
    }

    #[test]
    fn test_note_to_semitone_cs() {
        assert_eq!(note_to_semitone(NoteName::Cs), 1);
    }

    #[test]
    fn test_note_to_semitone_b() {
        assert_eq!(note_to_semitone(NoteName::B), 11);
    }

    #[test]
    fn test_note_to_semitone_eb() {
        assert_eq!(note_to_semitone(NoteName::Eb), 3);
    }

    #[test]
    fn test_semitone_to_note_0() {
        assert_eq!(semitone_to_note(0), NoteName::C);
    }

    #[test]
    fn test_semitone_to_note_1() {
        assert_eq!(semitone_to_note(1), NoteName::Cs);
    }

    #[test]
    fn test_semitone_to_note_11() {
        assert_eq!(semitone_to_note(11), NoteName::B);
    }

    #[test]
    fn test_c_major_diatonic_count() {
        let chords = diatonic_chords(NoteName::C, ScaleType::Major);
        assert_eq!(chords.len(), 7);
    }

    #[test]
    fn test_c_major_first_chord() {
        let chords = diatonic_chords(NoteName::C, ScaleType::Major);
        assert_eq!(chords[0].label, "C");
        assert_eq!(chords[0].quality, "");
        assert_eq!(chords[0].degree, 1);
    }

    #[test]
    fn test_c_major_second_chord_dm() {
        let chords = diatonic_chords(NoteName::C, ScaleType::Major);
        assert_eq!(chords[1].label, "Dm");
        assert_eq!(chords[1].quality, "m");
    }

    #[test]
    fn test_a_minor_first_chord() {
        let chords = diatonic_chords(NoteName::A, ScaleType::Minor);
        assert_eq!(chords[0].label, "Am");
        assert_eq!(chords[0].quality, "m");
    }

    #[test]
    fn test_dorian_intervals() {
        assert_eq!(scale_intervals(ScaleType::Dorian), &[0, 2, 3, 5, 7, 9, 10]);
    }

    #[test]
    fn test_diatonic_always_7() {
        let chords = diatonic_chords(NoteName::Fs, ScaleType::Lydian);
        assert_eq!(chords.len(), 7);
    }

    #[test]
    fn test_scale_note_semitones_c_major() {
        let semitones = scale_note_semitones(NoteName::C, ScaleType::Major);
        assert_eq!(semitones, vec![0, 2, 4, 5, 7, 9, 11]);
    }

    #[test]
    fn test_scale_note_semitones_a_minor() {
        let semitones = scale_note_semitones(NoteName::A, ScaleType::Minor);
        // A=9, intervals=[0,2,3,5,7,8,10] → [9,11,0,2,4,5,7]
        assert_eq!(semitones, vec![9, 11, 0, 2, 4, 5, 7]);
    }

    #[test]
    fn test_scale_note_semitones_count() {
        let semitones = scale_note_semitones(NoteName::Eb, ScaleType::Dorian);
        assert_eq!(semitones.len(), 7);
    }

    #[test]
    fn test_scale_degree_of_in_scale() {
        // C major: C=0 is degree I (index 0)
        assert_eq!(scale_degree_of(0, NoteName::C, ScaleType::Major), Some(0));
        // C major: D=2 is degree II (index 1)
        assert_eq!(scale_degree_of(2, NoteName::C, ScaleType::Major), Some(1));
        // C major: B=11 is degree VII (index 6)
        assert_eq!(scale_degree_of(11, NoteName::C, ScaleType::Major), Some(6));
    }

    #[test]
    fn test_scale_degree_of_out_of_scale() {
        // C major: C#=1 is not in scale
        assert_eq!(scale_degree_of(1, NoteName::C, ScaleType::Major), None);
        // C major: F#=6 is not in scale
        assert_eq!(scale_degree_of(6, NoteName::C, ScaleType::Major), None);
    }
}
