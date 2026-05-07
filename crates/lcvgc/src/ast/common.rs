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

/// 音価（音符の長さ）
/// Duration (note length)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    /// 全音符
    /// Whole note
    Whole,
    /// 二分音符
    /// Half note
    Half,
    /// 四分音符
    /// Quarter note
    Quarter,
    /// 八分音符
    /// Eighth note
    Eighth,
    /// 十六分音符
    /// Sixteenth note
    Sixteenth,
    /// 付点音符（内部音価を保持）
    /// Dotted note (holds the inner duration)
    Dotted(DottedInner),
}

/// 付点音符の内部音価（それ自体は付点にできない）
/// Inner duration for dotted notes (cannot itself be dotted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DottedInner {
    /// 全音符
    /// Whole note
    Whole,
    /// 二分音符
    /// Half note
    Half,
    /// 四分音符
    /// Quarter note
    Quarter,
    /// 八分音符
    /// Eighth note
    Eighth,
    /// 十六分音符
    /// Sixteenth note
    Sixteenth,
}

/// ゲート指定（音の長さの割合を制御）
/// Gate specification (controls the proportion of note duration)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSpec {
    /// ゲートの種類
    /// Gate kind
    pub kind: GateKind,
}

/// ゲートの種類
/// Gate kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    /// 通常ゲート（デフォルト）
    /// Normal gate (default)
    Normal,
    /// スタッカート（短いゲート）
    /// Staccato (short gate)
    Staccato,
    /// 直接指定（0-127のゲート値）
    /// Direct specification (gate value 0-127)
    Direct(u8),
}

impl Default for GateSpec {
    fn default() -> Self {
        GateSpec {
            kind: GateKind::Normal,
        }
    }
}
