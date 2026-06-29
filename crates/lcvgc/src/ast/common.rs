/// 音価（音符の長さ）
/// Duration (note length)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    /// 全音符
    /// Whole note
    Whole,
    /// 二分音符
    /// Half note
    Half,
    /// 四分音符
    /// Quarter note
    Quarter,
    /// 八分音符
    /// Eighth note
    Eighth,
    /// 十六分音符
    /// Sixteenth note
    Sixteenth,
    /// 付点音符（内部音価を保持）
    /// Dotted note (holds the inner duration)
    Dotted(DottedInner),
}

/// 付点音符の内部音価（それ自体は付点にできない）
/// Inner duration for dotted notes (cannot itself be dotted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DottedInner {
    /// 全音符
    /// Whole note
    Whole,
    /// 二分音符
    /// Half note
    Half,
    /// 四分音符
    /// Quarter note
    Quarter,
    /// 八分音符
    /// Eighth note
    Eighth,
    /// 十六分音符
    /// Sixteenth note
    Sixteenth,
}

/// ゲート指定（音の長さの割合を制御）
/// Gate specification (controls the proportion of note duration)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSpec {
    /// ゲートの種類
    /// Gate kind
    pub kind: GateKind,
}

/// ゲートの種類
/// Gate kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    /// 通常ゲート（デフォルト）
    /// Normal gate (default)
    Normal,
    /// スタッカート（短いゲート）
    /// Staccato (short gate)
    Staccato,
    /// 直接指定（0-127のゲート値）
    /// Direct specification (gate value 0-127)
    Direct(u8),
}

impl Default for GateSpec {
    fn default() -> Self {
        GateSpec {
            kind: GateKind::Normal,
        }
    }
}
