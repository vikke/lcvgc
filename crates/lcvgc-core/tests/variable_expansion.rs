//! 変数展開メカニズム（§6）の E2E テスト
//! End-to-end tests for the variable expansion mechanism (§6)

use lcvgc_core::engine::error::EngineError;
use lcvgc_core::engine::evaluator::Evaluator;

/// device 変数展開: `var dev = mutant_brain` → `device dev`
/// Device variable expansion
#[test]
fn var_expansion_device() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device mutant_brain {
  port Mutant Brain
}

var dev = mutant_brain

instrument bass {
  device dev
  channel 1
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.device, "mutant_brain");
}

/// channel 変数展開（数値変換）: `var ch = 3` → `channel ch`
/// Channel variable expansion (numeric conversion)
#[test]
fn var_expansion_channel() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var ch = 3

instrument bass {
  device mb
  channel ch
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.channel, 3);
}

/// gate_normal 変数展開
/// gate_normal variable expansion
#[test]
fn var_expansion_gate_normal() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var gn = 100

instrument bass {
  device mb
  channel 1
  gate_normal gn
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.gate_normal, Some(100));
}

/// gate_staccato 変数展開
/// gate_staccato variable expansion
#[test]
fn var_expansion_gate_staccato() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var gs = 50

instrument bass {
  device mb
  channel 1
  gate_staccato gs
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.gate_staccato, Some(50));
}

/// cc cc_number 変数展開
/// cc cc_number variable expansion
#[test]
fn var_expansion_cc_number() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var cc_num = 74

instrument bass {
  device mb
  channel 1
  cc filter cc_num
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.cc_mappings[0].alias, "filter");
    assert_eq!(inst.cc_mappings[0].cc_number, 74);
}

/// ブロックスコープ + シャドーイング: ブロック内 var がグローバルを上書き
/// Block scope + shadowing: block-local var overrides global
#[test]
fn var_expansion_block_scope_shadowing() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var ch = 1

instrument bass {
  var ch = 3
  device mb
  channel ch
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.channel, 3);
    // ブロック後はグローバルスコープに戻る
    // After block, global scope is restored
    assert_eq!(ev.scope().resolve("ch"), Some("1"));
}

/// ブロック後のグローバルスコープ復帰確認
/// Verify global scope restoration after block
#[test]
fn var_expansion_global_scope_restored() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var ch = 5

instrument bass {
  var ch = 1
  device mb
  channel ch
}

instrument lead {
  device mb
  channel ch
}
"#;
    ev.eval_source(source).unwrap();
    let bass = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(bass.channel, 1);
    let lead = ev.registry().get_instrument("lead").unwrap();
    assert_eq!(lead.channel, 5);
}

/// 複数のフィールドを同時に変数展開
/// Multiple fields expanded via variables simultaneously
#[test]
fn var_expansion_multiple_fields() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var dev = synth
var ch = 2
var gn = 90
var gs = 30

device synth {
  port IAC
}

instrument lead {
  device dev
  channel ch
  gate_normal gn
  gate_staccato gs
}
"#;
    ev.eval_source(source).unwrap();
    let inst = ev.registry().get_instrument("lead").unwrap();
    assert_eq!(inst.device, "synth");
    assert_eq!(inst.channel, 2);
    assert_eq!(inst.gate_normal, Some(90));
    assert_eq!(inst.gate_staccato, Some(30));
}

/// 未定義変数エラー
/// Undefined variable error
#[test]
fn var_expansion_undefined_variable() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
instrument bass {
  device mb
  channel missing_var
}
"#;
    let result = ev.eval_source(source);
    assert!(result.is_err());
    match result.unwrap_err() {
        EngineError::UndefinedVariable { name, field } => {
            assert_eq!(name, "missing_var");
            assert_eq!(field, "channel");
        }
        other => panic!("Expected UndefinedVariable, got: {:?}", other),
    }
}

/// 数値変換失敗エラー
/// Numeric conversion failure error
#[test]
fn var_expansion_invalid_value() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var ch = abc

instrument bass {
  device mb
  channel ch
}
"#;
    let result = ev.eval_source(source);
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

/// kit 内の変数展開
/// Variable expansion within kit definition
#[test]
fn var_expansion_kit_channel() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var drum_ch = 10

kit drums {
  device td3
  bd { channel drum_ch, note c2 }
}
"#;
    ev.eval_source(source).unwrap();
    let kit = ev.registry().get_kit("drums").unwrap();
    assert_eq!(kit.instruments[0].channel, 10);
}

/// kit の device 変数展開
/// Kit device variable expansion
#[test]
fn var_expansion_kit_device() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
var dev = td3

device td3 {
  port IAC
}

kit drums {
  device dev
  bd { channel 10, note c2 }
}
"#;
    ev.eval_source(source).unwrap();
    let kit = ev.registry().get_kit("drums").unwrap();
    assert_eq!(kit.device, "td3");
}

/// include 経由の変数展開
/// Variable expansion via include
#[test]
fn var_expansion_via_include() {
    let dir = tempfile::tempdir().unwrap();

    // 共通変数定義ファイル
    // Shared variable definition file
    let vars_file = dir.path().join("vars.cvg");
    std::fs::write(&vars_file, "var ch = 5\nvar dev = synth\n").unwrap();

    let main_file = dir.path().join("main.cvg");
    std::fs::write(
        &main_file,
        format!(
            "include {}\n\ndevice synth {{\n  port IAC\n}}\n\ninstrument bass {{\n  device dev\n  channel ch\n}}\n",
            vars_file.display()
        ),
    )
    .unwrap();

    let mut ev = Evaluator::new(120.0);
    ev.eval_file(&main_file).unwrap();
    let inst = ev.registry().get_instrument("bass").unwrap();
    assert_eq!(inst.device, "synth");
    assert_eq!(inst.channel, 5);
}
