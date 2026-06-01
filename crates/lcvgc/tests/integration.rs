//! 統合テスト: DSLソース → パース → 評価 → MIDI出力のE2Eフロー

use lcvgc::ast::clip::ClipBody;
use lcvgc::engine::compiler::compile_clip;
use lcvgc::engine::evaluator::{EvalResult, Evaluator};
use lcvgc::engine::midi_sink::{MidiSink, MockSink};
use lcvgc::midi::message::MidiMessage;

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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry());
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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

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
fn e2e_clip_octave_shift_up_raises_notes() {
    // `clip foo [>>] { ... }` で clip 全体が +1 オクターブされることを確認する。
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port Virtual MIDI
}

instrument lead {
  device synth
  channel 1
}

clip up [>>] {
  lead c:4:4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("up").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // C4(60) -> C5(72)
    let note_on = compiled
        .events
        .iter()
        .find_map(|e| match e.message {
            MidiMessage::NoteOn { note, .. } => Some(note),
            _ => None,
        })
        .expect("NoteOn should exist");
    assert_eq!(note_on, 72);
}

#[test]
fn e2e_clip_octave_shift_two_up_with_other_options() {
    // `[bars 1] [>> >>]` の併記で +2 オクターブされることを確認する。
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port Virtual MIDI
}

instrument lead {
  device synth
  channel 1
}

clip up2 [bars 1] [>> >>] {
  lead c:4:4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("up2").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // C4(60) -> C6(84)
    let note_on = compiled
        .events
        .iter()
        .find_map(|e| match e.message {
            MidiMessage::NoteOn { note, .. } => Some(note),
            _ => None,
        })
        .expect("NoteOn should exist");
    assert_eq!(note_on, 84);
}

#[test]
fn e2e_clip_octave_shift_down_lowers_notes() {
    // `[<<]` で clip 全体が -1 オクターブされることを確認する。
    let mut ev = Evaluator::new(120.0);
    let source = r#"
device synth {
  port Virtual MIDI
}

instrument lead {
  device synth
  channel 1
}

clip down [<<] {
  lead c:4:4
}
"#;
    ev.eval_source(source).unwrap();

    let clip = ev.registry().get_clip("down").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // C4(60) -> C3(48)
    let note_on = compiled
        .events
        .iter()
        .find_map(|e| match e.message {
            MidiMessage::NoteOn { note, .. } => Some(note),
            _ => None,
        })
        .expect("NoteOn should exist");
    assert_eq!(note_on, 48);
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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry());
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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

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
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

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

/// ドラムパターン内のスペースが無視されてパースされることを確認するE2Eテスト
///
/// E2E test: verify that spaces within drum patterns are ignored during parsing
#[test]
fn e2e_drum_pattern_with_spaces() {
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

clip beat_sp [bars 1] {
  use tr808
  resolution 16

  bd    x.  x.  x.  x.  x.  x.  x.  x.
  snare . . . . x . . . . . . . x . . .
  hh    x . o . x . o . x . o . x . o .
        . . 5 . . . 7 . . . 3 . . . 5 .
}
"#;
    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_source(source).unwrap();
    assert_eq!(results.len(), 3); // device, kit, clip

    let clip = ev.registry().get_clip("beat_sp").unwrap();
    if let ClipBody::Drum(body) = &clip.body {
        assert_eq!(body.rows.len(), 3);
        // bd: 16ヒット / 16 hits
        assert_eq!(body.rows[0].hits.len(), 16);
        assert_eq!(body.rows[0].instrument, "bd");
        // snare: 16ヒット / 16 hits
        assert_eq!(body.rows[1].hits.len(), 16);
        // hh: 16ヒット + 確率行付き / 16 hits with probability
        assert_eq!(body.rows[2].hits.len(), 16);
        assert!(body.rows[2].probability.is_some());
        let prob = body.rows[2].probability.as_ref().unwrap();
        assert_eq!(prob.len(), 16);
        assert_eq!(prob[2], 50); // '5' = 50%
        assert_eq!(prob[6], 70); // '7' = 70%
        assert_eq!(prob[10], 30); // '3' = 30%
    } else {
        panic!("Expected drum clip");
    }

    // コンパイルも成功することを確認
    // Verify compilation also succeeds
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry());
    assert!(compiled.is_ok());
}

