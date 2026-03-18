use nom::{branch::alt, bytes::complete::tag, combinator::value, IResult};

use crate::ast::scale::{ScaleDef, ScaleType};
use crate::parser::common::{note_name, ws1};

/// スケール種別キーワードをパースする（小文字のみ）。
/// Parse a scale type keyword (lowercase only).
fn scale_type(input: &str) -> IResult<&str, ScaleType> {
    alt((
        value(ScaleType::HarmonicMinor, tag("harmonic_minor")),
        value(ScaleType::MelodicMinor, tag("melodic_minor")),
        value(ScaleType::Mixolydian, tag("mixolydian")),
        value(ScaleType::Minor, tag("minor")),
        value(ScaleType::Major, tag("major")),
        value(ScaleType::Dorian, tag("dorian")),
        value(ScaleType::Phrygian, tag("phrygian")),
        value(ScaleType::Lydian, tag("lydian")),
        value(ScaleType::Locrian, tag("locrian")),
    ))(input)
}

/// スケール定義をパースする: `scale <root> <type>`
/// Parse `scale <root> <type>`.
pub fn parse_scale(input: &str) -> IResult<&str, ScaleDef> {
    let (input, _) = tag("scale")(input)?;
    let (input, _) = ws1(input)?;
    let (input, root) = note_name(input)?;
    let (input, _) = ws1(input)?;
    let (input, st) = scale_type(input)?;
    Ok((
        input,
        ScaleDef {
            root,
            scale_type: st,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::common::NoteName;

    #[test]
    fn test_scale_c_minor() {
        let (rest, sd) = parse_scale("scale c minor").unwrap();
        assert_eq!(rest, "");
        assert_eq!(sd.root, NoteName::C);
        assert_eq!(sd.scale_type, ScaleType::Minor);
    }

    #[test]
    fn test_scale_d_dorian() {
        let (rest, sd) = parse_scale("scale d dorian").unwrap();
        assert_eq!(rest, "");
        assert_eq!(sd.root, NoteName::D);
        assert_eq!(sd.scale_type, ScaleType::Dorian);
    }

    #[test]
    fn test_scale_all_types() {
        let cases = vec![
            ("scale c major", ScaleType::Major),
            ("scale c minor", ScaleType::Minor),
            ("scale c harmonic_minor", ScaleType::HarmonicMinor),
            ("scale c melodic_minor", ScaleType::MelodicMinor),
            ("scale c dorian", ScaleType::Dorian),
            ("scale c phrygian", ScaleType::Phrygian),
            ("scale c lydian", ScaleType::Lydian),
            ("scale c mixolydian", ScaleType::Mixolydian),
            ("scale c locrian", ScaleType::Locrian),
        ];
        for (input, expected) in cases {
            let (_, sd) = parse_scale(input).unwrap();
            assert_eq!(sd.scale_type, expected, "failed for: {}", input);
        }
    }

    #[test]
    fn test_scale_all_roots() {
        let cases = vec![
            ("scale c major", NoteName::C),
            ("scale d major", NoteName::D),
            ("scale e major", NoteName::E),
            ("scale f major", NoteName::F),
            ("scale g major", NoteName::G),
            ("scale a major", NoteName::A),
            ("scale b major", NoteName::B),
            ("scale c# major", NoteName::Cs),
            ("scale f# major", NoteName::Fs),
            ("scale g# major", NoteName::Gs),
        ];
        for (input, expected) in cases {
            let (_, sd) = parse_scale(input).unwrap();
            assert_eq!(sd.root, expected, "failed for: {}", input);
        }
    }
}
