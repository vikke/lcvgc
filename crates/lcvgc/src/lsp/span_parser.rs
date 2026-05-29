//! スパン情報付きパーサーモジュール
//! Span-aware parser module
//!
//! ソーステキストを位置情報（スパン）付きでパースし、
//! LSP機能に必要なブロック位置情報を提供する。
//! Parses source text with position information (spans),
//! providing block location data needed by LSP features.

use crate::ast::Block;
use crate::parser::parse_block;

/// ネストされたブロックコメント(`/* ... */`)をスキップし、残りの入力を返す
/// Skip a nested block comment (`/* ... */`) and return the remaining input.
///
/// Supports arbitrary nesting (e.g. `/* outer /* inner */ outer */`).
///
/// # Arguments
/// * `input` - Input string starting with `/*`
///
/// # Returns
/// - `Some(remaining)` if the comment was properly closed
/// - `None` if the comment is unclosed
fn skip_block_comment(input: &str) -> Option<&str> {
    let mut remaining = &input[2..]; // skip opening `/*`
    let mut depth: u32 = 1;
    while depth > 0 {
        let open = remaining.find("/*");
        let close = remaining.find("*/")?;
        match open {
            Some(o) if o < close => {
                depth += 1;
                remaining = &remaining[o + 2..];
            }
            _ => {
                depth -= 1;
                remaining = &remaining[close + 2..];
            }
        }
    }
    Some(remaining)
}

/// ソース内のバイトオフセット範囲
/// Byte offset range within the source text
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// 開始バイトオフセット
    /// Start byte offset
    pub start: usize,
    /// 終了バイトオフセット
    /// End byte offset
    pub end: usize,
}

/// スパン付きブロック
/// Block with span information
#[derive(Debug, Clone)]
pub struct SpannedBlock {
    /// パース済みブロック
    /// Parsed block
    pub block: Block,
    /// ブロック全体のスパン
    /// Span covering the entire block
    pub span: Span,
    /// ブロック名のスパン（名前付きブロックのみ）
    /// Span of the block name (only for named blocks)
    pub name_span: Option<Span>,
}

/// パースエラー（位置付き）
/// Parse error with position information
#[derive(Debug, Clone)]
pub struct SpanError {
    /// エラー発生箇所のスパン
    /// Span of the error location
    pub span: Span,
    /// エラーメッセージ
    /// Error message
    pub message: String,
}

/// パース結果
/// Parse outcome containing blocks and errors
pub struct ParseOutcome {
    /// パース成功したブロック一覧
    /// List of successfully parsed blocks
    pub blocks: Vec<SpannedBlock>,
    /// パースエラー一覧
    /// List of parse errors
    pub errors: Vec<SpanError>,
}

/// ブロック名を取得する
/// Retrieves the name of a block, if it has one
fn block_name(block: &Block) -> Option<&str> {
    match block {
        Block::Device(d) => Some(&d.name),
        Block::Instrument(i) => Some(&i.name),
        Block::Kit(k) => Some(&k.name),
        Block::Clip(c) => Some(&c.name),
        Block::Scene(s) => Some(&s.name),
        Block::Session(s) => Some(&s.name),
        Block::Var(v) => Some(&v.name),
        _ => None,
    }
}

/// 既知キーワード一覧（エラー回復用）
/// Known keywords for error recovery
const KEYWORDS: &[&str] = &[
    "device ",
    "instrument ",
    "kit ",
    "clip ",
    "scene ",
    "session ",
    "tempo ",
    "scale ",
    "var ",
    "include ",
    "play ",
    "stop",
];

/// 次のキーワードの開始位置を探す（エラー回復用）
/// Finds the start position of the next keyword (for error recovery)
fn find_next_keyword(source: &str) -> Option<usize> {
    for (i, _) in source.char_indices() {
        if i == 0 {
            continue;
        }
        // Check if position i is at start of a line
        if source.as_bytes()[i - 1] == b'\n' {
            let rest = &source[i..];
            let trimmed = rest.trim_start();
            let trim_offset = rest.len() - trimmed.len();
            for kw in KEYWORDS {
                if trimmed.starts_with(kw) {
                    return Some(i + trim_offset);
                }
            }
        }
    }
    None
}

