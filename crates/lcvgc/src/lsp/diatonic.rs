//! ダイアトニックコード生成モジュール
//! Diatonic chord generation module
//!
//! スケールのルート音とタイプからダイアトニックコードを生成する。
//! Generates diatonic chords from a scale's root note and type.

use crate::ast::common::NoteName;
use crate::ast::scale::ScaleType;

/// ダイアトニックコード情報
/// Diatonic chord information
///
/// スケール上の各度数に対応するコード情報を保持する。
/// Holds chord information corresponding to each degree of the scale.
#[derive(Debug, Clone, PartialEq)]
pub struct DiatonicChord {
    /// スケール上の度数（1〜7）
    /// Degree on the scale (1-7)
    pub degree: u8,
    /// コードのルート音
    /// Root note of the chord
    pub root: NoteName,
    /// コードのクオリティ（"", "m", "dim", "aug"）
    /// Chord quality ("", "m", "dim", "aug")
    pub quality: &'static str,
    /// 表示用ラベル（例: "Dm"）
    /// Display label (e.g., "Dm")
    pub label: String,
    /// 詳細説明（例: "II - minor"）
    /// Detail description (e.g., "II - minor")
    pub detail: String,
}

/// スケールタイプに対応する音程間隔（半音数）の配列を返す
/// Returns the interval array (in semitones) for the given scale type
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

/// ノート名を半音数（0〜11）に変換する
/// Converts a note name to semitone number (0-11)
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

/// 半音数（0〜11）をノート名に変換する
/// Converts a semitone number (0-11) to a note name
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

/// 半音数 (0..12) を DSL に直挿し可能な小文字音名に変換する。
/// `prefer_flat=true` でフラット系 (eb, ab, bb, gb, db)、
/// false でシャープ系 (c#, d#, f#, g#, a#) を返す。
///
/// Converts a semitone (0..12) to a lowercase DSL-insertable note name.
/// `prefer_flat = true` selects flat spelling, otherwise sharp spelling.
fn semitone_to_dsl_label(semitone: u8, prefer_flat: bool) -> &'static str {
    match (semitone % 12, prefer_flat) {
        (0, _) => "c",
        (1, false) => "c#",
        (1, true) => "db",
        (2, _) => "d",
        (3, false) => "d#",
        (3, true) => "eb",
        (4, _) => "e",
        (5, _) => "f",
        (6, false) => "f#",
        (6, true) => "gb",
        (7, _) => "g",
        (8, false) => "g#",
        (8, true) => "ab",
        (9, _) => "a",
        (10, false) => "a#",
        (10, true) => "bb",
        (11, _) => "b",
        _ => unreachable!(),
    }
}

/// 補完ラベル生成時にフラット表記を優先するスケール種か判定する。
///
/// minor 系・modal 系のうち flat 寄りのものは `eb` `ab` `bb` のような
/// 表記を採用する。Major / Lydian / Mixolydian は sharp 系として扱う。
///
/// Whether to prefer flat spelling for accidentals when rendering completion
/// labels. Minor-family and flat-leaning modes prefer flats; Major / Lydian /
/// Mixolydian use sharps.
pub fn scale_prefers_flat(scale_type: ScaleType) -> bool {
    match scale_type {
        ScaleType::Major | ScaleType::Lydian | ScaleType::Mixolydian => false,
        ScaleType::Minor
        | ScaleType::HarmonicMinor
        | ScaleType::MelodicMinor
        | ScaleType::Dorian
        | ScaleType::Phrygian
        | ScaleType::Locrian => true,
    }
}

/// コードクオリティ記号を英語名に変換する
/// Converts a chord quality symbol to its English name
fn quality_name(quality: &str) -> &str {
    match quality {
        "" => "major",
        "m" => "minor",
        "dim" => "diminished",
        "aug" => "augmented",
        _ => "unknown",
    }
}

/// ローマ数字による度数ラベル
/// Degree labels in Roman numerals
const DEGREE_LABELS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];

/// 指定されたルート音とスケールタイプから7つのダイアトニックコードを生成する
/// Generates 7 diatonic chords from the specified root note and scale type
pub fn diatonic_chords(root: NoteName, scale_type: ScaleType) -> Vec<DiatonicChord> {
    let intervals = scale_intervals(scale_type);
    let root_semi = note_to_semitone(root);
    // scale 由来のフラット/シャープ選好。minor 系なら flat、major 系なら sharp。
    // パーサが受理する小文字音名と一致するラベルを生成するための情報。
    //
    // Flat/sharp preference derived from the scale. Used to render lowercase
    // labels that the DSL parser accepts directly.
    let prefer_flat = scale_prefers_flat(scale_type);

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

            let chord_root_semi = (root_semi + first) % 12;
            let chord_root = semitone_to_note(chord_root_semi);
            let display = semitone_to_dsl_label(chord_root_semi, prefer_flat);

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

    /// c major のダイアトニックコード label は DSL に直挿し可能な
    /// 小文字表記 (`c, dm, em, f, g, am, bdim`) を返す。
    /// major 系は sharp 選好だが、c major はそもそも臨時記号を含まない。
    /// c major diatonic chord labels are lowercase and directly usable in DSL
    /// (`c, dm, em, f, g, am, bdim`).
    #[test]
    fn test_c_major_diatonic_labels_lowercase() {
        let labels: Vec<String> = diatonic_chords(NoteName::C, ScaleType::Major)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, vec!["c", "dm", "em", "f", "g", "am", "bdim"]);
    }

    /// c major 1度のメタ情報も維持する (quality / degree)。
    #[test]
    fn test_c_major_first_chord_meta() {
        let chords = diatonic_chords(NoteName::C, ScaleType::Major);
        assert_eq!(chords[0].label, "c");
        assert_eq!(chords[0].quality, "");
        assert_eq!(chords[0].degree, 1);
    }

    /// d minor は minor 系のため flat 選好。
    /// 期待: `dm, edim, f, gm, am, bb, c`。
    /// パーサが受理する小文字 + flat 表記であることが本修正の主眼。
    /// d minor is in the minor family, so flats are preferred:
    /// `dm, edim, f, gm, am, bb, c`.
    #[test]
    fn test_d_minor_diatonic_labels_lowercase_with_flat() {
        let labels: Vec<String> = diatonic_chords(NoteName::D, ScaleType::Minor)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, vec!["dm", "edim", "f", "gm", "am", "bb", "c"]);
    }

    /// a minor は flat/sharp が出ない自然マイナーだが、minor 系として
    /// flat 選好の経路を通っても結果は同じ (`am, bdim, c, dm, em, f, g`)。
    #[test]
    fn test_a_minor_diatonic_labels_lowercase() {
        let labels: Vec<String> = diatonic_chords(NoteName::A, ScaleType::Minor)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, vec!["am", "bdim", "c", "dm", "em", "f", "g"]);
    }

    /// e major (sharp 選好) の 4 度は `a`、5 度は `b` で sharp は出ないが、
    /// 1 度は `e`、2 度は `f#m` (= sharp 表記) になる。
    /// Confirms major-family scales use sharp spelling for accidentals.
    #[test]
    fn test_e_major_diatonic_uses_sharp() {
        let labels: Vec<String> = diatonic_chords(NoteName::E, ScaleType::Major)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, vec!["e", "f#m", "g#m", "a", "b", "c#m", "d#dim"]);
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
}
