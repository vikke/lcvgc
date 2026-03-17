use nom::{
    bytes::complete::tag,
    character::complete::char,
    multi::many1,
    sequence::{delimited, preceded, terminated},
    IResult,
};

#[cfg(test)]
use crate::ast::common::NoteName;
use crate::ast::kit::{KitDef, KitInstrument, KitInstrumentNote};
use crate::parser::common::{identifier, note_name, parse_u8, ws, ws1};

/// ノート（音名＋オクターブ）をパースする（例: `c2`, `f#2`, `a#2`）。
/// Parse a note: note name + octave (e.g. `c2`, `f#2`, `a#2`).
fn parse_instrument_note(input: &str) -> IResult<&str, KitInstrumentNote> {
    let (input, name) = note_name(input)?;
    let (input, octave) = parse_u8(input)?;
    Ok((input, KitInstrumentNote { name, octave }))
}

/// インストゥルメントの個々のプロパティを表す列挙型。
/// Enum representing a single instrument property parsed from a kit definition.
enum InstrumentProp {
    /// MIDIチャンネル番号
    /// MIDI channel number
    Channel(u8),
    /// 発音するノート（音名＋オクターブ）
    /// Note to trigger (note name + octave)
    Note(KitInstrumentNote),
    /// 通常発音時のゲート値
    /// Gate value for normal articulation
    GateNormal(u8),
    /// スタッカート時のゲート値
    /// Gate value for staccato articulation
    GateStaccato(u8),
}

/// カンマ区切りのインストゥルメントプロパティを1つパースする。
/// Parse a single comma-separated instrument property.
fn parse_instrument_prop(input: &str) -> IResult<&str, InstrumentProp> {
    let (input, key) = identifier(input)?;
    let (input, _) = ws1(input)?;
    match key {
        "channel" => {
            let (input, v) = parse_u8(input)?;
            Ok((input, InstrumentProp::Channel(v)))
        }
        "note" => {
            let (input, v) = parse_instrument_note(input)?;
            Ok((input, InstrumentProp::Note(v)))
        }
        "gate_normal" => {
            let (input, v) = parse_u8(input)?;
            Ok((input, InstrumentProp::GateNormal(v)))
        }
        "gate_staccato" => {
            let (input, v) = parse_u8(input)?;
            Ok((input, InstrumentProp::GateStaccato(v)))
        }
        _ => Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        ))),
    }
}

/// `{ ... }` 内のプロパティをパースする。カンマ区切り・改行区切り・混在のいずれにも対応。
/// Parse properties inside `{ ... }`, separated by commas and/or whitespace.
///
/// Supports both comma-separated (`channel 10, note c2`) and
/// newline-separated (one property per line) formats, as well as
/// any mix of the two.
fn parse_instrument_props(input: &str) -> IResult<&str, Vec<InstrumentProp>> {
    let (input, first) = parse_instrument_prop(input)?;
    let mut props = vec![first];
    let mut input = input;
    loop {
        let (trimmed, _) = ws(input)?;
        // Skip optional comma
        let trimmed = if let Ok((rest, _)) = char::<&str, nom::error::Error<&str>>(',')(trimmed) {
            ws(rest)?.0
        } else {
            trimmed
        };
        // Try to parse the next property; stop if none found
        if let Ok((rest, prop)) = parse_instrument_prop(trimmed) {
            props.push(prop);
            input = rest;
        } else {
            input = trimmed;
            break;
        }
    }
    Ok((input, props))
}

