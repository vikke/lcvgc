use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, space0, u16 as nom_u16},
    combinator::map,
    sequence::separated_pair,
    IResult,
};

use crate::ast::doc::{ExampleRender, SyntaxDoc};
use crate::ast::tempo::Tempo;
use crate::parser::common::ws1;

/// 相対テンポ値をパースする: `+N` または `-N`。
/// 符号と数値の間には行内空白 (スペース・タブ) を 0 個以上挟める。
/// 改行は許可しない (`tempo +\n5` は受理しない)。
///
/// Parse a relative tempo value: `+N` or `-N`.
/// Allows zero or more in-line whitespace (space/tab) between the sign and the
/// number. Newlines are not permitted (`tempo +\n5` is rejected).
fn relative_tempo(input: &str) -> IResult<&str, Tempo> {
    let positive = map(separated_pair(char('+'), space0, nom_u16), |(_, v)| {
        Tempo::Relative(v as i16)
    });
    let negative = map(separated_pair(char('-'), space0, nom_u16), |(_, v)| {
        Tempo::Relative(-(v as i16))
    });
    alt((positive, negative))(input)
}

/// テンポ定義をパースする: `tempo <value>`
/// Parse `tempo <value>`.
pub fn parse_tempo(input: &str) -> IResult<&str, Tempo> {
    let (input, _) = tag("tempo")(input)?;
    let (input, _) = ws1(input)?;
    alt((relative_tempo, map(nom_u16, Tempo::Absolute)))(input)
}

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[
    SyntaxDoc {
        id: "tempo",
        title_ja: "テンポ（絶対値）",
        title_en: "Tempo (absolute)",
        desc_ja: &[
            "曲全体のテンポ（BPM）を設定する。記法: tempo <BPM>",
            "eval するとすぐに反映される。",
        ],
        desc_en: &[
            "Sets the global tempo (BPM). Syntax: tempo <BPM>",
            "Takes effect as soon as it is evaluated.",
        ],
        example: "tempo 120",
        render: ExampleRender::Live,
        list: None,
    },
    SyntaxDoc {
        id: "tempo.relative",
        title_ja: "テンポ（相対値）",
        title_en: "Tempo (relative)",
        desc_ja: &[
            "現在のテンポから増減する。記法: tempo +<N> / tempo -<N>",
            "符号と数値の間には空白を挟んでもよい（改行は不可）。",
            "scene 内に書くと、ループごとにテンポを変えられる。",
        ],
        desc_en: &[
            "Raises or lowers the current tempo. Syntax: tempo +<N> / tempo -<N>",
            "Spaces may appear between the sign and the number (no newline).",
            "Inside a scene, it changes the tempo on every loop.",
        ],
        example: "tempo +5\ntempo -5",
        render: ExampleRender::Live,
        list: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_absolute() {
        let (rest, tempo) = parse_tempo("tempo 120").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Absolute(120));
    }

    #[test]
    fn test_tempo_absolute_140() {
        let (rest, tempo) = parse_tempo("tempo 140").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Absolute(140));
    }

    #[test]
    fn test_tempo_relative_positive() {
        let (rest, tempo) = parse_tempo("tempo +5").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(5));
    }

    #[test]
    fn test_tempo_relative_negative() {
        let (rest, tempo) = parse_tempo("tempo -10").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(-10));
    }

    /// `+` と数値の間にスペースが 1 つあっても受理する
    #[test]
    fn test_tempo_relative_positive_space_between_sign_and_number() {
        let (rest, tempo) = parse_tempo("tempo + 5").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(5));
    }

    /// `+` と数値の間にスペース複数 + タブ混在も受理する
    #[test]
    fn test_tempo_relative_positive_multiple_spaces_and_tabs() {
        let (rest, tempo) = parse_tempo("tempo +  \t  5").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(5));
    }

    /// `-` と数値の間にスペースがあっても受理する
    #[test]
    fn test_tempo_relative_negative_with_space() {
        let (rest, tempo) = parse_tempo("tempo - 10").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(-10));
    }

    /// `+` の直後に改行が入った場合は (行指向 DSL のため) Relative(5) としては
    /// パースされない。`tempo +` まで読んで数値が来ない、または `tempo` 自体が
    /// 失敗するなど挙動はあるが、`Tempo::Relative(5)` になっては困る。
    #[test]
    fn test_tempo_relative_rejects_newline_after_sign() {
        let result = parse_tempo("tempo +\n5");
        if let Ok((_, tempo)) = result {
            assert_ne!(tempo, Tempo::Relative(5));
        }
    }

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
