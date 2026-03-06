/// テンポ指定（絶対値または相対値）
/// Tempo specification (absolute or relative)
#[derive(Debug, Clone, PartialEq)]
pub enum Tempo {
    /// 絶対テンポ（BPM）
    /// Absolute tempo (BPM)
    Absolute(u16),
    /// 相対テンポ変更（現在のBPMからの差分）
    /// Relative tempo change (offset from current BPM)
    Relative(i16),
}
