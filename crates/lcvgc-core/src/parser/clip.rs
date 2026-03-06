use nom::{bytes::complete::tag, character::complete::char, combinator::opt, IResult};

use crate::ast::clip::*;
use crate::parser::clip_arpeggio::parse_arpeggio;
use crate::parser::clip_articulation::parse_articulation;
use crate::parser::clip_bar_jump::parse_bar_jump;
use crate::parser::clip_cc::{parse_cc_step, parse_cc_target, parse_cc_time};
use crate::parser::clip_drum::{expand_pipe, parse_hit_symbols, parse_probability_row};
use crate::parser::clip_note::parse_note_event;
use crate::parser::clip_options::parse_clip_options;
use crate::parser::clip_repetition::parse_repetition;
use crate::parser::common::{identifier, parse_u16, ws, ws1};

/// Parse a `clip NAME [options] { body }` block.
pub fn parse_clip(input: &str) -> IResult<&str, ClipDef> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("clip")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, options) = parse_clip_options(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = ws(input)?;

    // Peek to determine if this is a drum clip (starts with "use" keyword)
    if input.trim_start().starts_with("use ") {
        let (input, body) = parse_drum_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;
        Ok((
            input,
            ClipDef {
                name: name.to_string(),
                options,
                body: ClipBody::Drum(body),
            },
        ))
    } else {
        let (input, body) = parse_pitched_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;
        Ok((
            input,
            ClipDef {
                name: name.to_string(),
                options,
                body: ClipBody::Pitched(body),
            },
        ))
    }
}

/// Parse the body of a pitched clip.
fn parse_pitched_body(mut input: &str) -> IResult<&str, PitchedClipBody> {
    let mut lines: Vec<PitchedLine> = Vec::new();
    let mut cc_automations = Vec::new();

    loop {
        let (rest, _) = ws(input)?;
        input = rest;

        // Check for closing brace
        if input.starts_with('}') {
            break;
        }

        // Try CC automation (instrument.param pattern)
        if let Ok((_, _target)) = parse_cc_target(input) {
            // It's a CC line - try step first, then time
            if let Ok((rest, cc)) = parse_cc_step(input) {
                cc_automations.push(cc);
                input = rest;
                continue;
            }
            if let Ok((rest, cc)) = parse_cc_time(input) {
                cc_automations.push(cc);
                input = rest;
                continue;
            }
        }

        // Parse instrument name
        let (rest, inst_name) = identifier(input)?;
        let (rest, _) = ws1(rest)?;

        // Parse elements
        let mut elements = Vec::new();
        let mut current = rest;

        loop {
            let (r, _) = ws(current)?;
            current = r;

            if current.starts_with('}') || current.is_empty() {
                break;
            }

            // Check for newline with same or different instrument (end of this line)
            if let Ok((_, next_ident)) = identifier(current) {
                // If the next identifier is the same instrument name or a known keyword,
                // it might be a new line
                if next_ident == inst_name || next_ident == "resolution" {
                    break;
                }
                // Check if it looks like a CC line (has dot)
                let after_ident = &current[next_ident.len()..];
                if after_ident.starts_with('.') {
                    break;
                }
            }

            // Try bar jump
            if let Ok((r, bj)) = parse_bar_jump(current) {
                elements.push(PitchedElement::BarJump(bj));
                current = r;
                continue;
            }

            // Try repetition
            if let Ok((r, rep)) = parse_repetition(current) {
                elements.push(PitchedElement::Repetition(rep));
                current = r;
                continue;
            }

            // Try chord bracket [notes]:dur
            if current.starts_with('[') {
                let (r, chord) = parse_chord_bracket(current)?;
                elements.push(chord);
                current = r;
                continue;
            }

            // Try note event (single note or chord name)
            if let Ok((r, note)) = parse_note_event(current) {
                let (r, art) = parse_articulation(r)?;
                // Check for arpeggio on chord names
                let (r, _) = ws(r)?;
                if let Some((r2, _arp)) = parse_arpeggio(r) {
                    current = r2;
                    elements.push(PitchedElement::Note(note, art));
                    continue;
                }
                elements.push(PitchedElement::Note(note, art));
                current = r;
                continue;
            }

            // Can't parse anything else, break
            break;
        }

        if !elements.is_empty() {
            lines.push(PitchedLine {
                instrument: inst_name.to_string(),
                elements,
            });
        }

        input = current;
    }

    Ok((
        input,
        PitchedClipBody {
            lines,
            cc_automations,
        },
    ))
}

/// Parse a chord bracket: `[note1 note2 ...]:dur`
fn parse_chord_bracket(input: &str) -> IResult<&str, PitchedElement> {
    let (input, _) = char('[')(input)?;
    let mut notes = Vec::new();
    let mut current = input;

    loop {
        let (r, _) = ws(current)?;
        current = r;
        if current.starts_with(']') {
            current = &current[1..];
            break;
        }
        // Parse note_name and optional octave
        let (r, name) = crate::parser::common::note_name(current)?;
        let (r, oct) = opt(|i| {
            let (i, _) = char(':')(i)?;
            crate::parser::common::parse_u8(i)
        })(r)?;
        notes.push((name, oct));
        current = r;
    }

    // Parse :duration
    let (current, dur) = if current.starts_with(':') {
        let (r, _) = char(':')(current)?;
        let (r, d) = parse_u16(r)?;
        (r, Some(d))
    } else {
        (current, None)
    };

    let (current, dotted) = opt(tag("."))(current)?;
    let (current, art) = parse_articulation(current)?;

    // Check for arpeggio
    let (current, _) = ws(current)?;
    let (current, arp) = if let Some((r, a)) = parse_arpeggio(current) {
        (r, Some(a))
    } else {
        (current, None)
    };

    Ok((
        current,
        PitchedElement::ChordBracket {
            notes,
            duration: dur,
            dotted: dotted.is_some(),
            articulation: art,
            arpeggio: arp,
        },
    ))
}

