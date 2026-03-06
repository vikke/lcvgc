/// 抽象構文木定義モジュール
/// Abstract syntax tree definition module
pub mod ast;
/// 評価エンジンモジュール（コンパイラ・評価器・クロック等）
/// Evaluation engine module (compiler, evaluator, clock, etc.)
pub mod engine;
/// パースエラー定義モジュール
/// Parse error definition module
pub mod error;
/// LSP機能モジュール（補完・ホバー・診断・定義ジャンプ・シンボル）
/// LSP feature module (completion, hover, diagnostics, go-to-definition, symbols)
pub mod lsp;
/// MIDI入出力モジュール
/// MIDI I/O module
pub mod midi;
/// DSLパーサーモジュール
/// DSL parser module
pub mod parser;
/// TCPサーバーモジュール
/// TCP server module
pub mod server;
