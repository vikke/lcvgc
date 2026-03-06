use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, digit1, multispace1, one_of},
    combinator::{map, map_res, opt, value},
    multi::many0,
    IResult,
};

use crate::ast::common::*;

/// 予約語の一覧
/// List of reserved keywords
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

/// 空白とコメント（行コメント `//` およびブロックコメント `/* */`）を消費する
/// Consume whitespace and comments (line comments `//` and block comments `/* */`).
pub fn ws(input: &str) -> IResult<&str, ()> {
    let (input, _) = many0(alt((
        value((), multispace1),
        value((), line_comment),
        value((), block_comment),
    )))(input)?;
    Ok((input, ()))
}

/// 行コメントをパースする: `// ...`
/// Parse a line comment: `// ...`
fn line_comment(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag("//")(input)?;
    let (input, comment) = take_while(|c| c != '\n')(input)?;
    Ok((input, comment))
}

/// ブロックコメントをパースする: `/* ... */`（ネスト対応）
/// Parse a block comment: `/* ... */` with nested comment support.
///
/// Supports arbitrary nesting depth (e.g. `/* outer /* inner */ outer */`).
///
/// # Returns
/// - `Ok((remaining, ()))` on success
/// - `Err` if the opening `/*` is not found or comments are not properly closed
fn block_comment(input: &str) -> IResult<&str, ()> {
    let (input, _) = tag("/*")(input)?;
    let mut remaining = input;
    let mut depth: u32 = 1;
    while depth > 0 {
        let open = remaining.find("/*");
        let close = remaining.find("*/");
        match (open, close) {
            // Found both: process whichever comes first
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                remaining = &remaining[o + 2..];
            }
            (_, Some(c)) => {
                depth -= 1;
                remaining = &remaining[c + 2..];
            }
            // No closing `*/` found: unclosed comment
            (_, None) => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )));
            }
        }
    }
    Ok((remaining, ()))
}

/// 識別子をパースする（英字・数字・アンダースコア。先頭は英字またはアンダースコア）
/// Parse an identifier (letters, digits, underscores; must start with letter or underscore).
pub fn identifier(input: &str) -> IResult<&str, &str> {
    let start = input;
    let (input, _) = take_while1(|c: char| c.is_ascii_alphabetic() || c == '_')(input)?;
    let (input, _) = take_while(|c: char| c.is_ascii_alphanumeric() || c == '_')(input)?;
    let matched = &start[..start.len() - input.len()];
    Ok((input, matched))
}

/// 予約語でない識別子をパースする
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

/// 文字列が予約語かどうかを判定する
/// Check if a string is a reserved keyword.
pub fn is_reserved(s: &str) -> bool {
    RESERVED_KEYWORDS.contains(&s)
}

/// 音名をパースする（すべて小文字）
/// 順序が重要: 2文字の音名（c#, db等）を先に試し、その後1文字の音名を試す
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

/// オクターブ番号（0-9）をパースする
/// Parse an octave number (0-9).
pub fn octave(input: &str) -> IResult<&str, Octave> {
    map(one_of("0123456789"), |c: char| {
        Octave(c.to_digit(10).unwrap() as u8)
    })(input)
}

/// 音価をパースする: 1, 2, 4, 8, 16。`.` が続く場合は付点音符
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

/// u32整数をパースする
/// Parse a u32 integer.
pub fn parse_u32(input: &str) -> IResult<&str, u32> {
    map_res(digit1, |s: &str| s.parse::<u32>())(input)
}

/// u8整数をパースする
/// Parse a u8 integer.
pub fn parse_u8(input: &str) -> IResult<&str, u8> {
    map_res(digit1, |s: &str| s.parse::<u8>())(input)
}

/// u16整数をパースする
/// Parse a u16 integer.
pub fn parse_u16(input: &str) -> IResult<&str, u16> {
    map_res(digit1, |s: &str| s.parse::<u16>())(input)
}

