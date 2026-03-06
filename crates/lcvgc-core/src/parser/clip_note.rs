use nom::bytes::complete::tag;
use nom::combinator::opt;
use nom::IResult;

use crate::ast::clip_note::{ChordSuffix, NoteEvent};
use crate::parser::common::{note_name, parse_u16, parse_u8};

/// Parse a chord suffix using longest-match strategy.
/// Split into two nested alt() calls to stay within nom's 21-element tuple limit.
fn parse_chord_suffix(input: &str) -> IResult<&str, ChordSuffix> {
    use nom::branch::alt;

    alt((
        alt((
            |i| tag("mMaj7")(i).map(|(r, _)| (r, ChordSuffix::MinMaj7)),
            |i| tag("mM7")(i).map(|(r, _)| (r, ChordSuffix::MinMaj7)),
            |i| tag("Maj7")(i).map(|(r, _)| (r, ChordSuffix::Maj7)),
            |i| tag("M7")(i).map(|(r, _)| (r, ChordSuffix::Maj7)),
            |i| tag("m7b5")(i).map(|(r, _)| (r, ChordSuffix::Min7b5)),
            |i| tag("dim7")(i).map(|(r, _)| (r, ChordSuffix::Dim7)),
            |i| tag("add9")(i).map(|(r, _)| (r, ChordSuffix::Add9)),
            |i| tag("m13")(i).map(|(r, _)| (r, ChordSuffix::Min13)),
            |i| tag("sus4")(i).map(|(r, _)| (r, ChordSuffix::Sus4)),
            |i| tag("sus2")(i).map(|(r, _)| (r, ChordSuffix::Sus2)),
            |i| tag("Maj")(i).map(|(r, _)| (r, ChordSuffix::Maj)),
        )),
        alt((
            |i| tag("m7")(i).map(|(r, _)| (r, ChordSuffix::Min7)),
            |i| tag("m9")(i).map(|(r, _)| (r, ChordSuffix::Min9)),
            |i| tag("m6")(i).map(|(r, _)| (r, ChordSuffix::Min6)),
            |i| tag("dim")(i).map(|(r, _)| (r, ChordSuffix::Dim)),
            |i| tag("aug")(i).map(|(r, _)| (r, ChordSuffix::Aug)),
            |i| tag("M")(i).map(|(r, _)| (r, ChordSuffix::Maj)),
            |i| tag("13")(i).map(|(r, _)| (r, ChordSuffix::Thirteenth)),
            |i| tag("7")(i).map(|(r, _)| (r, ChordSuffix::Dom7)),
            |i| tag("6")(i).map(|(r, _)| (r, ChordSuffix::Sixth)),
            |i| tag("9")(i).map(|(r, _)| (r, ChordSuffix::Ninth)),
            |i| tag("m")(i).map(|(r, _)| (r, ChordSuffix::Min)),
        )),
    ))(input)
}

/// Parse octave and duration: `:oct:dur`, `::dur`, `:oct`, or nothing.
/// Returns (octave, duration, dotted).
fn parse_oct_dur(input: &str) -> IResult<&str, (Option<u8>, Option<u16>, bool)> {
    // No colon at all -> nothing
    let (input, first_colon) = opt(tag(":"))(input)?;
    if first_colon.is_none() {
        return Ok((input, (None, None, false)));
    }

    // After first ':', check for immediate second ':'  (::dur case)
    let (input, second_colon) = opt(tag(":"))(input)?;
    if second_colon.is_some() {
        // ::dur
        let (input, dur) = parse_u16(input)?;
        let (input, dotted) = opt(tag("."))(input)?;
        return Ok((input, (None, Some(dur), dotted.is_some())));
    }

    // Parse octave
    let (input, oct) = parse_u8(input)?;

    // Check for second colon
    let (input, second_colon) = opt(tag(":"))(input)?;
    if second_colon.is_none() {
        let (input, dotted) = opt(tag("."))(input)?;
        return Ok((input, (Some(oct), None, dotted.is_some())));
    }

    // Parse duration
    let (input, dur) = parse_u16(input)?;
    let (input, dotted) = opt(tag("."))(input)?;
    Ok((input, (Some(oct), Some(dur), dotted.is_some())))
}

/// Parse a rest: `r` optionally followed by `:dur`
fn parse_rest(input: &str) -> IResult<&str, NoteEvent> {
    let (input, _) = tag("r")(input)?;
    let (input, colon) = opt(tag(":"))(input)?;
    if colon.is_none() {
        return Ok((
            input,
            NoteEvent::Rest {
                duration: None,
                dotted: false,
            },
        ));
    }
    let (input, dur) = parse_u16(input)?;
    let (input, dotted) = opt(tag("."))(input)?;
    Ok((
        input,
        NoteEvent::Rest {
            duration: Some(dur),
            dotted: dotted.is_some(),
        },
    ))
}

