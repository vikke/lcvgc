/// 外部ファイルのインクルード定義
/// Include definition for an external file
#[derive(Debug, Clone, PartialEq)]
pub struct IncludeDef {
    /// インクルードするファイルパス
    /// File path to include
    pub path: String,
}
