use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, space0, space1},
    combinator::{map, opt},
    sequence::{delimited, preceded, tuple},
    IResult,
};

use crate::ast::playback::*;
use crate::parser::common::{identifier, parse_u32};

/// ブラケット内のリピート指定をパースする: `[repeat N]` または `[loop]`
/// Parse repeat spec inside brackets: `[repeat N]` or `[loop]`
fn parse_bracket_repeat(input: &str) -> IResult<&str, RepeatSpec> {
    delimited(
        tuple((space0, char('['))),
        alt((
            map(tuple((tag("repeat"), space1, parse_u32)), |(_, _, n)| {
                RepeatSpec::Count(n)
            }),
            map(tag("loop"), |_| RepeatSpec::Loop),
        )),
        char(']'),
    )(input)
}

/// 再生コマンドをパースする: `play NAME`, `play NAME [repeat N]`, `play NAME [loop]`,
/// `play session NAME`, `play session NAME [...]`
/// Parse: `play NAME`, `play NAME [repeat N]`, `play NAME [loop]`,
///        `play session NAME`, `play session NAME [...]`
pub fn parse_play(input: &str) -> IResult<&str, PlayCommand> {
    let (input, _) = tag("play")(input)?;
    let (input, _) = space1(input)?;

    // Try "session NAME" first
    let (input, target) =
        if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("session")(input) {
            let (rest, _) = space1(rest)?;
            let (rest, name) = identifier(rest)?;
            (rest, PlayTarget::Session(name.to_string()))
        } else {
            let (rest, name) = identifier(input)?;
            (rest, PlayTarget::Scene(name.to_string()))
        };

    let (input, repeat) = match opt(parse_bracket_repeat)(input)? {
        (rest, Some(r)) => (rest, r),
        (rest, None) => (rest, RepeatSpec::Once),
    };

    Ok((input, PlayCommand { target, repeat }))
}

/// 停止コマンドをパースする: `stop` または `stop NAME`
/// Parse: `stop` or `stop NAME`
pub fn parse_stop(input: &str) -> IResult<&str, StopCommand> {
    let (input, _) = tag("stop")(input)?;
    let (input, target) = opt(preceded(space1, identifier))(input)?;
    Ok((
        input,
        StopCommand {
            target: target.map(|s| s.to_string()),
        },
    ))
}

/// ポーズコマンドをパースする: `pause` または `pause NAME`（§10.4）
///
/// `NAME` は scene/session/clip のいずれかを指す識別子。
/// 名前付き場合の対象解決（scene/session/clip 判別、名前不一致時の no-op 扱い）は
/// Evaluator 側で行う。
///
/// Parse: `pause` or `pause NAME` (§10.4).
/// `NAME` is an identifier referring to a scene, session, or clip. Target
/// resolution and no-op handling on name mismatch are performed by the
/// evaluator.
pub fn parse_pause(input: &str) -> IResult<&str, PauseCommand> {
    let (input, _) = tag("pause")(input)?;
    let (input, target) = opt(preceded(space1, identifier))(input)?;
    Ok((
        input,
        PauseCommand {
            target: target.map(|s| s.to_string()),
        },
    ))
}

/// 再開コマンドをパースする: `resume` または `resume NAME`（§10.4）
///
/// `NAME` は Paused 中の scene/session 名、または clip 名。
/// 名前不一致時の no-op 扱いは Evaluator 側で行う。
///
/// Parse: `resume` or `resume NAME` (§10.4).
/// `NAME` is a paused scene/session name or a clip name. No-op handling on
/// name mismatch is performed by the evaluator.
pub fn parse_resume(input: &str) -> IResult<&str, ResumeCommand> {
    let (input, _) = tag("resume")(input)?;
    let (input, target) = opt(preceded(space1, identifier))(input)?;
    Ok((
        input,
        ResumeCommand {
            target: target.map(|s| s.to_string()),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- play scene tests ---

    #[test]
    fn play_scene_once() {
        let (rest, cmd) = parse_play("play verse").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Scene("verse".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Once);
    }

    #[test]
    fn play_scene_repeat() {
        let (rest, cmd) = parse_play("play chorus [repeat 8]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Scene("chorus".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Count(8));
    }

    #[test]
    fn play_scene_loop() {
        let (rest, cmd) = parse_play("play verse [loop]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Scene("verse".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Loop);
    }

    // --- play session tests ---

    #[test]
    fn play_session_once() {
        let (rest, cmd) = parse_play("play session main").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Session("main".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Once);
    }

    #[test]
    fn play_session_loop() {
        let (rest, cmd) = parse_play("play session main [loop]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Session("main".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Loop);
    }

    #[test]
    fn play_session_repeat() {
        let (rest, cmd) = parse_play("play session main [repeat 3]").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, PlayTarget::Session("main".into()));
        assert_eq!(cmd.repeat, RepeatSpec::Count(3));
    }

    // --- stop tests ---

    #[test]
    fn stop_all() {
        let (rest, cmd) = parse_stop("stop").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, None);
    }

    #[test]
    fn stop_specific() {
        let (rest, cmd) = parse_stop("stop drums_a").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, Some("drums_a".into()));
    }

    // --- pause tests (§10.4) ---

    /// 引数なしの pause は全体 pause
    /// Bare `pause` targets the whole playback
    #[test]
    fn pause_all() {
        let (rest, cmd) = parse_pause("pause").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, None);
    }

    /// 名前付き pause
    /// Pause with a target name
    #[test]
    fn pause_named() {
        let (rest, cmd) = parse_pause("pause drums_a").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, Some("drums_a".into()));
    }

    /// scene 名を target として pause
    /// Pause with a scene name as target
    #[test]
    fn pause_scene_name() {
        let (rest, cmd) = parse_pause("pause verse").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, Some("verse".into()));
    }

    // --- resume tests (§10.4) ---

    /// 引数なしの resume は全体 resume
    /// Bare `resume` targets the whole playback
    #[test]
    fn resume_all() {
        let (rest, cmd) = parse_resume("resume").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, None);
    }

    /// 名前付き resume
    /// Resume with a target name
    #[test]
    fn resume_named() {
        let (rest, cmd) = parse_resume("resume drums_a").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, Some("drums_a".into()));
    }

    /// session 名を target として resume
    /// Resume with a session name as target
    #[test]
    fn resume_session_name() {
        let (rest, cmd) = parse_resume("resume main").unwrap();
        assert_eq!(rest, "");
        assert_eq!(cmd.target, Some("main".into()));
    }
}
