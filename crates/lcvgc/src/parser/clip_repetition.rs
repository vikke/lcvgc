use nom::character::complete::char;
use nom::IResult;

use crate::parser::common::parse_u32;

/// 繰り返しは内容文字列と回数を保持。内容の具体的なパースは上位レイヤーが担当。
///
/// Holds the content string and repeat count. Concrete parsing of the content
/// is delegated to upper layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Repetition {
    /// 繰り返し対象の生テキスト（括弧の中身）。
    ///
    /// Raw text inside the parentheses to be repeated.
    pub content: String,
    /// 繰り返し回数（`*N` の N）。
    ///
    /// Number of repetitions (the N in `*N`).
    pub count: u32,
}

/// `(内容)*N` をパースする。ネストした括弧は対応を数えてスキップ。
///
/// Parses `(content)*N`. Nested parentheses are handled by tracking depth.
pub fn parse_repetition(input: &str) -> IResult<&str, Repetition> {
    let (input, _) = char('(')(input)?;

    // 対応する ')' を探す（ネスト対応）
    // Find the matching ')' (handles nested parentheses)
    let mut depth: u32 = 1;
    let mut end = 0;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Char,
        )));
    }

    let content = &input[..end];
    let rest = &input[end + 1..]; // skip ')'

    let (rest, _) = char('*')(rest)?;
    let (rest, count) = parse_u32(rest)?;

    Ok((
        rest,
        Repetition {
            content: content.to_string(),
            count,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_repetition() {
        let (rest, rep) = parse_repetition("(c:3:8 c eb)*4").unwrap();
        assert_eq!(rep.content, "c:3:8 c eb");
        assert_eq!(rep.count, 4);
        assert_eq!(rest, "");
    }

    #[test]
    fn test_drum_repetition() {
        let (rest, rep) = parse_repetition("(x.x.)*3").unwrap();
        assert_eq!(rep.content, "x.x.");
        assert_eq!(rep.count, 3);
        assert_eq!(rest, "");
    }

    #[test]
    fn test_nested_repetition() {
        let (rest, rep) = parse_repetition("((a b)*2 c)*3").unwrap();
        assert_eq!(rep.content, "(a b)*2 c");
        assert_eq!(rep.count, 3);
        assert_eq!(rest, "");
    }
}
