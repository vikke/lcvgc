#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpeggioDirection {
    Up,
    Down,
    UpDown,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arpeggio {
    pub direction: ArpeggioDirection,
    pub resolution: u16,
}

fn ws(input: &str) -> &str {
    input.trim_start()
}

fn parse_u16(input: &str) -> Option<(&str, u16)> {
    let end = input.find(|c: char| !c.is_ascii_digit()).unwrap_or(input.len());
    if end == 0 { return None; }
    let val: u16 = input[..end].parse().ok()?;
    Some((&input[end..], val))
}

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

    Some((input, Arpeggio { direction, resolution }))
}

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
