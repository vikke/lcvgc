pub mod clip;
pub mod clip_arpeggio;
pub mod clip_articulation;
pub mod clip_bar_jump;
pub mod clip_cc;
pub mod clip_drum;
pub mod clip_note;
pub mod clip_options;
pub mod clip_repetition;
pub mod clip_shorthand;
pub mod common;
pub mod device;
pub mod include;
pub mod instrument;
pub mod kit;
pub mod playback;
pub mod scale;
pub mod scene;
pub mod session;
pub mod tempo;
pub mod var;

use nom::IResult;

use crate::ast::Block;
use common::ws;

/// Parse a single top-level block by peeking at the first keyword.
pub fn parse_block(input: &str) -> IResult<&str, Block> {
    let (input, _) = ws(input)?;
    let trimmed = input.trim_start();

    if trimmed.starts_with("device ") {
        let (r, v) = device::parse_device(input)?;
        Ok((r, Block::Device(v)))
    } else if trimmed.starts_with("instrument ") {
        let (r, v) = instrument::parse_instrument(input)?;
        Ok((r, Block::Instrument(v)))
    } else if trimmed.starts_with("kit ") {
        let (r, v) = kit::parse_kit(input)?;
        Ok((r, Block::Kit(v)))
    } else if trimmed.starts_with("clip ") {
        let (r, v) = clip::parse_clip(input)?;
        Ok((r, Block::Clip(v)))
    } else if trimmed.starts_with("scene ") {
        let (r, v) = scene::parse_scene(input)?;
        Ok((r, Block::Scene(v)))
    } else if trimmed.starts_with("session ") {
        let (r, v) = session::parse_session(input)?;
        Ok((r, Block::Session(v)))
    } else if trimmed.starts_with("tempo ") {
        let (r, v) = tempo::parse_tempo(input)?;
        Ok((r, Block::Tempo(v)))
    } else if trimmed.starts_with("scale ") {
        let (r, v) = scale::parse_scale(input)?;
        Ok((r, Block::Scale(v)))
    } else if trimmed.starts_with("var ") {
        let (r, v) = var::parse_var(input)?;
        Ok((r, Block::Var(v)))
    } else if trimmed.starts_with("include ") {
        let (r, v) = include::parse_include(input)?;
        Ok((r, Block::Include(v)))
    } else if trimmed.starts_with("play ") {
        let (r, v) = playback::parse_play(input)?;
        Ok((r, Block::Play(v)))
    } else if trimmed.starts_with("stop") {
        let (r, v) = playback::parse_stop(input)?;
        Ok((r, Block::Stop(v)))
    } else {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }
}

/// Parse an entire source file into a list of blocks.
pub fn parse_source(input: &str) -> IResult<&str, Vec<Block>> {
    let mut blocks = Vec::new();
    let mut remaining = input;

    loop {
        let (r, _) = ws(remaining)?;
        remaining = r;
        if remaining.is_empty() {
            break;
        }
        let (r, block) = parse_block(remaining)?;
        blocks.push(block);
        remaining = r;
    }

    Ok((remaining, blocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_parse_single_tempo() {
        let (rest, block) = parse_block("tempo 120").unwrap();
        assert_eq!(rest, "");
        assert!(matches!(block, Block::Tempo(crate::ast::tempo::Tempo::Absolute(120))));
    }

    #[test]
    fn test_parse_multiple_blocks() {
        let input = r#"
tempo 120

device my_synth {
  port "IAC Driver"
}

clip bass_a [bars 1] {
  bass c:3:8 c eb f::4 g::2
}

scene intro {
  bass_a
}

play intro
"#;
        let (rest, blocks) = parse_source(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(blocks.len(), 5);
        assert!(matches!(blocks[0], Block::Tempo(_)));
        assert!(matches!(blocks[1], Block::Device(_)));
        assert!(matches!(blocks[2], Block::Clip(_)));
        assert!(matches!(blocks[3], Block::Scene(_)));
        assert!(matches!(blocks[4], Block::Play(_)));
    }

    #[test]
    fn test_parse_empty_source() {
        let (rest, blocks) = parse_source("").unwrap();
        assert_eq!(rest, "");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_parse_source_with_comments() {
        let input = r#"
// This is a comment
tempo 120
// Another comment
scale c minor
"#;
        let (rest, blocks) = parse_source(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(blocks.len(), 2);
    }
}
