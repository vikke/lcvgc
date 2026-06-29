//! 小節ジャンプの AST 型。
//! Bar-jump AST type.

/// 小節ジャンプを表す構造体。
/// Represents a bar jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarJump {
    /// ジャンプ先の小節番号（1始まり）
    /// Target bar number (1-based)
    pub bar_number: u32,
}
