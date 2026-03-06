use nom::character::complete::char;
use nom::IResult;

use crate::parser::common::parse_u32;

/// 小節ジャンプを表す構造体。
/// Represents a bar jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarJump {
    /// ジャンプ先の小節番号（1始まり）
    /// Target bar number (1-based)
    pub bar_number: u32,
}

/// `>N` 形式の小節ジャンプをパースする。
/// Parses a bar jump in `>N` format.
pub fn parse_bar_jump(input: &str) -> IResult<&str, BarJump> {
    let (input, _) = char('>')(input)?;
    let (input, bar_number) = parse_u32(input)?;
    Ok((input, BarJump { bar_number }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_jump_3() {
        let (rest, bj) = parse_bar_jump(">3").unwrap();
        assert_eq!(bj, BarJump { bar_number: 3 });
        assert_eq!(rest, "");
    }

    #[test]
    fn test_bar_jump_1() {
        let (rest, bj) = parse_bar_jump(">1").unwrap();
        assert_eq!(bj, BarJump { bar_number: 1 });
        assert_eq!(rest, "");
    }

    #[test]
    fn test_bar_jump_12() {
        let (rest, bj) = parse_bar_jump(">12").unwrap();
        assert_eq!(bj, BarJump { bar_number: 12 });
        assert_eq!(rest, "");
    }
}
