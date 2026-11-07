use nom::character::complete::char;
use nom::IResult;

use crate::ast::clip_bar_jump::BarJump;
use crate::ast::doc::{ExampleRender, SyntaxDoc};
use crate::parser::common::parse_u32;

/// `>N` 形式の小節ジャンプをパースする。
/// Parses a bar jump in `>N` format.
pub fn parse_bar_jump(input: &str) -> IResult<&str, BarJump> {
    let (input, _) = char('>')(input)?;
    let (input, bar_number) = parse_u32(input)?;
    Ok((input, BarJump { bar_number }))
}

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[SyntaxDoc {
    id: "clip_bar_jump",
    title_ja: "小節ジャンプ >N",
    title_en: "Bar jump >N",
    desc_ja: &[
        "記法: >N で N 小節目（1 始まり）の頭へ現在位置を移す。",
        "手前なら休符で埋め、超えていれば超過分を切り捨てる。",
        "bars の範囲外の >N はエラー。ドラムや CC の行でも使える。",
    ],
    desc_en: &[
        "Syntax: >N moves the position to the start of bar N (1-based).",
        "Short phrases are padded with rests; overflow is cut off.",
        ">N beyond the clip's bars is an error. Works in drum/CC lines too.",
    ],
    example: "clip jump_demo [bars 4] {\n\tbass c:3:1 d:3:2 >3 e:3:4 f g a >4 g:3:1\n}",
    render: ExampleRender::Live,
    list: None,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_jump_3() {
        let (rest, bj) = parse_bar_jump(">3").unwrap();
        assert_eq!(bj, BarJump { bar_number: 3 });
        assert_eq!(rest, "");
    }

    #[test]
    fn test_bar_jump_1() {
        let (rest, bj) = parse_bar_jump(">1").unwrap();
        assert_eq!(bj, BarJump { bar_number: 1 });
        assert_eq!(rest, "");
    }

    #[test]
    fn test_bar_jump_12() {
        let (rest, bj) = parse_bar_jump(">12").unwrap();
        assert_eq!(bj, BarJump { bar_number: 12 });
        assert_eq!(rest, "");
    }

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