/// Parse a note event: single note, chord name, or rest.
pub fn parse_note_event(input: &str) -> IResult<&str, NoteEvent> {
    // Try rest first
    if input.starts_with('r') && !input.starts_with("r#") {
        // Could be rest, but 'r' is not a valid note name in our system
        // Actually check: if next char after 'r' is colon, space, end, or nothing note-like
        // In our NoteName there's no 'r', so parse_rest should work
        if let Ok(result) = parse_rest(input) {
            return Ok(result);
        }
    }

    // Parse note name
    let (input, name) = note_name(input)?;

    // Try chord suffix (longest match)
    let (input, suffix) = opt(parse_chord_suffix)(input)?;

    // Parse octave/duration
    let (input, (octave, duration, dotted)) = parse_oct_dur(input)?;

    match suffix {
        Some(s) => Ok((
            input,
            NoteEvent::ChordName {
                root: name,
                suffix: s,
                octave,
                duration,
                dotted,
            },
        )),
        None => Ok((
            input,
            NoteEvent::Single {
                name,
                octave,
                duration,
                dotted,
            },
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::common::NoteName;

    // --- Single note tests ---

    #[test]
    fn test_single_full() {
        assert_eq!(
            parse_note_event("c:3:8"),
            Ok((
                "",
                NoteEvent::Single {
                    name: NoteName::C,
                    octave: Some(3),
                    duration: Some(8),
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_single_oct_omitted() {
        assert_eq!(
            parse_note_event("c::8"),
            Ok((
                "",
                NoteEvent::Single {
                    name: NoteName::C,
                    octave: None,
                    duration: Some(8),
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_single_both_omitted() {
        assert_eq!(
            parse_note_event("c"),
            Ok((
                "",
                NoteEvent::Single {
                    name: NoteName::C,
                    octave: None,
                    duration: None,
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_single_oct_only() {
        assert_eq!(
            parse_note_event("c:3"),
            Ok((
                "",
                NoteEvent::Single {
                    name: NoteName::C,
                    octave: Some(3),
                    duration: None,
                    dotted: false,
                }
            ))
        );
    }

    // --- Chord name tests ---

    #[test]
    fn test_chord_full() {
        assert_eq!(
            parse_note_event("cm7:4:2"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::C,
                    suffix: ChordSuffix::Min7,
                    octave: Some(4),
                    duration: Some(2),
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_chord_name_only() {
        assert_eq!(
            parse_note_event("cm7"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::C,
                    suffix: ChordSuffix::Min7,
                    octave: None,
                    duration: None,
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_chord_oct_omitted() {
        assert_eq!(
            parse_note_event("cm7::2"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::C,
                    suffix: ChordSuffix::Min7,
                    octave: None,
                    duration: Some(2),
                    dotted: false,
                }
            ))
        );
    }

    #[test]
    fn test_maj7_alias() {
        let expected = NoteEvent::ChordName {
            root: NoteName::C,
            suffix: ChordSuffix::Maj7,
            octave: Some(4),
            duration: Some(2),
            dotted: false,
        };
        assert_eq!(parse_note_event("cMaj7:4:2"), Ok(("", expected.clone())));
        assert_eq!(parse_note_event("cM7:4:2"), Ok(("", expected)));
    }

    #[test]
    fn test_min_maj7() {
        assert_eq!(
            parse_note_event("cmMaj7:4:2"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::C,
                    suffix: ChordSuffix::MinMaj7,
                    octave: Some(4),
                    duration: Some(2),
                    dotted: false,
                }
            ))
        );
    }

    // --- Rest tests ---

    #[test]
    fn test_rest() {
        assert_eq!(
            parse_note_event("r"),
            Ok((
                "",
                NoteEvent::Rest {
                    duration: None,
                    dotted: false
                }
            ))
        );
    }

    #[test]
    fn test_rest_with_duration() {
        assert_eq!(
            parse_note_event("r:8"),
            Ok((
                "",
                NoteEvent::Rest {
                    duration: Some(8),
                    dotted: false
                }
            ))
        );
    }

    // --- Dotted ---

    #[test]
    fn test_dotted() {
        assert_eq!(
            parse_note_event("c:3:4."),
            Ok((
                "",
                NoteEvent::Single {
                    name: NoteName::C,
                    octave: Some(3),
                    duration: Some(4),
                    dotted: true,
                }
            ))
        );
    }

    // --- Sharp/flat chord ---

    #[test]
    fn test_flat_chord() {
        assert_eq!(
            parse_note_event("ebm7:3:4"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::Eb,
                    suffix: ChordSuffix::Min7,
                    octave: Some(3),
                    duration: Some(4),
                    dotted: false,
                }
            ))
        );
    }

    // --- m suffix (minor without number) ---

    #[test]
    fn test_minor_chord() {
        // 'm' alone should NOT match as Min because it's not in our suffix list standalone
        // Actually we need 'm' -> Min. Let me check the spec...
        // The spec says: `min`(なし、`m`のみ) and `m` is in the list
        // So `cm` should be ChordName { C, Min, ... }
        assert_eq!(
            parse_note_event("cm"),
            Ok((
                "",
                NoteEvent::ChordName {
                    root: NoteName::C,
                    suffix: ChordSuffix::Min,
                    octave: None,
                    duration: None,
                    dotted: false,
                }
            ))
        );
    }
}