/// 1つ以上の空白文字（およびコメント）を消費する
/// Consume at least one whitespace character (and any comments).
pub fn ws1(input: &str) -> IResult<&str, ()> {
    let (input, _) = multispace1(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

/// 引用符なしのパス文字列をパースする: 行末まで取得し、末尾をtrimする
/// Parse an unquoted path string: reads until end of line and trims trailing whitespace
///
/// # Returns
/// パス文字列のスライス / Path string slice
pub fn path_string(input: &str) -> IResult<&str, &str> {
    let (remaining, taken) = take_while(|c| c != '\n')(input)?;
    let trimmed = taken.trim();
    if trimmed.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeWhile1,
        )));
    }
    Ok((remaining, trimmed))
}

/// 引用符付き文字列をパースする: "..."
/// Parse a quoted string: "..."
pub fn quoted_string(input: &str) -> IResult<&str, &str> {
    let (input, _) = char('"')(input)?;
    let (input, s) = take_while(|c| c != '"')(input)?;
    let (input, _) = char('"')(input)?;
    Ok((input, s))
}

/// 閉じブレース `}` の前までを取得してtrimする引用符なし値パーサー
/// Parse an unquoted value: reads until `}` and trims surrounding whitespace
///
/// # Returns
/// trim済みの値文字列 / Trimmed value string
pub fn unquoted_value(input: &str) -> IResult<&str, &str> {
    let (remaining, taken) = take_while(|c| c != '}')(input)?;
    let trimmed = taken.trim();
    if trimmed.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeWhile1,
        )));
    }
    Ok((remaining, trimmed))
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
        assert_eq!(non_reserved_identifier("bass_a"), Ok(("", "bass_a")));
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
        assert_eq!(
            duration("4."),
            Ok(("", Duration::Dotted(DottedInner::Quarter)))
        );
        assert_eq!(
            duration("8."),
            Ok(("", Duration::Dotted(DottedInner::Eighth)))
        );
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
    fn test_unquoted_value() {
        assert_eq!(
            unquoted_value("Mutant Brain\n}"),
            Ok(("}", "Mutant Brain"))
        );
        assert_eq!(unquoted_value("IAC Bus 1 }"), Ok(("}", "IAC Bus 1")));
        assert_eq!(
            unquoted_value("  volca keys  \n}"),
            Ok(("}", "volca keys"))
        );
    }

    #[test]
    fn test_unquoted_value_empty_fails() {
        assert!(unquoted_value("}").is_err());
        assert!(unquoted_value("  \n}").is_err());
    }

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("device"));
        assert!(is_reserved("clip"));
        assert!(!is_reserved("bass"));
        assert!(!is_reserved("tr808"));
    }

    #[test]
    fn test_path_string() {
        assert_eq!(path_string("./setup.cvg\n"), Ok(("\n", "./setup.cvg")));
        assert_eq!(path_string("./setup.cvg  \n"), Ok(("\n", "./setup.cvg")));
        assert_eq!(
            path_string("path/to/file.cvg"),
            Ok(("", "path/to/file.cvg"))
        );
    }

    #[test]
    fn test_path_string_empty_fails() {
        assert!(path_string("").is_err());
        assert!(path_string("\n").is_err());
    }

    #[test]
    fn test_block_comment_simple() {
        assert_eq!(ws("/* comment */rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_block_comment_multiline() {
        assert_eq!(ws("/* line1\nline2\nline3 */rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_block_comment_nested() {
        assert_eq!(ws("/* outer /* inner */ outer */rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_block_comment_deeply_nested() {
        assert_eq!(ws("/* a /* b /* c */ b */ a */rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_block_comment_with_whitespace() {
        assert_eq!(ws("  /* comment */  rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_block_comment_unclosed_fails() {
        // Unclosed block comment should fail, leaving input unconsumed
        assert_eq!(ws("/* unclosed"), Ok(("/* unclosed", ())));
    }

    #[test]
    fn test_block_comment_with_line_comment_inside() {
        assert_eq!(ws("/* contains // line comment */rest"), Ok(("rest", ())));
    }

    #[test]
    fn test_mixed_line_and_block_comments() {
        assert_eq!(ws("// line\n/* block */rest"), Ok(("rest", ())));
    }
}
