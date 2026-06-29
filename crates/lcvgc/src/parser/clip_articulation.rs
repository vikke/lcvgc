use nom::character::complete::char;
use nom::error::{Error, ErrorKind};
use nom::sequence::preceded;
use nom::{Err, IResult};

use crate::ast::clip_articulation::{Articulation, NoteSuffix};
use crate::parser::common::parse_u8;

/// 個別サフィックスの内部表現（順不同パーサ用）。
/// Internal representation of a single suffix token used by the order-independent parser.
#[derive(Debug, Clone, Copy)]
enum SuffixToken {
    Staccato,
    GateDirect(u8),
    Velocity(u8),
}

fn parse_single_suffix(input: &str) -> IResult<&str, SuffixToken> {
    // `'` (staccato)
    if let Ok((rest, _)) = char::<&str, Error<&str>>('\'')(input) {
        return Ok((rest, SuffixToken::Staccato));
    }
    // `gN` (gate direct)
    if let Ok((rest, n)) = preceded(char::<&str, Error<&str>>('g'), parse_u8)(input) {
        return Ok((rest, SuffixToken::GateDirect(n)));
    }
    // `vN` (velocity direct)
    if let Ok((rest, n)) = preceded(char::<&str, Error<&str>>('v'), parse_u8)(input) {
        return Ok((rest, SuffixToken::Velocity(n)));
    }
    Err(Err::Error(Error::new(input, ErrorKind::Alt)))
}

/// ノートサフィックス修飾子をパースする。
///
/// 受理する修飾子: `'` (staccato), `gN` (gate direct), `vN` (velocity direct)。
/// 順序は不同、各修飾子は高々 1 回、`'` と `gN` の同時指定は禁止。
/// 違反時は `Err::Failure` を返してパーサ全体のフォールバックを抑止する。
///
/// Parse the suffix modifiers of a pitched note.
/// Accepted modifiers: `'` (staccato), `gN` (gate direct), `vN` (velocity direct).
/// They are order-independent, each may appear at most once, and `'` and `gN`
/// must not be specified together. Any violation produces `Err::Failure` so
/// that the outer parser does not silently swallow it.
pub fn parse_note_suffix(input: &str) -> IResult<&str, NoteSuffix> {
    let mut staccato = false;
    let mut gate: Option<u8> = None;
    let mut velocity: Option<u8> = None;
    let mut cursor = input;
    let mut original_for_err = input;
    while let Ok((rest, tok)) = parse_single_suffix(cursor) {
        match tok {
            SuffixToken::Staccato => {
                if staccato {
                    return Err(Err::Failure(Error::new(original_for_err, ErrorKind::Many1)));
                }
                if gate.is_some() {
                    return Err(Err::Failure(Error::new(original_for_err, ErrorKind::Alt)));
                }
                staccato = true;
            }
            SuffixToken::GateDirect(n) => {
                if gate.is_some() {
                    return Err(Err::Failure(Error::new(original_for_err, ErrorKind::Many1)));
                }
                if staccato {
                    return Err(Err::Failure(Error::new(original_for_err, ErrorKind::Alt)));
                }
                gate = Some(n);
            }
            SuffixToken::Velocity(n) => {
                if velocity.is_some() {
                    return Err(Err::Failure(Error::new(original_for_err, ErrorKind::Many1)));
                }
                velocity = Some(n);
            }
        }
        original_for_err = rest;
        cursor = rest;
    }

    let articulation = if staccato {
        Articulation::Staccato
    } else if let Some(pct) = gate {
        Articulation::GateDirect(pct)
    } else {
        Articulation::Normal
    };

    Ok((
        cursor,
        NoteSuffix {
            articulation,
            velocity,
        },
    ))
}

