use nom::{bytes::complete::tag, IResult};

use crate::ast::instrument::{CcMapping, InstrumentDef, InstrumentNote};
use crate::ast::unresolved::UnresolvedVarRefs;
use crate::ast::var::VarDef;
use crate::parser::common::{
    identifier, note_name, parse_u8, parse_u8_or_identifier, ws, ws1, Either,
};
use crate::parser::var::parse_var;

/// デバイス参照をパースする: `device <identifier>`
/// Parse `device <identifier>`
fn parse_device(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag("device")(input)?;
    let (input, _) = ws1(input)?;
    identifier(input)
}

/// MIDIチャンネルをパースする: `channel <u8>` or `channel <var_ref>`（§6 変数展開）
/// Parse `channel <u8>` or `channel <var_ref>` (§6 variable expansion)
fn parse_channel(input: &str) -> IResult<&str, Either<u8, &str>> {
    let (input, _) = tag("channel")(input)?;
    let (input, _) = ws1(input)?;
    parse_u8_or_identifier(input)
}

/// 通常ゲート値をパースする: `gate_normal <u8>` or `gate_normal <var_ref>`（§6 変数展開）
/// Parse `gate_normal <u8>` or `gate_normal <var_ref>` (§6 variable expansion)
fn parse_gate_normal(input: &str) -> IResult<&str, Either<u8, &str>> {
    let (input, _) = tag("gate_normal")(input)?;
    let (input, _) = ws1(input)?;
    parse_u8_or_identifier(input)
}

/// スタッカートゲート値をパースする: `gate_staccato <u8>` or `gate_staccato <var_ref>`（§6 変数展開）
/// Parse `gate_staccato <u8>` or `gate_staccato <var_ref>` (§6 variable expansion)
fn parse_gate_staccato(input: &str) -> IResult<&str, Either<u8, &str>> {
    let (input, _) = tag("gate_staccato")(input)?;
    let (input, _) = ws1(input)?;
    parse_u8_or_identifier(input)
}

/// ノート（音名＋オクターブ）をパースする: `note <note_name><octave>`
/// Parse `note <note_name><octave>`
fn parse_note(input: &str) -> IResult<&str, InstrumentNote> {
    let (input, _) = tag("note")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = note_name(input)?;
    let (input, octave) = parse_u8(input)?;
    Ok((input, InstrumentNote { name, octave }))
}

/// CCマッピングをパースする: `cc <alias> <cc_number|var_ref>`（§6 変数展開）
/// Parse `cc <alias> <cc_number|var_ref>` (§6 variable expansion)
fn parse_cc(input: &str) -> IResult<&str, InstrumentProperty> {
    let (input, _) = tag("cc")(input)?;
    let (input, _) = ws1(input)?;
    let (input, alias) = identifier(input)?;
    let (input, _) = ws1(input)?;
    let (input, val) = parse_u8_or_identifier(input)?;
    match val {
        Either::Left(cc_number) => Ok((
            input,
            InstrumentProperty::Cc(CcMapping {
                alias: alias.to_string(),
                cc_number,
                cc_number_ref: None,
            }),
        )),
        Either::Right(var_ref) => Ok((
            input,
            InstrumentProperty::CcRef {
                alias: alias.to_string(),
                cc_number_ref: var_ref.to_string(),
            },
        )),
    }
}

/// インストゥルメントブロック内でパースされるプロパティ
/// Property parsed from inside an instrument block.
enum InstrumentProperty {
    /// デバイス名 / Device name
    Device(String),
    /// MIDIチャンネル / MIDI channel
    Channel(u8),
    /// channel の変数参照（§6 変数展開）/ Variable reference for channel (§6)
    ChannelRef(String),
    /// ノート（ドラム等の固定音） / Note (fixed pitch for drums, etc.)
    Note(InstrumentNote),
    /// 通常ゲート値 / Normal gate value
    GateNormal(u8),
    /// gate_normal の変数参照（§6 変数展開）/ Variable reference for gate_normal (§6)
    GateNormalRef(String),
    /// スタッカートゲート値 / Staccato gate value
    GateStaccato(u8),
    /// gate_staccato の変数参照（§6 変数展開）/ Variable reference for gate_staccato (§6)
    GateStaccatoRef(String),
    /// CCマッピング / CC mapping
    Cc(CcMapping),
    /// cc の変数参照（cc_number のみ）（§6 変数展開）/ CC with variable reference for cc_number (§6)
    CcRef {
        alias: String,
        cc_number_ref: String,
    },
    /// ブロック内ローカル変数定義（§6.1）/ Local variable definition (§6.1)
    Var(VarDef),
}

