use nom::{bytes::complete::tag, character::complete::char, IResult};

use crate::ast::session::*;
use crate::parser::common::{identifier, parse_u32, ws, ws1};

/// セッションエントリの修飾子をパースする: `[repeat N]` または `[loop]`
/// Parse a session entry modifier: `[repeat N]` or `[loop]`.
fn session_modifier(input: &str) -> IResult<&str, SessionRepeat> {
    let (input, _) = char('[')(input)?;
    let (input, _) = ws(input)?;

    // Try "repeat N" first, then "loop"
    if let Ok((input, _)) = tag::<&str, &str, nom::error::Error<&str>>("repeat")(input) {
        let (input, _) = ws1(input)?;
        let (input, n) = parse_u32(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char(']')(input)?;
        Ok((input, SessionRepeat::Count(n)))
    } else {
        let (input, _) = tag("loop")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char(']')(input)?;
        Ok((input, SessionRepeat::Loop))
    }
}

/// セッションエントリを1つパースする: シーン名とオプションの `[repeat N]` または `[loop]`
/// Parse a single session entry: `scene_name` with optional `[repeat N]` or `[loop]`.
fn session_entry(input: &str) -> IResult<&str, SessionEntry> {
    let (input, scene) = identifier(input)?;
    let (input, _) = ws(input)?;

    // Try to parse optional modifier
    let (input, repeat) = if input.starts_with('[') {
        let (input, modifier) = session_modifier(input)?;
        (input, modifier)
    } else {
        (input, SessionRepeat::Once)
    };

    Ok((
        input,
        SessionEntry {
            scene: scene.to_string(),
            repeat,
        },
    ))
}

/// セッション定義をパースする: `session NAME { entries... }`
/// Parse `session NAME { entries... }`.
pub fn parse_session(input: &str) -> IResult<&str, SessionDef> {
    let (input, _) = tag("session")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;

    let mut entries = Vec::new();
    let mut input = input;

    loop {
        let (rest, _) = ws(input)?;
        input = rest;

        if input.starts_with('}') {
            let (rest, _) = char('}')(input)?;
            input = rest;
            break;
        }

        let (rest, entry) = session_entry(input)?;
        entries.push(entry);
        input = rest;
    }

    Ok((
        input,
        SessionDef {
            name: name.to_string(),
            entries,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_simple_session_one_scene() {
        let (rest, session) = parse_session("session live { intro }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(session.name, "live");
        assert_eq!(session.entries.len(), 1);
        assert_eq!(session.entries[0].scene, "intro");
        assert_eq!(session.entries[0].repeat, SessionRepeat::Once);
    }

    #[test]
    fn test_session_with_repeat() {
        let (rest, session) = parse_session("session live { verse [repeat 4] }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(session.entries.len(), 1);
        assert_eq!(session.entries[0].scene, "verse");
        assert_eq!(session.entries[0].repeat, SessionRepeat::Count(4));
    }

    #[test]
    fn test_session_with_loop() {
        let (rest, session) = parse_session("session live { ambient [loop] }").unwrap();
        assert_eq!(rest, "");
        assert_eq!(session.entries.len(), 1);
        assert_eq!(session.entries[0].scene, "ambient");
        assert_eq!(session.entries[0].repeat, SessionRepeat::Loop);
    }

    #[test]
    fn test_session_mixed_entries() {
        let input = "session main {
            intro
            verse [repeat 4]
            chorus [repeat 2]
            bridge
            outro [loop]
        }";
        let (rest, session) = parse_session(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(session.name, "main");
        assert_eq!(session.entries.len(), 5);
        assert_eq!(session.entries[0].scene, "intro");
        assert_eq!(session.entries[0].repeat, SessionRepeat::Once);
        assert_eq!(session.entries[1].scene, "verse");
        assert_eq!(session.entries[1].repeat, SessionRepeat::Count(4));
        assert_eq!(session.entries[2].scene, "chorus");
        assert_eq!(session.entries[2].repeat, SessionRepeat::Count(2));
        assert_eq!(session.entries[3].scene, "bridge");
        assert_eq!(session.entries[3].repeat, SessionRepeat::Once);
        assert_eq!(session.entries[4].scene, "outro");
        assert_eq!(session.entries[4].repeat, SessionRepeat::Loop);
    }

    #[test]
    fn test_session_multiple_entries() {
        let input = "session set { intro verse chorus }";
        let (rest, session) = parse_session(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(session.entries.len(), 3);
        assert_eq!(session.entries[0].scene, "intro");
        assert_eq!(session.entries[1].scene, "verse");
        assert_eq!(session.entries[2].scene, "chorus");
        // All default to Once
        for entry in &session.entries {
            assert_eq!(entry.repeat, SessionRepeat::Once);
        }
    }
}
