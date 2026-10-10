use crate::domain::pitch::NoteName;

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

impl ScaleType {
    /// 全スケール種別（宣言順）。パーサと補完で共用するキーワード表の唯一の定義元。
    /// All scale types in declaration order. The single source of the keyword
    /// table shared by the parser and completion.
    pub const ALL: [ScaleType; 9] = [
        ScaleType::Major,
        ScaleType::Minor,
        ScaleType::HarmonicMinor,
        ScaleType::MelodicMinor,
        ScaleType::Dorian,
        ScaleType::Phrygian,
        ScaleType::Lydian,
        ScaleType::Mixolydian,
        ScaleType::Locrian,
    ];

    /// DSL 上のキーワード表記を返す（例: `"harmonic_minor"`）。
    /// Returns the DSL keyword spelling (e.g. `"harmonic_minor"`).
    ///
    /// # Returns
    /// スケール種別キーワード
    /// The scale type keyword
    pub fn keyword(self) -> &'static str {
        match self {
            ScaleType::Major => "major",
            ScaleType::Minor => "minor",
            ScaleType::HarmonicMinor => "harmonic_minor",
            ScaleType::MelodicMinor => "melodic_minor",
            ScaleType::Dorian => "dorian",
            ScaleType::Phrygian => "phrygian",
            ScaleType::Lydian => "lydian",
            ScaleType::Mixolydian => "mixolydian",
            ScaleType::Locrian => "locrian",
        }
    }

    /// キーワード文字列からスケール種別を引く。完全一致のみ受け付ける。
    /// Looks up a scale type by its keyword. Only exact matches are accepted.
    ///
    /// # Arguments
    /// * `keyword` - スケール種別キーワード（例: `"dorian"`） / Scale type keyword (e.g. `"dorian"`)
    ///
    /// # Returns
    /// 一致する種別。未知のキーワードなら `None`
    /// The matching type, or `None` for an unknown keyword
    pub fn from_keyword(keyword: &str) -> Option<ScaleType> {
        ScaleType::ALL.into_iter().find(|t| t.keyword() == keyword)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_9_scale_types() {
        assert_eq!(ScaleType::ALL.len(), 9);
    }

    #[test]
    fn keyword_matches_dsl_spelling() {
        let keywords: Vec<&str> = ScaleType::ALL.iter().map(|t| t.keyword()).collect();
        assert_eq!(
            keywords,
            vec![
                "major",
                "minor",
                "harmonic_minor",
                "melodic_minor",
                "dorian",
                "phrygian",
                "lydian",
                "mixolydian",
                "locrian"
            ]
        );
    }

    #[test]
    fn from_keyword_roundtrips_all() {
        for t in ScaleType::ALL {
            assert_eq!(ScaleType::from_keyword(t.keyword()), Some(t));
        }
    }

    #[test]
    fn from_keyword_rejects_unknown_and_partial() {
        assert_eq!(ScaleType::from_keyword("blues"), None);
        assert_eq!(ScaleType::from_keyword("harmonic"), None);
        assert_eq!(ScaleType::from_keyword("majorx"), None);
        assert_eq!(ScaleType::from_keyword(""), None);
    }
}
