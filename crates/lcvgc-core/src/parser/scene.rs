use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, one_of},
    combinator::{map, opt},
    IResult,
};

use crate::ast::scene::*;
#[cfg(test)]
use crate::ast::tempo::Tempo;
use crate::parser::common::{identifier, ws, ws1};
use crate::parser::tempo::parse_tempo;

/// Parse an optional weight suffix: `*3`
fn parse_weight(input: &str) -> IResult<&str, u32> {
    let (input, _) = char('*')(input)?;
    let (input, d) = nom::character::complete::u32(input)?;
    Ok((input, d))
}

/// Parse a single shuffle candidate: `clip_name` or `clip_name*3`
fn parse_shuffle_candidate(input: &str) -> IResult<&str, ShuffleCandidate> {
    let (input, name) = identifier(input)?;
    let (input, weight) = opt(parse_weight)(input)?;
    Ok((
        input,
        ShuffleCandidate {
            clip: name.to_string(),
            weight: weight.unwrap_or(1),
        },
    ))
}

/// Parse a trailing probability digit (1-9).
/// The digit must not be followed by alphanumeric characters.
/// Whitespace before the digit is consumed by the caller.
fn parse_probability(input: &str) -> IResult<&str, u8> {
    let (input, d) = one_of("123456789")(input)?;
    // Make sure the digit is not part of a longer token
    if input.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    Ok((input, d.to_digit(10).unwrap() as u8))
}

/// Parse a clip entry: `ident(*w)? (| ident(*w)?)* (prob)?`
fn parse_clip_entry(input: &str) -> IResult<&str, SceneEntry> {
    let (input, first) = parse_shuffle_candidate(input)?;
    let mut candidates = vec![first];

    let mut rest = input;
    loop {
        let (input, _) = ws(rest)?;
        if let Ok((input, _)) = char::<&str, nom::error::Error<&str>>('|')(input) {
            let (input, _) = ws(input)?;
            let (input, cand) = parse_shuffle_candidate(input)?;
            candidates.push(cand);
            rest = input;
        } else {
            rest = input;
            break;
        }
    }

    let (rest, prob) = opt(parse_probability)(rest)?;

    Ok((
        rest,
        SceneEntry::Clip {
            candidates,
            probability: prob,
        },
    ))
}

/// Parse a single scene entry (tempo or clip).
fn parse_scene_entry(input: &str) -> IResult<&str, SceneEntry> {
    alt((map(parse_tempo, SceneEntry::Tempo), parse_clip_entry))(input)
}

