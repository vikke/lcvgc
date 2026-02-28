use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, digit1, multispace1, one_of},
    combinator::{map, map_res, opt, value},
    multi::many0,
};

use crate::ast::common::*;

pub(crate) const RESERVED_KEYWORDS: &[&str] = &[
    "device",
    "instrument",
    "kit",
    "clip",
    "scene",
    "session",
    "include",
    "tempo",
    "play",
    "stop",
    "var",
    "port",
    "channel",
    "note",
    "gate_normal",
    "gate_staccato",
    "cc",
    "use",
    "resolution",
    "arp",
    "bars",
    "time",
    "scale",
    "repeat",
    "loop",
];

/// Consume whitespace and comments.
pub fn ws(input: &str) -> IResult<&str, ()> {
    let (input, _) = many0(alt((
        value((), multispace1),
        value((), line_comment),
    )))(input)?;
    Ok((input, ()))
}

/// Parse a line comment: `// ...`
fn line_comment(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag("//")(input)?;
    let (input, comment) = take_while(|c| c != '\n')(input)?;
    Ok((input, comment))
}

/// Parse an identifier (letters, digits, underscores; must start with letter or underscore).
pub fn identifier(input: &str) -> IResult<&str, &str> {
    let start = input;
    let (input, _) = take_while1(|c: char| c.is_ascii_alphabetic() || c == '_')(input)?;
    let (input, _) = take_while(|c: char| c.is_ascii_alphanumeric() || c == '_')(input)?;
    let matched = &start[..start.len() - input.len()];
    Ok((input, matched))
}

/// Parse an identifier that is not a reserved keyword.
pub fn non_reserved_identifier(input: &str) -> IResult<&str, &str> {
    let (rest, ident) = identifier(input)?;
    if RESERVED_KEYWORDS.contains(&ident) {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    } else {
        Ok((rest, ident))
    }
}

/// Check if a string is a reserved keyword.
pub fn is_reserved(s: &str) -> bool {
    RESERVED_KEYWORDS.contains(&s)
}

/// Parse a note name (all lowercase).
/// Order matters: try two-char names first (c#, db, etc.), then single-char.
pub fn note_name(input: &str) -> IResult<&str, NoteName> {
    alt((
        // Two-character note names (sharps and flats)
        value(NoteName::Cs, tag("c#")),
        value(NoteName::Db, tag("db")),
        value(NoteName::Ds, tag("d#")),
        value(NoteName::Eb, tag("eb")),
        value(NoteName::Fs, tag("f#")),
        value(NoteName::Gb, tag("gb")),
        value(NoteName::Gs, tag("g#")),
        value(NoteName::Ab, tag("ab")),
        value(NoteName::As, tag("a#")),
        value(NoteName::Bb, tag("bb")),
        // Single-character note names
        value(NoteName::C, tag("c")),
        value(NoteName::D, tag("d")),
        value(NoteName::E, tag("e")),
        value(NoteName::F, tag("f")),
        value(NoteName::G, tag("g")),
        value(NoteName::A, tag("a")),
        value(NoteName::B, tag("b")),
    ))(input)
}

/// Parse an octave number (0-9).
pub fn octave(input: &str) -> IResult<&str, Octave> {
    map(one_of("0123456789"), |c: char| {
        Octave(c.to_digit(10).unwrap() as u8)
    })(input)
}

/// Parse a duration value: 1, 2, 4, 8, 16, optionally followed by `.` for dotted.
pub fn duration(input: &str) -> IResult<&str, Duration> {
    let (input, num) = alt((
        value(16u16, tag("16")),
        value(1u16, tag("1")),
        value(2u16, tag("2")),
        value(4u16, tag("4")),
        value(8u16, tag("8")),
    ))(input)?;

    let (input, dotted) = opt(char('.'))(input)?;

    let dur = match (num, dotted.is_some()) {
        (1, false) => Duration::Whole,
        (2, false) => Duration::Half,
        (4, false) => Duration::Quarter,
        (8, false) => Duration::Eighth,
        (16, false) => Duration::Sixteenth,
        (1, true) => Duration::Dotted(DottedInner::Whole),
        (2, true) => Duration::Dotted(DottedInner::Half),
        (4, true) => Duration::Dotted(DottedInner::Quarter),
        (8, true) => Duration::Dotted(DottedInner::Eighth),
        (16, true) => Duration::Dotted(DottedInner::Sixteenth),
        _ => unreachable!(),
    };

    Ok((input, dur))
}

