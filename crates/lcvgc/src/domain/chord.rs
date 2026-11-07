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

impl ChordSuffix {
    /// 全 20 種のコードサフィックス。補完候補の総当たり生成に使う。
    /// All 20 chord suffixes. Used to enumerate completion candidates.
    pub const ALL: [ChordSuffix; 20] = [
        ChordSuffix::Maj,
        ChordSuffix::Maj7,
        ChordSuffix::Min,
        ChordSuffix::Min7,
        ChordSuffix::Dom7,
        ChordSuffix::Dim,
        ChordSuffix::Dim7,
        ChordSuffix::Aug,
        ChordSuffix::AugMaj7,
        ChordSuffix::Min7b5,
        ChordSuffix::MinMaj7,
        ChordSuffix::Sus4,
        ChordSuffix::Sus2,
        ChordSuffix::Sixth,
        ChordSuffix::Min6,
        ChordSuffix::Ninth,
        ChordSuffix::Min9,
        ChordSuffix::Add9,
        ChordSuffix::Thirteenth,
        ChordSuffix::Min13,
    ];

    /// DSL で受け付ける表記の一覧を返す。先頭が短い表記（代表）で、続きが別名。
    /// パーサ（`parser::clip_note::parse_chord_suffix`）が受け付ける表記と一致させること。
    ///
    /// Returns the DSL spellings accepted for this suffix. The first one is the
    /// short (representative) form, followed by aliases. Must match what the
    /// parser (`parser::clip_note::parse_chord_suffix`) accepts.
    ///
    /// # Returns
    /// 表記のスライス（例: `Maj7` → `["M7", "Maj7"]`）
    /// The slice of spellings (e.g. `Maj7` → `["M7", "Maj7"]`)
    pub fn spellings(self) -> &'static [&'static str] {
        match self {
            ChordSuffix::Maj => &["M", "Maj"],
            ChordSuffix::Maj7 => &["M7", "Maj7"],
            ChordSuffix::Min => &["m"],
            ChordSuffix::Min7 => &["m7"],
            ChordSuffix::Dom7 => &["7"],
            ChordSuffix::Dim => &["dim"],
            ChordSuffix::Dim7 => &["dim7"],
            ChordSuffix::Aug => &["aug"],
            ChordSuffix::AugMaj7 => &["M7#5", "Maj7#5"],
            ChordSuffix::Min7b5 => &["m7b5"],
            ChordSuffix::MinMaj7 => &["mM7", "mMaj7"],
            ChordSuffix::Sus4 => &["sus4"],
            ChordSuffix::Sus2 => &["sus2"],
            ChordSuffix::Sixth => &["6"],
            ChordSuffix::Min6 => &["m6"],
            ChordSuffix::Ninth => &["9"],
            ChordSuffix::Min9 => &["m9"],
            ChordSuffix::Add9 => &["add9"],
            ChordSuffix::Thirteenth => &["13"],
            ChordSuffix::Min13 => &["m13"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_20_distinct_suffixes() {
        assert_eq!(ChordSuffix::ALL.len(), 20);
        for (i, a) in ChordSuffix::ALL.iter().enumerate() {
            for b in &ChordSuffix::ALL[i + 1..] {
                assert_ne!(a, b, "duplicated suffix in ALL: {a:?}");
            }
        }
    }

    #[test]
    fn spellings_first_is_short_form() {
        // 先頭は短い表記（補完で代表として使う）
        // The first spelling is the short form (used as the representative)
        assert_eq!(ChordSuffix::Maj.spellings()[0], "M");
        assert_eq!(ChordSuffix::Maj7.spellings()[0], "M7");
        assert_eq!(ChordSuffix::MinMaj7.spellings()[0], "mM7");
        assert_eq!(ChordSuffix::AugMaj7.spellings()[0], "M7#5");
    }

    #[test]
    fn spellings_include_long_aliases() {
        assert_eq!(ChordSuffix::Maj.spellings(), &["M", "Maj"]);
        assert_eq!(ChordSuffix::Maj7.spellings(), &["M7", "Maj7"]);
        assert_eq!(ChordSuffix::MinMaj7.spellings(), &["mM7", "mMaj7"]);
        assert_eq!(ChordSuffix::AugMaj7.spellings(), &["M7#5", "Maj7#5"]);
        assert_eq!(ChordSuffix::Sus4.spellings(), &["sus4"]);
    }

    #[test]
    fn total_spellings_is_24() {
        let total: usize = ChordSuffix::ALL.iter().map(|s| s.spellings().len()).sum();
        assert_eq!(total, 24);
    }
}
