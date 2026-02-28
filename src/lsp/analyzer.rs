use crate::engine::registry::Registry;
use crate::lsp::span_parser::{SpanError, SpannedBlock};

/// LSP用ドキュメント解析器
pub struct LspAnalyzer {
    pub registry: Registry,
    pub spanned_blocks: Vec<SpannedBlock>,
    pub errors: Vec<SpanError>,
    source: String,
}

impl LspAnalyzer {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            spanned_blocks: Vec::new(),
            errors: Vec::new(),
            source: String::new(),
        }
    }

    /// ソース更新・再解析
    pub fn update(&mut self, _new_source: String) {
        todo!()
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// 指定オフセットのブロックを取得
    pub fn block_at_offset(&self, _offset: usize) -> Option<&SpannedBlock> {
        todo!()
    }
}
