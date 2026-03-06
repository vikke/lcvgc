//! LSPドキュメント解析モジュール
//!
//! エディタで開かれたドキュメントのソースコードを解析し、
//! ブロック情報・エラー情報・レジストリを管理する。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::span_parser::{span_parse_source, SpanError, SpannedBlock};
use crate::ast::Block;
use crate::engine::registry::Registry;

/// LSP用ドキュメント解析器
///
/// ソースコードを受け取り、スパン付きブロック・パースエラー・
/// レジストリ（名前解決用）を保持する。
pub struct LspAnalyzer {
    /// ブロック名前解決用レジストリ
    registry: Registry,
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

    /// ソースを更新し、ファイルパスを基にinclude先も再帰的に解決する
    /// Updates source and recursively resolves includes based on file path
    ///
    /// # Arguments
    /// * `new_source` - 新しいソーステキスト / New source text
    /// * `file_path` - メインファイルのパス（include相対パス解決用） / Main file path (for resolving relative include paths)
    pub fn update_with_file_path(&mut self, new_source: String, file_path: &Path) {
        // まず通常のupdateを実行
        // First perform the normal update
        self.update(new_source);

        // include先を再帰的に解決してregistryに登録
        // Recursively resolve includes and register them in the registry
        let base_dir = file_path.parent().unwrap_or(Path::new("."));
        let mut visited = HashSet::new();
        if let Ok(canonical) = file_path.canonicalize() {
            visited.insert(canonical);
        }
        self.resolve_includes_recursive(base_dir, &self.spanned_blocks.clone(), &mut visited);
    }

    /// include先ファイルを再帰的に解決し、registryにブロックを登録する（内部メソッド）
    /// Recursively resolves included files and registers blocks in the registry (internal method)
    ///
    /// includeはファイル先頭にのみ許可される。非includeブロックが出現した時点で
    /// includeフェーズを終了する。
    /// Includes are only allowed at the top of the file. The include phase ends
    /// when a non-include block is encountered.
    ///
    /// # Arguments
    /// * `base_dir` - includeパス解決のベースディレクトリ / Base directory for resolving include paths
    /// * `blocks` - パース済みのスパン付きブロック / Parsed spanned blocks
    /// * `visited` - 循環検出用の訪問済みパスセット / Set of visited paths for cycle detection
    fn resolve_includes_recursive(
        &mut self,
        base_dir: &Path,
        blocks: &[SpannedBlock],
        visited: &mut HashSet<PathBuf>,
    ) {
        for sb in blocks {
            if let Block::Include(ref inc) = sb.block {
                let include_path = base_dir.join(&inc.path);

                // ファイルの正規化パスを取得（失敗時はスキップ）
                // Get canonical path (skip on failure)
                let canonical = match include_path.canonicalize() {
                    Ok(c) => c,
                    Err(_) => continue, // ファイル未検出時は静かにスキップ
                };

                // 循環検出: 既に訪問済みならスキップ
                // Cycle detection: skip if already visited
                if !visited.insert(canonical.clone()) {
                    continue;
                }

                // ファイル読み込み（失敗時はスキップ）
                // Read file (skip on failure)
                let source = match std::fs::read_to_string(&canonical) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // パースしてregistryに登録
                // Parse and register in registry
                let outcome = span_parse_source(&source);
                for child_sb in &outcome.blocks {
                    self.registry.register_block(child_sb.block.clone());
                }

                // include先のincludeも再帰的に解決（先頭のみ）
                // Recursively resolve includes in included files (top only)
                let child_base = canonical.parent().unwrap_or(Path::new("."));
                self.resolve_includes_recursive(child_base, &outcome.blocks, visited);
            } else {
                // 非includeブロックが出現したらincludeフェーズ終了
                // Non-include block encountered, end include phase
                break;
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

    /// include先のクリップ定義がregistryに登録されることを検証
    /// Verifies that clip definitions from included files are registered in the registry
    #[test]
    fn update_with_file_path_resolves_include() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        // include先ファイル: クリップ定義
        let inc_path = dir.path().join("bass.cvg");
        let mut f = std::fs::File::create(&inc_path).unwrap();
        writeln!(f, "clip bass {{\n  c4\n}}").unwrap();

        // メインファイル: include + tempo
        let main_path = dir.path().join("main.cvg");
        let main_source = "include bass.cvg\ntempo 120";

        let mut a = LspAnalyzer::new();
        a.update_with_file_path(main_source.into(), &main_path);

        // include先のクリップがregistryに登録されている
        assert!(a.registry().get_clip("bass").is_some());
        // メインファイルのtempoも登録されている
        assert!(a.registry().tempo().is_some());
    }

    /// ネストされたinclude解決を検証
    /// Verifies nested include resolution
    #[test]
    fn update_with_file_path_nested_include() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        // 2段目のinclude
        let deep_path = dir.path().join("deep.cvg");
        let mut f = std::fs::File::create(&deep_path).unwrap();
        writeln!(f, "clip deep {{\n  e4\n}}").unwrap();

        // 1段目のinclude
        let mid_path = dir.path().join("mid.cvg");
        let mut f = std::fs::File::create(&mid_path).unwrap();
        writeln!(f, "include deep.cvg\nclip mid {{\n  d4\n}}").unwrap();

        // メインファイル
        let main_path = dir.path().join("main.cvg");
        let main_source = "include mid.cvg\ntempo 120";

        let mut a = LspAnalyzer::new();
        a.update_with_file_path(main_source.into(), &main_path);

        assert!(a.registry().get_clip("deep").is_some());
        assert!(a.registry().get_clip("mid").is_some());
        assert!(a.registry().tempo().is_some());
    }

    /// 循環includeが無限ループにならないことを検証
    /// Verifies that circular includes do not cause infinite loops
    #[test]
    fn update_with_file_path_circular_include() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        let a_path = dir.path().join("a.cvg");
        let b_path = dir.path().join("b.cvg");

        // a.cvg includes b.cvg, b.cvg includes a.cvg
        let mut f = std::fs::File::create(&a_path).unwrap();
        writeln!(f, "include b.cvg\nclip a {{\n  c4\n}}").unwrap();
        let mut f = std::fs::File::create(&b_path).unwrap();
        writeln!(f, "include a.cvg\nclip b {{\n  d4\n}}").unwrap();

        let main_source = std::fs::read_to_string(&a_path).unwrap();
        let mut analyzer = LspAnalyzer::new();
        analyzer.update_with_file_path(main_source, &a_path);

        // 循環してもパニックしない。両方のクリップが登録される
        assert!(analyzer.registry().get_clip("a").is_some());
        assert!(analyzer.registry().get_clip("b").is_some());
    }

