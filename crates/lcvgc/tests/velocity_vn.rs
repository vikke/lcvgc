//! メロディノートの `vN` velocity 指定構文に関する統合テスト。
//!
//! DSL ソースをパース → コンパイル → MIDI イベント列にし、NoteOn の velocity が
//! 正しく反映されることを検証する。
//!
//! Integration tests for the per-note velocity suffix `vN` on pitched notes.
//! Each test parses a DSL source, compiles it to MIDI events, and asserts the
//! velocity values of the resulting NoteOn messages.

use lcvgc::engine::compiler::compile_clip;
use lcvgc::engine::evaluator::Evaluator;
use lcvgc::midi::message::MidiMessage;

/// 指定 DSL を eval し、`clip_name` をコンパイルした結果の NoteOn velocity 列を返す。
/// 順序は events のもの (tick 昇順 → 同 tick 内は NoteOn 優先)。
///
/// Evaluate the given source, compile the clip named `clip_name`, and return
/// the NoteOn velocities in event order.
fn note_on_velocities(source: &str, clip_name: &str) -> Vec<u8> {
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).expect("eval_source should succeed");
    let clip = ev.registry().get_clip(clip_name).expect("clip exists");
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).expect("compile");
    compiled
        .events
        .iter()
        .filter_map(|e| match e.message {
            MidiMessage::NoteOn { velocity, .. } => Some(velocity),
            _ => None,
        })
        .collect()
}

/// `vN` 未指定なら NoteOn の velocity は既定値 100 になる。
#[test]
fn velocity_default_is_100() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4 d e f
}
"#;
    assert_eq!(note_on_velocities(source, "riff"), vec![100, 100, 100, 100]);
}

/// 単音に `vN` を付けるとそのノートだけ velocity が上書きされる。
#[test]
fn velocity_per_note_override() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4v127 d:4 e:4v40 f:4
}
"#;
    assert_eq!(note_on_velocities(source, "riff"), vec![127, 100, 40, 100]);
}

/// `vN` は順不同で `'` / `gN` / `.` と組み合わせ可能。
/// 注: 付点 `.` は音価直後の位置に書く既存仕様 (c:3:4.) のため、
/// このテストでは `vN` を `.` より後ろに置くケースのみ検証する。
#[test]
fn velocity_combined_with_other_suffixes() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4.v90 d:4'v110 e:4g95v100 f:4v50
}
"#;
    assert_eq!(note_on_velocities(source, "riff"), vec![90, 110, 100, 50]);
}

/// `'` と `vN` の順不同性: `v100'` の順でも正しく取れること。
#[test]
fn velocity_order_independent_with_staccato() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4v100' d:4'v100
}
"#;
    // 両方とも staccato + velocity 100 → 同じ velocity 列
    assert_eq!(note_on_velocities(source, "riff"), vec![100, 100]);
}

/// `gN` と `vN` の順不同性: `v110g95` の順でも正しく取れること。
#[test]
fn velocity_order_independent_with_gate() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4v110g95 d:4g95v110
}
"#;
    assert_eq!(note_on_velocities(source, "riff"), vec![110, 110]);
}

/// 0 と 127 の境界値が受理されること。
#[test]
fn velocity_boundary_values() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4v0 d:4v127
}
"#;
    assert_eq!(note_on_velocities(source, "riff"), vec![0, 127]);
}

/// `vN` の重複指定 (`v100v90`) はパースエラーで eval が失敗する。
#[test]
fn velocity_duplicate_is_parse_error() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4v100v90
}
"#;
    let mut ev = Evaluator::new(120.0);
    let r = ev.eval_source(source);
    assert!(r.is_err(), "duplicate vN must fail to parse, got {:?}", r);
}

/// `'` と `gN` の両立 (`'g95`) はパースエラーで eval が失敗する。
/// (vN 追加とは独立した不変条件だが、回帰防止のためここでも検証する)
#[test]
fn staccato_and_gate_conflict_is_parse_error() {
    let source = r#"
device d { port virtual }
instrument bass { device d channel 1 }

clip riff [bars 1] {
  bass c:3:4'g95
}
"#;
    let mut ev = Evaluator::new(120.0);
    let r = ev.eval_source(source);
    assert!(r.is_err(), "'+gN conflict must fail to parse, got {:?}", r);
}

/// ChordBracket にも `vN` が反映され、全構成音の NoteOn に同じ velocity が適用される。
#[test]
fn velocity_on_chord_bracket() {
    let source = r#"
device d { port virtual }
instrument pad { device d channel 1 }

clip chord [bars 1] {
  pad [c:4 e g]:4v77
}
"#;
    let vels = note_on_velocities(source, "chord");
    assert_eq!(vels, vec![77, 77, 77], "chord NoteOn velocities differ");
}
