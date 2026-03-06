use crate::ast::Block;
use crate::parser::parse_block;

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Span付きBlock
#[derive(Debug, Clone)]
pub struct SpannedBlock {
    pub block: Block,
    pub span: Span,
    pub name_span: Option<Span>,
}

/// パースエラー（位置付き）
#[derive(Debug, Clone)]
pub struct SpanError {
    pub span: Span,
    pub message: String,
}

/// パース結果
pub struct ParseOutcome {
    pub blocks: Vec<SpannedBlock>,
    pub errors: Vec<SpanError>,
}

/// ブロック名を取得
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

/// 既知キーワードで始まる行を探してスキップ
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

/// ソーステキストをSpan付きでパース
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
                let err_msg = format!("{}", e);
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
}