/// 文字列を表示用に「先頭1行・最大 max 文字」へ切り詰める。
/// 改行で打ち切り、長い場合は末尾に省略記号 `…` を付ける。
/// マルチバイト境界で切らないよう `char_indices` で安全に分割する。
///
/// Truncate a string for display to its first line and at most `max` chars,
/// appending `…` when truncated. Splits on a char boundary to stay UTF-8 safe.
///
/// # Arguments
/// * `s` - 切り詰め対象の文字列 / Source string
/// * `max` - 最大文字数（char 単位）/ Maximum length in chars
///
/// # Returns
/// 表示用に整えた断片文字列 / A snippet suitable for display
fn snippet_for_display(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or("").trim();
    let mut out = String::new();
    for (count, (_, ch)) in first_line.char_indices().enumerate() {
        if count >= max {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

/// パース失敗時の簡潔なエラーメッセージを組み立てる。
///
/// nom デフォルト Error の `Display` は失敗位置以降のソース全文を
/// 埋め込むため、そのまま診断メッセージにすると巨大で読めなくなる。
/// 本関数は (1) ブロック先頭から推定したブロック種別と、
/// (2) 失敗位置付近の短い断片のみを使い、簡潔な日本語文言を作る。
///
/// Builds a concise parse-error message. nom's default Error `Display` embeds
/// the entire remaining source, which makes diagnostics unreadable. This uses
/// only (1) the block kind inferred from the block's leading keyword and
/// (2) a short snippet around the failure point.
///
/// # Arguments
/// * `block_input` - `parse_block` に渡したブロック先頭の入力（種別推定用）
///   / The block's leading input passed to `parse_block` (for kind inference)
/// * `err` - nom が返したパースエラー / The parse error returned by nom
///
/// # Returns
/// 診断表示用の簡潔なエラーメッセージ / A concise error message for diagnostics
fn describe_parse_error(block_input: &str, err: &nom::Err<nom::error::Error<&str>>) -> String {
    // ブロック種別判定用のキーワード一覧。`parse_block` の分岐に揃える。
    // 回復用の `KEYWORDS` とは用途が異なるため別途定義する。
    // longest-match のため、接頭辞が衝突しない範囲で語順は問わない。
    const BLOCK_KEYWORDS: &[&str] = &[
        "device", "instrument", "kit", "clip", "scene", "session", "tempo", "scale", "var",
        "include", "play", "stop", "pause", "resume", "unmute", "mute",
    ];
    // ブロック種別を先頭キーワードから推定する（消費済みでも block_input には残っている）。
    let head = block_input.trim_start();
    let kind = BLOCK_KEYWORDS
        .iter()
        .find(|kw| head.starts_with(**kw))
        .copied();

    // 失敗位置付近の断片を取り出す。nom Error が指す input を優先し、
    // 取れない場合（Incomplete 等）はブロック先頭を使う。
    let failure_input = match err {
        nom::Err::Error(e) | nom::Err::Failure(e) => e.input,
        nom::Err::Incomplete(_) => block_input,
    };
    let snippet = snippet_for_display(failure_input, 40);

    match kind {
        Some(k) => format!("{k} ブロックの構文エラー: '{snippet}' 付近で解釈に失敗しました"),
        None => format!("構文エラー: '{snippet}' を解釈できませんでした"),
    }
}

/// ソーステキストをスパン付きでパースする
/// Parses source text with span information
///
/// コメントをスキップしつつブロックを順次パースし、
/// エラー発生時は次のキーワードまでスキップして回復を試みる。
/// Parses blocks sequentially while skipping comments,
/// and attempts recovery by skipping to the next keyword on error.
///
/// # Arguments
/// * `source` - パース対象のソーステキスト / Source text to parse
///
/// # Returns
/// パース結果（成功ブロックとエラーの両方を含む）
/// Parse outcome containing both successful blocks and errors
pub fn span_parse_source(source: &str) -> ParseOutcome {
    let mut blocks = Vec::new();
    let mut errors = Vec::new();
    let original = source;
    let mut remaining = source;

    loop {
        // Skip whitespace and comments (line `//` and block `/* */`)
        remaining = remaining.trim_start();
        loop {
            if remaining.starts_with("//") {
                // Line comment: skip to end of line
                if let Some(nl) = remaining.find('\n') {
                    remaining = &remaining[nl + 1..];
                } else {
                    remaining = "";
                }
                remaining = remaining.trim_start();
            } else if remaining.starts_with("/*") {
                // Block comment: skip with nesting support
                if let Some(end) = skip_block_comment(remaining) {
                    remaining = end;
                } else {
                    // Unclosed block comment: treat rest as comment
                    remaining = "";
                }
                remaining = remaining.trim_start();
            } else {
                break;
            }
        }

        if remaining.is_empty() {
            break;
        }

        let start = original.len() - remaining.len();

        match parse_block(remaining) {
            Ok((rest, block)) => {
                let end = original.len() - rest.len();
                let span = Span { start, end };

                let name_span = block_name(&block).and_then(|name| {
                    let region = &original[start..end];
                    region.find(name).map(|pos| Span {
                        start: start + pos,
                        end: start + pos + name.len(),
                    })
                });

                blocks.push(SpannedBlock {
                    block,
                    span,
                    name_span,
                });
                remaining = rest;
            }
            Err(e) => {
                let err_msg = describe_parse_error(remaining, &e);
                // Try to skip to next keyword
                match find_next_keyword(remaining) {
                    Some(skip_to) => {
                        let error_end = start + skip_to;
                        errors.push(SpanError {
                            span: Span {
                                start,
                                end: error_end,
                            },
                            message: err_msg,
                        });
                        remaining = &original[error_end..];
                    }
                    None => {
                        // No recovery possible, record error for rest of source
                        errors.push(SpanError {
                            span: Span {
                                start,
                                end: original.len(),
                            },
                            message: err_msg,
                        });
                        break;
                    }
                }
            }
        }
    }

    ParseOutcome { blocks, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source() {
        let out = span_parse_source("");
        assert!(out.blocks.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn single_tempo() {
        let src = "tempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
        let b = &out.blocks[0];
        assert_eq!(b.span.start, 0);
        assert_eq!(b.span.end, 9);
        assert!(matches!(b.block, Block::Tempo(_)));
    }

    #[test]
    fn device_block_with_name_span() {
        let src = "device my_synth {\n  port \"IAC\"\n}";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        let b = &out.blocks[0];
        assert!(matches!(b.block, Block::Device(_)));
        let ns = b.name_span.unwrap();
        assert_eq!(&src[ns.start..ns.end], "my_synth");
    }

    #[test]
    fn multiple_blocks() {
        let src = "tempo 120\n\ntempo 140";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 2);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let out = span_parse_source("   \n\n  \t  ");
        assert!(out.blocks.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn leading_trailing_whitespace() {
        let src = "  \n  tempo 120  \n  ";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        let b = &out.blocks[0];
        // span should start at the 't' of tempo, not at leading whitespace
        assert_eq!(&src[b.span.start..b.span.start + 5], "tempo");
    }

    #[test]
    fn clip_name_span() {
        let src = "clip bass_a [bars 1] {\n  piano c:4:4\n}";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        let b = &out.blocks[0];
        assert!(matches!(b.block, Block::Clip(_)));
        let ns = b.name_span.unwrap();
        assert_eq!(&src[ns.start..ns.end], "bass_a");
    }

    #[test]
    fn scene_name_span() {
        let src = "scene intro {\n  bass_a\n}";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        let b = &out.blocks[0];
        assert!(matches!(b.block, Block::Scene(_)));
        let ns = b.name_span.unwrap();
        assert_eq!(&src[ns.start..ns.end], "intro");
    }

    #[test]
    fn tempo_has_no_name_span() {
        let src = "tempo 120";
        let out = span_parse_source(src);
        assert!(out.blocks[0].name_span.is_none());
    }

    #[test]
    fn error_with_recovery() {
        let src = "INVALID STUFF\ntempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(out.errors.len(), 1);
        assert!(matches!(out.blocks[0].block, Block::Tempo(_)));
    }

    #[test]
    fn error_no_recovery() {
        let src = "INVALID STUFF";
        let out = span_parse_source(src);
        assert!(out.blocks.is_empty());
        assert_eq!(out.errors.len(), 1);
        assert_eq!(out.errors[0].span.start, 0);
        assert_eq!(out.errors[0].span.end, src.len());
    }

    // --- エラーメッセージ整形 (案2) ---
    // --- Error message formatting (approach B-light) ---

    /// パースエラーのメッセージに、失敗位置以降のソース「全文」が
    /// 混入していないことを検証する（再発防止）。
    /// nom のデフォルト Error の Display は残りソース全文を埋め込むため、
    /// これを直接使わず簡潔な文言にしていることを保証する。
    ///
    /// Verify that a parse-error message does NOT embed the entire remaining
    /// source after the failure point (regression guard). nom's default Error
    /// `Display` would otherwise dump the full remaining input.
    #[test]
    fn error_message_excludes_full_source() {
        // clip ヘッダで `[bar 1]` (正しくは `[bars 1]`) と書き間違えたケース。
        // clip option を解釈できず `{` 位置で失敗し、残り入力に長い本文が続く。
        let src = "clip tuning [bar 1] {\n\tfm c:1:1\n\tbass c:1:1\n\tlead c:1:1\n\tpad c:1:1\n}";
        let out = span_parse_source(src);
        assert_eq!(out.errors.len(), 1, "1 件のエラーになるはず");
        let msg = &out.errors[0].message;

        // 本文の後続行 (例: `bass c:1:1`) がメッセージに含まれていないこと。
        assert!(
            !msg.contains("bass c:1:1"),
            "メッセージに後続ソース行が混入している: {msg}"
        );
        // nom の Debug 表現 (`Error {{ input:`) が露出していないこと。
        assert!(
            !msg.contains("Error { input"),
            "nom の Debug 表現が露出している: {msg}"
        );
        // メッセージが過度に長くないこと。
        assert!(
            msg.len() < 200,
            "メッセージが長すぎる ({} bytes): {msg}",
            msg.len()
        );
    }

    /// パースエラーのメッセージに、失敗したブロック種別 (例: clip) と
    /// 失敗位置付近の断片が含まれ、ユーザーが原因箇所を推察できることを検証する。
    ///
    /// Verify the message names the failing block kind (e.g. clip) and shows a
    /// short snippet around the failure point, so the user can locate the cause.
    #[test]
    fn error_message_includes_block_kind_and_snippet() {
        let src = "clip tuning [bar 1] {\n\tfm c:1:1\n}";
        let out = span_parse_source(src);
        assert_eq!(out.errors.len(), 1);
        let msg = &out.errors[0].message;

        // 失敗したブロック種別が分かること。
        assert!(
            msg.contains("clip"),
            "ブロック種別 clip が含まれない: {msg}"
        );
        // 失敗位置付近の断片 (`[bar 1]`) が含まれ、原因を推察できること。
        assert!(msg.contains("[bar 1]"), "失敗位置の断片が含まれない: {msg}");
    }

    /// 先頭キーワードに一致しない不明な入力では、ブロック種別を
    /// 特定できないため「構文エラー」系の汎用文言になることを検証する。
    ///
    /// For input that matches no leading keyword, the message falls back to a
    /// generic "syntax error" form since the block kind is unknown.
    #[test]
    fn error_message_generic_for_unknown_input() {
        let src = "INVALID STUFF";
        let out = span_parse_source(src);
        assert_eq!(out.errors.len(), 1);
        let msg = &out.errors[0].message;
        assert!(msg.contains("構文エラー"), "汎用文言になっていない: {msg}");
        assert!(!msg.contains("Error { input"), "Debug 表現が露出: {msg}");
    }

    #[test]
    fn span_covers_full_block_text() {
        let src = "device synth {\n  port \"IAC\"\n}";
        let out = span_parse_source(src);
        let b = &out.blocks[0];
        let block_text = &src[b.span.start..b.span.end];
        assert!(block_text.starts_with("device"));
        assert!(block_text.ends_with("}"));
    }

    #[test]
    fn comment_lines_skipped() {
        let src = "// comment\ntempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn block_comment_skipped() {
        let src = "/* block comment */tempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn block_comment_multiline_skipped() {
        let src = "/* line1\nline2\nline3 */\ntempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn nested_block_comment_skipped() {
        let src = "/* outer /* inner */ outer */\ntempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn mixed_comments_skipped() {
        let src = "// line comment\n/* block */\ntempo 120";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
    }

    /// CCタイム形式オートメーションを含むクリップのパーステスト
    /// Test span_parse_source with a clip containing CC time-format automation
    #[test]
    fn clip_with_cc_time_automation() {
        let src = "clip bass_a [bars 4] {\n  bass c:3:4\n  vbass.cutoff 40@1.1-100@4.4\n}";
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
        match &out.blocks[0].block {
            Block::Clip(clip) => {
                assert_eq!(clip.name, "bass_a");
                match &clip.body {
                    crate::ast::clip::ClipBody::Pitched(body) => {
                        assert_eq!(body.cc_automations.len(), 1);
                    }
                    _ => panic!("expected pitched"),
                }
            }
            _ => panic!("expected clip"),
        }
    }

    /// 複数CCパターン（タイム形式+ステップ形式）を含むクリップのパーステスト
    /// Test span_parse_source with a clip containing multiple CC patterns (time + step)
    #[test]
    fn clip_with_multiple_cc_patterns() {
        let src = r#"clip bass_a [bars 4] {
  bass c:3:4 eb::4
  vbass.cutoff 40@1.1-100@4.4
  vbass.resonance 0 10 20 30
}"#;
        let out = span_parse_source(src);
        assert_eq!(out.blocks.len(), 1);
        assert!(out.errors.is_empty());
        match &out.blocks[0].block {
            Block::Clip(clip) => match &clip.body {
                crate::ast::clip::ClipBody::Pitched(body) => {
                    assert_eq!(body.lines.len(), 1);
                    assert_eq!(body.cc_automations.len(), 2);
                }
                _ => panic!("expected pitched"),
            },
            _ => panic!("expected clip"),
        }
    }
}
