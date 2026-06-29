//! アーティキュレーション関連の AST 型（奏法・ノートサフィックス）。
//! Articulation-related AST types (playing technique, note suffix).

/// アーティキュレーション（奏法）を表す列挙型
/// Enum representing articulation (playing technique)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Articulation {
    /// 通常奏法
    /// Normal articulation
    #[default]
    Normal,
    /// スタッカート（短く切る奏法）
    /// Staccato (short, detached notes)
    Staccato,
    /// ゲート値の直接指定（0-100のパーセンテージ）
    /// Direct gate value specification (0-100 percentage)
    GateDirect(u8),
}

/// ノートサフィックス修飾子のパース結果。
///
/// `articulation` は最終的に決まるアーティキュレーションを示す。
/// `velocity` は `vN` が与えられたときの上書き値 (`None` のときコンパイラ既定 100)。
///
/// Parse result of note suffix modifiers.
/// `articulation` is the resolved articulation (Normal / Staccato / GateDirect).
/// `velocity` is the explicit Note On velocity from `vN` (`None` if absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoteSuffix {
    pub articulation: Articulation,
    pub velocity: Option<u8>,
}
