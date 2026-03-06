use nom::branch::alt;
use nom::character::complete::char;
use nom::combinator::map;
use nom::sequence::preceded;
use nom::IResult;

use crate::parser::common::parse_u8;

/// アーティキュレーション（奏法）を表す列挙型
/// Enum representing articulation (playing technique)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Articulation {
    /// 通常奏法
    /// Normal articulation
    Normal,
    /// スタッカート（短く切る奏法）
    /// Staccato (short, detached notes)
    Staccato,
    /// ゲート値の直接指定（0-100のパーセンテージ）
    /// Direct gate value specification (0-100 percentage)
    GateDirect(u8),
}

/// スタッカート記号 `'` をパースする。
/// Parse the staccato symbol `'`.
fn parse_staccato(input: &str) -> IResult<&str, Articulation> {
    map(char('\''), |_| Articulation::Staccato)(input)
}

/// ゲート値直接指定 `gNN` をパースする。
/// Parse a direct gate value `gNN`.
fn parse_gate_direct(input: &str) -> IResult<&str, Articulation> {
    map(preceded(char('g'), parse_u8), Articulation::GateDirect)(input)
}

/// 通常アーティキュレーション（フォールバック）をパースする。
/// Parse normal articulation (fallback).
fn parse_normal(input: &str) -> IResult<&str, Articulation> {
    Ok((input, Articulation::Normal))
}

/// アーティキュレーションをパースする（スタッカート、ゲート直接指定、または通常）。
/// Parse an articulation (staccato, direct gate, or normal).
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
