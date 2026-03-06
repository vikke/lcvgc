//! LSP機能モジュール
//! LSP feature module
//!
//! デーモンサーバーに統合されたLSP機能群。
//! 補完・ホバー・診断・定義ジャンプ・ドキュメントシンボルを提供する。
//! LSP feature set integrated into the daemon server.
//! Provides completion, hover, diagnostics, go to definition, and document symbols.

/// LSPドキュメント解析器
/// LSP document analyzer
pub mod analyzer;
/// 補完候補プロバイダ
/// Completion candidate provider
pub mod completion;
/// 補完コンテキスト判定
/// Completion context detection
pub mod context;
/// 診断プロバイダ（パースエラー＋未定義参照）
/// Diagnostics provider (parse errors + undefined references)
pub mod diagnostic;
/// ダイアトニックコード算出ユーティリティ
/// Diatonic chord calculation utility
pub mod diatonic;
/// 定義ジャンププロバイダ
/// Go to definition provider
pub mod goto_def;
/// ホバー情報プロバイダ
/// Hover information provider
pub mod hover;
/// スパン情報付きパーサー
/// Span-aware parser
pub mod span_parser;
/// ドキュメントシンボルプロバイダ
/// Document symbol provider
pub mod symbols;
