use nom::character::complete::{char, multispace0};
use nom::combinator::opt;
use nom::sequence::preceded;
use nom::IResult;

use crate::ast::clip_repetition::Repetition;
use crate::parser::common::parse_u32;

/// `(内容)*N` をパースする。ネストした括弧は対応を数えてスキップ。
///
/// 空白の取り扱い:
/// - 括弧の中身 (`内容`) はそのまま `content` に格納し、`parse_repetition_content`
///   で後段パースされる。中身の空白・改行はそちら側で吸収される。
/// - `)` と `*`、`*` と数字 (N) の間には **空白 (スペース・タブ・改行) を任意個数**
///   挟める。
/// - `*N` 全体を省略した `(...)` は count=1 のグルーピング括弧として扱う。
///   `(c d e f)` は `c d e f` と同義。
///
/// Parses `(content)*N`. Nested parentheses are handled by tracking depth.
///
/// Whitespace policy:
/// - The text inside parens is stored verbatim in `content` and is re-parsed
///   later (`parse_repetition_content`). Inner whitespace/newlines are
///   absorbed there.
/// - Any amount of whitespace (spaces, tabs, newlines) is allowed between
///   `)` and `*`, and between `*` and the count `N`.
/// - When the entire `*N` is omitted, `(...)` acts as a grouping bracket with
///   count=1 (e.g. `(c d e f)` is equivalent to `c d e f`).
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
    let after_close = &input[end + 1..]; // skip ')'

    // `)` 以降に `*N` (前後の空白・改行を任意個数許容) が続けば繰り返し回数を採用、
    // 無ければ count=1 のグルーピング扱い。`*` の検出には `opt` を使い、見つからない
    // 場合は `after_close` をそのまま残す (後続トークンを消費しない)。
    //
    // After `)`, look for an optional `*N` (with any whitespace around it).
    // If absent, treat the construct as a grouping bracket with count=1
    // without consuming any trailing tokens.
    let star_parser = preceded(multispace0, char('*'));
    let (rest, count) = match opt(star_parser)(after_close)? {
        (rest, Some(_)) => {
            let (rest, _) = multispace0(rest)?;
            let (rest, n) = parse_u32(rest)?;
            (rest, n)
        }
        (rest, None) => (rest, 1),
    };

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

    // --- 空白許容 / グルーピング (PR #79) ---

    /// `)` と `*` の間にスペースがあっても受理
    #[test]
    fn test_space_between_close_paren_and_star() {
        let (rest, rep) = parse_repetition("(c d) *4").unwrap();
        assert_eq!(rep.content, "c d");
        assert_eq!(rep.count, 4);
        assert_eq!(rest, "");
    }

    /// `*` と数字の間にスペースがあっても受理
    #[test]
    fn test_space_between_star_and_number() {
        let (rest, rep) = parse_repetition("(c d)* 4").unwrap();
        assert_eq!(rep.content, "c d");
        assert_eq!(rep.count, 4);
        assert_eq!(rest, "");
    }

    /// `*` 前後にスペース + タブ + 改行が混在しても受理
    #[test]
    fn test_multiline_space_around_star() {
        let (rest, rep) = parse_repetition("(\n  c d e f\n  g a b c\n) *\n 8").unwrap();
        assert_eq!(rep.content, "\n  c d e f\n  g a b c\n");
        assert_eq!(rep.count, 8);
        assert_eq!(rest, "");
    }

    /// `*N` を省略した `(...)` は count=1 のグルーピングとして扱う
    #[test]
    fn test_grouping_no_repeat_count() {
        let (rest, rep) = parse_repetition("(c d e f)").unwrap();
        assert_eq!(rep.content, "c d e f");
        assert_eq!(rep.count, 1);
        assert_eq!(rest, "");
    }

    /// `*N` 省略 + 括弧内に空白
    #[test]
    fn test_grouping_with_inner_whitespace() {
        let (rest, rep) = parse_repetition("( c d e f )").unwrap();
        assert_eq!(rep.content, " c d e f ");
        assert_eq!(rep.count, 1);
        assert_eq!(rest, "");
    }

    /// `*N` 省略 + 括弧の後ろに別要素が続く場合は、別要素を消費しない
    /// (`(c d) e f` は `(c d)` までで止まり残りは `e f`)
    #[test]
    fn test_grouping_does_not_consume_trailing_tokens() {
        let (rest, rep) = parse_repetition("(c d) e f").unwrap();
        assert_eq!(rep.content, "c d");
        assert_eq!(rep.count, 1);
        assert_eq!(rest, " e f");
    }
}
