//! 統合テスト: DSLソース → パース → 評価 → MIDI出力のE2Eフロー

use lcvgc_core::ast::clip::ClipBody;
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
  port Mutant Brain
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
  port Virtual MIDI
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
  port Virtual MIDI
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
  port test
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
  port Drums
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
    writeln!(tmpfile, "device test {{").unwrap();
    writeln!(tmpfile, "  port test").unwrap();
    writeln!(tmpfile, "}}").unwrap();

    let mut ev = Evaluator::new(120.0);
    let results = ev.load_file(tmpfile.path().to_str().unwrap()).unwrap();
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], EvalResult::TempoChanged(130.0)));
}

/// ピッチドクリップの繰り返し ()*N がコンパイルされ正しいノート数を生成するE2Eテスト
///
/// E2E test: pitched clip repetition ()*N compiles to correct note count
#[test]
fn e2e_pitched_repetition() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port Virtual MIDI
}

instrument bass {
  device synth
  channel 1
}

clip rep_test [bars 2] {
  bass (c:3:8 c eb)*4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("rep_test").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry()).unwrap();

    // 3 notes * 4 reps = 12 NoteOn events
    let note_on_count = compiled
        .events
        .iter()
        .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
        .count();
    assert_eq!(note_on_count, 12);
}

/// ドラムクリップの繰り返し ()*N がパース・コンパイルされ正しいヒット数を生成するE2Eテスト
///
/// E2E test: drum clip repetition ()*N parses and compiles to correct hit count
#[test]
fn e2e_drum_repetition() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device drums_dev {
  port Drums
}

kit tr808 {
  device drums_dev
  bd { channel 10, note c2 }
  hh { channel 10, note f#2 }
}

clip drum_rep [bars 1] {
  use tr808
  resolution 16
  hh (x.x.)*4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("drum_rep").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry()).unwrap();

    // (x.x.)*4 → x.x.x.x.x.x.x.x. = 8 Normal hits
    let note_on_count = compiled
        .events
        .iter()
        .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
        .count();
    assert_eq!(note_on_count, 8);
}

/// ピッチドクリップの繰り返しでオクターブ・音長が引き継がれるE2Eテスト
///
/// E2E test: pitched repetition carries octave and duration across iterations
#[test]
fn e2e_pitched_repetition_state_carry() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port Virtual MIDI
}

instrument bass {
  device synth
  channel 1
}

clip carry_test [bars 2] {
  bass (c:3:8)*2
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("carry_test").unwrap();
    let compiled = compile_clip(clip, ev.clock(), ev.registry()).unwrap();

    // 2 NoteOn events, both C3 = note 48
    let note_ons: Vec<_> = compiled
        .events
        .iter()
        .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
        .collect();
    assert_eq!(note_ons.len(), 2);
    for ev in &note_ons {
        assert!(matches!(ev.message, MidiMessage::NoteOn { note: 48, .. }));
    }
    // 2nd note at tick 240 (8th note at 120bpm)
    assert_eq!(note_ons[1].tick, 240);
}

/// ドラム確率行の `|` ショートカットがパースされ正しい確率ベクタを生成するE2Eテスト
///
/// E2E test: drum probability row with `|` shorthand parses into correct probability vector
#[test]
fn e2e_drum_probability_with_pipe() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port test
}

kit tr808 {
  device synth
  bd { channel 10, note c2 }
  snare { channel 10, note d2 }
  hh { channel 10, note f#2 }
}

clip beat [bars 1] {
  use tr808
  resolution 16
  bd    x|x|x|x|
        .5|.7|.3|.5|
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("beat").unwrap();
    match &clip.body {
        ClipBody::Drum(body) => {
            assert_eq!(body.rows.len(), 1);
            let row = &body.rows[0];
            assert_eq!(row.instrument, "bd");
            // ヒット行: x|x|x|x| → x...x...x...x... (16ステップ)
            // Hit row: x|x|x|x| → x...x...x...x... (16 steps)
            assert_eq!(row.hits.len(), 16);
            // 確率行: .5|.7|.3|.5| → .5...7...3...5.. (16ステップ)
            // Probability row: .5|.7|.3|.5| → .5...7...3...5.. (16 steps)
            let prob = row
                .probability
                .as_ref()
                .expect("probability should be Some");
            assert_eq!(prob.len(), 16);
            // 各拍の値を検証 / Verify values per beat
            // .5.. → [100, 50, 100, 100]
            assert_eq!(prob[0], 100);
            assert_eq!(prob[1], 50);
            assert_eq!(prob[2], 100);
            assert_eq!(prob[3], 100);
            // .7.. → [100, 70, 100, 100]
            assert_eq!(prob[4], 100);
            assert_eq!(prob[5], 70);
            assert_eq!(prob[6], 100);
            assert_eq!(prob[7], 100);
            // .3.. → [100, 30, 100, 100]
            assert_eq!(prob[8], 100);
            assert_eq!(prob[9], 30);
            assert_eq!(prob[10], 100);
            assert_eq!(prob[11], 100);
            // .5.. → [100, 50, 100, 100]
            assert_eq!(prob[12], 100);
            assert_eq!(prob[13], 50);
            assert_eq!(prob[14], 100);
            assert_eq!(prob[15], 100);
        }
        _ => panic!("expected Drum clip body"),
    }
}

/// ドラム確率行の `()*N` 繰り返しがパースされ正しい確率ベクタを生成するE2Eテスト
///
/// E2E test: drum probability row with `()*N` repetition parses into correct probability vector
#[test]
fn e2e_drum_probability_with_repetition() {
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port test
}

kit tr808 {
  device synth
  bd { channel 10, note c2 }
  hh { channel 10, note f#2 }
}

clip beat2 [bars 1] {
  use tr808
  resolution 16
  hh    (x.o.)*4
        (..5.)*4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("beat2").unwrap();
    match &clip.body {
        ClipBody::Drum(body) => {
            assert_eq!(body.rows.len(), 1);
            let row = &body.rows[0];
            assert_eq!(row.instrument, "hh");
            // ヒット行: (x.o.)*4 → x.o.x.o.x.o.x.o. (16ステップ)
            // Hit row: (x.o.)*4 → x.o.x.o.x.o.x.o. (16 steps)
            assert_eq!(row.hits.len(), 16);
            // 確率行: (..5.)*4 → ..5...5...5...5. (16ステップ)
            // Probability row: (..5.)*4 → ..5...5...5...5. (16 steps)
            let prob = row
                .probability
                .as_ref()
                .expect("probability should be Some");
            assert_eq!(prob.len(), 16);
            // 各繰り返しの値を検証 / Verify values per repetition
            // ..5. → [100, 100, 50, 100]
            for i in 0..4 {
                let base = i * 4;
                assert_eq!(prob[base], 100, "step {} should be 100", base);
                assert_eq!(prob[base + 1], 100, "step {} should be 100", base + 1);
                assert_eq!(prob[base + 2], 50, "step {} should be 50", base + 2);
                assert_eq!(prob[base + 3], 100, "step {} should be 100", base + 3);
            }
        }
        _ => panic!("expected Drum clip body"),
    }
}
