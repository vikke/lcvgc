//! 変数参照の解決モジュール
//! Variable reference resolver module
//!
//! パーサーが記録した未解決変数参照を ScopeChain で解決し、
//! InstrumentDef/KitDef の数値フィールドを上書きする。
//! Resolves unresolved variable references recorded by the parser
//! via ScopeChain, overwriting numeric fields of InstrumentDef/KitDef.

use crate::ast::instrument::InstrumentDef;
use crate::ast::kit::KitDef;
use crate::domain::channel::MidiChannel;
use crate::engine::error::EngineError;
use crate::engine::scope::ScopeChain;

/// 変数参照を u8 値に解決するヘルパー
/// Helper to resolve a variable reference to a u8 value
fn resolve_u8(scope: &ScopeChain, var_name: &str, field: &str) -> Result<u8, EngineError> {
    let value = scope
        .resolve(var_name)
        .ok_or_else(|| EngineError::UndefinedVariable {
            name: var_name.to_string(),
            field: field.to_string(),
        })?;
    value
        .parse::<u8>()
        .map_err(|_| EngineError::InvalidVariableValue {
            name: var_name.to_string(),
            value: value.to_string(),
            expected_type: "u8".to_string(),
        })
}

/// 変数参照を `MidiChannel` (1-based DSL 値として解釈) に解決するヘルパー
/// Helper to resolve a variable reference into a `MidiChannel`,
/// interpreting the value as 1-based (matching DSL notation).
fn resolve_channel(
    scope: &ScopeChain,
    var_name: &str,
    field: &str,
) -> Result<MidiChannel, EngineError> {
    let raw = resolve_u8(scope, var_name, field)?;
    MidiChannel::from_one_based(raw).map_err(|_| EngineError::InvalidVariableValue {
        name: var_name.to_string(),
        value: raw.to_string(),
        expected_type: "MIDI channel (1-16)".to_string(),
    })
}

/// InstrumentDef の未解決変数参照を解決する（§6 変数展開）
/// Resolve unresolved variable references in an InstrumentDef (§6 variable expansion)
///
/// # Arguments
/// * `inst` - 解決対象のインストゥルメント定義（可変参照）
/// * `scope` - 変数スコープチェーン
///
/// # Errors
/// * `EngineError::UndefinedVariable` - 変数が未定義の場合
/// * `EngineError::InvalidVariableValue` - 変数値が期待される型に変換できない場合
pub fn resolve_instrument(inst: &mut InstrumentDef, scope: &ScopeChain) -> Result<(), EngineError> {
    // device の変数参照を解決（String なのでそのまま）
    // Resolve device variable reference (String, used as-is)
    if let Some(ref var_name) = inst.unresolved.device {
        let value = scope
            .resolve(var_name)
            .ok_or_else(|| EngineError::UndefinedVariable {
                name: var_name.clone(),
                field: "device".to_string(),
            })?;
        inst.device = value.to_string();
    }

    // channel の変数参照を解決（DSL の値と同じく 1-based として MidiChannel に変換）
    // Resolve channel variable reference; the value is interpreted as 1-based
    // (same convention as DSL) and converted into a `MidiChannel`.
    if let Some(ref var_name) = inst.unresolved.channel {
        inst.channel = resolve_channel(scope, var_name, "channel")?;
    }

    // gate_normal の変数参照を解決
    // Resolve gate_normal variable reference
    if let Some(ref var_name) = inst.unresolved.gate_normal {
        inst.gate_normal = Some(resolve_u8(scope, var_name, "gate_normal")?);
    }

    // gate_staccato の変数参照を解決
    // Resolve gate_staccato variable reference
    if let Some(ref var_name) = inst.unresolved.gate_staccato {
        inst.gate_staccato = Some(resolve_u8(scope, var_name, "gate_staccato")?);
    }

    // velocity_normal の変数参照を解決
    // Resolve velocity_normal variable reference
    if let Some(ref var_name) = inst.unresolved.velocity_normal {
        inst.velocity_normal = Some(resolve_u8(scope, var_name, "velocity_normal")?);
    }

    // velocity_accent の変数参照を解決
    // Resolve velocity_accent variable reference
    if let Some(ref var_name) = inst.unresolved.velocity_accent {
        inst.velocity_accent = Some(resolve_u8(scope, var_name, "velocity_accent")?);
    }

    // velocity_ghost の変数参照を解決
    // Resolve velocity_ghost variable reference
    if let Some(ref var_name) = inst.unresolved.velocity_ghost {
        inst.velocity_ghost = Some(resolve_u8(scope, var_name, "velocity_ghost")?);
    }

    // CC マッピングの変数参照を解決
    // Resolve CC mapping variable references
    for cc in &mut inst.cc_mappings {
        if let Some(ref var_name) = cc.cc_number_ref {
            cc.cc_number = resolve_u8(scope, var_name, "cc_number")?;
        }
    }

    Ok(())
}

