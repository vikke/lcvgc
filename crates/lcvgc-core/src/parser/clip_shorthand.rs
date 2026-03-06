/// 省略記法ノートのオクターブ・音価キャリーオーバー状態を管理する構造体。
/// デフォルト値: octave=4, duration=4（4分音符）。
///
/// Manages octave/duration carry-over state for shorthand note notation.
/// Default: octave=4, duration=4 (quarter note).
#[derive(Debug, Clone, PartialEq)]
pub struct CarryOverState {
    /// 現在のオクターブ値 (0-10)
    ///
    /// Current octave value (0-10)
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
    pub fn resolve(&mut self, octave: Option<u8>, duration: Option<u16>, dotted: bool) -> ResolvedNote {
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
}

impl Default for CarryOverState {
    /// デフォルト値（octave=4, duration=4）で `CarryOverState` を生成する。
    ///
    /// Creates a `CarryOverState` with default values (octave=4, duration=4).
    fn default() -> Self {
        Self::new()
    }
}

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
}
