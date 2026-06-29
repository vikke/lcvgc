use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::char;
use nom::IResult;

use crate::ast::scale::{ScaleDef, ScaleType};
use crate::parser::common::{note_name, parse_u32, parse_u8, ws, ws1};

/// クリップに付与できるオプション群を保持する構造体。
/// `[bars N]`、`[time N/N]`、`[scale ROOT TYPE]` の各指定を格納する。
///
/// A struct that holds clip-level options.
/// Stores `[bars N]`, `[time N/N]`, and `[scale ROOT TYPE]` specifications.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipOptions {
    /// 小節数の指定（例: `[bars 4]`）。
    ///
    /// Number of bars (e.g. `[bars 4]`).
    pub bars: Option<u32>,
    /// 拍子の指定（分子, 分母）（例: `[time 3/4]`）。
    ///
    /// Time signature as (numerator, denominator) (e.g. `[time 3/4]`).
    pub time_sig: Option<(u8, u8)>,
    /// スケールの指定（例: `[scale c minor]`）。
    ///
    /// Scale specification (e.g. `[scale c minor]`).
    pub scale: Option<ScaleDef>,
    /// オクターブシフト量（例: `[>>]` で +1、`[<<]` で -1、`[>> >>]` で +2）。
    /// clip 全体のピッチを 12 半音単位で上下させる。`>>` と `<<` は合算される。
    /// 既定値 0（シフトなし）。ピッチド clip にのみ効果がある。
    ///
    /// Octave shift amount (e.g. `[>>]` is +1, `[<<]` is -1, `[>> >>]` is +2).
    /// Transposes the whole clip in 12-semitone steps. `>>` and `<<` accumulate.
    /// Defaults to 0 (no shift). Only affects pitched clips.
    pub octave_shift: i8,
}

/// スケールタイプのキーワードをパースする。
///
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

/// `[bars N]` 形式の小節数指定をパースする。
///
/// Parse `[bars N]` option.
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

/// `[time N/N]` 形式の拍子指定をパースする。
///
/// Parse `[time N/N]` option.
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

/// `[scale ROOT TYPE]` 形式のスケール指定をパースする。
///
/// Parse `[scale ROOT TYPE]` option.
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

/// `[>>]` / `[<<]` 形式のオクターブシフト指定をパースする。
/// 括弧内には `>>`（+1 オクターブ）と `<<`（-1 オクターブ）を空白区切りで
/// 1 個以上並べられ、それらの合計がシフト量となる（例: `[>> >>]` で +2、
/// `[>> <<]` で ±0）。
///
/// Parse a `[>>]` / `[<<]` octave-shift option. The bracket holds one or more
/// `>>` (+1 octave) / `<<` (-1 octave) tokens separated by whitespace; their
/// sum is the shift amount (e.g. `[>> >>]` is +2, `[>> <<]` is 0).
fn parse_octave_shift_option(input: &str) -> IResult<&str, ClipOptions> {
    let (input, _) = char('[')(input)?;
    let (mut input, _) = ws(input)?;

    let mut shift: i32 = 0;
    let mut count = 0usize;
    loop {
        if let Ok((r, _)) = tag::<_, _, nom::error::Error<&str>>(">>")(input) {
            shift += 1;
            input = r;
        } else if let Ok((r, _)) = tag::<_, _, nom::error::Error<&str>>("<<")(input) {
            shift -= 1;
            input = r;
        } else {
            break;
        }
        count += 1;
        let (r, _) = ws(input)?;
        input = r;
    }

    // `>>` / `<<` が 1 つも無い `[]` はこのオプションとして扱わない。
    // An empty bracket with no `>>` / `<<` is not an octave-shift option.
    if count == 0 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }

    let (input, _) = char(']')(input)?;
    Ok((
        input,
        ClipOptions {
            // シフト量は i8 に収める。極端な多重指定は飽和させる。
            // Clamp the accumulated shift into i8; saturate on extreme repeats.
            octave_shift: shift.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
            ..Default::default()
        },
    ))
}

/// 単一のクリップオプション括弧をパースする。
///
/// Parse a single clip option bracket.
fn parse_single_option(input: &str) -> IResult<&str, ClipOptions> {
    alt((
        parse_bars_option,
        parse_time_option,
        parse_scale_option,
        parse_octave_shift_option,
    ))(input)
}

/// 2つの `ClipOptions` をマージする。`other` に設定されたフィールドが優先される。
///
/// Merge two `ClipOptions`, with `other` overriding fields set in it.
fn merge(base: ClipOptions, other: ClipOptions) -> ClipOptions {
    ClipOptions {
        bars: other.bars.or(base.bars),
        time_sig: other.time_sig.or(base.time_sig),
        scale: other.scale.or(base.scale),
        // オクターブシフトは括弧をまたいで合算する（`[>>] [>>]` で +2）。
        // Octave shifts accumulate across brackets (`[>>] [>>]` is +2).
        octave_shift: base.octave_shift.saturating_add(other.octave_shift),
    }
}

/// `[bars 1] [time 3/4] [scale c minor]` のようなクリップオプションを
/// 0個以上パースする。オプションは任意の順序で指定可能。
///
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
    use crate::ast::scale::{ScaleDef, ScaleType};
    use crate::domain::pitch::NoteName;

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

    #[test]
    fn test_octave_shift_up() {
        let (rest, opts) = parse_clip_options("[>>]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 1);
    }

    #[test]
    fn test_octave_shift_down() {
        let (rest, opts) = parse_clip_options("[<<]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, -1);
    }

    #[test]
    fn test_octave_shift_up_two() {
        let (rest, opts) = parse_clip_options("[>> >>]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 2);
    }

    #[test]
    fn test_octave_shift_mixed_cancels() {
        let (rest, opts) = parse_clip_options("[>> <<]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 0);
    }

    #[test]
    fn test_octave_shift_mixed_nets_up() {
        let (rest, opts) = parse_clip_options("[>> >> <<]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 1);
    }

    #[test]
    fn test_octave_shift_with_other_options() {
        let (rest, opts) = parse_clip_options("[bars 2] [>>] [scale c minor]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.bars, Some(2));
        assert_eq!(opts.octave_shift, 1);
        assert_eq!(
            opts.scale,
            Some(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Minor,
            })
        );
    }

    #[test]
    fn test_octave_shift_separate_brackets_accumulate() {
        let (rest, opts) = parse_clip_options("[>>] [>>]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 2);
    }

    #[test]
    fn test_no_options_octave_shift_default_zero() {
        let (rest, opts) = parse_clip_options("").unwrap();
        assert_eq!(rest, "");
        assert_eq!(opts.octave_shift, 0);
    }

    #[test]
    fn test_octave_shift_stops_at_brace() {
        let (rest, opts) = parse_clip_options("[>>] {").unwrap();
        assert_eq!(rest, "{");
        assert_eq!(opts.octave_shift, 1);
    }
}
