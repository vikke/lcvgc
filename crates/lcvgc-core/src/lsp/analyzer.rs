//! LSPドキュメント解析モジュール
//!
//! エディタで開かれたドキュメントのソースコードを解析し、
//! ブロック情報・エラー情報・レジストリを管理する。

use super::span_parser::{span_parse_source, SpanError, SpannedBlock};
use crate::engine::registry::Registry;
use crate::server::protocol::IncludeSource;

/// LSP用ドキュメント解析器
///
/// ソースコードを受け取り、スパン付きブロック・パースエラー・
/// レジストリ（名前解決用）を保持する。
pub struct LspAnalyzer {
    /// ブロック名前解決用レジストリ
    registry: Registry,
    /// Evaluator由来のベースレジストリ（update時にリセットされない）
    /// Base registry from Evaluator (not reset on update)
    base_registry: Option<Registry>,
    /// スパン情報付きの解析済みブロック一覧
    spanned_blocks: Vec<SpannedBlock>,
    /// パース時に発生したエラー一覧
    errors: Vec<SpanError>,
    /// 現在のソーステキスト
    source: String,
}

impl Default for LspAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LspAnalyzer {
    /// 空の解析器を生成する
    ///
    /// # Returns
    /// 初期状態（空のレジストリ・ブロック・エラー・ソース）の `LspAnalyzer`
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            base_registry: None,
            spanned_blocks: Vec::new(),
            errors: Vec::new(),
            source: String::new(),
        }
    }

    /// 既存のレジストリをベースにした解析器を生成する
    /// Creates an analyzer with an existing registry as the base
    ///
    /// Evaluator が保持する registry を clone して渡すことで、
    /// eval 済みの定義（instrument, kit, device 等）を LSP 機能で参照可能にする。
    /// By cloning and passing the registry held by the Evaluator,
    /// eval'd definitions (instrument, kit, device, etc.) become available to LSP features.
    ///
    /// # Arguments
    /// * `base_registry` - ベースとなるレジストリ / Base registry to start from
    pub fn with_base_registry(base_registry: Registry) -> Self {
        Self {
            registry: base_registry.clone(),
            base_registry: Some(base_registry),
            spanned_blocks: Vec::new(),
            errors: Vec::new(),
            source: String::new(),
        }
    }

    /// ソースを更新し、再解析を実行する
    ///
    /// 内部状態（レジストリ・ブロック・エラー）をすべてクリアした上で、
    /// 新しいソースを解析して結果を保持する。
    ///
    /// # Arguments
    /// * `new_source` - 新しいソーステキスト
    pub fn update(&mut self, new_source: String) {
        self.source = new_source;
        // ベースレジストリがある場合はそこから復元、なければ空で初期化
        // Restore from base registry if available, otherwise initialize empty
        self.registry = self.base_registry.as_ref().cloned().unwrap_or_default();
        self.spanned_blocks.clear();
        self.errors.clear();

        let outcome = span_parse_source(&self.source);
        for sb in &outcome.blocks {
            self.registry.register_block(sb.block.clone());
        }
        self.spanned_blocks = outcome.blocks;
        self.errors = outcome.errors;
    }

    /// 現在のソーステキストを返す
    ///
    /// # Returns
    /// ソーステキストの参照
    pub fn source(&self) -> &str {
        &self.source
    }

    /// 解析済みのスパン付きブロック一覧を返す
    ///
    /// # Returns
    /// `SpannedBlock` のスライス
    pub fn blocks(&self) -> &[SpannedBlock] {
        &self.spanned_blocks
    }

    /// パース時に発生したエラー一覧を返す
    ///
    /// # Returns
    /// `SpanError` のスライス
    pub fn errors(&self) -> &[SpanError] {
        &self.errors
    }

    /// レジストリ（名前解決用）を返す
    ///
    /// # Returns
    /// `Registry` の参照
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// 指定バイトオフセットを含むブロックを取得する
    ///
    /// # Arguments
    /// * `offset` - ソース内のバイトオフセット
    ///
    /// # Returns
    /// オフセットがブロック範囲内にある場合は `Some(&SpannedBlock)`、
    /// どのブロックにも含まれない場合は `None`
    pub fn block_at_offset(&self, offset: usize) -> Option<&SpannedBlock> {
        self.spanned_blocks
            .iter()
            .find(|sb| offset >= sb.span.start && offset < sb.span.end)
    }

    /// Lua側から受け取ったinclude sourcesを使ってレジストリを更新する
    /// Updates registry using include sources received from Lua side
    ///
    /// # Arguments
    /// * `new_source` - メインファイルのソーステキスト / Main file source text
    /// * `include_sources` - インクルードファイルのソース情報リスト / List of include file source info
    pub fn update_with_include_sources(
        &mut self,
        new_source: String,
        include_sources: &[IncludeSource],
    ) {
        // まず通常のupdateを実行
        // First perform the normal update
        self.update(new_source);

        // include_sourcesを順にパースしてregistryに登録
        // Parse include_sources in order and register in registry
        for inc_src in include_sources {
            let outcome = span_parse_source(&inc_src.source);
            for sb in &outcome.blocks {
                self.registry.register_block(sb.block.clone());
            }
        }
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

    /// include_sourcesでクリップ定義がregistryに登録されることを検証
    /// Verifies that clip definitions from include_sources are registered in the registry
    #[test]
    fn update_with_include_sources_resolves_clip() {
        let mut a = LspAnalyzer::new();
        let include_sources = vec![IncludeSource {
            path: "bass.cvg".into(),
            source: "clip bass {\n  c4\n}".into(),
        }];
        a.update_with_include_sources("include bass.cvg\ntempo 120".into(), &include_sources);
        assert!(a.registry().get_clip("bass").is_some());
        assert!(a.registry().tempo().is_some());
    }

    /// 複数のinclude_sourcesが全てregistryに登録されることを検証
    /// Verifies that multiple include_sources are all registered in the registry
    #[test]
    fn update_with_include_sources_multiple() {
        let mut a = LspAnalyzer::new();
        let include_sources = vec![
            IncludeSource {
                path: "bass.cvg".into(),
                source: "clip bass {\n  c4\n}".into(),
            },
            IncludeSource {
                path: "drums.cvg".into(),
                source: "clip drums {\n  c4\n}".into(),
            },
        ];
        a.update_with_include_sources("tempo 120".into(), &include_sources);
        assert!(a.registry().get_clip("bass").is_some());
        assert!(a.registry().get_clip("drums").is_some());
    }

    /// 空のinclude_sourcesでも正常動作することを検証
    /// Verifies that empty include_sources works correctly
    #[test]
    fn update_with_include_sources_empty() {
        let mut a = LspAnalyzer::new();
        a.update_with_include_sources("tempo 120".into(), &[]);
        assert!(a.registry().tempo().is_some());
    }

    /// ベースレジストリの定義がupdate後も参照可能であることを検証
    /// Verifies that base registry definitions remain accessible after update
    #[test]
    fn with_base_registry_preserves_definitions() {
        // Evaluator相当のregistryを構築
        // Build a registry equivalent to what Evaluator would hold
        let mut base = Registry::new();
        let outcome = span_parse_source(
            "instrument vbass {\n  device volca_bass\n  channel 1\n}\ndevice volca_bass {\n  port test\n}",
        );
        for sb in &outcome.blocks {
            base.register_block(sb.block.clone());
        }

        let mut a = LspAnalyzer::with_base_registry(base);
        // 子ファイルのソースのみでupdate（instrumentの定義なし）
        // Update with child file source only (no instrument definition)
        a.update("clip bass_main [bars 4] {\n  vbass c:2:16 c c c\n}".into());

        // ベースのinstrument定義が残っている
        // Base instrument definition is preserved
        assert!(a.registry().get_instrument("vbass").is_some());
        assert!(a.registry().get_device("volca_bass").is_some());
        // 子ファイルのclip定義も登録されている
        // Child file's clip definition is also registered
        assert!(a.registry().get_clip("bass_main").is_some());
    }

    /// ベースレジストリなしの場合、従来通り動作することを検証
    /// Verifies that without base registry, behavior is unchanged
    #[test]
    fn without_base_registry_update_resets() {
        let mut a = LspAnalyzer::new();
        a.update("instrument vbass {\n  device d\n  channel 1\n}".into());
        assert!(a.registry().get_instrument("vbass").is_some());
        // 別のソースでupdate → 以前の定義は消える
        // Update with different source -> previous definitions are cleared
        a.update("tempo 120".into());
        assert!(a.registry().get_instrument("vbass").is_none());
        assert!(a.registry().tempo().is_some());
    }

    /// ベースレジストリの定義がソースの定義で上書きされることを検証
    /// Verifies that base registry definitions are overridden by source definitions
    #[test]
    fn with_base_registry_source_overrides_base() {
        let mut base = Registry::new();
        let outcome = span_parse_source("tempo 120");
        for sb in &outcome.blocks {
            base.register_block(sb.block.clone());
        }

        let mut a = LspAnalyzer::with_base_registry(base);
        a.update("tempo 140".into());

        // ソースの定義（140）がベース（120）を上書きする
        // Source definition (140) overrides base (120)
        let tempo = a.registry().tempo().unwrap();
        assert_eq!(*tempo, crate::ast::tempo::Tempo::Absolute(140));
    }
}
