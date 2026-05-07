use nom::{
    bytes::complete::tag, character::complete::char, combinator::opt, multi::separated_list1,
    IResult,
};

use crate::ast::clip_cc::*;
use crate::parser::common::{identifier, parse_u32, parse_u8, ws, ws1};

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

/// スペース区切りのu8値リストをパース
///
/// Parses a whitespace-separated list of u8 values.
pub fn parse_cc_step_values(input: &str) -> IResult<&str, Vec<u8>> {
    separated_list1(ws1, parse_u8)(input)
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

/// ステップ方式の全行パース: `bass.cutoff    0 10 20 30`
///
/// Parses a full step-mode CC automation line: `bass.cutoff    0 10 20 30`
pub fn parse_cc_step(input: &str) -> IResult<&str, CcAutomation> {
    let (input, _) = ws(input)?;
    let (input, target) = parse_cc_target(input)?;
    let (input, _) = ws1(input)?;
    let (input, values) = parse_cc_step_values(input)?;
    let (input, _) = ws(input)?;
    Ok((input, CcAutomation::Step(CcStepValues { target, values })))
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

    #[test]
    fn test_step_values() {
        let (rest, values) = parse_cc_step_values("0 10 20 30").unwrap();
        assert_eq!(rest, "");
        assert_eq!(values, vec![0, 10, 20, 30]);
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

    #[test]
    fn test_full_step_line() {
        let input = "bass.cutoff    0 10 20 30 40 50 60 70 80 90 100 110 120 127 127 127";
        let (rest, auto) = parse_cc_step(input).unwrap();
        assert_eq!(rest, "");
        match auto {
            CcAutomation::Step(step) => {
                assert_eq!(step.target.instrument, "bass");
                assert_eq!(step.target.param, "cutoff");
                assert_eq!(
                    step.values,
                    vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 127, 127, 127]
                );
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
