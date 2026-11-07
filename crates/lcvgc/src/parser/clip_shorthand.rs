use crate::ast::doc::{ExampleRender, SyntaxDoc};

/// 省略記法ノートのオクターブ・音価キャリーオーバー状態を管理する構造体。
/// デフォルト値: octave=4, duration=4（4分音符）。
///
/// Manages octave/duration carry-over state for shorthand note notation.
/// Default: octave=4, duration=4 (quarter note).
#[derive(Debug, Clone, PartialEq)]
pub struct CarryOverState {
    /// 現在のオクターブ値 (0-9)
    ///
    /// Current octave value (0-9)
    pub octave: u8,
    /// 現在の音価（分母表記: 4=4分音符, 8=8分音符 等）
    ///
    /// Current duration in denominator notation (4=quarter, 8=eighth, etc.)
    pub duration: u16,
}

/// 省略記法から解決されたノート情報を保持する構造体。
///
/// Holds resolved note information from shorthand notation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedNote {
    /// 解決後のオクターブ値
    ///
    /// Resolved octave value
    pub octave: u8,
    /// 解決後の音価（分母表記）
    ///
    /// Resolved duration in denominator notation
    pub duration: u16,
    /// 付点の有無
    ///
    /// Whether the note is dotted
    pub dotted: bool,
}

/// 省略記法から解決された音価情報を保持する構造体（オクターブを含まない）。
/// Rest や ChordBracket など、音価のみキャリーする要素で使用する。
///
/// Holds resolved duration information from shorthand notation (without octave).
/// Used for elements that only carry duration, such as Rest and ChordBracket.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDuration {
    /// 解決後の音価（分母表記）
    ///
    /// Resolved duration in denominator notation
    pub duration: u16,
    /// 付点の有無
    ///
    /// Whether the note is dotted
    pub dotted: bool,
}

impl CarryOverState {
    /// 新しい `CarryOverState` をデフォルト値（octave=4, duration=4）で生成する。
    ///
    /// Creates a new `CarryOverState` with default values (octave=4, duration=4).
    ///
    /// # 戻り値 / Returns
    ///
    /// デフォルト状態の `CarryOverState`
    ///
    /// A `CarryOverState` with default state.
    pub fn new() -> Self {
        CarryOverState {
            octave: 4,
            duration: 4,
        }
    }

    /// オプショナルなオクターブ・音価を解決し、内部状態を更新してノート情報を返す。
    ///
    /// Resolves a note with optional octave/duration, updating internal state.
    ///
    /// # 引数 / Arguments
    ///
    /// * `octave` - オクターブ値。`None` の場合は前回の値を引き継ぐ / Octave value. If `None`, carries over from previous state.
    /// * `duration` - 音価。`None` の場合は前回の値を引き継ぐ / Duration value. If `None`, carries over from previous state.
    /// * `dotted` - 付点の有無 / Whether the note is dotted.
    ///
    /// # 戻り値 / Returns
    ///
    /// 解決済みのノート情報 [`ResolvedNote`]
    ///
    /// Resolved note information as [`ResolvedNote`].
    pub fn resolve(
        &mut self,
        octave: Option<u8>,
        duration: Option<u16>,
        dotted: bool,
    ) -> ResolvedNote {
        if let Some(oct) = octave {
            self.octave = oct;
        }
        if let Some(dur) = duration {
            self.duration = dur;
        }
        ResolvedNote {
            octave: self.octave,
            duration: self.duration,
            dotted,
        }
    }

    /// 音価のみを解決し、内部の duration 状態を更新する。octave は変更しない。
    /// Rest や ChordBracket など、オクターブのキャリーが不要な要素向け。
    ///
    /// Resolves duration only, updating internal duration state. Octave is unchanged.
    /// For elements like Rest and ChordBracket that don't carry octave.
    ///
    /// # 引数 / Arguments
    ///
    /// * `duration` - 音価。`None` の場合は前回の値を引き継ぐ / Duration value. If `None`, carries over from previous state.
    /// * `dotted` - 付点の有無 / Whether the note is dotted.
    ///
    /// # 戻り値 / Returns
    ///
    /// 解決済みの音価情報 [`ResolvedDuration`]
    ///
    /// Resolved duration information as [`ResolvedDuration`].
    pub fn resolve_duration_only(
        &mut self,
        duration: Option<u16>,
        dotted: bool,
    ) -> ResolvedDuration {
        if let Some(dur) = duration {
            self.duration = dur;
        }
        ResolvedDuration {
            duration: self.duration,
            dotted,
        }
    }
}

