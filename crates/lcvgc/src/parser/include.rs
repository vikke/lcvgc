use nom::{bytes::complete::tag, IResult};

use crate::ast::doc::{ExampleRender, SyntaxDoc};
use crate::ast::include::IncludeDef;
use crate::parser::common::{path_string, ws};

/// インクルード文をパースする: `include PATH`
/// Parse `include PATH` (unquoted, reads until end of line)
pub fn parse_include(input: &str) -> IResult<&str, IncludeDef> {
    let (input, _) = tag("include")(input)?;
    let (input, _) = ws(input)?;
    let (input, path) = path_string(input)?;
    Ok((
        input,
        IncludeDef {
            path: path.to_string(),
        },
    ))
}

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[SyntaxDoc {
    id: "include",
    title_ja: "インクルード",
    title_en: "Include",
    desc_ja: &[
        "記法: include パス（引用符不要、行末までがパス）",
        "別の .cvg ファイルを読み込む。ファイルの先頭にだけ書ける",
        "同じファイルの 2 回目以降の include は読み飛ばされる",
    ],
    desc_en: &[
        "Syntax: include PATH (no quotes; the path runs to end of line)",
        "Loads another .cvg file. Allowed only at the top of a file",
        "Including the same file again is silently skipped",
    ],
    example: "include path/to/file.cvg",
    render: ExampleRender::Commented,
    list: None,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_relative_path() {
        let (rest, inc) = parse_include("include ./setup.cvg\n").unwrap();
        assert_eq!(inc.path, "./setup.cvg");
        assert_eq!(rest, "\n");
    }

    #[test]
    fn include_directory_path() {
        let (rest, inc) = parse_include("include ./clips/drums.cvg\n").unwrap();
        assert_eq!(inc.path, "./clips/drums.cvg");
        assert_eq!(rest, "\n");
    }

    #[test]
    fn include_no_trailing_newline() {
        let (rest, inc) = parse_include("include path/to/deep/file.cvg").unwrap();
        assert_eq!(inc.path, "path/to/deep/file.cvg");
        assert_eq!(rest, "");
    }

    #[test]
    fn include_trims_trailing_spaces() {
        let (rest, inc) = parse_include("include ./setup.cvg   \n").unwrap();
        assert_eq!(inc.path, "./setup.cvg");
        assert_eq!(rest, "\n");
    }

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
