//! 文法チュートリアルの組み立て
//! Grammar tutorial rendering.
//!
//! 各パーサ（`parser/*.rs`）が持つ構文ドキュメント `DOCS` を学習順に並べ、
//! 説明をコメント、例文をコードとした `.cvg` テキストを組み立てる。
//! 例文は各パーサのテストで解析済みなので、生成結果もそのまま有効な DSL になる。
//!
//! Collects the `DOCS` owned by each parser (`parser/*.rs`) in learning order and
//! renders a `.cvg` text: descriptions become comments, examples become code.
//! Examples are verified by each parser's tests, so the output is valid DSL.

use crate::ast::doc::{ExampleRender, KeywordList, SyntaxDoc};
use crate::ast::scale::ScaleType;
use crate::domain::chord::ChordSuffix;
use crate::domain::pitch::NoteName;
use crate::parser;

/// チュートリアルの言語
/// Language of the tutorial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// 日本語 / Japanese
    Ja,
    /// 英語 / English
    En,
}

impl Lang {
    /// 言語コードから言語を決める。`ja` で始まれば日本語、それ以外（省略を含む）は英語
    ///
    /// Picks the language from a code: Japanese if it starts with `ja`,
    /// otherwise (including `None`) English.
    ///
    /// # Arguments
    /// * `code` - 言語コード（例: `"ja"`, `"ja_JP.UTF-8"`, `"en"`）/ language code
    ///
    /// # Returns
    /// 決まった言語 / the chosen language
    pub fn from_code(code: Option<&str>) -> Lang {
        match code {
            Some(c) if c.starts_with("ja") => Lang::Ja,
            _ => Lang::En,
        }
    }
}

/// チュートリアルの 1 節（1 つのパーサモジュールの DOCS）
/// One tutorial section: the DOCS of one parser module.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    /// パーサのモジュール名（`parser/<module>.rs`）/ parser module name
    pub module: &'static str,
    /// そのモジュールの構文ドキュメント / the module's syntax docs
    pub docs: &'static [SyntaxDoc],
}

/// チュートリアルに載せる節の一覧（学習順）
///
/// `parser/` に構文を担当するファイルを足したら、ここにも登録する。
/// 登録漏れは `sections_cover_every_parser_file` テストで検出する。
///
/// Sections in learning order. Register every syntax-owning file in `parser/`
/// here; the `sections_cover_every_parser_file` test catches omissions.
pub const SECTIONS: &[Section] = &[
    Section {
        module: "common",
        docs: parser::common::DOCS,
    },
    Section {
        module: "include",
        docs: parser::include::DOCS,
    },
    Section {
        module: "tempo",
        docs: parser::tempo::DOCS,
    },
    Section {
        module: "scale",
        docs: parser::scale::DOCS,
    },
    Section {
        module: "var",
        docs: parser::var::DOCS,
    },
    Section {
        module: "device",
        docs: parser::device::DOCS,
    },
    Section {
        module: "instrument",
        docs: parser::instrument::DOCS,
    },
    Section {
        module: "kit",
        docs: parser::kit::DOCS,
    },
    Section {
        module: "clip",
        docs: parser::clip::DOCS,
    },
    Section {
        module: "clip_options",
        docs: parser::clip_options::DOCS,
    },
    Section {
        module: "clip_note",
        docs: parser::clip_note::DOCS,
    },
    Section {
        module: "clip_shorthand",
        docs: parser::clip_shorthand::DOCS,
    },
    Section {
        module: "clip_articulation",
        docs: parser::clip_articulation::DOCS,
    },
    Section {
        module: "clip_repetition",
        docs: parser::clip_repetition::DOCS,
    },
    Section {
        module: "clip_bar_jump",
        docs: parser::clip_bar_jump::DOCS,
    },
    Section {
        module: "clip_arpeggio",
        docs: parser::clip_arpeggio::DOCS,
    },
    Section {
        module: "clip_drum",
        docs: parser::clip_drum::DOCS,
    },
    Section {
        module: "clip_cc",
        docs: parser::clip_cc::DOCS,
    },
    Section {
        module: "scene",
        docs: parser::scene::DOCS,
    },
    Section {
        module: "session",
        docs: parser::session::DOCS,
    },
    Section {
        module: "playback",
        docs: parser::playback::DOCS,
    },
];

/// 登録済みの全節からチュートリアル本文を組み立てる
///
/// Renders the tutorial text from all registered sections.
///
/// # Arguments
/// * `lang` - 説明文の言語 / language of the descriptions
///
/// # Returns
/// `.cvg` としてそのまま使えるチュートリアル本文 / tutorial text usable as a `.cvg` file
pub fn render(lang: Lang) -> String {
    render_sections(SECTIONS, lang)
}

