/// CC行のヘッダー: instrument.param_name
#[derive(Debug, Clone, PartialEq)]
pub struct CcTarget {
    pub instrument: String,
    pub param: String,
}

/// ステップ方式: スペース区切りの値リスト
#[derive(Debug, Clone, PartialEq)]
pub struct CcStepValues {
    pub target: CcTarget,
    pub values: Vec<u8>, // 0-127
}

/// 時間指定のポイント
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimePoint {
    pub value: u8,
    pub bar: u32,  // 1-based
    pub beat: u32, // 1-based
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    None,
    Linear,
    Exponential,
}

/// 時間指定方式のセグメント
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimeSegment {
    pub from: CcTimePoint,
    pub to: Option<(Interpolation, CcTimePoint)>,
}

/// 時間指定方式
#[derive(Debug, Clone, PartialEq)]
pub struct CcTimeValues {
    pub target: CcTarget,
    pub segments: Vec<CcTimeSegment>,
}

/// CCオートメーション（いずれか）
#[derive(Debug, Clone, PartialEq)]
pub enum CcAutomation {
    Step(CcStepValues),
    Time(CcTimeValues),
}
