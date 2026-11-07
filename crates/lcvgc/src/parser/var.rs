use nom::{
    bytes::complete::{tag, take_while1},
    character::complete::char,
    IResult,
};

use crate::ast::doc::{ExampleRender, SyntaxDoc};
use crate::ast::var::VarDef;
use crate::parser::common::{non_reserved_identifier, ws};

/// 変数定義をパースする: `var NAME = VALUE`
/// VALUEは次の空白文字または入力末尾まで消費される。
/// Parse `var NAME = VALUE`
/// VALUE is consumed until the next whitespace or end of input.
pub fn parse_var(input: &str) -> IResult<&str, VarDef> {
    let (input, _) = tag("var")(input)?;
    let (input, _) = ws(input)?;
    let (input, name) = non_reserved_identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('=')(input)?;
    let (input, _) = ws(input)?;
    let (input, value) = take_while1(|c: char| !c.is_whitespace())(input)?;
    Ok((
        input,
        VarDef {
            name: name.to_string(),
            value: value.to_string(),
        },
    ))
}

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[
    SyntaxDoc {
        id: "var",
        title_ja: "変数",
        title_en: "Variables",
        desc_ja: &[
            "記法: var 名前 = 値（値は空白を含まない 1 語）",
            "予約語（device, clip, tempo など）は名前に使えない",
        ],
        desc_en: &[
            "Syntax: var NAME = VALUE (VALUE is a single word without spaces)",
            "Reserved words (device, clip, tempo, ...) cannot be names",
        ],
        example: "var bass_ch = 1\nvar default_gate = 80",
        render: ExampleRender::Live,
        list: None,
    },
    SyntaxDoc {
        id: "var.reference",
        title_ja: "変数の参照",
        title_en: "Referring to variables",
        desc_ja: &[
            "参照は $ を付けず名前をそのまま書く。変数が無ければ文字どおりの値",
            "{ } の中でも var を書ける。内側の定義が外側より優先される",
            "例（device と instrument は後の節で説明する）:",
        ],
        desc_en: &[
            "Write the name as is, without $. Unknown names are taken literally",
            "var also works inside { }; the inner definition wins",
            "Example (device and instrument are explained in later sections):",
        ],
        example: "instrument var_demo {\n\tvar bass_ch = 3\n\tdevice my_synth\n\tchannel bass_ch\n\tgate_normal default_gate\n}",
        render: ExampleRender::Commented,
        list: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_identifier_value() {
        let (rest, var) = parse_var("var dev = mutant_brain").unwrap();
        assert_eq!(var.name, "dev");
        assert_eq!(var.value, "mutant_brain");
        assert_eq!(rest, "");
    }

    #[test]
    fn var_numeric_value() {
        let (rest, var) = parse_var("var gate = 80").unwrap();
        assert_eq!(var.name, "gate");
        assert_eq!(var.value, "80");
        assert_eq!(rest, "");
    }

    #[test]
    fn var_reserved_word_as_name_fails() {
        assert!(parse_var("var var = something").is_err());
        assert!(parse_var("var include = something").is_err());
        assert!(parse_var("var device = something").is_err());
    }

    /// §10.4: pause / resume は予約語なので変数名に使えない
    /// §10.4: `pause` and `resume` are reserved keywords and cannot be used as variable names
    #[test]
    fn var_pause_resume_reserved() {
        assert!(parse_var("var pause = something").is_err());
        assert!(parse_var("var resume = something").is_err());
    }

    /// §10.4: mute / unmute は予約語なので変数名に使えない
    /// §10.4: `mute` and `unmute` are reserved keywords and cannot be used as variable names
    #[test]
    fn var_mute_unmute_reserved() {
        assert!(parse_var("var mute = something").is_err());
        assert!(parse_var("var unmute = something").is_err());
    }

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
