use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};

/// 先頭の空白文字を除去する。
/// Trim leading whitespace.
fn ws(input: &str) -> &str {
    input.trim_start()
}

/// u16整数をパースする。
/// Parse a u16 integer.
fn parse_u16(input: &str) -> Option<(&str, u16)> {
    let end = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    if end == 0 {
        return None;
    }
    let val: u16 = input[..end].parse().ok()?;
    Some((&input[end..], val))
}

/// `arp(direction)` または `arp(direction, resolution)` 形式のアルペジオ指定をパースする。
/// 第2引数は省略可。
///
/// Parse an arpeggio specification in either `arp(direction)` or
/// `arp(direction, resolution)` form. The second argument is optional.
pub fn parse_arpeggio(input: &str) -> Option<(&str, Arpeggio)> {
    let input = input.strip_prefix("arp")?;
    let input = ws(input);
    let input = input.strip_prefix('(')?;
    let input = ws(input);

    let (input, direction) = parse_direction(input)?;
    let input = ws(input);

    // resolution は省略可。`,` を見たら必ず数値が続く必要がある。
    // The resolution is optional; if a comma appears it must be followed by digits.
    let (input, resolution) = if let Some(after_comma) = input.strip_prefix(',') {
        let after_comma = ws(after_comma);
        let (rest, val) = parse_u16(after_comma)?;
        (rest, Some(val))
    } else {
        (input, None)
    };

    let input = ws(input);
    let input = input.strip_prefix(')')?;

    Some((
        input,
        Arpeggio {
            direction,
            resolution,
        },
    ))
}

/// アルペジオの方向キーワードをパースする。
/// Parse an arpeggio direction keyword.
fn parse_direction(input: &str) -> Option<(&str, ArpeggioDirection)> {
    if let Some(rest) = input.strip_prefix("updown") {
        Some((rest, ArpeggioDirection::UpDown))
    } else if let Some(rest) = input.strip_prefix("up") {
        Some((rest, ArpeggioDirection::Up))
    } else if let Some(rest) = input.strip_prefix("down") {
        Some((rest, ArpeggioDirection::Down))
    } else if let Some(rest) = input.strip_prefix("random") {
        Some((rest, ArpeggioDirection::Random))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arp_up_16() {
        let (rest, arp) = parse_arpeggio("arp(up, 16)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Up);
        assert_eq!(arp.resolution, Some(16));
    }

    #[test]
    fn test_arp_down_16() {
        let (rest, arp) = parse_arpeggio("arp(down, 16)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Down);
        assert_eq!(arp.resolution, Some(16));
    }

    #[test]
    fn test_arp_updown_16() {
        let (rest, arp) = parse_arpeggio("arp(updown, 16)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::UpDown);
        assert_eq!(arp.resolution, Some(16));
    }

    #[test]
    fn test_arp_random_8() {
        let (rest, arp) = parse_arpeggio("arp(random, 8)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Random);
        assert_eq!(arp.resolution, Some(8));
    }

    #[test]
    fn test_arp_with_spaces() {
        let (rest, arp) = parse_arpeggio("arp( up , 16 )").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Up);
        assert_eq!(arp.resolution, Some(16));
    }

    /// resolution を省略した `arp(up)` 形式が、direction のみを保持してパースされること。
    /// `arp(up)` (no resolution) parses with direction only.
    #[test]
    fn test_arp_up_no_resolution() {
        let (rest, arp) = parse_arpeggio("arp(up)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Up);
        assert_eq!(arp.resolution, None);
    }

    /// 空白入りの `arp( down )` も resolution なしで成功する。
    /// `arp( down )` (whitespace, no resolution) parses successfully.
    #[test]
    fn test_arp_down_no_resolution_with_spaces() {
        let (rest, arp) = parse_arpeggio("arp( down )").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Down);
        assert_eq!(arp.resolution, None);
    }

    /// `arp(up,)` のように `,` の後に数値が無い場合はパース失敗とする。
    /// `arp(up,)` (trailing comma without digits) must fail.
    #[test]
    fn test_arp_trailing_comma_fails() {
        assert!(parse_arpeggio("arp(up,)").is_none());
    }

    /// `arp(up,  )` のように `,` のあと空白だけでもパース失敗。
    /// `arp(up,  )` (comma followed by only whitespace) must fail.
    #[test]
    fn test_arp_comma_only_whitespace_fails() {
        assert!(parse_arpeggio("arp(up,  )").is_none());
    }
}
