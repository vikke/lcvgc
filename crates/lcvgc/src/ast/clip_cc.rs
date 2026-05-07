/// CC行のヘッダー: instrument.param_name
/// CC line header: instrument.param_name
#[derive(Debug, Clone, PartialEq)]
pub struct CcTarget {
    /// インストゥルメント名
    /// Instrument name
    pub instrument: String,
    /// パラメータ名
    /// Parameter name
    pub param: String,
}

/// ステップ方式: スペース区切りの値リスト
/// Step mode: space-separated value list
#[derive(Debug, Clone, PartialEq)]
pub struct CcStepValues {
    /// 対象CCターゲット
    /// Target CC destination
    pub target: CcTarget,
    /// ステップ値のリスト (0-127)
    /// List of step values (0-127)
    pub values: Vec<u8>, // 0-127
}

/// 時間指定のポイント
/// A time-specified point
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimePoint {
    /// CC値 (0-127)
    /// CC value (0-127)
    pub value: u8,
    /// 小節番号（1始まり）
    /// Bar number (1-based)
    pub bar: u32, // 1-based
    /// 拍番号（1始まり）
    /// Beat number (1-based)
    pub beat: u32, // 1-based
}

/// 補間方式
/// Interpolation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// 補間なし（ステップ変化）
    /// No interpolation (step change)
    None,
    /// 線形補間
    /// Linear interpolation
    Linear,
    /// 指数補間
    /// Exponential interpolation
    Exponential,
}

/// 時間指定方式のセグメント
/// A time-specified segment
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimeSegment {
    /// セグメントの開始ポイント
    /// Starting point of the segment
    pub from: CcTimePoint,
    /// セグメントの終了ポイント（補間方式付き、オプション）
    /// Ending point of the segment with interpolation method (optional)
    pub to: Option<(Interpolation, CcTimePoint)>,
}

/// 時間指定方式
/// Time-specified mode
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimeValues {
    /// 対象CCターゲット
    /// Target CC destination
    pub target: CcTarget,
    /// 時間セグメントのリスト
    /// List of time segments
    pub segments: Vec<CcTimeSegment>,
}

/// CCオートメーション（いずれか）
/// CC automation (either step or time mode)
#[derive(Debug, Clone, PartialEq)]
pub enum CcAutomation {
    /// ステップ方式のCCオートメーション
    /// Step-mode CC automation
    Step(CcStepValues),
    /// 時間指定方式のCCオートメーション
    /// Time-specified CC automation
    Time(CcTimeValues),
}