/// KitDef の未解決変数参照を解決する（§6 変数展開）
/// Resolve unresolved variable references in a KitDef (§6 variable expansion)
///
/// # Arguments
/// * `kit` - 解決対象のキット定義（可変参照）
/// * `scope` - 変数スコープチェーン
///
/// # Errors
/// * `EngineError::UndefinedVariable` - 変数が未定義の場合
/// * `EngineError::InvalidVariableValue` - 変数値が期待される型に変換できない場合
pub fn resolve_kit(kit: &mut KitDef, scope: &ScopeChain) -> Result<(), EngineError> {
    // kit 内の各インストゥルメントの未解決参照を解決
    // Resolve unresolved references in each kit instrument
    for inst in &mut kit.instruments {
        // channel の変数参照を解決（DSL の値と同じく 1-based として MidiChannel に変換）
        // Resolve channel variable reference; interpreted as 1-based.
        if let Some(ref var_name) = inst.unresolved.channel {
            inst.channel = resolve_channel(scope, var_name, "channel")?;
        }

        // gate_normal の変数参照を解決
        // Resolve gate_normal variable reference
        if let Some(ref var_name) = inst.unresolved.gate_normal {
            inst.gate_normal = Some(resolve_u8(scope, var_name, "gate_normal")?);
        }

        // gate_staccato の変数参照を解決
        // Resolve gate_staccato variable reference
        if let Some(ref var_name) = inst.unresolved.gate_staccato {
            inst.gate_staccato = Some(resolve_u8(scope, var_name, "gate_staccato")?);
        }

        // velocity_normal の変数参照を解決
        // Resolve velocity_normal variable reference
        if let Some(ref var_name) = inst.unresolved.velocity_normal {
            inst.velocity_normal = Some(resolve_u8(scope, var_name, "velocity_normal")?);
        }

        // velocity_accent の変数参照を解決
        // Resolve velocity_accent variable reference
        if let Some(ref var_name) = inst.unresolved.velocity_accent {
            inst.velocity_accent = Some(resolve_u8(scope, var_name, "velocity_accent")?);
        }

        // velocity_ghost の変数参照を解決
        // Resolve velocity_ghost variable reference
        if let Some(ref var_name) = inst.unresolved.velocity_ghost {
            inst.velocity_ghost = Some(resolve_u8(scope, var_name, "velocity_ghost")?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::kit::{KitInstrument, KitInstrumentNote};
    use crate::ast::unresolved::{UnresolvedKitInstrumentVarRefs, UnresolvedVarRefs};
    use crate::domain::pitch::NoteName;

    #[test]
    fn resolve_instrument_device() {
        let mut scope = ScopeChain::new();
        scope.define_global("dev".into(), "mutant_brain".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: String::new(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                device: Some("dev".into()),
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.device, "mutant_brain");
    }

    #[test]
    fn resolve_instrument_channel() {
        let mut scope = ScopeChain::new();
        scope.define_global("ch".into(), "3".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: MidiChannel::from_zero_based(0).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                channel: Some("ch".into()),
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.channel.as_one_based(), 3);
    }

    #[test]
    fn resolve_instrument_gate_normal_and_staccato() {
        let mut scope = ScopeChain::new();
        scope.define_global("gn".into(), "100".into());
        scope.define_global("gs".into(), "50".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(0),
            gate_staccato: Some(0),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                gate_normal: Some("gn".into()),
                gate_staccato: Some("gs".into()),
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.gate_normal, Some(100));
        assert_eq!(inst.gate_staccato, Some(50));
    }

    /// instrument の velocity_normal/accent/ghost の変数参照が解決されること（§6）。
    /// Verify velocity_normal/accent/ghost variable references are resolved on an instrument (§6).
    #[test]
    fn resolve_instrument_velocity_refs() {
        let mut scope = ScopeChain::new();
        scope.define_global("vn".into(), "90".into());
        scope.define_global("va".into(), "120".into());
        scope.define_global("vg".into(), "30".into());

        let mut inst = InstrumentDef {
            name: "piano".into(),
            device: "mb".into(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: Some(0),
            velocity_accent: Some(0),
            velocity_ghost: Some(0),
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                velocity_normal: Some("vn".into()),
                velocity_accent: Some("va".into()),
                velocity_ghost: Some("vg".into()),
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.velocity_normal, Some(90));
        assert_eq!(inst.velocity_accent, Some(120));
        assert_eq!(inst.velocity_ghost, Some(30));
    }

    #[test]
    fn resolve_instrument_cc_number() {
        use crate::ast::instrument::CcMapping;

        let mut scope = ScopeChain::new();
        scope.define_global("cc_num".into(), "74".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![CcMapping {
                alias: "filter".into(),
                cc_number: 0,
                cc_number_ref: Some("cc_num".into()),
            }],
            local_vars: vec![],
            unresolved: Default::default(),
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.cc_mappings[0].cc_number, 74);
    }

    #[test]
    fn resolve_instrument_undefined_variable() {
        let scope = ScopeChain::new();

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: String::new(),
            channel: MidiChannel::from_zero_based(0).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                channel: Some("missing_var".into()),
                ..Default::default()
            },
        };

        let result = resolve_instrument(&mut inst, &scope);
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::UndefinedVariable { name, field } => {
                assert_eq!(name, "missing_var");
                assert_eq!(field, "channel");
            }
            other => panic!("Expected UndefinedVariable, got: {:?}", other),
        }
    }

    #[test]
    fn resolve_instrument_invalid_value() {
        let mut scope = ScopeChain::new();
        scope.define_global("ch".into(), "abc".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: MidiChannel::from_zero_based(0).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                channel: Some("ch".into()),
                ..Default::default()
            },
        };

        let result = resolve_instrument(&mut inst, &scope);
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::InvalidVariableValue {
                name,
                value,
                expected_type,
            } => {
                assert_eq!(name, "ch");
                assert_eq!(value, "abc");
                assert_eq!(expected_type, "u8");
            }
            other => panic!("Expected InvalidVariableValue, got: {:?}", other),
        }
    }

    #[test]
    fn resolve_kit_channel() {
        let mut scope = ScopeChain::new();
        scope.define_global("drum_ch".into(), "10".into());

        let mut kit = KitDef {
            name: "drums".into(),
            device: "td3".into(),
            instruments: vec![KitInstrument {
                name: "bd".into(),
                channel: MidiChannel::from_zero_based(0).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: None,
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                unresolved: UnresolvedKitInstrumentVarRefs {
                    channel: Some("drum_ch".into()),
                    ..Default::default()
                },
            }],
        };

        resolve_kit(&mut kit, &scope).unwrap();
        assert_eq!(kit.instruments[0].channel.as_one_based(), 10);
    }

    /// kit インストゥルメントの velocity_normal/accent/ghost の変数参照が解決されること（§6）。
    /// Verify velocity_normal/accent/ghost variable references are resolved on a kit instrument (§6).
    #[test]
    fn resolve_kit_velocity_refs() {
        let mut scope = ScopeChain::new();
        scope.define_global("vn".into(), "90".into());
        scope.define_global("va".into(), "120".into());
        scope.define_global("vg".into(), "30".into());

        let mut kit = KitDef {
            name: "drums".into(),
            device: "td3".into(),
            instruments: vec![KitInstrument {
                name: "sn".into(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::D,
                    octave: 2,
                },
                gate_normal: None,
                gate_staccato: None,
                velocity_normal: Some(0),
                velocity_accent: Some(0),
                velocity_ghost: Some(0),
                unresolved: UnresolvedKitInstrumentVarRefs {
                    velocity_normal: Some("vn".into()),
                    velocity_accent: Some("va".into()),
                    velocity_ghost: Some("vg".into()),
                    ..Default::default()
                },
            }],
        };

        resolve_kit(&mut kit, &scope).unwrap();
        assert_eq!(kit.instruments[0].velocity_normal, Some(90));
        assert_eq!(kit.instruments[0].velocity_accent, Some(120));
        assert_eq!(kit.instruments[0].velocity_ghost, Some(30));
    }

    #[test]
    fn resolve_no_unresolved_refs_is_noop() {
        let scope = ScopeChain::new();

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(100),
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.device, "mb");
        assert_eq!(inst.channel.as_one_based(), 1);
        assert_eq!(inst.gate_normal, Some(100));
    }
}
