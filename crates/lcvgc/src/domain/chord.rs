//! コード種別に関する基盤ドメイン型。
//! Foundational domain type for chord quality.

/// コードサフィックス（和音の種類）
/// Chord suffix (chord quality)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordSuffix {
    /// メジャー
    /// Major
    Maj,
    /// メジャーセブンス
    /// Major seventh
    Maj7,
    /// マイナー
    /// Minor
    Min,
    /// マイナーセブンス
    /// Minor seventh
    Min7,
    /// ドミナントセブンス
    /// Dominant seventh
    Dom7,
    /// ディミニッシュ
    /// Diminished
    Dim,
    /// ディミニッシュセブンス
    /// Diminished seventh
    Dim7,
    /// オーギュメント
    /// Augmented
    Aug,
    /// オーギュメントメジャーセブンス
    /// Augmented major seventh
    AugMaj7,
    /// マイナーセブンフラットファイブ（ハーフディミニッシュ）
    /// Minor seventh flat five (half-diminished)
    Min7b5,
    /// マイナーメジャーセブンス
    /// Minor-major seventh
    MinMaj7,
    /// サスフォー
    /// Suspended fourth
    Sus4,
    /// サスツー
    /// Suspended second
    Sus2,
    /// シックスス
    /// Sixth
    Sixth,
    /// マイナーシックスス
    /// Minor sixth
    Min6,
    /// ナインス
    /// Ninth
    Ninth,
    /// マイナーナインス
    /// Minor ninth
    Min9,
    /// アドナインス
    /// Add ninth
    Add9,
    /// サーティーンス
    /// Thirteenth
    Thirteenth,
    /// マイナーサーティーンス
    /// Minor thirteenth
    Min13,
}
