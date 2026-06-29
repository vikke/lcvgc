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
