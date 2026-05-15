use nom::{
    bytes::complete::{tag, take_while},
    character::complete::char,
    combinator::opt,
    multi::separated_list1,
    IResult,
};

use crate::ast::clip_cc::*;
use crate::parser::cell_normalize::CellToken;
use crate::parser::common::{identifier, parse_u32, parse_u8, ws, ws1};

/// CC step 行向けの `(...)*N` 文字列展開。
///
/// drum の `expand_repetition` と異なり、繰り返し境界に **空白を 1 つ挟む**
/// ことで `(0 64)*2` → `0 64 0 64` のように **数値区切り** を保つ。
/// drum はトークンが 1 文字 (`x` / `.` / `X` 等) のため連結で問題ないが、
/// CC は十進数のスペース区切りなので境界に空白が必要。
///
/// CC-step variant of repetition expansion. Unlike the drum version,
/// repeats are joined with a single space so that numeric tokens stay
/// separated. e.g. `(0 64)*2` → `0 64 0 64`.
fn expand_cc_repetition(input: &str) -> String {
    let mut s = input.to_string();
    while let Some((open, close)) = find_innermost_paren(&s) {
        let inner = s[open + 1..close].to_string();
        let after = &s[close + 1..];

        // `) [ws] * [ws] N` の検出 (drum と同様の空白許容)
        // Detect `) [ws] * [ws] N` (whitespace allowed around `*`).
        let after_ws1 = after.trim_start();
        if let Some(after_star) = after_ws1.strip_prefix('*') {
            let after_ws2 = after_star.trim_start();
            let digits_len = after_ws2
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_ws2.len());
            if digits_len > 0 {
                let n: usize = after_ws2[..digits_len].parse().unwrap_or(1);
                // **空白を挟んで** N 回連結し、数値境界を保つ。
                // Join with a single space to keep numeric boundaries.
                let pieces: Vec<&str> = (0..n).map(|_| inner.as_str()).collect();
                let repeated = pieces.join(" ");
                let rest = &after_ws2[digits_len..];
                s = format!("{}{}{}", &s[..open], repeated, rest);
                continue;
            }
        }

        // `*N` 省略は単純グルーピング (drum と同じ)
        // No `*N` → grouping bracket, strip parens.
        s = format!("{}{}{}", &s[..open], inner, after);
    }
    s
}

/// 最内の `(...)` ペアのバイト位置を返す。
/// Returns the byte indices of the innermost `(...)` pair, or `None`.
fn find_innermost_paren(s: &str) -> Option<(usize, usize)> {
    let close = s.find(')')?;
    let open = s[..close].rfind('(')?;
    Some((open, close))
}

/// `instrument.param` 形式のCCターゲットをパース
///
/// Parses a CC target in the format `instrument.param`.
pub fn parse_cc_target(input: &str) -> IResult<&str, CcTarget> {
    let (input, instrument) = identifier(input)?;
    let (input, _) = char('.')(input)?;
    let (input, param) = identifier(input)?;
    Ok((
        input,
        CcTarget {
            instrument: instrument.to_string(),
            param: param.to_string(),
        },
    ))
}

