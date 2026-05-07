use crate::ast::common::NoteName;

/// スケールの種類
/// Scale type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleType {
    /// メジャー（長調）
    /// Major
    Major,
    /// ナチュラルマイナー（自然短音階）
    /// Natural minor
    Minor,
    /// ハーモニックマイナー（和声短音階）
    /// Harmonic minor
    HarmonicMinor,
    /// メロディックマイナー（旋律短音階）
    /// Melodic minor
    MelodicMinor,
    /// ドリアン
    /// Dorian
    Dorian,
    /// フリジアン
    /// Phrygian
    Phrygian,
    /// リディアン
    /// Lydian
    Lydian,
    /// ミクソリディアン
    /// Mixolydian
    Mixolydian,
    /// ロクリアン
    /// Locrian
    Locrian,
}

/// スケール定義
/// Scale definition
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleDef {
    /// ルート音名
    /// Root note name
    pub root: NoteName,
    /// スケールの種類
    /// Scale type
    pub scale_type: ScaleType,
}
