use nom::character::complete::char;
use nom::combinator::map;
use nom::sequence::preceded;
use nom::branch::alt;
use nom::IResult;

use crate::parser::common::parse_u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Articulation {
    Normal,
    Staccato,
    GateDirect(u8),
}

fn parse_staccato(input: &str) -> IResult<&str, Articulation> {
    map(char('\''), |_| Articulation::Staccato)(input)
}

fn parse_gate_direct(input: &str) -> IResult<&str, Articulation> {
    map(preceded(char('g'), parse_u8), Articulation::GateDirect)(input)
}

fn parse_normal(input: &str) -> IResult<&str, Articulation> {
    Ok((input, Articulation::Normal))
}

pub fn parse_articulation(input: &str) -> IResult<&str, Articulation> {
    alt((parse_staccato, parse_gate_direct, parse_normal))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staccato() {
        let (remaining, art) = parse_articulation("'rest").unwrap();
        assert_eq!(art, Articulation::Staccato);
        assert_eq!(remaining, "rest");
    }

    #[test]
    fn test_gate_direct() {
        let (remaining, art) = parse_articulation("g95rest").unwrap();
        assert_eq!(art, Articulation::GateDirect(95));
        assert_eq!(remaining, "rest");
    }

    #[test]
    fn test_gate_direct_with_space() {
        let (remaining, art) = parse_articulation("g30 next").unwrap();
        assert_eq!(art, Articulation::GateDirect(30));
        assert_eq!(remaining, " next");
    }

    #[test]
    fn test_normal_with_space() {
        let (remaining, art) = parse_articulation(" next").unwrap();
        assert_eq!(art, Articulation::Normal);
        assert_eq!(remaining, " next");
    }

    #[test]
    fn test_normal_empty() {
        let (remaining, art) = parse_articulation("").unwrap();
        assert_eq!(art, Articulation::Normal);
        assert_eq!(remaining, "");
    }
}