/// CC step 行の生文字列を `CellToken<Option<u8>>` 列にトークナイズする。
///
/// 認識する要素:
///   - `0`-`127` の連続数字 → `CellToken::Cell(Some(u8))`
///   - `.` → `CellToken::Cell(None)` (この step では CC を送出しない)
///   - `|` → `CellToken::Pipe` (拍境界スナップ)
///   - `>N` → `CellToken::BarJump(N)` (小節 N へ絶対位置スナップ; 1 始まり)
///   - 空白 (スペース・タブ) → 区切りとして読み飛ばす
///
/// `(...)*N` は文字列レイヤで `clip_drum::expand_repetition` により事前展開
/// 済みであることを想定しているため、ここでは扱わない。
///
/// Tokenize a raw CC-step line into `CellToken<Option<u8>>`s. Recognises
/// decimal numbers in `0..=127`, `.` (skip), `|` (beat-boundary snap),
/// and `>N` (1-based absolute bar jump). Whitespace (spaces and tabs) is
/// treated as a separator. `(...)*N` is expected to have been expanded
/// at the string layer by `clip_drum::expand_repetition` before calling.
///
/// # Errors
/// 未知の文字が現れた場合、または数値が `0..=127` の範囲外の場合に `Err` を返す。
/// Returns `Err` for unknown characters or out-of-range numbers.
pub fn tokenize_cc_step_pattern(input: &str) -> Result<Vec<CellToken<Option<u8>>>, String> {
    let mut out: Vec<CellToken<Option<u8>>> = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        if c == '.' {
            out.push(CellToken::Cell(None));
            i += 1;
            continue;
        }
        if c == '|' {
            out.push(CellToken::Pipe);
            i += 1;
            continue;
        }
        if c == '>' {
            // `>N` 形式: `>` の次に数字
            // `>N` form: `>` followed by digits
            let mut j = i + 1;
            let start = j;
            while j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
                j += 1;
            }
            if j == start {
                return Err(format!(
                    "CC step トークナイズ: `>` の後に数字が必要です (位置 {})",
                    i
                ));
            }
            let n: u32 = input[start..j]
                .parse()
                .map_err(|e| format!("CC step トークナイズ: 小節番号のパースに失敗: {}", e))?;
            out.push(CellToken::BarJump(n));
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
                j += 1;
            }
            let raw = &input[i..j];
            let v: u32 = raw
                .parse()
                .map_err(|e| format!("CC step トークナイズ: 数値パース失敗 '{}': {}", raw, e))?;
            if v > 127 {
                return Err(format!(
                    "CC step トークナイズ: 値 {} は 0..=127 の範囲外です",
                    v
                ));
            }
            out.push(CellToken::Cell(Some(v as u8)));
            i = j;
            continue;
        }
        return Err(format!(
            "CC step トークナイズ: 認識できない文字 '{}' (位置 {})",
            c, i
        ));
    }
    Ok(out)
}

/// `value@bar.beat` 形式のタイムポイントをパース
///
/// Parses a time point in the format `value@bar.beat`.
pub fn parse_cc_time_point(input: &str) -> IResult<&str, CcTimePoint> {
    let (input, value) = parse_u8(input)?;
    let (input, _) = char('@')(input)?;
    let (input, bar) = parse_u32(input)?;
    let (input, _) = char('.')(input)?;
    let (input, beat) = parse_u32(input)?;
    Ok((input, CcTimePoint { value, bar, beat }))
}

/// タイムセグメントをパース
///
/// `0@1.1` or `0@1.1-127@3.1` or `0@1.1-exp127@4.4`
///
/// Parses a time segment. Supports single points, linear ranges,
/// and exponential interpolation ranges.
pub fn parse_cc_time_segment(input: &str) -> IResult<&str, CcTimeSegment> {
    let (input, from) = parse_cc_time_point(input)?;
    let (input, to) = opt(|input| {
        let (input, _) = char('-')(input)?;
        let (input, interp) =
            if let Ok((input, _)) = tag::<&str, &str, nom::error::Error<&str>>("exp")(input) {
                (input, Interpolation::Exponential)
            } else {
                (input, Interpolation::Linear)
            };
        let (input, point) = parse_cc_time_point(input)?;
        Ok((input, (interp, point)))
    })(input)?;
    Ok((input, CcTimeSegment { from, to }))
}

