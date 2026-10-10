//! 音高に関する基盤ドメイン型（音名・オクターブ）。
//! Foundational domain types for pitch (note name, octave).

/// 音名（半音階の全12音を異名同音を含めて表現）
/// Note name (represents all 12 chromatic pitches including enharmonic equivalents)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteName {
    /// ド
    /// C natural
    C,
    /// ド#（嬰ハ）
    /// C sharp
    Cs,
    /// レb（変ニ）
    /// D flat
    Db,
    /// レ
    /// D natural
    D,
    /// レ#（嬰ニ）
    /// D sharp
    Ds,
    /// ミb（変ホ）
    /// E flat
    Eb,
    /// ミ
    /// E natural
    E,
    /// ファ
    /// F natural
    F,
    /// ファ#（嬰ヘ）
    /// F sharp
    Fs,
    /// ソb（変ト）
    /// G flat
    Gb,
    /// ソ
    /// G natural
    G,
    /// ソ#（嬰ト）
    /// G sharp
    Gs,
    /// ラb（変イ）
    /// A flat
    Ab,
    /// ラ
    /// A natural
    A,
    /// ラ#（嬰イ）
    /// A sharp
    As,
    /// シb（変ロ）
    /// B flat
    Bb,
    /// シ
    /// B natural
    B,
}

impl NoteName {
    /// 全17音名（半音階順、同じ高さはシャープ → フラットの順）。補完候補の総当たり生成に使う。
    /// All 17 note names in chromatic order (sharp before flat for the same pitch).
    /// Used to enumerate completion candidates.
    pub const ALL: [NoteName; 17] = [
        NoteName::C,
        NoteName::Cs,
        NoteName::Db,
        NoteName::D,
        NoteName::Ds,
        NoteName::Eb,
        NoteName::E,
        NoteName::F,
        NoteName::Fs,
        NoteName::Gb,
        NoteName::G,
        NoteName::Gs,
        NoteName::Ab,
        NoteName::A,
        NoteName::As,
        NoteName::Bb,
        NoteName::B,
    ];

    /// 英小文字のみの DSL 表記を返す。シャープは `#` ではなく `s` 形式（例: `cs`）。
    /// nvim の標準 `iskeyword` が `#` を含まないため、補完ラベルにはこの表記を使う。
    ///
    /// Returns the DSL spelling made only of lowercase letters. Sharps use the
    /// `s` form (e.g. `cs`) instead of `#`, because nvim's default `iskeyword`
    /// does not include `#`; completion labels use this spelling.
    ///
    /// # Returns
    /// 音名キーワード（例: `"c"`, `"cs"`, `"eb"`）
    /// The note-name keyword (e.g. `"c"`, `"cs"`, `"eb"`)
    pub fn keyword(self) -> &'static str {
        match self {
            NoteName::C => "c",
            NoteName::Cs => "cs",
            NoteName::Db => "db",
            NoteName::D => "d",
            NoteName::Ds => "ds",
            NoteName::Eb => "eb",
            NoteName::E => "e",
            NoteName::F => "f",
            NoteName::Fs => "fs",
            NoteName::Gb => "gb",
            NoteName::G => "g",
            NoteName::Gs => "gs",
            NoteName::Ab => "ab",
            NoteName::A => "a",
            NoteName::As => "as",
            NoteName::Bb => "bb",
            NoteName::B => "b",
        }
    }
}

/// オクターブ（0-9の範囲）
/// Octave (range 0-9)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Octave(pub u8);

impl Octave {
    /// 指定された値からオクターブを生成する。0-9の範囲外の場合は`None`を返す。
    /// Creates an octave from the given value. Returns `None` if the value is outside the range 0-9.
    pub fn new(value: u8) -> Option<Self> {
        if value <= 9 {
            Some(Octave(value))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_17_note_names_in_chromatic_order() {
        let keywords: Vec<&str> = NoteName::ALL.iter().map(|n| n.keyword()).collect();
        assert_eq!(
            keywords,
            vec![
                "c", "cs", "db", "d", "ds", "eb", "e", "f", "fs", "gb", "g", "gs", "ab", "a", "as",
                "bb", "b"
            ]
        );
    }

    #[test]
    fn keyword_uses_s_form_for_sharps() {
        // シャープは `#` ではなく `s` 形式で表記する（nvim の iskeyword に `#` が無いため）
        // Sharps use the `s` form instead of `#` (nvim's iskeyword lacks `#`)
        assert_eq!(NoteName::Cs.keyword(), "cs");
        assert_eq!(NoteName::As.keyword(), "as");
    }

    #[test]
    fn keyword_contains_only_ascii_alphabetic() {
        for n in NoteName::ALL {
            assert!(
                n.keyword().chars().all(|c| c.is_ascii_lowercase()),
                "non-alphabetic keyword: {}",
                n.keyword()
            );
        }
    }
}
