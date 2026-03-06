use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::char;
use nom::IResult;

use crate::ast::scale::{ScaleDef, ScaleType};
use crate::parser::common::{note_name, parse_u32, parse_u8, ws, ws1};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipOptions {
    pub bars: Option<u32>,
    pub time_sig: Option<(u8, u8)>,
    pub scale: Option<ScaleDef>,
}

/// Parse a scale type keyword.
fn scale_type(input: &str) -> IResult<&str, ScaleType> {
    alt((
        |i| tag("harmonic_minor")(i).map(|(r, _)| (r, ScaleType::HarmonicMinor)),
        |i| tag("melodic_minor")(i).map(|(r, _)| (r, ScaleType::MelodicMinor)),
        |i| tag("major")(i).map(|(r, _)| (r, ScaleType::Major)),
        |i| tag("minor")(i).map(|(r, _)| (r, ScaleType::Minor)),
        |i| tag("dorian")(i).map(|(r, _)| (r, ScaleType::Dorian)),
        |i| tag("phrygian")(i).map(|(r, _)| (r, ScaleType::Phrygian)),
        |i| tag("lydian")(i).map(|(r, _)| (r, ScaleType::Lydian)),
        |i| tag("mixolydian")(i).map(|(r, _)| (r, ScaleType::Mixolydian)),
        |i| tag("locrian")(i).map(|(r, _)| (r, ScaleType::Locrian)),
    ))(input)
}

/// Parse `[bars N]`
fn parse_bars_option(input: &str) -> IResult<&str, ClipOptions> {
    let (input, _) = char('[')(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("bars")(input)?;
    let (input, _) = ws1(input)?;
    let (input, n) = parse_u32(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char(']')(input)?;
    Ok((
        input,
        ClipOptions {
            bars: Some(n),
            ..Default::default()
        },
    ))
}

/// Parse `[time N/N]`
fn parse_time_option(input: &str) -> IResult<&str, ClipOptions> {
    let (input, _) = char('[')(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("time")(input)?;
    let (input, _) = ws1(input)?;
    let (input, num) = parse_u8(input)?;
    let (input, _) = char('/')(input)?;
    let (input, den) = parse_u8(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char(']')(input)?;
    Ok((
        input,
        ClipOptions {
            time_sig: Some((num, den)),
            ..Default::default()
        },
    ))
}

/// Parse `[scale ROOT TYPE]`
fn parse_scale_option(input: &str) -> IResult<&str, ClipOptions> {
    let (input, _) = char('[')(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("scale")(input)?;
    let (input, _) = ws1(input)?;
    let (input, root) = note_name(input)?;
    let (input, _) = ws1(input)?;
    let (input, st) = scale_type(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char(']')(input)?;
    Ok((
        input,
        ClipOptions {
            scale: Some(ScaleDef {
                root,
                scale_type: st,
            }),
            ..Default::default()
        },
    ))
}

/// Parse a single clip option bracket.
fn parse_single_option(input: &str) -> IResult<&str, ClipOptions> {
    alt((parse_bars_option, parse_time_option, parse_scale_option))(input)
}

/// Merge two ClipOptions, with `other` overriding fields set in it.
fn merge(base: ClipOptions, other: ClipOptions) -> ClipOptions {
    ClipOptions {
        bars: other.bars.or(base.bars),
        time_sig: other.time_sig.or(base.time_sig),
        scale: other.scale.or(base.scale),
    }
}

/// Parse zero or more clip options like `[bars 1] [time 3/4] [scale c minor]`.
/// Options can appear in any order.
pub fn parse_clip_options(input: &str) -> IResult<&str, ClipOptions> {
    let mut result = ClipOptions::default();
    let mut remaining = input;

    loop {
        let (r, _) = ws(remaining)?;
        remaining = r;
        match parse_single_option(remaining) {
            Ok((r, opt)) => {
                result = merge(result, opt);
                remaining = r;
            }
            Err(_) => break,
        }
    }

    Ok((remaining, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::common::NoteName;
    use crate::ast::scale::{ScaleDef, ScaleType};

    #[test]
    fn test_bars_only() {
        let (rest, opts) = parse_clip_options("[bars 1]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.bars, Some(1));
        assert_eq!(opts.time_sig, None);
        assert_eq!(opts.scale, None);
    }

    #[test]
    fn test_time_only() {
        let (rest, opts) = parse_clip_options("[time 3/4]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.time_sig, Some((3, 4)));
        assert_eq!(opts.bars, None);
    }

    #[test]
    fn test_scale_only() {
        let (rest, opts) = parse_clip_options("[scale c minor]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            opts.scale,
            Some(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Minor,
            })
        );
    }

    #[test]
    fn test_bars_and_scale() {
        let (rest, opts) = parse_clip_options("[bars 4] [scale c minor]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.bars, Some(4));
        assert_eq!(
            opts.scale,
            Some(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Minor,
            })
        );
    }

    #[test]
    fn test_full_options() {
        let (rest, opts) = parse_clip_options("[bars 2] [time 3/4] [scale d dorian]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.bars, Some(2));
        assert_eq!(opts.time_sig, Some((3, 4)));
        assert_eq!(
            opts.scale,
            Some(ScaleDef {
                root: NoteName::D,
                scale_type: ScaleType::Dorian,
            })
        );
    }

    #[test]
    fn test_no_options() {
        let (rest, opts) = parse_clip_options("").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts, ClipOptions::default());
    }

    #[test]
    fn test_reversed_order() {
        let (rest, opts) = parse_clip_options("[scale c minor] [bars 2]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.bars, Some(2));
        assert_eq!(
            opts.scale,
            Some(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Minor,
            })
        );
    }

    #[test]
    fn test_stops_at_non_option() {
        let (rest, opts) = parse_clip_options("[bars 2] { }").unwrap();
        assert_eq!(rest, "{ }");
        assert_eq!(opts.bars, Some(2));
    }
}
