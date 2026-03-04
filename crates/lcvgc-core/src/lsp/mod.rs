//! LSP機能モジュール
//!
//! デーモンサーバーに統合されたLSP機能群。
//! 補完・ホバー・診断・定義ジャンプ・ドキュメントシンボルを提供する。

/// LSPドキュメント解析器
pub mod analyzer;
/// 補完候補プロバイダ
pub mod completion;
/// 補完コンテキスト判定
pub mod context;
/// 診断プロバイダ（パースエラー＋未定義参照）
pub mod diagnostic;
/// ダイアトニックコード算出ユーティリティ
pub mod diatonic;
/// 定義ジャンププロバイダ
pub mod goto_def;
/// ホバー情報プロバイダ
pub mod hover;
/// スパン情報付きパーサー
pub mod span_parser;
/// ドキュメントシンボルプロバイダ
pub mod symbols;
