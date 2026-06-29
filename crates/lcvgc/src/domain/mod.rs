//! 依存ゼロの共有ドメイン語彙モジュール。
//!
//! `ast` / `midi` / `parser` / `engine` / `lsp` が共有する基盤的な値型
//! （音名・オクターブ・コード種別・MIDIチャンネル）をここに集約し、
//! どのモジュールも `domain` へ一方向に依存することでモジュール間の
//! 循環依存を解消する。`domain` 自身はクレート内の他モジュールに依存しない。
//!
//! Dependency-free shared domain vocabulary module.
//!
//! Hosts the foundational value types shared across `ast` / `midi` /
//! `parser` / `engine` / `lsp` (note name, octave, chord quality, MIDI
//! channel). Every module depends on `domain` in a single direction, which
//! breaks the inter-module dependency cycles. `domain` itself depends on no
//! other in-crate module.

/// コード種別ドメイン型
/// Chord quality domain type
pub mod chord;
/// 音高ドメイン型（音名・オクターブ）
/// Pitch domain types (note name, octave)
pub mod pitch;