/// ステップ方式の全行パース: `bass.cutoff    0 . 20 | (30 40)*2 >3 64`
///
/// CC step 行は **改行までを 1 行として** 切り出し、文字列段で
/// `(...)*N` を展開してからトークナイズする (drum と整合)。`.`/`|`/`>N`/`*N`
/// のメタトークンを許可し、AST には正規化前の `CellToken` 列として保持する。
/// 拍境界 (`|`) や 小節境界 (`>N`) の最終解決は clip 全体の resolution と
/// bars を踏まえてコンパイル時に行う。
///
/// Parses a full step-mode CC automation line. The cell text after the
/// `instrument.param` header is taken up to end-of-line, `(...)*N` is
/// expanded at the string layer, then tokenized. Meta tokens
/// `.` / `|` / `>N` are accepted; final beat / bar resolution is deferred
/// to compile time so it can use the clip's resolution and bars.
pub fn parse_cc_step(input: &str) -> IResult<&str, CcAutomation> {
    let (input, _) = ws(input)?;
    let (input, target) = parse_cc_target(input)?;
    let (input, _) = ws1(input)?;
    // 改行までを 1 行として切り出す
    // Take the cell payload up to end-of-line as a single line.
    let (input, line) = take_while(|c: char| c != '\n')(input)?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeWhile1,
        )));
    }
    // (...)*N を文字列段で展開し、メタトークン込みでトークナイズする
    // Expand `(...)*N` at the string layer, then tokenize cells/meta.
    let expanded = expand_cc_repetition(trimmed);
    let cells = tokenize_cc_step_pattern(&expanded).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))
    })?;
    if cells.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeWhile1,
        )));
    }
    Ok((input, CcAutomation::Step(CcStepValues { target, cells })))
}