/// インストゥルメント1行をパースする: `name { channel 10, note c2, ... }`
/// Parse a single instrument line: `name { channel 10, note c2, ... }`
fn parse_instrument(input: &str) -> IResult<&str, KitInstrument> {
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, props) = delimited(
        char('{'),
        preceded(ws, terminated(parse_instrument_props, ws)),
        char('}'),
    )(input)?;

    let mut channel = None;
    let mut note = None;
    let mut gate_normal = None;
    let mut gate_staccato = None;

    for prop in props {
        match prop {
            InstrumentProp::Channel(v) => channel = Some(v),
            InstrumentProp::Note(v) => note = Some(v),
            InstrumentProp::GateNormal(v) => gate_normal = Some(v),
            InstrumentProp::GateStaccato(v) => gate_staccato = Some(v),
        }
    }

    let channel = channel.ok_or_else(|| {
        nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;
    let note = note.ok_or_else(|| {
        nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;

    Ok((
        input,
        KitInstrument {
            name: name.to_string(),
            channel,
            note,
            gate_normal,
            gate_staccato,
            unresolved: Default::default(),
        },
    ))
}

/// キットブロック全体をパースする。
/// Parse a full kit block.
pub fn parse_kit(input: &str) -> IResult<&str, KitDef> {
    let (input, _) = tag("kit")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = ws(input)?;

    // Parse device line
    let (input, _) = tag("device")(input)?;
    let (input, _) = ws1(input)?;
    let (input, device) = identifier(input)?;
    let (input, _) = ws(input)?;

    // Parse instruments
    let (input, instruments) = many1(terminated(parse_instrument, ws))(input)?;

    let (input, _) = char('}')(input)?;

    Ok((
        input,
        KitDef {
            name: name.to_string(),
            device: device.to_string(),
            instruments,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_kit_with_multiple_instruments() {
        let input = r#"kit tr808 {
  device mutant_brain
  bd    { channel 10, note c2, gate_normal 50, gate_staccato 20 }
  snare { channel 10, note d2 }
  hh    { channel 10, note f#2, gate_normal 30, gate_staccato 10 }
  oh    { channel 10, note a#2, gate_normal 80 }
  clap  { channel 10, note d#2 }
}"#;
        let (rest, kit) = parse_kit(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(kit.name, "tr808");
        assert_eq!(kit.device, "mutant_brain");
        assert_eq!(kit.instruments.len(), 5);

        // bd
        let bd = &kit.instruments[0];
        assert_eq!(bd.name, "bd");
        assert_eq!(bd.channel, 10);
        assert_eq!(
            bd.note,
            KitInstrumentNote {
                name: NoteName::C,
                octave: 2
            }
        );
        assert_eq!(bd.gate_normal, Some(50));
        assert_eq!(bd.gate_staccato, Some(20));

        // snare - minimal (channel + note only)
        let snare = &kit.instruments[1];
        assert_eq!(snare.name, "snare");
        assert_eq!(snare.channel, 10);
        assert_eq!(
            snare.note,
            KitInstrumentNote {
                name: NoteName::D,
                octave: 2
            }
        );
        assert_eq!(snare.gate_normal, None);
        assert_eq!(snare.gate_staccato, None);

        // hh - sharp note
        let hh = &kit.instruments[2];
        assert_eq!(
            hh.note,
            KitInstrumentNote {
                name: NoteName::Fs,
                octave: 2
            }
        );
        assert_eq!(hh.gate_normal, Some(30));
        assert_eq!(hh.gate_staccato, Some(10));

        // oh - a#2
        let oh = &kit.instruments[3];
        assert_eq!(
            oh.note,
            KitInstrumentNote {
                name: NoteName::As,
                octave: 2
            }
        );
        assert_eq!(oh.gate_normal, Some(80));
        assert_eq!(oh.gate_staccato, None);

        // clap - d#2
        let clap = &kit.instruments[4];
        assert_eq!(
            clap.note,
            KitInstrumentNote {
                name: NoteName::Ds,
                octave: 2
            }
        );
    }

    #[test]
    fn test_minimal_instrument() {
        let input = r#"kit minimal {
  device test_dev
  kick { channel 1, note c4 }
}"#;
        let (_, kit) = parse_kit(input).unwrap();
        assert_eq!(kit.instruments.len(), 1);
        let kick = &kit.instruments[0];
        assert_eq!(kick.name, "kick");
        assert_eq!(kick.channel, 1);
        assert_eq!(
            kick.note,
            KitInstrumentNote {
                name: NoteName::C,
                octave: 4
            }
        );
        assert_eq!(kick.gate_normal, None);
        assert_eq!(kick.gate_staccato, None);
    }

    #[test]
    fn test_sharp_and_flat_notes() {
        let input = r#"kit notes_test {
  device dev
  a { channel 1, note f#2 }
  b { channel 1, note a#2 }
  c { channel 1, note d#2 }
}"#;
        let (_, kit) = parse_kit(input).unwrap();
        assert_eq!(kit.instruments[0].note.name, NoteName::Fs);
        assert_eq!(kit.instruments[1].note.name, NoteName::As);
        assert_eq!(kit.instruments[2].note.name, NoteName::Ds);
    }

    /// 改行区切り（カンマなし）のkit定義がパースできることを検証する。
    /// Verify that kit definitions with newline-separated properties (no commas) can be parsed.
    /// tree-sitter文法と一貫した構文をサポートする。
    /// Supports syntax consistent with tree-sitter grammar.
    #[test]
    fn test_kit_with_newline_separated_props() {
        let input = r#"kit tr808 {
  device my_synth
  kick {
    channel 10
    note c2
  }
  snare {
    channel 10
    note d2
  }
  hihat {
    channel 10
    note f#2
  }
}"#;
        let (rest, kit) = parse_kit(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(kit.name, "tr808");
        assert_eq!(kit.device, "my_synth");
        assert_eq!(kit.instruments.len(), 3);

        let kick = &kit.instruments[0];
        assert_eq!(kick.name, "kick");
        assert_eq!(kick.channel, 10);
        assert_eq!(
            kick.note,
            KitInstrumentNote {
                name: NoteName::C,
                octave: 2
            }
        );

        let snare = &kit.instruments[1];
        assert_eq!(snare.name, "snare");
        assert_eq!(snare.channel, 10);
        assert_eq!(
            snare.note,
            KitInstrumentNote {
                name: NoteName::D,
                octave: 2
            }
        );

        let hihat = &kit.instruments[2];
        assert_eq!(hihat.name, "hihat");
        assert_eq!(hihat.channel, 10);
        assert_eq!(
            hihat.note,
            KitInstrumentNote {
                name: NoteName::Fs,
                octave: 2
            }
        );
    }

    /// カンマと改行が混在するkit定義がパースできることを検証する。
    /// Verify that kit definitions with mixed comma and newline separators can be parsed.
    #[test]
    fn test_kit_with_mixed_separators() {
        let input = r#"kit mixed {
  device dev
  kick { channel 10, note c2 }
  snare {
    channel 10
    note d2
  }
}"#;
        let (rest, kit) = parse_kit(input).unwrap();
        assert_eq!(rest, "");
        assert_eq!(kit.instruments.len(), 2);
        assert_eq!(kit.instruments[0].name, "kick");
        assert_eq!(kit.instruments[1].name, "snare");
    }

    #[test]
    fn test_parse_instrument_note() {
        let (_, note) = parse_instrument_note("c2").unwrap();
        assert_eq!(
            note,
            KitInstrumentNote {
                name: NoteName::C,
                octave: 2
            }
        );

        let (_, note) = parse_instrument_note("f#2").unwrap();
        assert_eq!(
            note,
            KitInstrumentNote {
                name: NoteName::Fs,
                octave: 2
            }
        );

        let (_, note) = parse_instrument_note("a#2").unwrap();
        assert_eq!(
            note,
            KitInstrumentNote {
                name: NoteName::As,
                octave: 2
            }
        );
    }
}