    /// 存在しないincludeファイルを静かにスキップすることを検証
    /// Verifies that missing include files are silently skipped
    #[test]
    fn update_with_file_path_missing_include() {
        let dir = tempfile::tempdir().unwrap();
        let main_path = dir.path().join("main.cvg");
        let main_source = "include nonexistent.cvg\ntempo 120";

        let mut a = LspAnalyzer::new();
        a.update_with_file_path(main_source.into(), &main_path);

        // 存在しないincludeをスキップし、tempoは正常に登録される
        assert!(a.registry().tempo().is_some());
        // エラーにならない（静かにスキップ）
        assert!(a.errors().is_empty());
    }

    /// メインファイルのブロックが保持されることを検証
    /// Verifies that main file blocks are preserved
    #[test]
    fn update_with_file_path_preserves_main_blocks() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        let inc_path = dir.path().join("inc.cvg");
        let mut f = std::fs::File::create(&inc_path).unwrap();
        writeln!(f, "clip inc {{\n  c4\n}}").unwrap();

        let main_path = dir.path().join("main.cvg");
        let main_source = "include inc.cvg\ntempo 120\nvar x = 42";

        let mut a = LspAnalyzer::new();
        a.update_with_file_path(main_source.into(), &main_path);

        // メインファイルのブロック数（include, tempo, var）
        assert_eq!(a.blocks().len(), 3);
        // include先のクリップもregistryに登録
        assert!(a.registry().get_clip("inc").is_some());
    }

    /// 先頭以外のincludeはregistryに解決されないことを検証
    /// Verifies that non-top includes are not resolved in registry
    #[test]
    fn update_with_file_path_non_top_include_not_resolved() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        let inc_path = dir.path().join("late.cvg");
        let mut f = std::fs::File::create(&inc_path).unwrap();
        writeln!(f, "clip late {{\n  c4\n}}").unwrap();

        let main_path = dir.path().join("main.cvg");
        // tempoの後にinclude → 先頭以外なので解決されない
        let main_source = "tempo 120\ninclude late.cvg";

        let mut a = LspAnalyzer::new();
        a.update_with_file_path(main_source.into(), &main_path);

        // 先頭以外のincludeは解決されない
        assert!(a.registry().get_clip("late").is_none());
        // tempoは登録される
        assert!(a.registry().tempo().is_some());
    }
}