/// 時間指定方式の全行パース: `bass.cutoff 0@1.1-127@3.1 64@4.1`
///
/// Parses a full time-mode CC automation line: `bass.cutoff 0@1.1-127@3.1 64@4.1`
pub fn parse_cc_time(input: &str) -> IResult<&str, CcAutomation> {
    let (input, _) = ws(input)?;
    let (input, target) = parse_cc_target(input)?;
    let (input, _) = ws1(input)?;
    let (input, segments) = separated_list1(ws1, parse_cc_time_segment)(input)?;
    let (input, _) = ws(input)?;
    Ok((input, CcAutomation::Time(CcTimeValues { target, segments })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CC ターゲット `instrument.param` がパースできる
    /// `instrument.param` CC target parses correctly.
    #[test]
    fn test_cc_target() {
        let (rest, target) = parse_cc_target("bass.cutoff").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            target,
            CcTarget {
                instrument: "bass".to_string(),
                param: "cutoff".to_string(),
            }
        );
    }

    // ------------------------------------------------------------------
    // expand_cc_repetition
    // ------------------------------------------------------------------

    /// CC step は数値間に空白を挟んで展開する
    /// CC-step expansion preserves numeric whitespace boundaries.
    #[test]
    fn cc_repetition_keeps_numeric_boundary() {
        assert_eq!(expand_cc_repetition("(0 64)*2"), "0 64 0 64");
    }

    /// ネストした `(...)*N` も内側から展開
    /// Nested repetitions expand from the innermost.
    #[test]
    fn cc_repetition_nested() {
        // ((0 1)*2)*2 → 内側 "0 1 0 1" → 外側 "0 1 0 1 0 1 0 1"
        let out = expand_cc_repetition("((0 1)*2)*2");
        // 空白の正規化はしないので余分な空白が混じる可能性があるが、
        // 数値境界が崩れないこと (連結数字が無いこと) だけ確認。
        assert!(!out.contains("10")); // 連結 "10" は出てこないはず
        let nums: Vec<&str> = out.split_ascii_whitespace().collect();
        assert_eq!(nums, vec!["0", "1", "0", "1", "0", "1", "0", "1"]);
    }

    /// `*N` 省略はグルーピング扱い (count=1)
    /// Omitting `*N` treats parens as grouping (count=1).
    #[test]
    fn cc_repetition_grouping_no_count() {
        let out = expand_cc_repetition("(0 64)");
        let nums: Vec<&str> = out.split_ascii_whitespace().collect();
        assert_eq!(nums, vec!["0", "64"]);
    }

    // ------------------------------------------------------------------
    // tokenize_cc_step_pattern
    // ------------------------------------------------------------------

    /// 数値のみは Some(u8) のセル列になる
    /// Numbers-only input becomes `Cell(Some(u8))` sequence.
    #[test]
    fn tokenize_numbers_only() {
        let cells = tokenize_cc_step_pattern("0 10 20 30").unwrap();
        assert_eq!(
            cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Cell(Some(10)),
                CellToken::Cell(Some(20)),
                CellToken::Cell(Some(30)),
            ]
        );
    }

    /// `.` が `Cell(None)` に変換される
    /// `.` becomes `Cell(None)`.
    #[test]
    fn tokenize_dot_becomes_none() {
        let cells = tokenize_cc_step_pattern("0 . 20 .").unwrap();
        assert_eq!(
            cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Cell(None),
                CellToken::Cell(Some(20)),
                CellToken::Cell(None),
            ]
        );
    }

    /// `|` が `Pipe` トークンになる
    /// `|` becomes `Pipe`.
    #[test]
    fn tokenize_pipe() {
        let cells = tokenize_cc_step_pattern("0 | 20").unwrap();
        assert_eq!(
            cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Pipe,
                CellToken::Cell(Some(20)),
            ]
        );
    }

    /// `>N` が `BarJump(N)` トークンになる
    /// `>N` becomes `BarJump(N)`.
    #[test]
    fn tokenize_bar_jump() {
        let cells = tokenize_cc_step_pattern("0 >3 64").unwrap();
        assert_eq!(
            cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::BarJump(3),
                CellToken::Cell(Some(64)),
            ]
        );
    }

    /// 値が 0..=127 の範囲外であればエラー
    /// Out-of-range value returns an error.
    #[test]
    fn tokenize_out_of_range_is_error() {
        let err = tokenize_cc_step_pattern("128");
        assert!(err.is_err());
    }

    /// 認識できない文字はエラー
    /// Unknown characters yield an error.
    #[test]
    fn tokenize_unknown_char_is_error() {
        let err = tokenize_cc_step_pattern("0 a 20");
        assert!(err.is_err());
    }

    /// `>` の後に数字が無いとエラー
    /// `>` without a following number errors out.
    #[test]
    fn tokenize_bar_jump_without_number_is_error() {
        let err = tokenize_cc_step_pattern(">");
        assert!(err.is_err());
    }

    /// 連続するメタトークンも正しく分解される
    /// Consecutive meta tokens are split correctly.
    #[test]
    fn tokenize_consecutive_meta_tokens() {
        let cells = tokenize_cc_step_pattern(".|.>2.").unwrap();
        assert_eq!(
            cells,
            vec![
                CellToken::Cell(None),
                CellToken::Pipe,
                CellToken::Cell(None),
                CellToken::BarJump(2),
                CellToken::Cell(None),
            ]
        );
    }

    // ------------------------------------------------------------------
    // parse_cc_step (line-level)
    // ------------------------------------------------------------------

    /// 既存互換: 数値のみのステップ列
    /// Legacy: numbers-only step line.
    #[test]
    fn parse_step_numbers_only() {
        let (_, auto) = parse_cc_step("bass.cutoff 0 10 20 30").unwrap();
        let CcAutomation::Step(step) = auto else {
            panic!("expected Step");
        };
        assert_eq!(step.target.instrument, "bass");
        assert_eq!(step.target.param, "cutoff");
        assert_eq!(
            step.cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Cell(Some(10)),
                CellToken::Cell(Some(20)),
                CellToken::Cell(Some(30)),
            ]
        );
    }

    /// `(...)*N` が文字列段で展開されてからトークナイズされる
    /// `(...)*N` is expanded at string layer before tokenization.
    #[test]
    fn parse_step_repetition_expanded() {
        let (_, auto) = parse_cc_step("bass.cutoff (0 64)*2").unwrap();
        let CcAutomation::Step(step) = auto else {
            panic!("expected Step");
        };
        assert_eq!(
            step.cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Cell(Some(64)),
                CellToken::Cell(Some(0)),
                CellToken::Cell(Some(64)),
            ]
        );
    }

    /// `.` / `|` / `>N` を混在させた行
    /// Mixed line with `.` / `|` / `>N`.
    #[test]
    fn parse_step_with_meta_tokens() {
        let (_, auto) = parse_cc_step("bass.cutoff 0 . | >3 64").unwrap();
        let CcAutomation::Step(step) = auto else {
            panic!("expected Step");
        };
        assert_eq!(
            step.cells,
            vec![
                CellToken::Cell(Some(0)),
                CellToken::Cell(None),
                CellToken::Pipe,
                CellToken::BarJump(3),
                CellToken::Cell(Some(64)),
            ]
        );
    }

    /// 改行手前までを 1 行として扱う (次行は消費しない)
    /// Stops at the line boundary; the next line is not consumed.
    #[test]
    fn parse_step_stops_at_newline() {
        let input = "bass.cutoff 0 10\nbass.cutoff 20 30";
        let (rest, _) = parse_cc_step(input).unwrap();
        // 改行以降が残る
        assert!(rest.starts_with('\n'));
    }

    #[test]
    fn test_time_point() {
        let (rest, point) = parse_cc_time_point("64@2.1").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            point,
            CcTimePoint {
                value: 64,
                bar: 2,
                beat: 1
            }
        );
    }

    #[test]
    fn test_time_segment_linear() {
        let (rest, seg) = parse_cc_time_segment("0@1.1-127@3.1").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            seg.from,
            CcTimePoint {
                value: 0,
                bar: 1,
                beat: 1
            }
        );
        assert_eq!(
            seg.to,
            Some((
                Interpolation::Linear,
                CcTimePoint {
                    value: 127,
                    bar: 3,
                    beat: 1
                }
            ))
        );
    }

    #[test]
    fn test_time_segment_exp() {
        let (rest, seg) = parse_cc_time_segment("0@1.1-exp127@4.4").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            seg.from,
            CcTimePoint {
                value: 0,
                bar: 1,
                beat: 1
            }
        );
        assert_eq!(
            seg.to,
            Some((
                Interpolation::Exponential,
                CcTimePoint {
                    value: 127,
                    bar: 4,
                    beat: 4
                }
            ))
        );
    }

    #[test]
    fn test_time_segment_no_interp() {
        let (rest, seg) = parse_cc_time_segment("64@4.1").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            seg.from,
            CcTimePoint {
                value: 64,
                bar: 4,
                beat: 1
            }
        );
        assert_eq!(seg.to, None);
    }

    /// 全行: 数値のみの step 行
    /// Full line: numbers-only step line.
    #[test]
    fn test_full_step_line() {
        let input = "bass.cutoff    0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127";
        let (rest, auto) = parse_cc_step(input).unwrap();
        assert_eq!(rest, "");
        match auto {
            CcAutomation::Step(step) => {
                assert_eq!(step.target.instrument, "bass");
                assert_eq!(step.target.param, "cutoff");
                let expected: Vec<CellToken<Option<u8>>> = [
                    0u8, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 127, 127, 127,
                ]
                .iter()
                .map(|v| CellToken::Cell(Some(*v)))
                .collect();
                assert_eq!(step.cells, expected);
            }
            _ => panic!("expected Step"),
        }
    }

    #[test]
    fn test_full_time_line() {
        let input = "bass.cutoff 0@1.1-127@3.1 64@4.1";
        let (rest, auto) = parse_cc_time(input).unwrap();
        assert_eq!(rest, "");
        match auto {
            CcAutomation::Time(time) => {
                assert_eq!(time.target.instrument, "bass");
                assert_eq!(time.target.param, "cutoff");
                assert_eq!(time.segments.len(), 2);
                assert_eq!(
                    time.segments[0].to,
                    Some((
                        Interpolation::Linear,
                        CcTimePoint {
                            value: 127,
                            bar: 3,
                            beat: 1
                        }
                    ))
                );
                assert_eq!(
                    time.segments[1].from,
                    CcTimePoint {
                        value: 64,
                        bar: 4,
                        beat: 1
                    }
                );
                assert_eq!(time.segments[1].to, None);
            }
            _ => panic!("expected Time"),
        }
    }
}
