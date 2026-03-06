/// アルペジオの方向を表す列挙型
/// Enum representing the direction of an arpeggio
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpeggioDirection {
    /// 上昇
    /// Ascending
    Up,
    /// 下降
    /// Descending
    Down,
    /// 上昇→下降の往復
    /// Ascending then descending (ping-pong)
    UpDown,
    /// ランダム順
    /// Random order
    Random,
}

/// アルペジオ設定（方向と分解能）
/// Arpeggio settings (direction and resolution)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arpeggio {
    /// アルペジオの方向
    /// Direction of the arpeggio
    pub direction: ArpeggioDirection,
    /// 分解能（音符の細かさ、例: 16 = 16分音符）
    /// Resolution (note subdivision, e.g. 16 = sixteenth notes)
    pub resolution: u16,
}

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

/// `arp(direction, resolution)` 形式のアルペジオ指定をパースする。
/// Parse an arpeggio specification in the form `arp(direction, resolution)`.
pub fn parse_arpeggio(input: &str) -> Option<(&str, Arpeggio)> {
    let input = input.strip_prefix("arp")?;
    let input = ws(input);
    let input = input.strip_prefix('(')?;
    let input = ws(input);

    let (input, direction) = parse_direction(input)?;
    let input = ws(input);
    let input = input.strip_prefix(',')?;
    let input = ws(input);

    let (input, resolution) = parse_u16(input)?;
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
        assert_eq!(arp.resolution, 16);
    }

    #[test]
    fn test_arp_down_16() {
        let (rest, arp) = parse_arpeggio("arp(down, 16)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Down);
        assert_eq!(arp.resolution, 16);
    }

    #[test]
    fn test_arp_updown_16() {
        let (rest, arp) = parse_arpeggio("arp(updown, 16)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::UpDown);
        assert_eq!(arp.resolution, 16);
    }

    #[test]
    fn test_arp_random_8() {
        let (rest, arp) = parse_arpeggio("arp(random, 8)").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Random);
        assert_eq!(arp.resolution, 8);
    }

    #[test]
    fn test_arp_with_spaces() {
        let (rest, arp) = parse_arpeggio("arp( up , 16 )").unwrap();
        assert_eq!(rest, "");
        assert_eq!(arp.direction, ArpeggioDirection::Up);
        assert_eq!(arp.resolution, 16);
    }
}