/// Parse a u32 integer.
pub fn parse_u32(input: &str) -> IResult<&str, u32> {
    map_res(digit1, |s: &str| s.parse::<u32>())(input)
}

/// Parse a u8 integer.
pub fn parse_u8(input: &str) -> IResult<&str, u8> {
    map_res(digit1, |s: &str| s.parse::<u8>())(input)
}

/// Parse a u16 integer.
pub fn parse_u16(input: &str) -> IResult<&str, u16> {
    map_res(digit1, |s: &str| s.parse::<u16>())(input)
}

/// Consume at least one whitespace character (and any comments).
pub fn ws1(input: &str) -> IResult<&str, ()> {
    let (input, _) = multispace1(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

/// Parse a quoted string: "..."
pub fn quoted_string(input: &str) -> IResult<&str, &str> {
    let (input, _) = char('"')(input)?;
    let (input, s) = take_while(|c| c != '"')(input)?;
    let (input, _) = char('"')(input)?;
    Ok((input, s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_ws_spaces() {
        assert_eq!(ws("   rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_ws_comment() {
        assert_eq!(ws("// comment\nrest"), Ok(("rest", ())));
    }

    #[test]
    fn test_ws_mixed() {
        assert_eq!(ws("  // comment\n  rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_ws_empty() {
        assert_eq!(ws("rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_identifier() {
        assert_eq!(identifier("bass_a rest"), Ok((" rest", "bass_a")));
        assert_eq!(identifier("tr808"), Ok(("", "tr808")));
        assert_eq!(identifier("_private"), Ok(("", "_private")));
    }

    #[test]
    fn test_identifier_starts_with_digit_fails() {
        assert!(identifier("808tr").is_err());
    }

    #[test]
    fn test_non_reserved_identifier() {
        assert_eq!(
            non_reserved_identifier("bass_a"),
            Ok(("", "bass_a"))
        );
        assert!(non_reserved_identifier("device").is_err());
        assert!(non_reserved_identifier("clip").is_err());
    }

    #[test]
    fn test_note_name_single() {
        assert_eq!(note_name("c"), Ok(("", NoteName::C)));
        assert_eq!(note_name("a"), Ok(("", NoteName::A)));
        assert_eq!(note_name("b"), Ok(("", NoteName::B)));
    }

    #[test]
    fn test_note_name_sharp() {
        assert_eq!(note_name("c#"), Ok(("", NoteName::Cs)));
        assert_eq!(note_name("f#"), Ok(("", NoteName::Fs)));
    }

    #[test]
    fn test_note_name_flat() {
        assert_eq!(note_name("eb"), Ok(("", NoteName::Eb)));
        assert_eq!(note_name("bb"), Ok(("", NoteName::Bb)));
        assert_eq!(note_name("db"), Ok(("", NoteName::Db)));
    }

    #[test]
    fn test_note_name_with_remaining() {
        assert_eq!(note_name("c:3:8"), Ok((":3:8", NoteName::C)));
        assert_eq!(note_name("eb:5"), Ok((":5", NoteName::Eb)));
    }

    #[test]
    fn test_octave() {
        assert_eq!(octave("0"), Ok(("", Octave(0))));
        assert_eq!(octave("4"), Ok(("", Octave(4))));
        assert_eq!(octave("9"), Ok(("", Octave(9))));
    }

    #[test]
    fn test_duration_simple() {
        assert_eq!(duration("1"), Ok(("", Duration::Whole)));
        assert_eq!(duration("2"), Ok(("", Duration::Half)));
        assert_eq!(duration("4"), Ok(("", Duration::Quarter)));
        assert_eq!(duration("8"), Ok(("", Duration::Eighth)));
        assert_eq!(duration("16"), Ok(("", Duration::Sixteenth)));
    }

    #[test]
    fn test_duration_dotted() {
        assert_eq!(duration("4."), Ok(("", Duration::Dotted(DottedInner::Quarter))));
        assert_eq!(duration("8."), Ok(("", Duration::Dotted(DottedInner::Eighth))));
    }

    #[test]
    fn test_parse_u32() {
        assert_eq!(parse_u32("120"), Ok(("", 120)));
        assert_eq!(parse_u32("16rest"), Ok(("rest", 16)));
    }

    #[test]
    fn test_quoted_string() {
        assert_eq!(quoted_string("\"Mutant Brain\""), Ok(("", "Mutant Brain")));
        assert_eq!(quoted_string("\"volca keys\""), Ok(("", "volca keys")));
    }

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("device"));
        assert!(is_reserved("clip"));
        assert!(!is_reserved("bass"));
        assert!(!is_reserved("tr808"));
    }
}
