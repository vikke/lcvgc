/// クリップパーサーモジュール
/// Clip parser module
pub mod clip;
/// クリップ・アルペジオパーサーモジュール
/// Clip arpeggio parser module
pub mod clip_arpeggio;
/// クリップ・アーティキュレーションパーサーモジュール
/// Clip articulation parser module
pub mod clip_articulation;
/// クリップ・小節ジャンプパーサーモジュール
/// Clip bar jump parser module
pub mod clip_bar_jump;
/// クリップCCパーサーモジュール
/// Clip CC (Control Change) parser module
pub mod clip_cc;
/// クリップ・ドラムパーサーモジュール
/// Clip drum parser module
pub mod clip_drum;
/// クリップ・ノートパーサーモジュール
/// Clip note parser module
pub mod clip_note;
/// クリップオプションパーサーモジュール
/// Clip options parser module
pub mod clip_options;
/// クリップ・リピートパーサーモジュール
/// Clip repetition parser module
pub mod clip_repetition;
/// クリップ省略記法パーサーモジュール
/// Clip shorthand parser module
pub mod clip_shorthand;
/// 共通パーサーユーティリティモジュール
/// Common parser utility module
pub mod common;
/// デバイスパーサーモジュール
/// Device parser module
pub mod device;
/// インクルードパーサーモジュール
/// Include parser module
pub mod include;
/// インストゥルメントパーサーモジュール
/// Instrument parser module
pub mod instrument;
/// キットパーサーモジュール
/// Kit parser module
pub mod kit;
/// 再生コマンドパーサーモジュール
/// Playback command parser module
pub mod playback;
/// スケールパーサーモジュール
/// Scale parser module
pub mod scale;
/// シーンパーサーモジュール
/// Scene parser module
pub mod scene;
/// セッションパーサーモジュール
/// Session parser module
pub mod session;
/// テンポパーサーモジュール
/// Tempo parser module
pub mod tempo;
/// 変数パーサーモジュール
/// Variable parser module
pub mod var;

use nom::IResult;

use crate::ast::Block;
use common::ws;

/// 先頭キーワードを見て、トップレベルブロックを1つパースする。
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
    } else if trimmed.starts_with("pause") {
        let (r, v) = playback::parse_pause(input)?;
        Ok((r, Block::Pause(v)))
    } else if trimmed.starts_with("resume") {
        let (r, v) = playback::parse_resume(input)?;
        Ok((r, Block::Resume(v)))
    } else if trimmed.starts_with("unmute ") {
        let (r, v) = playback::parse_unmute(input)?;
        Ok((r, Block::Unmute(v)))
    } else if trimmed.starts_with("mute ") {
        let (r, v) = playback::parse_mute(input)?;
        Ok((r, Block::Mute(v)))
    } else {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }
}

/// ソースファイル全体をブロックのリストにパースする。
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
        assert!(matches!(
            block,
            Block::Tempo(crate::ast::tempo::Tempo::Absolute(120))
        ));
    }

    #[test]
    fn test_parse_multiple_blocks() {
        let input = r#"
tempo 120

device my_synth {
  port IAC Driver
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

    /// §10.4: pause / resume ブロックが parse_block でパースできる
    /// §10.4: parse_block dispatches pause / resume correctly
    #[test]
    fn test_parse_pause_resume_block() {
        let (_, b) = parse_block("pause").unwrap();
        assert!(matches!(b, Block::Pause(_)));

        let (_, b) = parse_block("pause verse").unwrap();
        if let Block::Pause(cmd) = b {
            assert_eq!(cmd.target, Some("verse".to_string()));
        } else {
            panic!("expected Block::Pause");
        }

        let (_, b) = parse_block("resume").unwrap();
        assert!(matches!(b, Block::Resume(_)));

        let (_, b) = parse_block("resume drums_a").unwrap();
        if let Block::Resume(cmd) = b {
            assert_eq!(cmd.target, Some("drums_a".to_string()));
        } else {
            panic!("expected Block::Resume");
        }
    }

    /// §10.4: pause/resume を含む複数ブロックの統合パース
    /// §10.4: integration parse with pause/resume among other blocks
    #[test]
    fn test_parse_source_with_pause_resume() {
        let input = r#"
tempo 120
play verse
pause
resume
pause drums_a
resume drums_a
stop
"#;
        let (rest, blocks) = parse_source(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(blocks.len(), 7);
        assert!(matches!(blocks[0], Block::Tempo(_)));
        assert!(matches!(blocks[1], Block::Play(_)));
        assert!(matches!(blocks[2], Block::Pause(_)));
        assert!(matches!(blocks[3], Block::Resume(_)));
        assert!(matches!(blocks[4], Block::Pause(_)));
        assert!(matches!(blocks[5], Block::Resume(_)));
        assert!(matches!(blocks[6], Block::Stop(_)));
    }

    /// §10.4: mute / unmute ブロックが parse_block でパースできる
    /// §10.4: parse_block dispatches mute / unmute correctly
    #[test]
    fn test_parse_mute_unmute_block() {
        let (_, b) = parse_block("mute drums_a").unwrap();
        if let Block::Mute(cmd) = b {
            assert_eq!(cmd.target, "drums_a".to_string());
        } else {
            panic!("expected Block::Mute");
        }

        let (_, b) = parse_block("unmute drums_a").unwrap();
        if let Block::Unmute(cmd) = b {
            assert_eq!(cmd.target, "drums_a".to_string());
        } else {
            panic!("expected Block::Unmute");
        }
    }

    /// §10.4: unmute は mute より先に判定される（`unmute` が `mute ` prefix に
    /// マッチしてしまわない）
    /// §10.4: parser dispatches `unmute` before `mute` so that the `unmute` prefix
    /// is not mistakenly handled as `mute`
    #[test]
    fn test_parse_unmute_before_mute() {
        let (_, b) = parse_block("unmute a").unwrap();
        assert!(matches!(b, Block::Unmute(_)));
    }
}
