//! 未解決変数参照型
//! Unresolved variable reference types
//!
//! パーサーが変数参照を検出した場合に、数値フィールドの代わりに
//! 変数名を保持するための構造体群。evaluator が ScopeChain で解決する。
//! Structs that hold variable names instead of numeric field values
//! when the parser detects variable references. The evaluator resolves them via ScopeChain.

/// インストゥルメント定義の未解決変数参照
/// Unresolved variable references for instrument definitions
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnresolvedVarRefs {
    /// device フィールドの変数参照
    /// Variable reference for the device field
    pub device: Option<String>,
    /// channel フィールドの変数参照
    /// Variable reference for the channel field
    pub channel: Option<String>,
    /// gate_normal フィールドの変数参照
    /// Variable reference for the gate_normal field
    pub gate_normal: Option<String>,
    /// gate_staccato フィールドの変数参照
    /// Variable reference for the gate_staccato field
    pub gate_staccato: Option<String>,
}

/// CCマッピングの未解決変数参照
/// Unresolved variable reference for CC mappings
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnresolvedCcVarRef {
    /// cc_number フィールドの変数参照
    /// Variable reference for the cc_number field
    pub cc_number_ref: Option<String>,
}

/// キットインストゥルメントの未解決変数参照
/// Unresolved variable references for kit instruments
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnresolvedKitInstrumentVarRefs {
    /// channel フィールドの変数参照
    /// Variable reference for the channel field
    pub channel: Option<String>,
    /// gate_normal フィールドの変数参照
    /// Variable reference for the gate_normal field
    pub gate_normal: Option<String>,
    /// gate_staccato フィールドの変数参照
    /// Variable reference for the gate_staccato field
    pub gate_staccato: Option<String>,
}