/// 指定した節の並びからチュートリアル本文を組み立てる
///
/// 冒頭に見出しを置き、各構文について「見出し → 説明 → キーワード一覧 → 例文 → 空行」の順に出す。
///
/// Renders the tutorial from the given sections: a header, then for each doc
/// "title → description → keyword list → example → blank line".
///
/// # Arguments
/// * `sections` - 出力する節（この順に出す）/ sections to render, in order
/// * `lang` - 説明文の言語 / language of the descriptions
///
/// # Returns
/// チュートリアル本文 / the tutorial text
pub fn render_sections(sections: &[Section], lang: Lang) -> String {
    let mut lines: Vec<String> = header(lang).iter().map(|s| s.to_string()).collect();
    for doc in sections.iter().flat_map(|s| s.docs.iter()) {
        lines.extend(render_doc(doc, lang));
    }
    // 各構文の後ろの空行が末尾に残らないよう、最後は改行 1 つにそろえる
    // Trim the trailing blank line left after the last doc; end with one newline
    let mut text = lines.join("\n").trim_end().to_string();
    text.push('\n');
    text
}

/// チュートリアル冒頭の見出し行
/// Header lines at the top of the tutorial.
fn header(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Ja => &[
            "// ============================================================",
            "// lcvgc DSL チュートリアル",
            "// lcvgc が文法から生成したサンプルです。例文はパーサで検証済みです。",
            "// 不要な部分は自由に削除してください。",
            "// ============================================================",
            "",
        ],
        Lang::En => &[
            "// ============================================================",
            "// lcvgc DSL tutorial",
            "// Generated by lcvgc from its grammar. Every example is parser-checked.",
            "// Feel free to delete any parts you don't need.",
            "// ============================================================",
            "",
        ],
    }
}

/// 1 つの構文ドキュメントを行の並びにする
///
/// Renders one syntax doc into lines.
///
/// # Arguments
/// * `doc` - 構文ドキュメント / the syntax doc
/// * `lang` - 説明文の言語 / language of the description
///
/// # Returns
/// 出力する行（最後は空行）/ output lines, ending with a blank line
fn render_doc(doc: &SyntaxDoc, lang: Lang) -> Vec<String> {
    let (title, desc) = match lang {
        Lang::Ja => (doc.title_ja, doc.desc_ja),
        Lang::En => (doc.title_en, doc.desc_en),
    };
    let mut lines = vec![format!("// --- {} ---", title)];
    lines.extend(desc.iter().map(|d| comment_line(d)));
    if let Some(kind) = doc.list {
        let label = match lang {
            Lang::Ja => "使える値",
            Lang::En => "Available",
        };
        lines.push(format!("// {}: {}", label, keyword_list(kind).join(" ")));
    }
    for line in doc.example.trim_end_matches('\n').lines() {
        lines.push(match doc.render {
            ExampleRender::Live => line.to_string(),
            ExampleRender::Commented => comment_line(line),
        });
    }
    lines.push(String::new());
    lines
}

/// 1 行をコメントにする（空行は `//` だけにして末尾空白を残さない）
/// Turns a line into a comment (`//` alone for an empty line, no trailing space).
fn comment_line(line: &str) -> String {
    if line.is_empty() {
        "//".to_string()
    } else {
        format!("// {}", line)
    }
}

