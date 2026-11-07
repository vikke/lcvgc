use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
use crate::ast::doc::{ExampleRender, SyntaxDoc};

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

/// この構文の説明と例文（文法チュートリアルの素材。`lsp::tutorial` が集める）
/// Syntax docs for this parser (tutorial material collected by `lsp::tutorial`).
pub const DOCS: &[SyntaxDoc] = &[
    SyntaxDoc {
        id: "clip_arpeggio",
        title_ja: "アルペジオ arp(方向, 音価)",
        title_en: "Arpeggio arp(direction, step)",
        desc_ja: &[
            "和音の後に arp(方向, 音価) を付けると構成音を順に鳴らす。",
            "方向: up / down / updown / random。音価は 1 音の長さ（16 など）。",
            "構成音を 1 巡したら次の要素へ進む（updown は両端を重ねない）。",
            "和音は角括弧 [c e g] でもコード名 cm7 でもよい。",
        ],
        desc_en: &[
            "Put arp(direction, step) after a chord to play its notes in turn.",
            "Direction: up / down / updown / random. Step is one note's length.",
            "One pass over the notes, then on to the next element (updown: c e g e).",
            "The chord can be a bracket chord [c e g] or a chord name like cm7.",
        ],
        example: "clip arp_demo [bars 1] {\n\tlead [c:4 eb:4 g:4 bb:4]:4 arp(up, 16) cm7:4:1 arp(updown, 8)\n}",
        render: ExampleRender::Live,
        list: None,
    },
    SyntaxDoc {
        id: "clip_arpeggio.default_step",
        title_ja: "アルペジオの音価の省略",
        title_en: "Omitting the arpeggio step",
        desc_ja: &[
            "arp(方向) のように音価を省くと、和音側の音価が 1 音の長さになる。",
            "両方に書いた場合は arp 側が優先される（am7:4:4 arp(down, 8) は 8 分）。",
        ],
        desc_en: &[
            "With arp(direction) the chord's own duration is used per note.",
            "If both are given, the arp step wins (am7:4:4 arp(down, 8) uses 8ths).",
        ],
        example: "clip arp_step_demo [bars 1] {\n\tlead [c:4 eb:4 g:4 bb:4]:8 arp(up) am7:4:4 arp(down, 8)\n}",
        render: ExampleRender::Live,
        list: None,
    },
];

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

    /// 構文ドキュメントの例文がパーサで最後まで解析できる
    /// Every syntax-doc example parses fully.
    #[test]
    fn docs_examples_parse() {
        assert!(!DOCS.is_empty());
        crate::parser::assert_docs_parse(DOCS);
    }
}
