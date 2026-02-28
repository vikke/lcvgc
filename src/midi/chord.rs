use crate::ast::clip_note::ChordSuffix;
use crate::ast::common::NoteName;
use crate::midi::note::note_number;

/// ChordSuffixから構成音のインターバル（半音数）リストを返す
pub fn chord_intervals(suffix: &ChordSuffix) -> Vec<u8> {
    match suffix {
        ChordSuffix::Maj => vec![0, 4, 7],
        ChordSuffix::Min => vec![0, 3, 7],
        ChordSuffix::Maj7 => vec![0, 4, 7, 11],
        ChordSuffix::Min7 => vec![0, 3, 7, 10],
        ChordSuffix::Dom7 => vec![0, 4, 7, 10],
        ChordSuffix::Dim => vec![0, 3, 6],
        ChordSuffix::Dim7 => vec![0, 3, 6, 9],
        ChordSuffix::Aug => vec![0, 4, 8],
        ChordSuffix::Min7b5 => vec![0, 3, 6, 10],
        ChordSuffix::MinMaj7 => vec![0, 3, 7, 11],
        ChordSuffix::Sus4 => vec![0, 5, 7],
        ChordSuffix::Sus2 => vec![0, 2, 7],
        ChordSuffix::Sixth => vec![0, 4, 7, 9],
        ChordSuffix::Min6 => vec![0, 3, 7, 9],
        ChordSuffix::Ninth => vec![0, 4, 7, 10, 14],
        ChordSuffix::Min9 => vec![0, 3, 7, 10, 14],
        ChordSuffix::Add9 => vec![0, 4, 7, 14],
        ChordSuffix::Thirteenth => vec![0, 4, 7, 10, 14, 21],
        ChordSuffix::Min13 => vec![0, 3, 7, 10, 14, 21],
    }
}

/// ルート音 + サフィックス → MIDIノート番号のリスト
///
/// 127を超えるノートはクランプされる
pub fn chord_notes(root: NoteName, octave: u8, suffix: &ChordSuffix) -> Vec<u8> {
    let base = note_number(root, octave);
    chord_intervals(suffix)
        .iter()
        .map(|&interval| {
            let n = base as u16 + interval as u16;
            if n > 127 { 127 } else { n as u8 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_triad() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Maj), vec![60, 64, 67]);
    }

    #[test]
    fn minor_triad() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Min), vec![60, 63, 67]);
    }

    #[test]
    fn maj7() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Maj7), vec![60, 64, 67, 71]);
    }

    #[test]
    fn min7() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Min7), vec![60, 63, 67, 70]);
    }

    #[test]
    fn dom7() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Dom7), vec![60, 64, 67, 70]);
    }

    #[test]
    fn dim_triad() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Dim), vec![60, 63, 66]);
    }

    #[test]
    fn dim7() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Dim7), vec![60, 63, 66, 69]);
    }

    #[test]
    fn aug_triad() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Aug), vec![60, 64, 68]);
    }

    #[test]
    fn min7b5() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Min7b5), vec![60, 63, 66, 70]);
    }

    #[test]
    fn min_maj7() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::MinMaj7), vec![60, 63, 67, 71]);
    }

    #[test]
    fn sus4() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Sus4), vec![60, 65, 67]);
    }

    #[test]
    fn sus2() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Sus2), vec![60, 62, 67]);
    }

    #[test]
    fn sixth() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Sixth), vec![60, 64, 67, 69]);
    }

    #[test]
    fn min6() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Min6), vec![60, 63, 67, 69]);
    }

    #[test]
    fn ninth() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Ninth), vec![60, 64, 67, 70, 74]);
    }

    #[test]
    fn min9() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Min9), vec![60, 63, 67, 70, 74]);
    }

    #[test]
    fn add9() {
        assert_eq!(chord_notes(NoteName::C, 4, &ChordSuffix::Add9), vec![60, 64, 67, 74]);
    }

    #[test]
    fn thirteenth() {
        assert_eq!(
            chord_notes(NoteName::C, 4, &ChordSuffix::Thirteenth),
            vec![60, 64, 67, 70, 74, 81]
        );
    }

    #[test]
    fn min13() {
        assert_eq!(
            chord_notes(NoteName::C, 4, &ChordSuffix::Min13),
            vec![60, 63, 67, 70, 74, 81]
        );
    }

    #[test]
    fn different_root_a_minor() {
        // A4=69, Am=[69,72,76]
        assert_eq!(chord_notes(NoteName::A, 4, &ChordSuffix::Min), vec![69, 72, 76]);
    }

    #[test]
    fn high_octave_clamps_to_127() {
        // G9=127, G9 maj=[127, 127, 127] (131,134 clamped)
        let notes = chord_notes(NoteName::G, 9, &ChordSuffix::Maj);
        assert_eq!(notes, vec![127, 127, 127]);
    }

    #[test]
    fn intervals_count_matches_notes_count() {
        let suffix = ChordSuffix::Thirteenth;
        assert_eq!(chord_intervals(&suffix).len(), chord_notes(NoteName::C, 4, &suffix).len());
    }
}
