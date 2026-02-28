use crate::ast::Block;

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

/// ソーステキストをSpan付きでパース
pub fn span_parse_source(_source: &str) -> ParseOutcome {
    todo!()
}