/// Parse the body of a drum clip.
fn parse_drum_body(input: &str) -> IResult<&str, DrumClipBody> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("use")(input)?;
    let (input, _) = ws1(input)?;
    let (input, kit) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("resolution")(input)?;
    let (input, _) = ws1(input)?;
    let (input, resolution) = parse_u16(input)?;

    let beats_per_step = resolution as usize / 4; // for 4/4 time

    let mut rows: Vec<crate::ast::clip_drum::DrumRow> = Vec::new();
    let mut cc_automations = Vec::new();
    let mut current = input;

    loop {
        let (r, _) = ws(current)?;
        current = r;

        if current.starts_with('}') || current.is_empty() {
            break;
        }

        // Try CC automation
        if let Ok((_, _target)) = parse_cc_target(current) {
            if let Ok((r, cc)) = parse_cc_step(current) {
                cc_automations.push(cc);
                current = r;
                continue;
            }
            if let Ok((r, cc)) = parse_cc_time(current) {
                cc_automations.push(cc);
                current = r;
                continue;
            }
        }

        // Parse instrument name
        let (r, inst_name) = identifier(current)?;

        // Check if this is the start of a probability row (all digits and dots, no alpha after spaces)
        // Probability rows don't start with an identifier
        // Actually, probability rows are indented and start directly with digits/dots
        // Let's check: if inst_name looks like it could be an instrument
        let (r, _) = ws(r)?;

        // Read the pattern until end of line
        let line_end = r.find('\n').unwrap_or(r.len());
        let pattern = r[..line_end].trim();

        if pattern.is_empty() {
            current = &r[line_end..];
            continue;
        }

        // Check if this could be a probability row (all chars are 0-9 or .)
        let is_prob = pattern.chars().all(|c| c.is_ascii_digit() || c == '.');

        if is_prob && !rows.is_empty() {
            // It's a probability row for the last drum row
            let prob = parse_probability_row(pattern);
            if let Some(last) = rows.last_mut() {
                last.probability = Some(prob);
            }
        } else {
            // It's a hit pattern row
            let expanded = expand_pipe(pattern, beats_per_step);
            let hits = parse_hit_symbols(&expanded);
            rows.push(crate::ast::clip_drum::DrumRow {
                instrument: inst_name.to_string(),
                hits,
                probability: None,
            });
        }

        current = &r[line_end..];
    }

    Ok((
        current,
        DrumClipBody {
            kit: kit.to_string(),
            resolution,
            rows,
            cc_automations,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip_note::{ChordSuffix, NoteEvent};
    use crate::ast::common::NoteName;

    #[test]
    fn test_simple_pitched_clip() {
        let input = r#"clip bass_a [bars 1] {
  bass c:3:8 c eb f::4 g::2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "bass_a");
        assert_eq!(clip.options.bars, Some(1));
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.lines[0].instrument, "bass");
                assert_eq!(body.lines[0].elements.len(), 5);
            }
            _ => panic!("expected pitched"),
        }
    }

    #[test]
    fn test_simple_drum_clip() {
        let input = r#"clip drums_a [bars 1] {
  use tr808
  resolution 16

  bd    x...x...x...x...
  snare ....x.......x...
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "drums_a");
        match &clip.body {
            ClipBody::Drum(body) => {
                assert_eq!(body.kit, "tr808");
                assert_eq!(body.resolution, 16);
                assert_eq!(body.rows.len(), 2);
                assert_eq!(body.rows[0].instrument, "bd");
                assert_eq!(body.rows[0].hits.len(), 16);
                assert_eq!(body.rows[1].instrument, "snare");
            }
            _ => panic!("expected drum"),
        }
    }

    #[test]
    fn test_clip_no_options() {
        let input = r#"clip bass_poly {
  bass c:3:4 eb::4 f::4
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(clip.name, "bass_poly");
        assert_eq!(clip.options.bars, None);
    }

    #[test]
    fn test_pitched_chord_name() {
        let input = r#"clip chords [bars 4] {
  keys cm7:4:2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 1);
                assert_eq!(body.lines[0].elements.len(), 1);
                match &body.lines[0].elements[0] {
                    PitchedElement::Note(NoteEvent::ChordName { root, suffix, .. }, _) => {
                        assert_eq!(*root, NoteName::C);
                        assert_eq!(*suffix, ChordSuffix::Min7);
                    }
                    other => panic!("expected chord name, got {:?}", other),
                }
            }
            _ => panic!("expected pitched"),
        }
    }

    #[test]
    fn test_multiline_pitched() {
        let input = r#"clip bass_a [bars 2] {
  bass c:3:8 c eb f::4 g::2
  bass ab:3:8 g f eb::4 c::2
}"#;
        let (rest, clip) = parse_clip(input).unwrap();
        assert_eq!(rest, "");
        match &clip.body {
            ClipBody::Pitched(body) => {
                assert_eq!(body.lines.len(), 2);
                assert_eq!(body.lines[0].instrument, "bass");
                assert_eq!(body.lines[1].instrument, "bass");
            }
            _ => panic!("expected pitched"),
        }
    }
}
