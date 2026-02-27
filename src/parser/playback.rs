use nom::{
    bytes::complete::tag,
    character::complete::{char, space0, space1},
    combinator::{map, opt},
    sequence::{delimited, preceded, tuple},
    branch::alt,
    IResult,
};

use crate::ast::playback::*;
use crate::parser::common::{identifier, parse_u32};

/// Parse repeat spec inside brackets: `[repeat N]` or `[loop]`
fn parse_bracket_repeat(input: &str) -> IResult<&str, RepeatSpec> {
    delimited(
        tuple((space0, char('['))),
        alt((
            map(
                tuple((tag("repeat"), space1, parse_u32)),
                |(_, _, n)| RepeatSpec::Count(n),
            ),
            map(tag("loop"), |_| RepeatSpec::Loop),
        )),
        char(']'),
    )(input)
}

/// Parse: `play NAME`, `play NAME [repeat N]`, `play NAME [loop]`,
///        `play session NAME`, `play session NAME [...]`
pub fn parse_play(input: &str) -> IResult<&str, PlayCommand> {
    let (input, _) = tag("play")(input)?;
    let (input, _) = space1(input)?;

    // Try "session NAME" first
    let (input, target) = if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("session")(input) {
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

/// Parse: `stop` or `stop NAME`
pub fn parse_stop(input: &str) -> IResult<&str, StopCommand> {
    let (input, _) = tag("stop")(input)?;
    let (input, target) = opt(preceded(space1, identifier))(input)?;
    Ok((input, StopCommand {
        target: target.map(|s| s.to_string()),
    }))
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
}