/// キーワード一覧を一覧定数から作る
///
/// Builds a keyword list from the list constants.
///
/// # Arguments
/// * `kind` - 一覧の種類 / kind of list
///
/// # Returns
/// DSL に書けるキーワードの並び / keywords as written in the DSL
fn keyword_list(kind: KeywordList) -> Vec<&'static str> {
    match kind {
        KeywordList::NoteNames => NoteName::ALL.iter().map(|n| n.keyword()).collect(),
        KeywordList::ChordSuffixes => ChordSuffix::ALL
            .iter()
            .flat_map(|s| s.spellings().iter().copied())
            .collect(),
        KeywordList::ScaleTypes => ScaleType::ALL.iter().map(|t| t.keyword()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// テスト用の構文ドキュメント / docs used by the tests
    const LIVE: SyntaxDoc = SyntaxDoc {
        id: "test.live",
        title_ja: "テンポ",
        title_en: "Tempo",
        desc_ja: &["テンポを設定する", ""],
        desc_en: &["Sets the tempo", ""],
        example: "tempo 120\n",
        render: ExampleRender::Live,
        list: None,
    };
    const COMMENTED: SyntaxDoc = SyntaxDoc {
        id: "test.commented",
        title_ja: "インクルード",
        title_en: "Include",
        desc_ja: &[],
        desc_en: &[],
        example: "include a.cvg\n\ninclude b.cvg",
        render: ExampleRender::Commented,
        list: None,
    };
    const LISTED: SyntaxDoc = SyntaxDoc {
        id: "test.listed",
        title_ja: "スケール",
        title_en: "Scale",
        desc_ja: &[],
        desc_en: &[],
        example: "scale c_minor",
        render: ExampleRender::Live,
        list: Some(KeywordList::ScaleTypes),
    };
    const TEST_SECTIONS: &[Section] = &[Section {
        module: "test",
        docs: &[LIVE, COMMENTED, LISTED],
    }];

    #[test]
    fn lang_from_code() {
        assert_eq!(Lang::from_code(Some("ja")), Lang::Ja);
        assert_eq!(Lang::from_code(Some("ja_JP.UTF-8")), Lang::Ja);
        assert_eq!(Lang::from_code(Some("en")), Lang::En);
        assert_eq!(Lang::from_code(Some("fr")), Lang::En);
        assert_eq!(Lang::from_code(None), Lang::En);
    }

    #[test]
    fn renders_header_title_description_and_live_example() {
        let text = render_sections(TEST_SECTIONS, Lang::Ja);
        assert!(text.starts_with("// ===="));
        assert!(text.contains("// lcvgc DSL チュートリアル\n"));
        assert!(text.contains("// --- テンポ ---\n// テンポを設定する\n//\ntempo 120\n\n"));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn renders_english_text() {
        let text = render_sections(TEST_SECTIONS, Lang::En);
        assert!(text.contains("// lcvgc DSL tutorial\n"));
        assert!(text.contains("// --- Tempo ---\n// Sets the tempo\n"));
        assert!(!text.contains("テンポ"));
    }

    #[test]
    fn commented_example_is_commented_out_line_by_line() {
        let text = render_sections(TEST_SECTIONS, Lang::En);
        assert!(text.contains("// include a.cvg\n//\n// include b.cvg\n"));
        assert!(!text.lines().any(|l| l.starts_with("include")));
    }

    #[test]
    fn keyword_list_is_generated_from_constants() {
        let text = render_sections(TEST_SECTIONS, Lang::En);
        let expected: Vec<&str> = ScaleType::ALL.iter().map(|t| t.keyword()).collect();
        assert!(text.contains(&format!("// Available: {}\n", expected.join(" "))));
        let ja = render_sections(TEST_SECTIONS, Lang::Ja);
        assert!(ja.contains("// 使える値: "));
    }

    #[test]
    fn keyword_lists_cover_all_constants() {
        assert_eq!(
            keyword_list(KeywordList::NoteNames).len(),
            NoteName::ALL.len()
        );
        assert_eq!(
            keyword_list(KeywordList::ScaleTypes).len(),
            ScaleType::ALL.len()
        );
        let spellings: usize = ChordSuffix::ALL.iter().map(|s| s.spellings().len()).sum();
        assert_eq!(keyword_list(KeywordList::ChordSuffixes).len(), spellings);
    }

    #[test]
    fn rendered_test_sections_parse_fully() {
        for lang in [Lang::Ja, Lang::En] {
            let text = render_sections(TEST_SECTIONS, lang);
            let (rest, _) = parser::parse_source(&text).expect("tutorial should parse");
            assert!(rest.trim().is_empty(), "rest = {:?}", rest);
        }
    }

    /// 構文を担当しない補助ファイル（SECTIONS に載せない）
    /// Helper files in `parser/` that own no syntax (not in SECTIONS).
    const NON_SYNTAX_PARSER_FILES: &[&str] = &["mod", "cell_normalize"];

    #[test]
    fn sections_cover_every_parser_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/parser");
        let files: BTreeSet<String> = std::fs::read_dir(&dir)
            .expect("read src/parser")
            .filter_map(|e| {
                let name = e.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".rs").map(|s| s.to_string())
            })
            .filter(|m| !NON_SYNTAX_PARSER_FILES.contains(&m.as_str()))
            .collect();
        let registered: BTreeSet<String> = SECTIONS.iter().map(|s| s.module.to_string()).collect();
        assert_eq!(
            files, registered,
            "parser/ のファイルと lsp::tutorial::SECTIONS が一致しません / parser files and SECTIONS differ"
        );
    }

    #[test]
    fn sections_have_no_duplicate_modules() {
        let mut seen = BTreeSet::new();
        for s in SECTIONS {
            assert!(seen.insert(s.module), "duplicate section: {}", s.module);
        }
    }

    #[test]
    fn rendered_tutorial_parses_fully() {
        for lang in [Lang::Ja, Lang::En] {
            let text = render(lang);
            let (rest, _) = parser::parse_source(&text).expect("tutorial should parse");
            assert!(rest.trim().is_empty(), "rest = {:?}", rest);
        }
    }

    #[test]
    fn every_section_has_docs() {
        for s in SECTIONS {
            assert!(!s.docs.is_empty(), "parser::{}::DOCS is empty", s.module);
        }
    }

    #[test]
    fn doc_ids_are_unique() {
        let mut seen = BTreeSet::new();
        for doc in SECTIONS.iter().flat_map(|s| s.docs.iter()) {
            assert!(seen.insert(doc.id), "duplicate SyntaxDoc id: {}", doc.id);
        }
    }

    #[test]
    fn rendered_tutorial_ends_with_single_newline() {
        for lang in [Lang::Ja, Lang::En] {
            let text = render(lang);
            assert!(text.ends_with('\n'));
            assert!(!text.ends_with("\n\n"), "trailing blank line");
        }
    }
}