/// インストゥルメントプロパティ行を1つパースする
/// Parse a single instrument property line.
fn parse_property(input: &str) -> IResult<&str, InstrumentProperty> {
    if let Ok((rest, v)) = parse_var(input) {
        return Ok((rest, InstrumentProperty::Var(v)));
    }
    if let Ok((rest, dev)) = parse_device(input) {
        return Ok((rest, InstrumentProperty::Device(dev.to_string())));
    }
    if let Ok((rest, ch)) = parse_channel(input) {
        return Ok((
            rest,
            match ch {
                Either::Left(v) => InstrumentProperty::Channel(v),
                Either::Right(r) => InstrumentProperty::ChannelRef(r.to_string()),
            },
        ));
    }
    if let Ok((rest, note)) = parse_note(input) {
        return Ok((rest, InstrumentProperty::Note(note)));
    }
    if let Ok((rest, gn)) = parse_gate_normal(input) {
        return Ok((
            rest,
            match gn {
                Either::Left(v) => InstrumentProperty::GateNormal(v),
                Either::Right(r) => InstrumentProperty::GateNormalRef(r.to_string()),
            },
        ));
    }
    if let Ok((rest, gs)) = parse_gate_staccato(input) {
        return Ok((
            rest,
            match gs {
                Either::Left(v) => InstrumentProperty::GateStaccato(v),
                Either::Right(r) => InstrumentProperty::GateStaccatoRef(r.to_string()),
            },
        ));
    }
    if let Ok((rest, cc)) = parse_cc(input) {
        return Ok((rest, cc));
    }
    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Alt,
    )))
}

