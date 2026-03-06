/// ドラムステップシーケンサーパターンのヒットシンボル
/// Hit symbol in a drum step sequencer pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitSymbol {
    /// `x` — 通常ヒット、ベロシティ100
    /// `x` — normal hit, velocity 100
    Normal,
    /// `X` — アクセントヒット、ベロシティ127
    /// `X` — accent hit, velocity 127
    Accent,
    /// `o` — ゴーストノート、ベロシティ40
    /// `o` — ghost note, velocity 40
    Ghost,
    /// `.` — 休符（無音）
    /// `.` — rest (silence)
    Rest,
}

impl HitSymbol {
    /// このヒットのMIDIベロシティを返す。休符の場合は`None`を返す。
    /// Returns the MIDI velocity for this hit, or `None` for a rest.
    pub fn velocity(self) -> Option<u8> {
        match self {
            HitSymbol::Normal => Some(100),
            HitSymbol::Accent => Some(127),
            HitSymbol::Ghost => Some(40),
            HitSymbol::Rest => None,
        }
    }
}

/// ドラムクリップ内の単一インストゥルメント行
/// A single instrument row in a drum clip.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumRow {
    /// インストゥルメント名
    /// Instrument name
    pub instrument: String,
    /// ヒットシンボルのリスト
    /// List of hit symbols
    pub hits: Vec<HitSymbol>,
    /// ステップごとの発音確率 0-100。`None`の場合は全ステップ100%
    /// Per-step firing probability 0-100. `None` means all steps are 100%.
    pub probability: Option<Vec<u8>>,
}
