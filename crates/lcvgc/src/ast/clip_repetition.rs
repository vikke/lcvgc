//! 繰り返しの AST 型。
//! Repetition AST type.

/// 繰り返しは内容文字列と回数を保持。内容の具体的なパースは上位レイヤーが担当。
///
/// Holds the content string and repeat count. Concrete parsing of the content
/// is delegated to upper layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Repetition {
    /// 繰り返し対象の生テキスト（括弧の中身）。
    ///
    /// Raw text inside the parentheses to be repeated.
    pub content: String,
    /// 繰り返し回数（`*N` の N）。
    ///
    /// Number of repetitions (the N in `*N`).
    pub count: u32,
}