/// アーティキュレーションをパースする（スタッカート、ゲート直接指定、または通常）。
/// 既存呼び出しとの互換のため `Articulation` のみを返すラッパー。
/// `vN` は無視せず内部で受理するが、戻り値には反映されない。完全な情報を扱う
/// 呼び出し元は [`parse_note_suffix`] を使うこと。
///
/// Parse an articulation (staccato, direct gate, or normal).
/// Compatibility wrapper that returns just the `Articulation`. `vN` suffixes
/// are consumed by the underlying parser but the velocity value is discarded.
/// Callers that need the velocity must use [`parse_note_suffix`].
pub fn parse_articulation(input: &str) -> IResult<&str, Articulation> {
    let (rest, suffix) = parse_note_suffix(input)?;
    Ok((rest, suffix.articulation))
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

    // --- ここから vN / 順不同 / 重複 / 衝突 のテスト ---
    // Tests for vN suffix, order-independence, duplicates, and `'`+gN conflict.

    /// `vN` 単独で velocity が取れること。
    #[test]
    fn test_velocity_only() {
        let (rest, suf) = parse_note_suffix("v100 next").unwrap();
        assert_eq!(suf.articulation, Articulation::Normal);
        assert_eq!(suf.velocity, Some(100));
        assert_eq!(rest, " next");
    }

    /// `vN` の N が 127 まで受理されること。
    #[test]
    fn test_velocity_127() {
        let (_, suf) = parse_note_suffix("v127").unwrap();
        assert_eq!(suf.velocity, Some(127));
    }

    /// `'` と `vN` の組合せ (順序 1: ' が先)。
    #[test]
    fn test_staccato_then_velocity() {
        let (rest, suf) = parse_note_suffix("'v80 ").unwrap();
        assert_eq!(suf.articulation, Articulation::Staccato);
        assert_eq!(suf.velocity, Some(80));
        assert_eq!(rest, " ");
    }

    /// `'` と `vN` の組合せ (順序 2: v が先 — 順不同)。
    #[test]
    fn test_velocity_then_staccato() {
        let (rest, suf) = parse_note_suffix("v80' ").unwrap();
        assert_eq!(suf.articulation, Articulation::Staccato);
        assert_eq!(suf.velocity, Some(80));
        assert_eq!(rest, " ");
    }

    /// `gN` と `vN` の組合せ (順序 1: gN が先)。
    #[test]
    fn test_gate_then_velocity() {
        let (rest, suf) = parse_note_suffix("g95v110 ").unwrap();
        assert_eq!(suf.articulation, Articulation::GateDirect(95));
        assert_eq!(suf.velocity, Some(110));
        assert_eq!(rest, " ");
    }

    /// `gN` と `vN` の組合せ (順序 2: v が先 — 順不同)。
    #[test]
    fn test_velocity_then_gate() {
        let (rest, suf) = parse_note_suffix("v110g95 ").unwrap();
        assert_eq!(suf.articulation, Articulation::GateDirect(95));
        assert_eq!(suf.velocity, Some(110));
        assert_eq!(rest, " ");
    }

    /// `vN` の重複指定はパースエラー (Failure)。
    #[test]
    fn test_velocity_duplicate_is_error() {
        let r = parse_note_suffix("v100v90");
        assert!(
            matches!(r, Err(nom::Err::Failure(_))),
            "expected Failure, got {:?}",
            r
        );
    }

    /// `gN` の重複指定はパースエラー (Failure)。
    #[test]
    fn test_gate_duplicate_is_error() {
        let r = parse_note_suffix("g30g50");
        assert!(
            matches!(r, Err(nom::Err::Failure(_))),
            "expected Failure, got {:?}",
            r
        );
    }

    /// `'` の重複指定はパースエラー (Failure)。
    #[test]
    fn test_staccato_duplicate_is_error() {
        let r = parse_note_suffix("''");
        assert!(
            matches!(r, Err(nom::Err::Failure(_))),
            "expected Failure, got {:?}",
            r
        );
    }

    /// `'` と `gN` の両立はパースエラー (Failure)。
    #[test]
    fn test_staccato_and_gate_conflict_is_error_order1() {
        let r = parse_note_suffix("'g95");
        assert!(
            matches!(r, Err(nom::Err::Failure(_))),
            "expected Failure, got {:?}",
            r
        );
    }

    /// `gN` と `'` の両立 (順序逆) もパースエラー (Failure)。
    #[test]
    fn test_staccato_and_gate_conflict_is_error_order2() {
        let r = parse_note_suffix("g95'");
        assert!(
            matches!(r, Err(nom::Err::Failure(_))),
            "expected Failure, got {:?}",
            r
        );
    }

    /// `vN` だけで Normal articulation が維持されること。
    #[test]
    fn test_velocity_only_normal_articulation() {
        let (_, suf) = parse_note_suffix("v64").unwrap();
        assert_eq!(suf.articulation, Articulation::Normal);
        assert_eq!(suf.velocity, Some(64));
    }

    /// サフィックス無しなら Normal + velocity=None。
    #[test]
    fn test_empty_suffix_is_normal_no_velocity() {
        let (rest, suf) = parse_note_suffix(" next").unwrap();
        assert_eq!(suf.articulation, Articulation::Normal);
        assert_eq!(suf.velocity, None);
        assert_eq!(rest, " next");
    }
}