// ============================================================
// 同 instrument 連続行の連結 / `---` 並列レイヤー分離の E2E テスト
// 設計B: PitchedLine.is_layer_start で layer 境界を表現する。
// 同 instrument の連続行は前行の carry-over を継承して時系列連結。
// `---` または別 instrument は新 layer (tick 0 起点)。
// ------------------------------------------------------------
// E2E tests for line merging within the same instrument and
// `---` parallel layer separation.
// ============================================================

/// 同 instrument の連続行が時系列に連結されることを検証する。
/// 4 行に分けた書き方と 1 行に並べた書き方で MIDI イベント (tick / note / order) が
/// 完全に一致するはず。
///
/// Verify that consecutive lines for the same instrument are merged into one
/// timeline. The 4-line and 1-line forms must produce identical MIDI events
/// (tick, note, ordering).
#[test]
fn e2e_consecutive_same_instrument_lines_are_merged() {
    let split = r#"
device d { port test }
instrument chord { device d channel 1 }

clip c_split [bars 4] {
  chord dm:4:1 bb:3:1
  chord c:4:1 dm:4:1
}
"#;
    let merged = r#"
device d { port test }
instrument chord { device d channel 1 }

clip c_merged [bars 4] {
  chord dm:4:1 bb:3:1 c:4:1 dm:4:1
}
"#;

    let collect_note_ons = |source: &str, clip_name: &str| -> Vec<(u64, u8)> {
        let mut ev = Evaluator::new(120.0);
        ev.eval_source(source).unwrap();
        let clip = ev.registry().get_clip(clip_name).unwrap();
        let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();
        compiled
            .events
            .iter()
            .filter_map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => Some((e.tick, note)),
                _ => None,
            })
            .collect()
    };

    let split_notes = collect_note_ons(split, "c_split");
    let merged_notes = collect_note_ons(merged, "c_merged");

    assert_eq!(
        split_notes, merged_notes,
        "split form must produce same NoteOn(tick, note) sequence as merged form"
    );
    // 念のため、 想定される NoteOn 数を確認:
    //   dm:4:1   (3 ノート: D F A) +
    //   bb:3:1   (1 ノート: 単音 Bb3) +
    //   c:4:1    (1 ノート: 単音 C4) +
    //   dm:4:1   (3 ノート) = 8 NoteOn
    // (Bb / C はサフィックス無しの音名表記なので単音、 dm はサフィックス
    // `m` でマイナーコード)
    assert_eq!(split_notes.len(), 8);
}

/// `---` を挟むと前後の行が並列レイヤーとして扱われ、両方が tick 0 から発音される
/// ことを検証する。
///
/// Verify that `---` makes the surrounding lines into parallel layers, both
/// starting from tick 0.
#[test]
fn e2e_dash_divider_creates_parallel_layer() {
    let source = r#"
device d { port test }
instrument chord { device d channel 1 }

clip layered [bars 1] {
  chord c:4:1
  ---
  chord g:4:1
}
"#;
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).unwrap();
    let clip = ev.registry().get_clip("layered").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // tick 0 で C4 と G4 の NoteOn が両方鳴っているはず
    // Both C4 (60) and G4 (67) NoteOn events must be at tick 0.
    let note_ons_at_zero: Vec<u8> = compiled
        .events
        .iter()
        .filter_map(|e| match (e.tick, e.message) {
            (0, MidiMessage::NoteOn { note, .. }) => Some(note),
            _ => None,
        })
        .collect();

    assert!(
        note_ons_at_zero.contains(&60),
        "C4 NoteOn at tick 0 expected, got {note_ons_at_zero:?}"
    );
    assert!(
        note_ons_at_zero.contains(&67),
        "G4 NoteOn at tick 0 expected, got {note_ons_at_zero:?}"
    );
}

/// 連続行で octave / duration の carry-over が継続されることを検証する。
/// 2 行目の `bb` は前行の `o3, :1` を継承して Bb3 全音符として扱われるべき。
///
/// Verify carry-over (octave / duration / dotted) is preserved across
/// consecutive same-instrument lines.
#[test]
fn e2e_carry_over_continues_across_consecutive_lines() {
    let source = r#"
device d { port test }
instrument lead { device d channel 1 }

clip carry [bars 2] {
  lead c:3:1
  lead bb
}
"#;
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).unwrap();
    let clip = ev.registry().get_clip("carry").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // NoteOn を tick 順に取り出す
    let mut note_ons: Vec<(u64, u8)> = compiled
        .events
        .iter()
        .filter_map(|e| match e.message {
            MidiMessage::NoteOn { note, .. } => Some((e.tick, note)),
            _ => None,
        })
        .collect();
    note_ons.sort();

    assert_eq!(note_ons.len(), 2);
    // C3 = 48 at tick 0
    assert_eq!(note_ons[0], (0, 48));
    // Bb3 = 58 at tick = 1 whole note (1920 ticks at PPQ 480)
    // 全音符 1 個分 (= 4 拍 × 480) 後に Bb3 が鳴る
    assert_eq!(note_ons[1].1, 58, "carry-over should resolve to Bb3 (58)");
    assert!(
        note_ons[1].0 > 0,
        "second NoteOn must come AFTER the first (merged timeline), got tick={}",
        note_ons[1].0
    );
}

