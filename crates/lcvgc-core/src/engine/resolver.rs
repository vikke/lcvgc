//! 変数参照の解決モジュール
//! Variable reference resolver module
//!
//! パーサーが記録した未解決変数参照を ScopeChain で解決し、
//! InstrumentDef/KitDef の数値フィールドを上書きする。
//! Resolves unresolved variable references recorded by the parser
//! via ScopeChain, overwriting numeric fields of InstrumentDef/KitDef.

use crate::ast::instrument::InstrumentDef;
use crate::ast::kit::KitDef;
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

    // channel の変数参照を解決（u8 に変換）
    // Resolve channel variable reference (convert to u8)
    if let Some(ref var_name) = inst.unresolved.channel {
        inst.channel = resolve_u8(scope, var_name, "channel")?;
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
        // channel の変数参照を解決
        // Resolve channel variable reference
        if let Some(ref var_name) = inst.unresolved.channel {
            inst.channel = resolve_u8(scope, var_name, "channel")?;
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
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::common::NoteName;
    use crate::ast::kit::{KitInstrument, KitInstrumentNote};
    use crate::ast::unresolved::{UnresolvedKitInstrumentVarRefs, UnresolvedVarRefs};

    #[test]
    fn resolve_instrument_device() {
        let mut scope = ScopeChain::new();
        scope.define_global("dev".into(), "mutant_brain".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: String::new(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
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
            channel: 0,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                channel: Some("ch".into()),
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.channel, 3);
    }

    #[test]
    fn resolve_instrument_gate_normal_and_staccato() {
        let mut scope = ScopeChain::new();
        scope.define_global("gn".into(), "100".into());
        scope.define_global("gs".into(), "50".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: 1,
            note: None,
            gate_normal: Some(0),
            gate_staccato: Some(0),
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: UnresolvedVarRefs {
                gate_normal: Some("gn".into()),
                gate_staccato: Some("gs".into()),
                ..Default::default()
            },
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.gate_normal, Some(100));
        assert_eq!(inst.gate_staccato, Some(50));
    }

    #[test]
    fn resolve_instrument_cc_number() {
        use crate::ast::instrument::CcMapping;

        let mut scope = ScopeChain::new();
        scope.define_global("cc_num".into(), "74".into());

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
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
            channel: 0,
            note: None,
            gate_normal: None,
            gate_staccato: None,
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
            channel: 0,
            note: None,
            gate_normal: None,
            gate_staccato: None,
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
                channel: 0,
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: None,
                gate_staccato: None,
                unresolved: UnresolvedKitInstrumentVarRefs {
                    channel: Some("drum_ch".into()),
                    ..Default::default()
                },
            }],
        };

        resolve_kit(&mut kit, &scope).unwrap();
        assert_eq!(kit.instruments[0].channel, 10);
    }

    #[test]
    fn resolve_no_unresolved_refs_is_noop() {
        let scope = ScopeChain::new();

        let mut inst = InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: 1,
            note: None,
            gate_normal: Some(100),
            gate_staccato: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        };

        resolve_instrument(&mut inst, &scope).unwrap();
        assert_eq!(inst.device, "mb");
        assert_eq!(inst.channel, 1);
        assert_eq!(inst.gate_normal, Some(100));
    }
}
