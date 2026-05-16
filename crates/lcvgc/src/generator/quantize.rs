//! tick → lcvgc Duration への量子化。
//!
//! lcvgc DSL の音価は `1, 2, 4, 8, 16` (+ 付点) に限られる。任意の tick 長
//! を best-fit でこれらの組み合わせに分解する。1 つで表せない場合は **複数
//! の音価のタイ** として返す (例: 五分の三 → 2分 + 16 分相当はサポート
//! 範囲外なので 16 分単位の最近接整数倍に丸め、丸めた長さを 1/16 の整数
//! 倍に分解)。
//!
//! Quantizes a tick count into one or more lcvgc DSL durations. Anything
//! finer than a sixteenth is rounded to the nearest 16th-note grid.

/// lcvgc DSL の音価を文字列で返すヘルパ。
///
/// `1, 2, 4, 8, 16` または `1., 2., 4., 8., 16.` のいずれか。
/// Returns the textual representation of an lcvgc duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationToken {
    /// 全音符 (1)
    Whole,
    /// 二分音符 (2)
    Half,
    /// 四分音符 (4)
    Quarter,
    /// 八分音符 (8)
    Eighth,
    /// 十六分音符 (16)
    Sixteenth,
    /// 付点全音符 (1.)
    DottedWhole,
    /// 付点二分音符 (2.)
    DottedHalf,
    /// 付点四分音符 (4.)
    DottedQuarter,
    /// 付点八分音符 (8.)
    DottedEighth,
    /// 付点十六分音符 (16.)
    DottedSixteenth,
}

impl DurationToken {
    /// 1 四分音符を `4` として表す lcvgc DSL の音価文字列。
    /// DSL text form (e.g. "4", "8.", ...).
    pub fn as_str(self) -> &'static str {
        match self {
            DurationToken::Whole => "1",
            DurationToken::Half => "2",
            DurationToken::Quarter => "4",
            DurationToken::Eighth => "8",
            DurationToken::Sixteenth => "16",
            DurationToken::DottedWhole => "1.",
            DurationToken::DottedHalf => "2.",
            DurationToken::DottedQuarter => "4.",
            DurationToken::DottedEighth => "8.",
            DurationToken::DottedSixteenth => "16.",
        }
    }

    /// PPQ (= 1 四分音符) を 1 単位とする「sixteenth 単位の長さ」(1 四分音符 = 4)
    /// に対する、このトークンの sixteenth 数を返す。
    ///
    /// `Quarter` は 4 sixteenth、`DottedHalf` は 8 + 4 = 12 sixteenth 等。
    /// Returns the length of this token measured in sixteenth-notes.
    pub fn sixteenths(self) -> u32 {
        match self {
            DurationToken::Whole => 16,
            DurationToken::Half => 8,
            DurationToken::Quarter => 4,
            DurationToken::Eighth => 2,
            DurationToken::Sixteenth => 1,
            DurationToken::DottedWhole => 16 + 8,
            DurationToken::DottedHalf => 8 + 4,
            DurationToken::DottedQuarter => 4 + 2,
            DurationToken::DottedEighth => 2 + 1,
            // 付点 16 分 = 16 分 + 32 分。32 分は表現不能なので emit しない (greedy 分解
            // でも選ばれない長さ)。便宜上 sixteenth=1 を返すと greedy の判定で
            // Sixteenth と区別が付かなくなるため、敢えて 0 を返して候補から外す。
            DurationToken::DottedSixteenth => 0,
        }
    }
}

/// greedy 分解の候補。長い順に並べる (DottedSixteenth は除外)。
/// Greedy decomposition candidates, longest first.
const GREEDY_CANDIDATES: &[DurationToken] = &[
    DurationToken::DottedWhole,
    DurationToken::Whole,
    DurationToken::DottedHalf,
    DurationToken::Half,
    DurationToken::DottedQuarter,
    DurationToken::Quarter,
    DurationToken::DottedEighth,
    DurationToken::Eighth,
    DurationToken::Sixteenth,
];

/// tick 長を 16 分音符グリッドで量子化したうえで、`DurationToken` の列に分解する。
///
/// 0 sixteenth に量子化された場合は 1 sixteenth に切り上げる (最小単位)。
/// 量子化誤差は `quantization_error_ticks` で返す。
///
/// Quantizes a tick count onto a 16th-note grid and decomposes it into tokens.
///
/// # Arguments
/// * `ticks` - 量子化対象の tick 数
/// * `ppq` - 1 四分音符あたりの tick 数
///
/// # Returns
/// `(tokens, sixteenths_used, quantization_error_ticks)`
pub fn quantize_ticks(ticks: u64, ppq: u32) -> (Vec<DurationToken>, u32, i64) {
    let ppq = ppq.max(1) as u64;
    let sixteenth_ticks = ppq / 4; // 16 分音符あたりの tick 数
    let sixteenth_ticks = sixteenth_ticks.max(1);
    let raw_sixteenths = (ticks + sixteenth_ticks / 2) / sixteenth_ticks; // 四捨五入
    let sixteenths = raw_sixteenths.max(1) as u32;
    let quantized_ticks = sixteenths as u64 * sixteenth_ticks;
    let error = ticks as i64 - quantized_ticks as i64;

    let tokens = decompose_sixteenths(sixteenths);
    (tokens, sixteenths, error)
}