/// インストゥルメントブロック全体をパースする: `instrument <name> { ... }`
/// Parse a full `instrument <name> { ... }` block.
pub fn parse_instrument(input: &str) -> IResult<&str, InstrumentDef> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("instrument")(input)?;
    let (input, _) = ws1(input)?;
    let (input, name) = identifier(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("{")(input)?;

    let mut device = None;
    let mut channel = None;
    let mut note = None;
    let mut gate_normal = None;
    let mut gate_staccato = None;
    let mut cc_mappings = Vec::new();
    let mut local_vars = Vec::new();
    let mut unresolved = UnresolvedVarRefs::default();

    let mut rest = input;
    loop {
        let (input, _) = ws(rest)?;
        if let Ok((input, _)) = tag::<&str, &str, nom::error::Error<&str>>("}")(input) {
            rest = input;
            break;
        }
        let (input, prop) = parse_property(input)?;
        match prop {
            InstrumentProperty::Device(d) => device = Some(d),
            InstrumentProperty::Channel(c) => channel = Some(c),
            InstrumentProperty::ChannelRef(r) => {
                unresolved.channel = Some(r);
                channel = Some(0); // placeholder
            }
            InstrumentProperty::Note(n) => note = Some(n),
            InstrumentProperty::GateNormal(g) => gate_normal = Some(g),
            InstrumentProperty::GateNormalRef(r) => {
                unresolved.gate_normal = Some(r);
                gate_normal = Some(0); // placeholder
            }
            InstrumentProperty::GateStaccato(g) => gate_staccato = Some(g),
            InstrumentProperty::GateStaccatoRef(r) => {
                unresolved.gate_staccato = Some(r);
                gate_staccato = Some(0); // placeholder
            }
            InstrumentProperty::Cc(cc) => cc_mappings.push(cc),
            InstrumentProperty::CcRef {
                alias,
                cc_number_ref,
            } => {
                cc_mappings.push(CcMapping {
                    alias,
                    cc_number: 0, // placeholder
                    cc_number_ref: Some(cc_number_ref),
                });
            }
            InstrumentProperty::Var(v) => local_vars.push(v),
        }
        rest = input;
    }

    let device = device.ok_or_else(|| {
        nom::Err::Failure(nom::error::Error::new(rest, nom::error::ErrorKind::Tag))
    })?;
    let channel = channel.ok_or_else(|| {
        nom::Err::Failure(nom::error::Error::new(rest, nom::error::ErrorKind::Tag))
    })?;

    Ok((
        rest,
        InstrumentDef {
            name: name.to_string(),
            device,
            channel,
            note,
            gate_normal,
            gate_staccato,
            cc_mappings,
            local_vars,
            unresolved,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::common::NoteName;

    #[test]
    fn test_full_instrument() {
        let input = r#"instrument bass {
  device mutant_brain
  channel 1
  gate_normal 80
  gate_staccato 40
  cc cutoff 74
  cc resonance 71
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.name, "bass");
        assert_eq!(inst.device, "mutant_brain");
        assert_eq!(inst.channel, 1);
        assert_eq!(inst.note, None);
        assert_eq!(inst.gate_normal, Some(80));
        assert_eq!(inst.gate_staccato, Some(40));
        assert_eq!(inst.cc_mappings.len(), 2);
        assert_eq!(inst.cc_mappings[0].alias, "cutoff");
        assert_eq!(inst.cc_mappings[0].cc_number, 74);
        assert_eq!(inst.cc_mappings[1].alias, "resonance");
        assert_eq!(inst.cc_mappings[1].cc_number, 71);
    }

    #[test]
    fn test_minimal_instrument() {
        let input = r#"instrument synth {
  device mb
  channel 3
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.name, "synth");
        assert_eq!(inst.device, "mb");
        assert_eq!(inst.channel, 3);
        assert_eq!(inst.note, None);
        assert_eq!(inst.gate_normal, None);
        assert_eq!(inst.gate_staccato, None);
        assert!(inst.cc_mappings.is_empty());
    }

    #[test]
    fn test_drum_instrument_with_note() {
        let input = r#"instrument bd {
  device mutant_brain
  channel 10
  note c2
  gate_normal 50
  gate_staccato 20
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.name, "bd");
        assert_eq!(inst.channel, 10);
        let note = inst.note.unwrap();
        assert_eq!(note.name, NoteName::C);
        assert_eq!(note.octave, 2);
    }

    #[test]
    fn test_arbitrary_property_order() {
        let input = r#"instrument lead {
  gate_staccato 30
  cc vibrato 1
  channel 2
  gate_normal 90
  device mb
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.name, "lead");
        assert_eq!(inst.device, "mb");
        assert_eq!(inst.channel, 2);
        assert_eq!(inst.gate_normal, Some(90));
        assert_eq!(inst.gate_staccato, Some(30));
        assert_eq!(inst.cc_mappings.len(), 1);
        assert_eq!(inst.cc_mappings[0].alias, "vibrato");
        assert_eq!(inst.cc_mappings[0].cc_number, 1);
    }

    /// ブロック内 var 定義をパースできること（§6.1）
    /// Verify that var definitions inside instrument blocks are parsed (§6.1)
    #[test]
    fn test_instrument_with_local_vars() {
        let input = r#"instrument bass {
  var ch = 3
  device mutant_brain
  channel 3
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.name, "bass");
        assert_eq!(inst.device, "mutant_brain");
        assert_eq!(inst.channel, 3);
        assert_eq!(inst.local_vars.len(), 1);
        assert_eq!(inst.local_vars[0].name, "ch");
        assert_eq!(inst.local_vars[0].value, "3");
    }

    /// 複数の var がブロック内でパースできること
    /// Verify that multiple vars can be parsed inside a block
    #[test]
    fn test_instrument_with_multiple_local_vars() {
        let input = r#"instrument lead {
  var dev = mb
  var ch = 2
  device mb
  channel 2
}"#;
        let (rest, inst) = parse_instrument(input).unwrap();
        assert_eq!(rest.trim(), "");
        assert_eq!(inst.local_vars.len(), 2);
        assert_eq!(inst.local_vars[0].name, "dev");
        assert_eq!(inst.local_vars[0].value, "mb");
        assert_eq!(inst.local_vars[1].name, "ch");
        assert_eq!(inst.local_vars[1].value, "2");
    }

    /// var なしのインストゥルメントで local_vars が空であること
    /// Verify local_vars is empty when no vars are defined
    #[test]
    fn test_instrument_no_local_vars() {
        let input = r#"instrument synth {
  device mb
  channel 1
}"#;
        let (_, inst) = parse_instrument(input).unwrap();
        assert!(inst.local_vars.is_empty());
    }

    #[test]
    fn test_instrument_with_channel_var_ref() {
        let input = "instrument bass {\n  device mutant_brain\n  channel bass_ch\n}";
        let (_, inst) = parse_instrument(input).unwrap();
        assert_eq!(inst.name, "bass");
        assert_eq!(inst.device, "mutant_brain");
        assert_eq!(inst.channel, 0); // placeholder
        assert_eq!(inst.unresolved.channel, Some("bass_ch".to_string()));
    }

    #[test]
    fn test_instrument_with_device_var_ref() {
        let input = "instrument bass {\n  device dev\n  channel 1\n}";
        let (_, inst) = parse_instrument(input).unwrap();
        assert_eq!(inst.name, "bass");
        assert_eq!(inst.channel, 1);
        // device "dev" は予約語でないidentifierなので通常通りパースされる
        // （device は文字列なので変数参照の区別はパーサー段階では不要）
        assert_eq!(inst.device, "dev");
    }

    #[test]
    fn test_instrument_with_gate_var_refs() {
        let input = "instrument bass {\n  device mb\n  channel 1\n  gate_normal gn_val\n  gate_staccato gs_val\n}";
        let (_, inst) = parse_instrument(input).unwrap();
        assert_eq!(inst.unresolved.gate_normal, Some("gn_val".to_string()));
        assert_eq!(inst.unresolved.gate_staccato, Some("gs_val".to_string()));
    }

    #[test]
    fn test_instrument_with_cc_var_ref() {
        let input = "instrument bass {\n  device mb\n  channel 1\n  cc filter cc_num\n}";
        let (_, inst) = parse_instrument(input).unwrap();
        assert_eq!(inst.cc_mappings.len(), 1);
        assert_eq!(inst.cc_mappings[0].alias, "filter");
        assert_eq!(inst.cc_mappings[0].cc_number, 0); // placeholder
        assert_eq!(
            inst.cc_mappings[0].cc_number_ref,
            Some("cc_num".to_string())
        );
    }
}