/// `---` で分離された後の layer は carry-over がリセットされることを検証する。
///
/// Verify carry-over is reset after a `---` divider.
#[test]
fn e2e_carry_over_resets_after_dash_divider() {
    let source = r#"
device d { port test }
instrument lead { device d channel 1 }

clip reset [bars 2] {
  lead c:3:1
  ---
  lead bb
}
"#;
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).unwrap();
    let clip = ev.registry().get_clip("reset").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    // 2 番目の `bb` は新 layer のため、 carry-over はデフォルト (o4, :4) に戻る
    // → Bb4 = 70 が tick 0 から発音される
    // The second `bb` is a new layer, so carry-over resets to default (o4, :4)
    // → Bb4 (70) starts at tick 0.
    let note_ons_at_zero: Vec<u8> = compiled
        .events
        .iter()
        .filter_map(|e| match (e.tick, e.message) {
            (0, MidiMessage::NoteOn { note, .. }) => Some(note),
            _ => None,
        })
        .collect();

    assert!(
        note_ons_at_zero.contains(&48),
        "C3 NoteOn at tick 0 expected, got {note_ons_at_zero:?}"
    );
    assert!(
        note_ons_at_zero.contains(&70),
        "Bb4 NoteOn at tick 0 expected (carry-over reset), got {note_ons_at_zero:?}"
    );
}

/// 別 instrument が現れた場合は従来どおり tick 0 起点の並列レイヤーになる
/// ことを検証する。 carry-over も独立。
///
/// Verify that a different instrument starts a new parallel layer (tick 0)
/// with independent carry-over (existing behavior preserved).
#[test]
fn e2e_different_instrument_remains_parallel_layer() {
    let source = r#"
device d { port test }
instrument lead { device d channel 1 }
instrument bass { device d channel 2 }

clip mix [bars 1] {
  lead c:5:4
  bass c:3:4
}
"#;
    let mut ev = Evaluator::new(120.0);
    ev.eval_source(source).unwrap();
    let clip = ev.registry().get_clip("mix").unwrap();
    let compiled = compile_clip(clip, &ev.clock_snapshot(), ev.registry()).unwrap();

    let note_ons_at_zero: Vec<u8> = compiled
        .events
        .iter()
        .filter_map(|e| match (e.tick, e.message) {
            (0, MidiMessage::NoteOn { note, .. }) => Some(note),
            _ => None,
        })
        .collect();

    assert!(note_ons_at_zero.contains(&72), "C5 (lead) at tick 0");
    assert!(note_ons_at_zero.contains(&48), "C3 (bass) at tick 0");
}

/// ドラム clip に `---` が現れても no-op として許容されることを検証する。
/// 現状はピッチド clip 専用機能だが、将来の拡張のため drum body は
/// `---` をエラーにせず単に消費する。
///
/// Verify `---` is accepted as a no-op inside drum clips (reserved for
/// future use).
#[test]
fn e2e_dash_divider_in_drum_clip_is_noop() {
    let source = r#"
device drums_dev { port Drums }

kit tr808 {
  device drums_dev
  bd    { channel 10, note c2 }
  snare { channel 10, note d2 }
}

clip beat_layered [bars 1] {
  use tr808
  resolution 16
  bd    x...x...x...x...
  ---
  snare ....x.......x...
}
"#;
    let mut ev = Evaluator::new(120.0);
    let results = ev.eval_source(source);
    assert!(
        results.is_ok(),
        "drum clip with `---` should parse successfully, got {results:?}"
    );
    let clip = ev.registry().get_clip("beat_layered").unwrap();
    if let ClipBody::Drum(body) = &clip.body {
        // bd / snare の 2 行が認識されている
        assert_eq!(body.rows.len(), 2);
    } else {
        panic!("expected drum clip");
    }
}
