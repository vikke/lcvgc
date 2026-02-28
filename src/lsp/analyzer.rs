use crate::engine::registry::Registry;
use crate::lsp::span_parser::{span_parse_source, SpanError, SpannedBlock};

/// LSP用ドキュメント解析器
pub struct LspAnalyzer {
    registry: Registry,
    spanned_blocks: Vec<SpannedBlock>,
    errors: Vec<SpanError>,
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
    pub fn update(&mut self, new_source: String) {
        self.source = new_source;
        self.registry = Registry::new();
        self.spanned_blocks.clear();
        self.errors.clear();

        let outcome = span_parse_source(&self.source);
        for sb in &outcome.blocks {
            self.registry.register_block(sb.block.clone());
        }
        self.spanned_blocks = outcome.blocks;
        self.errors = outcome.errors;
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn blocks(&self) -> &[SpannedBlock] {
        &self.spanned_blocks
    }

    pub fn errors(&self) -> &[SpanError] {
        &self.errors
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// 指定オフセットのブロックを取得
    pub fn block_at_offset(&self, offset: usize) -> Option<&SpannedBlock> {
        self.spanned_blocks
            .iter()
            .find(|sb| offset >= sb.span.start && offset < sb.span.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Block;

    #[test]
    fn new_is_empty() {
        let a = LspAnalyzer::new();
        assert!(a.blocks().is_empty());
        assert!(a.errors().is_empty());
        assert!(a.source().is_empty());
    }

    #[test]
    fn update_with_tempo() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120".into());
        assert_eq!(a.blocks().len(), 1);
        assert!(a.registry().tempo().is_some());
    }

    #[test]
    fn update_with_device() {
        let mut a = LspAnalyzer::new();
        a.update("device synth {\n  port \"IAC\"\n}".into());
        assert_eq!(a.blocks().len(), 1);
        assert!(a.registry().get_device("synth").is_some());
    }

    #[test]
    fn update_replaces_previous() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120".into());
        assert_eq!(a.blocks().len(), 1);
        a.update("tempo 140\ntempo 160".into());
        assert_eq!(a.blocks().len(), 2);
    }

    #[test]
    fn block_at_offset_middle() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120".into());
        let b = a.block_at_offset(4).unwrap();
        assert!(matches!(b.block, Block::Tempo(_)));
    }

    #[test]
    fn block_at_offset_before_first() {
        let mut a = LspAnalyzer::new();
        a.update("  tempo 120".into());
        assert!(a.block_at_offset(0).is_none());
    }

    #[test]
    fn block_at_offset_after_last() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120  ".into());
        assert!(a.block_at_offset(10).is_none());
    }

    #[test]
    fn source_returns_current() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120".into());
        assert_eq!(a.source(), "tempo 120");
    }

    #[test]
    fn blocks_length_matches() {
        let mut a = LspAnalyzer::new();
        a.update("tempo 120\ntempo 140".into());
        assert_eq!(a.blocks().len(), 2);
    }

    #[test]
    fn errors_from_invalid() {
        let mut a = LspAnalyzer::new();
        a.update("GARBAGE".into());
        assert!(a.blocks().is_empty());
        assert!(!a.errors().is_empty());
    }

    #[test]
    fn update_clears_errors() {
        let mut a = LspAnalyzer::new();
        a.update("GARBAGE".into());
        assert!(!a.errors().is_empty());
        a.update("tempo 120".into());
        assert!(a.errors().is_empty());
    }

    #[test]
    fn registry_has_var() {
        let mut a = LspAnalyzer::new();
        a.update("var bpm = 120".into());
        assert_eq!(a.registry().get_var("bpm"), Some("120"));
    }
}