/// 16 分音符単位の整数長を、`DurationToken` の列に greedy 分解する。
/// Greedy-decomposes a sixteenth-note count into duration tokens.
pub fn decompose_sixteenths(mut sixteenths: u32) -> Vec<DurationToken> {
    let mut out = Vec::new();
    while sixteenths > 0 {
        let mut picked = None;
        for tok in GREEDY_CANDIDATES {
            let len = tok.sixteenths();
            if len > 0 && len <= sixteenths {
                picked = Some(*tok);
                break;
            }
        }
        match picked {
            Some(tok) => {
                sixteenths -= tok.sixteenths();
                out.push(tok);
            }
            None => break, // 到達しないはず (Sixteenth=1 で必ず分解可能)
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(ticks: u64, ppq: u32) -> Vec<&'static str> {
        let (tokens, _, _) = quantize_ticks(ticks, ppq);
        tokens.into_iter().map(|t| t.as_str()).collect()
    }

    #[test]
    fn quarter_note_at_480_ppq() {
        assert_eq!(q(480, 480), vec!["4"]);
    }

    #[test]
    fn eighth_note_at_480_ppq() {
        assert_eq!(q(240, 480), vec!["8"]);
    }

    #[test]
    fn sixteenth_note_at_480_ppq() {
        assert_eq!(q(120, 480), vec!["16"]);
    }

    #[test]
    fn half_note_at_480_ppq() {
        assert_eq!(q(960, 480), vec!["2"]);
    }

    #[test]
    fn whole_note_at_480_ppq() {
        assert_eq!(q(1920, 480), vec!["1"]);
    }

    #[test]
    fn dotted_quarter_at_480_ppq() {
        // 付点四分 = 4分 + 8分 = 6 sixteenths
        assert_eq!(q(720, 480), vec!["4."]);
    }

    #[test]
    fn three_quarters_decomposed_to_dotted_half() {
        // 3 拍 = 半 + 4分 = 付点二分 (12 sixteenths)
        assert_eq!(q(1440, 480), vec!["2."]);
    }

    #[test]
    fn five_sixteenths_decomposed_to_quarter_plus_sixteenth() {
        // 5 sixteenths は 4+1 = quarter + sixteenth
        assert_eq!(q(600, 480), vec!["4", "16"]);
    }

    #[test]
    fn zero_ticks_rounds_up_to_one_sixteenth() {
        // 0 tick は最低 1 sixteenth 扱い
        let (tokens, sixteenths, _err) = quantize_ticks(0, 480);
        assert_eq!(tokens, vec![DurationToken::Sixteenth]);
        assert_eq!(sixteenths, 1);
    }

    #[test]
    fn quantization_error_is_reported() {
        // 250 tick (16分=120, 8分=240, 中途半端な 250) は 8 分に四捨五入される
        let (tokens, sixteenths, error) = quantize_ticks(250, 480);
        assert_eq!(tokens, vec![DurationToken::Eighth]);
        assert_eq!(sixteenths, 2);
        // 入力 250 − 量子化後 240 = 10 tick の誤差
        assert_eq!(error, 10);
    }

    #[test]
    fn different_ppq_96() {
        // PPQ 96 → 16分 = 24 tick
        assert_eq!(q(24, 96), vec!["16"]);
        assert_eq!(q(48, 96), vec!["8"]);
        assert_eq!(q(96, 96), vec!["4"]);
    }

    #[test]
    fn decompose_seven_sixteenths_is_dotted_quarter_plus_sixteenth() {
        // 7 = 6 + 1 = 付点四分 + 16分
        assert_eq!(
            decompose_sixteenths(7),
            vec![DurationToken::DottedQuarter, DurationToken::Sixteenth]
        );
    }

    #[test]
    fn three_sixteenths_decompose_to_dotted_eighth() {
        // 3 sixteenths は付点8分 (2+1=3 sixteenths) として一発で表現できる。
        assert_eq!(decompose_sixteenths(3), vec![DurationToken::DottedEighth]);
    }

    #[test]
    fn dotted_sixteenth_token_is_never_chosen() {
        // 付点16分 (sixteenths() = 0 を返すよう細工してあるトークン) は
        // greedy 分解で選ばれない。表現不能な細かい長さは出てこない。
        let any = (1..=24)
            .flat_map(decompose_sixteenths)
            .any(|t| matches!(t, DurationToken::DottedSixteenth));
        assert!(!any);
    }
}