/// Parse a scene definition: `scene NAME { entries... }`
pub fn parse_scene(input: &str) -> IResult<&str, SceneDef> {
    let (input, _) = tag("scene")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;

    let mut entries = Vec::new();
    let mut rest = input;
    loop {
        let (input, _) = ws(rest)?;
        if let Ok((input, _)) = char::<&str, nom::error::Error<&str>>('}')(input) {
            rest = input;
            break;
        }
        let (input, entry) = parse_scene_entry(input)?;
        entries.push(entry);
        rest = input;
    }

    Ok((
        rest,
        SceneDef {
            name: name.to_string(),
            entries,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_clip() {
        let (rest, scene) = parse_scene("scene intro { bass_a }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.name, "intro");
        assert_eq!(scene.entries.len(), 1);
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![ShuffleCandidate {
                    clip: "bass_a".into(),
                    weight: 1,
                }],
                probability: None,
            }
        );
    }

    #[test]
    fn test_multiple_clips() {
        let (rest, scene) = parse_scene("scene verse { bass_a drums_a }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.entries.len(), 2);
    }

    #[test]
    fn test_clip_with_probability() {
        let (rest, scene) = parse_scene("scene verse { bass_a 7 }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![ShuffleCandidate {
                    clip: "bass_a".into(),
                    weight: 1,
                }],
                probability: Some(7),
            }
        );
    }

    #[test]
    fn test_shuffle() {
        let (rest, scene) = parse_scene("scene verse { bass_a | bass_b }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![
                    ShuffleCandidate {
                        clip: "bass_a".into(),
                        weight: 1,
                    },
                    ShuffleCandidate {
                        clip: "bass_b".into(),
                        weight: 1,
                    },
                ],
                probability: None,
            }
        );
    }

    #[test]
    fn test_weighted_shuffle() {
        let (rest, scene) = parse_scene("scene verse { bass_a*3 | bass_b }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![
                    ShuffleCandidate {
                        clip: "bass_a".into(),
                        weight: 3,
                    },
                    ShuffleCandidate {
                        clip: "bass_b".into(),
                        weight: 1,
                    },
                ],
                probability: None,
            }
        );
    }

    #[test]
    fn test_shuffle_with_probability() {
        let (rest, scene) = parse_scene("scene verse { bass_a | bass_b 8 }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![
                    ShuffleCandidate {
                        clip: "bass_a".into(),
                        weight: 1,
                    },
                    ShuffleCandidate {
                        clip: "bass_b".into(),
                        weight: 1,
                    },
                ],
                probability: Some(8),
            }
        );
    }

    #[test]
    fn test_tempo_absolute() {
        let (rest, scene) = parse_scene("scene intro { tempo 120 }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.entries[0], SceneEntry::Tempo(Tempo::Absolute(120)));
    }

    #[test]
    fn test_tempo_relative_positive() {
        let (rest, scene) = parse_scene("scene bridge { tempo +5 }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.entries[0], SceneEntry::Tempo(Tempo::Relative(5)));
    }

    #[test]
    fn test_tempo_relative_negative() {
        let (rest, scene) = parse_scene("scene bridge { tempo -10 }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.entries[0], SceneEntry::Tempo(Tempo::Relative(-10)));
    }

    #[test]
    fn test_combined_scene() {
        let input = "scene main {
            tempo 120
            bass_a | bass_b*2 8
            drums_a
            fx_hit 3
        }";
        let (rest, scene) = parse_scene(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.name, "main");
        assert_eq!(scene.entries.len(), 4);

        assert_eq!(scene.entries[0], SceneEntry::Tempo(Tempo::Absolute(120)));

        assert_eq!(
            scene.entries[1],
            SceneEntry::Clip {
                candidates: vec![
                    ShuffleCandidate {
                        clip: "bass_a".into(),
                        weight: 1,
                    },
                    ShuffleCandidate {
                        clip: "bass_b".into(),
                        weight: 2,
                    },
                ],
                probability: Some(8),
            }
        );

        assert_eq!(
            scene.entries[2],
            SceneEntry::Clip {
                candidates: vec![ShuffleCandidate {
                    clip: "drums_a".into(),
                    weight: 1,
                }],
                probability: None,
            }
        );

        assert_eq!(
            scene.entries[3],
            SceneEntry::Clip {
                candidates: vec![ShuffleCandidate {
                    clip: "fx_hit".into(),
                    weight: 1,
                }],
                probability: Some(3),
            }
        );
    }

    #[test]
    fn test_empty_scene() {
        let (rest, scene) = parse_scene("scene empty {}").unwrap();
        assert_eq!(rest, "");
        assert_eq!(scene.name, "empty");
        assert_eq!(scene.entries.len(), 0);
    }

    #[test]
    fn test_three_way_shuffle() {
        let (rest, scene) = parse_scene("scene verse { bass_a | bass_b | bass_c }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            scene.entries[0],
            SceneEntry::Clip {
                candidates: vec![
                    ShuffleCandidate {
                        clip: "bass_a".into(),
                        weight: 1,
                    },
                    ShuffleCandidate {
                        clip: "bass_b".into(),
                        weight: 1,
                    },
                    ShuffleCandidate {
                        clip: "bass_c".into(),
                        weight: 1,
                    },
                ],
                probability: None,
            }
        );
    }
}
