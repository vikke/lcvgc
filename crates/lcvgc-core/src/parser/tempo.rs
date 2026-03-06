use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{char, u16 as nom_u16},
    combinator::map,
    sequence::{pair, preceded},
    IResult,
};

use crate::ast::tempo::Tempo;
use crate::parser::common::ws1;

/// 相対テンポ値をパースする: `+N` または `-N`
/// Parse a relative tempo value: `+N` or `-N`.
fn relative_tempo(input: &str) -> IResult<&str, Tempo> {
    let positive = map(preceded(char('+'), nom_u16), |v| Tempo::Relative(v as i16));
    let negative = map(pair(char('-'), nom_u16), |(_, v)| {
        Tempo::Relative(-(v as i16))
    });
    alt((positive, negative))(input)
}

/// テンポ定義をパースする: `tempo <value>`
/// Parse `tempo <value>`.
pub fn parse_tempo(input: &str) -> IResult<&str, Tempo> {
    let (input, _) = tag_no_case("tempo")(input)?;
    let (input, _) = ws1(input)?;
    alt((relative_tempo, map(nom_u16, Tempo::Absolute)))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempo_absolute() {
        let (rest, tempo) = parse_tempo("tempo 120").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Absolute(120));
    }

    #[test]
    fn test_tempo_absolute_140() {
        let (rest, tempo) = parse_tempo("tempo 140").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Absolute(140));
    }

    #[test]
    fn test_tempo_relative_positive() {
        let (rest, tempo) = parse_tempo("tempo +5").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(5));
    }

    #[test]
    fn test_tempo_relative_negative() {
        let (rest, tempo) = parse_tempo("tempo -10").unwrap();
        assert_eq!(rest, "");
        assert_eq!(tempo, Tempo::Relative(-10));
    }
}
