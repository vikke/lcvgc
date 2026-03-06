//! 統合テスト: DSLソース → パース → 評価 → MIDI出力のE2Eフロー

use lcvgc_core::engine::compiler::compile_clip;
use lcvgc_core::engine::evaluator::{EvalResult, Evaluator};
use lcvgc_core::engine::midi_sink::{MidiSink, MockSink};
use lcvgc_core::midi::message::MidiMessage;

/// DSLソースを評価して結果を返す
fn eval(source: &str) -> Vec<EvalResult> {
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).expect("eval_source should succeed")
}

#[test]
fn e2e_tempo_and_device_registration() {
    let source = r#"
tempo 140

device mb {
  port "Mutant Brain"
}

instrument bass {
  device mb
  channel 1
}
"#;
    let results = eval(source);
    assert_eq!(results.len(), 3);
    assert!(matches!(results[0], EvalResult::TempoChanged(140.0)));
    assert!(
        matches!(&results[1], EvalResult::Registered { kind, name } if kind == "Device" && name == "mb")
    );
    assert!(
        matches!(&results[2], EvalResult::Registered { kind, name } if kind == "Instrument" && name == "bass")
    );
}

#[test]
fn e2e_clip_registration_and_compile() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port "Virtual MIDI"
}

instrument lead {
  device synth
  channel 1
}

clip melody [bars 1] {
  lead c:4:4 d e f
}
"#;
    let results = ev.eval_source(source).unwrap();
    assert_eq!(results.len(), 3);

    let clip = ev.registry().get_clip("melody").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry());
    assert!(compiled.is_ok());
    let compiled = compiled.unwrap();
    assert!(!compiled.events.is_empty());
}

#[test]
fn e2e_compiled_clip_produces_midi_messages() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port "Virtual MIDI"
}

instrument piano {
  device synth
  channel 1
}

clip riff [bars 1] {
  piano c:3:4 e g
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("riff").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry()).unwrap();

    // MockSinkにMIDIメッセージを送信
    let mut sink = MockSink::default();
    for event in &compiled.events {
        sink.send(&event.message).unwrap();
    }

    // NoteOnが3つ存在するはず（C3, E3, G3）
    let note_ons: Vec<_> = sink
        .sent
        .iter()
        .filter(|m| matches!(m, MidiMessage::NoteOn { .. }))
        .collect();
    assert_eq!(note_ons.len(), 3);
}

#[test]
fn e2e_scale_and_var_definition() {
    let source = r#"
scale c major
var key = cm
"#;
    let results = eval(source);
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], EvalResult::ScaleChanged));
    assert!(matches!(&results[1], EvalResult::VarDefined { name } if name == "key"));
}

#[test]
fn e2e_scene_and_session() {
    let source = r#"
device d {
  port "test"
}

instrument i {
  device d
  channel 1
}

clip c1 [bars 1] {
  i c:4:4
}

scene verse {
  c1
}

session song {
  verse [repeat 2]
}
"#;
    let results = eval(source);
    assert_eq!(results.len(), 5);
    assert!(matches!(&results[3], EvalResult::Registered { kind, .. } if kind == "Scene"));
    assert!(matches!(&results[4], EvalResult::Registered { kind, .. } if kind == "Session"));
}

#[test]
fn e2e_play_and_stop() {
    let source = r#"
scene test_scene {}

play test_scene
stop
"#;
    let results = eval(source);
    assert_eq!(results.len(), 3);
    assert!(matches!(results[1], EvalResult::PlayStarted));
    assert!(matches!(results[2], EvalResult::Stopped));
}

#[test]
fn e2e_parse_error_returns_err() {
    let mut ev = Evaluator::new(120.0);
    let result = ev.eval_source("invalid !@# syntax {{{}}}");
    assert!(result.is_err());
}

#[test]
fn e2e_tempo_relative() {
    let source = r#"
tempo 120
tempo +20
"#;
    let results = eval(source);
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], EvalResult::TempoChanged(120.0)));
    assert!(matches!(results[1], EvalResult::TempoChanged(140.0)));
}

#[test]
fn e2e_drum_clip() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device drums_dev {
  port "Drums"
}

kit tr808 {
  device drums_dev
  bd    { channel 10, note c2 }
  snare { channel 10, note d2 }
  hh    { channel 10, note f#2 }
}

clip beat [bars 1] {
  use tr808
  resolution 16
  bd    x...x...x...x...
  snare ....x.......x...
  hh    x.x.x.x.x.x.x.x
}
"#;
    let results = ev.eval_source(source).unwrap();
    assert_eq!(results.len(), 3);

    let clip = ev.registry().get_clip("beat").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry());
    assert!(compiled.is_ok());
}

#[test]
fn e2e_file_load() {
    use std::io::Write;
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    writeln!(tmpfile, "tempo 130").unwrap();
    writeln!(tmpfile).unwrap();
    writeln!(tmpfile, r#"device test {{ port "test" }}"#).unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.load_file(tmpfile.path().to_str().unwrap()).unwrap();
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], EvalResult::TempoChanged(130.0)));
}
