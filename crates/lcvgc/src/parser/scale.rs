use nom::{
    bytes::complete::{tag, take_while1},
    combinator::map_opt,
    IResult,
};

use crate::ast::scale::{ScaleDef, ScaleType};
use crate::parser::common::{note_name, ws1};

/// スケール種別キーワードをパースする（小文字と `_` からなる語を
/// `ScaleType` のキーワード表と完全一致で照合する）。
///
/// Parse a scale type keyword: a word of lowercase letters and `_`,
/// matched exactly against the `ScaleType` keyword table.
fn scale_type(input: &str) -> IResult<&str, ScaleType> {
    map_opt(
        take_while1(|c: char| c.is_ascii_lowercase() || c == '_'),
        ScaleType::from_keyword,
    )(input)
}

/// `<root>_<type>` 形式のスケールトークンをパースする（例: `c_major`, `cs_minor`）。
/// トップレベルの `scale` 文と clip の `[scale ...]` オプションで共用する。
///
/// Parse a `<root>_<type>` scale token (e.g. `c_major`, `cs_minor`).
/// Shared by the top-level `scale` statement and the clip `[scale ...]` option.
///
/// # Arguments
/// * `input` - ルート音から始まる入力 / Input starting at the root note
///
/// # Returns
/// 残りの入力とスケール定義 / The remaining input and the scale definition
///
/// # Errors
/// ルート音・`_`・種別キーワードのいずれかが不正な場合
/// When the root note, `_`, or the type keyword is invalid
pub fn scale_token(input: &str) -> IResult<&str, ScaleDef> {
    let (input, root) = note_name(input)?;
    let (input, _) = tag("_")(input)?;
    let (input, scale_type) = scale_type(input)?;
    Ok((input, ScaleDef { root, scale_type }))
}

/// スケール定義をパースする: `scale <root>_<type>`
/// Parse `scale <root>_<type>`.
pub fn parse_scale(input: &str) -> IResult<&str, ScaleDef> {
    let (input, _) = tag("scale")(input)?;
    let (input, _) = ws1(input)?;
    scale_token(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::pitch::NoteName;

    #[test]
    fn test_scale_c_minor() {
        let (rest, sd) = parse_scale("scale c_minor").unwrap();
        assert_eq!(rest, "");
        assert_eq!(sd.root, NoteName::C);
        assert_eq!(sd.scale_type, ScaleType::Minor);
    }

    #[test]
    fn test_scale_d_dorian() {
        let (rest, sd) = parse_scale("scale d_dorian").unwrap();
        assert_eq!(rest, "");
        assert_eq!(sd.root, NoteName::D);
        assert_eq!(sd.scale_type, ScaleType::Dorian);
    }

    #[test]
    fn test_scale_all_types() {
        let cases = vec![
            ("scale c_major", ScaleType::Major),
            ("scale c_minor", ScaleType::Minor),
            ("scale c_harmonic_minor", ScaleType::HarmonicMinor),
            ("scale c_melodic_minor", ScaleType::MelodicMinor),
            ("scale c_dorian", ScaleType::Dorian),
            ("scale c_phrygian", ScaleType::Phrygian),
            ("scale c_lydian", ScaleType::Lydian),
            ("scale c_mixolydian", ScaleType::Mixolydian),
            ("scale c_locrian", ScaleType::Locrian),
        ];
        for (input, expected) in cases {
            let (_, sd) = parse_scale(input).unwrap();
            assert_eq!(sd.scale_type, expected, "failed for: {}", input);
        }
    }

    #[test]
    fn test_scale_all_roots() {
        let cases = vec![
            ("scale c_major", NoteName::C),
            ("scale d_major", NoteName::D),
            ("scale e_major", NoteName::E),
            ("scale f_major", NoteName::F),
            ("scale g_major", NoteName::G),
            ("scale a_major", NoteName::A),
            ("scale b_major", NoteName::B),
            ("scale c#_major", NoteName::Cs),
            ("scale f#_major", NoteName::Fs),
            ("scale g#_major", NoteName::Gs),
        ];
        for (input, expected) in cases {
            let (_, sd) = parse_scale(input).unwrap();
            assert_eq!(sd.root, expected, "failed for: {}", input);
        }
    }

    #[test]
    fn test_scale_s_alias_root() {
        let (rest, sd) = parse_scale("scale cs_minor").unwrap();
        assert_eq!(rest, "");
        assert_eq!(sd.root, NoteName::Cs);
        assert_eq!(sd.scale_type, ScaleType::Minor);
        let (_, sd) = parse_scale("scale as_dorian").unwrap();
        assert_eq!(sd.root, NoteName::As);
    }

    #[test]
    fn test_scale_old_space_separated_is_error() {
        assert!(parse_scale("scale c major").is_err());
        assert!(parse_scale("scale c# minor").is_err());
    }

    #[test]
    fn test_scale_unknown_or_partial_type_is_error() {
        assert!(parse_scale("scale c_blues").is_err());
        assert!(parse_scale("scale c_harmonic").is_err());
        assert!(parse_scale("scale c_majorx").is_err());
        assert!(parse_scale("scale c_").is_err());
        assert!(parse_scale("scale c").is_err());
    }

    #[test]
    fn test_scale_token_stops_at_non_keyword_char() {
        assert_eq!(
            scale_token("eb_lydian]"),
            Ok((
                "]",
                ScaleDef {
                    root: NoteName::Eb,
                    scale_type: ScaleType::Lydian
                }
            ))
        );
    }

    #[test]
    fn test_scale_token_all_roots_and_types() {
        for root in NoteName::ALL {
            for st in ScaleType::ALL {
                let input = format!("{}_{}", root.keyword(), st.keyword());
                assert_eq!(
                    scale_token(&input),
                    Ok((
                        "",
                        ScaleDef {
                            root,
                            scale_type: st
                        }
                    )),
                    "failed for: {}",
                    input
                );
            }
        }
    }
}
