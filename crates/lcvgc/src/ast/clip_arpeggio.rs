//! アルペジオ関連の AST 型（方向・設定）。
//! Arpeggio-related AST types (direction and settings).

/// アルペジオの方向を表す列挙型
/// Enum representing the direction of an arpeggio
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpeggioDirection {
    /// 上昇
    /// Ascending
    Up,
    /// 下降
    /// Descending
    Down,
    /// 上昇→下降の往復
    /// Ascending then descending (ping-pong)
    UpDown,
    /// ランダム順
    /// Random order
    Random,
}

/// アルペジオ設定（方向と任意の音価）
/// Arpeggio settings (direction and optional per-step duration)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arpeggio {
    /// アルペジオの方向
    /// Direction of the arpeggio
    pub direction: ArpeggioDirection,
    /// 1音あたりの音価（例: 16 = 16分音符）。`None` の場合は和音側の duration を採用する。
    /// Per-step duration (e.g. 16 = sixteenth note). When `None` the chord-side
    /// duration is used as the per-step duration.
    pub resolution: Option<u16>,
}