impl Default for CarryOverState {
    /// デフォルト値（octave=4, duration=4）で `CarryOverState` を生成する。
    ///
    /// Creates a `CarryOverState` with default values (octave=4, duration=4).
    fn default() -> Self {
        Self::new()
    }
}

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[
    SyntaxDoc {
        id: "clip_shorthand.note",
        title_ja: "省略記法（オクターブ・音価の引き継ぎ）",
        title_en: "Shorthand (carrying octave and length)",
        desc_ja: &[
            "オクターブと音価は省略すると直前の値を引き継ぐ（clip 先頭は 4, 4）。",
            "c は両方引き継ぎ、c:3 はオクターブだけ、c::2 は音価だけ変える。",
            "同じ楽器名の行が続くと連結され、引き継ぎも行をまたいで続く。",
        ],
        desc_en: &[
            "Omitted octave/length carry over from the previous note (start: 4, 4).",
            "c carries both, c:3 changes the octave, c::2 changes only the length.",
            "Consecutive lines of one instrument are joined; carry-over continues.",
        ],
        example:
            "clip shorthand_demo [bars 2] {\n\tbass c:3:8 c eb f g::2\n\tbass ab::8 g f eb c::2\n}",
        render: ExampleRender::Live,
        list: None,
    },
    SyntaxDoc {
        id: "clip_shorthand.chord",
        title_ja: "コード名の省略記法",
        title_en: "Shorthand for chord names",
        desc_ja: &[
            "コード名も単音と同じ規則で引き継ぐ（:: でオクターブ省略）。",
            "単音とコード名を混ぜても、引き継ぎは一貫して続く。",
        ],
        desc_en: &[
            "Chord names follow the same carry-over rules (:: skips the octave).",
            "Carry-over stays consistent when notes and chords are mixed.",
        ],
        example: "clip shorthand_chord_demo [bars 2] {\n\tlead cm7:4:2 fm7\n\tlead g7::1\n}",
        render: ExampleRender::Live,
        list: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let state = CarryOverState::new();
        assert_eq!(state.octave, 4);
        assert_eq!(state.duration, 4);
    }

    #[test]
    fn full_specify() {
        let mut state = CarryOverState::new();
        let note = state.resolve(Some(3), Some(8), false);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 3,
                duration: 8,
                dotted: false
            }
        );
        assert_eq!(state.octave, 3);
        assert_eq!(state.duration, 8);
    }

    #[test]
    fn both_omitted() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(None, None, false);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 3,
                duration: 8,
                dotted: false
            }
        );
    }

    #[test]
    fn octave_omitted_duration_changed() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(None, Some(4), false);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 3,
                duration: 4,
                dotted: false
            }
        );
    }

    #[test]
    fn octave_changed_duration_omitted() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let note = state.resolve(Some(5), None, false);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 5,
                duration: 8,
                dotted: false
            }
        );
    }

    #[test]
    fn sequential_resolve() {
        let mut state = CarryOverState::new();
        // c:3:8
        let n1 = state.resolve(Some(3), Some(8), false);
        assert_eq!(
            n1,
            ResolvedNote {
                octave: 3,
                duration: 8,
                dotted: false
            }
        );
        // c (both omitted)
        let n2 = state.resolve(None, None, false);
        assert_eq!(
            n2,
            ResolvedNote {
                octave: 3,
                duration: 8,
                dotted: false
            }
        );
        // f::4 (octave omitted, duration changed)
        let n3 = state.resolve(None, Some(4), false);
        assert_eq!(
            n3,
            ResolvedNote {
                octave: 3,
                duration: 4,
                dotted: false
            }
        );
    }

    #[test]
    fn dotted_note() {
        let mut state = CarryOverState::new();
        let note = state.resolve(Some(3), Some(4), true);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 3,
                duration: 4,
                dotted: true
            }
        );
    }

    #[test]
    fn resolve_duration_only_updates_duration() {
        let mut state = CarryOverState::new();
        state.resolve(Some(5), Some(8), false);
        let rd = state.resolve_duration_only(Some(16), false);
        assert_eq!(
            rd,
            ResolvedDuration {
                duration: 16,
                dotted: false
            }
        );
        // octave は変わらない / octave remains unchanged
        assert_eq!(state.octave, 5);
        assert_eq!(state.duration, 16);
    }

    #[test]
    fn resolve_duration_only_carries_previous() {
        let mut state = CarryOverState::new();
        state.resolve(Some(3), Some(8), false);
        let rd = state.resolve_duration_only(None, true);
        assert_eq!(
            rd,
            ResolvedDuration {
                duration: 8,
                dotted: true
            }
        );
        assert_eq!(state.octave, 3);
    }

    #[test]
    fn resolve_then_duration_only_preserves_octave() {
        let mut state = CarryOverState::new();
        // resolve でoctave=5, duration=8 に設定
        state.resolve(Some(5), Some(8), false);
        // resolve_duration_only で duration のみ変更
        let rd = state.resolve_duration_only(Some(4), false);
        assert_eq!(rd.duration, 4);
        // その後の resolve で octave=5 が保持されている
        let note = state.resolve(None, None, false);
        assert_eq!(
            note,
            ResolvedNote {
                octave: 5,
                duration: 4,
                dotted: false
            }
        );
    }

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
