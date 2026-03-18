use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody, PitchedElement, PitchedLine};
use crate::ast::clip_drum::HitSymbol;
use crate::ast::clip_note::NoteEvent;
use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::registry::Registry;
use crate::midi::chord::chord_notes;
use crate::midi::message::MidiMessage;
use crate::midi::note::note_number;
use crate::parser::clip_articulation::Articulation;

/// tickベースMIDIイベント
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiEvent {
    pub tick: u64,
    pub message: MidiMessage,
}

/// コンパイル済みクリップ
/// Compiled clip containing MIDI events and metadata
#[derive(Debug, Clone)]
pub struct CompiledClip {
    /// tick順にソート済みイベントリスト
    pub events: Vec<MidiEvent>,
    /// クリップの全体長（tick単位）
    pub total_ticks: u64,
    /// コンパイル時の警告メッセージ（bars超過など）
    /// Warning messages generated during compilation (e.g., bars overflow)
    pub warnings: Vec<String>,
}

/// クリップ定義をtickベースMIDIイベント列にコンパイルする
pub fn compile_clip(
    clip: &ClipDef,
    clock: &Clock,
    registry: &Registry,
) -> Result<CompiledClip, EngineError> {
    let mut events = match &clip.body {
        ClipBody::Pitched(body) => compile_pitched(body, clock, registry, clip.options.bars)?,
        ClipBody::Drum(body) => compile_drum(body, clock, registry)?,
    };

    let mut warnings = Vec::new();

    // bars制約の適用
    let total_ticks = if let Some(bars) = clip.options.bars {
        let bar_ticks = clock.ticks_per_bar();
        let max_ticks = bar_ticks * bars as u64;
        // bars超過検出: 超過イベントがあればワーニング生成
        // Detect bars overflow: generate warning if events exceed bar limit
        let overflow_count = events.iter().filter(|e| e.tick >= max_ticks).count();
        if overflow_count > 0 {
            warnings.push(format!(
                "clip '{}': bars={} を超過するイベントが {}個あり、切り捨てられました",
                clip.name, bars, overflow_count
            ));
        }
        // 超過イベントを切り捨て
        events.retain(|e| e.tick < max_ticks);
        max_ticks
    } else {
        // bars未指定: イベントの最大tick + 1（最低でも0）
        events.iter().map(|e| e.tick + 1).max().unwrap_or(0)
    };

    // tick順にソート（同一tickではNoteOnをNoteOffより先に）
    events.sort_by(|a, b| {
        a.tick.cmp(&b.tick).then_with(|| {
            let a_priority = event_sort_priority(&a.message);
            let b_priority = event_sort_priority(&b.message);
            a_priority.cmp(&b_priority)
        })
    });

    Ok(CompiledClip {
        events,
        total_ticks,
        warnings,
    })
}

/// ソート優先度: NoteOn(0) < CC(1) < NoteOff(2)
fn event_sort_priority(msg: &MidiMessage) -> u8 {
    match msg {
        MidiMessage::NoteOn { .. } => 0,
        MidiMessage::ControlChange { .. } => 1,
        MidiMessage::NoteOff { .. } => 2,
        MidiMessage::ProgramChange { .. } => 1,
    }
}

/// ピッチドクリップのコンパイル
fn compile_pitched(
    body: &PitchedClipBody,
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<Vec<MidiEvent>, EngineError> {
    let mut events = Vec::new();
    for line in &body.lines {
        let line_events = compile_pitched_line(line, clock, registry, bars)?;
        events.extend(line_events);
    }
    // TODO: CC automationのコンパイル
    Ok(events)
}

/// ピッチドライン1行のコンパイル
fn compile_pitched_line(
    line: &PitchedLine,
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<Vec<MidiEvent>, EngineError> {
    let inst = registry
        .get_instrument(&line.instrument)
        .ok_or_else(|| EngineError::UnknownInstrument(line.instrument.clone()))?;

    let channel = inst.channel;
    let gate_normal = inst.gate_normal.unwrap_or(80);
    let gate_staccato = inst.gate_staccato.unwrap_or(40);

    let mut events = Vec::new();
    let mut current_tick: u64 = 0;
    let mut current_octave: u8 = 4;
    let mut current_duration: u16 = 4;

    compile_elements(
        &line.elements,
        clock,
        channel,
        gate_normal,
        gate_staccato,
        &mut current_tick,
        &mut current_octave,
        &mut current_duration,
        &mut events,
        bars,
    )?;

    Ok(events)
}

/// ピッチド要素列をMIDIイベントにコンパイルする（再帰対応）。
/// Repetition の展開時に再帰呼び出しされる。
///
/// Compile a slice of pitched elements into MIDI events (supports recursion for Repetition).
#[allow(clippy::too_many_arguments)]
fn compile_elements(
    elements: &[PitchedElement],
    clock: &Clock,
    channel: u8,
    gate_normal: u8,
    gate_staccato: u8,
    current_tick: &mut u64,
    current_octave: &mut u8,
    current_duration: &mut u16,
    events: &mut Vec<MidiEvent>,
    bars: Option<u32>,
) -> Result<(), EngineError> {
    for element in elements {
        match element {
            PitchedElement::Note(note_event, articulation) => match note_event {
                NoteEvent::Single {
                    name,
                    octave,
                    duration,
                    dotted,
                } => {
                    let oct = octave.unwrap_or(*current_octave);
                    let dur = duration.unwrap_or(*current_duration);
                    *current_octave = oct;
                    *current_duration = dur;

                    let note = note_number(*name, oct);
                    let note_ticks = clock.duration_to_ticks(dur, *dotted);
                    let gate_percent =
                        resolve_gate_percent(articulation, gate_normal, gate_staccato);
                    let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);

                    events.push(MidiEvent {
                        tick: *current_tick,
                        message: MidiMessage::NoteOn {
                            channel,
                            note,
                            velocity: 100,
                        },
                    });
                    events.push(MidiEvent {
                        tick: *current_tick + gate_ticks,
                        message: MidiMessage::NoteOff {
                            channel,
                            note,
                            velocity: 0,
                        },
                    });

                    *current_tick += note_ticks;
                }
                NoteEvent::Rest { duration, dotted } => {
                    let dur = duration.unwrap_or(*current_duration);
                    *current_duration = dur;
                    let note_ticks = clock.duration_to_ticks(dur, *dotted);
                    *current_tick += note_ticks;
                }
                NoteEvent::ChordName {
                    root,
                    suffix,
                    octave,
                    duration,
                    dotted,
                } => {
                    // コード名→MIDIノート群に展開してNoteOn/NoteOffを生成
                    // Expand chord name to MIDI notes and generate NoteOn/NoteOff events
                    let oct = octave.unwrap_or(*current_octave);
                    let dur = duration.unwrap_or(*current_duration);
                    *current_octave = oct;
                    *current_duration = dur;

                    let notes = chord_notes(*root, oct, suffix);
                    let note_ticks = clock.duration_to_ticks(dur, *dotted);
                    let gate_percent =
                        resolve_gate_percent(articulation, gate_normal, gate_staccato);
                    let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);

                    for &note in &notes {
                        events.push(MidiEvent {
                            tick: *current_tick,
                            message: MidiMessage::NoteOn {
                                channel,
                                note,
                                velocity: 100,
                            },
                        });
                        events.push(MidiEvent {
                            tick: *current_tick + gate_ticks,
                            message: MidiMessage::NoteOff {
                                channel,
                                note,
                                velocity: 0,
                            },
                        });
                    }

                    *current_tick += note_ticks;
                }
            },
            PitchedElement::ChordBracket {
                notes,
                duration,
                dotted,
                articulation,
                arpeggio: _, // TODO: アルペジオ対応
            } => {
                // 和音ブラケット→同時発音のNoteOn/NoteOff生成
                // Chord bracket → generate simultaneous NoteOn/NoteOff events
                let dur = duration.unwrap_or(*current_duration);
                *current_duration = dur;

                let note_ticks = clock.duration_to_ticks(dur, *dotted);
                let gate_percent = resolve_gate_percent(articulation, gate_normal, gate_staccato);
                let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);

                for &(name, oct_opt) in notes {
                    let oct = oct_opt.unwrap_or(*current_octave);
                    let note = note_number(name, oct);
                    events.push(MidiEvent {
                        tick: *current_tick,
                        message: MidiMessage::NoteOn {
                            channel,
                            note,
                            velocity: 100,
                        },
                    });
                    events.push(MidiEvent {
                        tick: *current_tick + gate_ticks,
                        message: MidiMessage::NoteOff {
                            channel,
                            note,
                            velocity: 0,
                        },
                    });
                }

                *current_tick += note_ticks;
            }
            PitchedElement::Repetition(rep) => {
                let inner_elements = crate::parser::clip::parse_repetition_content(&rep.content)
                    .map_err(EngineError::CompileError)?;
                for _ in 0..rep.count {
                    compile_elements(
                        &inner_elements,
                        clock,
                        channel,
                        gate_normal,
                        gate_staccato,
                        current_tick,
                        current_octave,
                        current_duration,
                        events,
                        bars,
                    )?;
                }
            }
            PitchedElement::BarJump(jump) => {
                // bars制約がある場合、bar_numberが範囲外ならエラー
                // If bars constraint exists, validate bar_number is within range
                if let Some(max_bars) = bars {
                    if jump.bar_number > max_bars {
                        return Err(EngineError::CompileError(format!(
                            ">{}はbars={}の範囲外です",
                            jump.bar_number, max_bars
                        )));
                    }
                }
                let bar_ticks = clock.ticks_per_bar();
                *current_tick = (jump.bar_number as u64 - 1) * bar_ticks;
            }
        }
    }

    Ok(())
}

/// アーティキュレーションからゲート比率を解決
fn resolve_gate_percent(art: &Articulation, gate_normal: u8, gate_staccato: u8) -> u8 {
    match art {
        Articulation::Normal => gate_normal,
        Articulation::Staccato => gate_staccato,
        Articulation::GateDirect(pct) => *pct,
    }
}

/// 最小Gate Off 5ms保証付きでgate_ticksを計算する（§7.7）
/// Calculate gate_ticks with minimum 5ms Gate Off guarantee (§7.7)
///
/// gate_percent=100 の場合はレガート（off=0）でそのまま返す。
/// それ以外の場合、off期間が5ms未満ならgate_ticksをクランプする。
fn apply_min_gate_off(note_ticks: u64, gate_percent: u8, clock: &Clock) -> u64 {
    if gate_percent == 100 {
        return note_ticks;
    }
    let gate_ticks = note_ticks * gate_percent as u64 / 100;
    let tick_us = clock.tick_duration_us();
    // 5ms = 5000us → 最小off ticks（切り上げ）
    let min_off_ticks = if tick_us > 0 {
        5000_u64.div_ceil(tick_us)
    } else {
        0
    };
    let max_gate = note_ticks.saturating_sub(min_off_ticks);
    gate_ticks.min(max_gate)
}

/// ドラムクリップのコンパイル
fn compile_drum(
    body: &crate::ast::clip::DrumClipBody,
    clock: &Clock,
    registry: &Registry,
) -> Result<Vec<MidiEvent>, EngineError> {
    let kit = registry
        .get_kit(&body.kit)
        .ok_or_else(|| EngineError::UnknownKit(body.kit.clone()))?;

    let ticks_per_step = clock.duration_to_ticks(body.resolution, false);

    let mut events = Vec::new();

    for row in &body.rows {
        let kit_inst = kit
            .instruments
            .iter()
            .find(|i| i.name == row.instrument)
            .ok_or_else(|| EngineError::UnknownInstrument(row.instrument.clone()))?;

        let channel = kit_inst.channel;
        let note = note_number(kit_inst.note.name, kit_inst.note.octave);
        let gate_percent = kit_inst.gate_normal.unwrap_or(80);

        for (i, hit) in row.hits.iter().enumerate() {
            if *hit == HitSymbol::Rest {
                continue;
            }

            let velocity = hit.velocity().unwrap_or(0);
            if velocity == 0 {
                continue;
            }

            let tick = i as u64 * ticks_per_step;
            let gate_ticks = apply_min_gate_off(ticks_per_step, gate_percent, clock);

            events.push(MidiEvent {
                tick,
                message: MidiMessage::NoteOn {
                    channel,
                    note,
                    velocity,
                },
            });
            events.push(MidiEvent {
                tick: tick + gate_ticks,
                message: MidiMessage::NoteOff {
                    channel,
                    note,
                    velocity: 0,
                },
            });
        }
    }

    // TODO: ドラムCC automationのコンパイル
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipDef, PitchedClipBody, PitchedLine};
    use crate::ast::clip_note::NoteEvent;
    use crate::ast::common::NoteName;
    use crate::ast::instrument::InstrumentDef;
    use crate::ast::kit::{KitDef, KitInstrument, KitInstrumentNote};
    use crate::parser::clip_options::ClipOptions;

    fn make_registry_with_bass() -> Registry {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "bass".to_string(),
            device: "dev".to_string(),
            channel: 1,
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));
        registry
    }

    fn make_pitched_clip(name: &str, bars: Option<u32>, lines: Vec<PitchedLine>) -> ClipDef {
        ClipDef {
            name: name.to_string(),
            options: ClipOptions {
                bars,
                time_sig: None,
                scale: None,
            },
            body: ClipBody::Pitched(PitchedClipBody {
                lines,
                cc_automations: vec![],
            }),
        }
    }

    fn single_note(
        name: NoteName,
        octave: Option<u8>,
        duration: Option<u16>,
        dotted: bool,
    ) -> PitchedElement {
        PitchedElement::Note(
            NoteEvent::Single {
                name,
                octave,
                duration,
                dotted,
            },
            Articulation::Normal,
        )
    }

    #[test]
    fn single_note_c4_quarter_at_120bpm() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.events.len(), 2);
        assert_eq!(compiled.events[0].tick, 0);
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn {
                channel: 1,
                note: 60,
                velocity: 100
            }
        ));
        // gate_normal=80%, 480ticks * 80% = 384ticks
        assert_eq!(compiled.events[1].tick, 384);
        assert!(matches!(
            compiled.events[1].message,
            MidiMessage::NoteOff {
                channel: 1,
                note: 60,
                ..
            }
        ));
    }

    #[test]
    fn two_notes_carry_forward() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    single_note(NoteName::Eb, None, None, false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.events.len(), 4);

        // 2nd note at tick 240 (8th = 240 ticks), Eb3 = 51
        let second_on = compiled
            .events
            .iter()
            .find(|e| e.tick == 240 && matches!(e.message, MidiMessage::NoteOn { .. }));
        assert!(second_on.is_some());
        assert!(matches!(
            second_on.unwrap().message,
            MidiMessage::NoteOn { note: 51, .. }
        ));
    }

    #[test]
    fn rest_advances_position() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    PitchedElement::Note(
                        NoteEvent::Rest {
                            duration: Some(4),
                            dotted: false,
                        },
                        Articulation::Normal,
                    ),
                    single_note(NoteName::C, Some(4), Some(4), false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_on = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOn { .. }));
        assert_eq!(note_on.unwrap().tick, 480);
    }

    #[test]
    fn staccato_uses_gate_staccato() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Note(
                    NoteEvent::Single {
                        name: NoteName::C,
                        octave: Some(4),
                        duration: Some(4),
                        dotted: false,
                    },
                    Articulation::Staccato,
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // gate_staccato=40%, 480*40% = 192
        let note_off = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOff { .. }));
        assert_eq!(note_off.unwrap().tick, 192);
    }

    #[test]
    fn gate_direct_percent() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Note(
                    NoteEvent::Single {
                        name: NoteName::C,
                        octave: Some(4),
                        duration: Some(4),
                        dotted: false,
                    },
                    Articulation::GateDirect(95),
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // 480 * 95% = 456
        let note_off = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOff { .. }));
        assert_eq!(note_off.unwrap().tick, 456);
    }

    #[test]
    fn bars_truncates_events() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            Some(1),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(1), false),
                    single_note(NoteName::D, None, None, false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 1920);
        let d_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { note: 62, .. }))
            .collect();
        assert!(d_events.is_empty());
        // bars超過時にワーニングが生成される
        assert_eq!(compiled.warnings.len(), 1);
        assert!(compiled.warnings[0].contains("超過"));
    }

    #[test]
    fn bars_pads_total_ticks() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            Some(2),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 3840);
        // bars未超過時はワーニングなし
        assert!(compiled.warnings.is_empty());
    }

    #[test]
    fn dotted_note_duration() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), true)],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // 付点四分 = 720 ticks, gate 80% = 576
        let note_off = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOff { .. }));
        assert_eq!(note_off.unwrap().tick, 576);
    }

    #[test]
    fn unknown_instrument_error() {
        let registry = Registry::default();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![],
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(result.is_err());
    }

    #[test]
    fn drum_clip_basic() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "tr808".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "bd".to_string(),
                channel: 10,
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: Some(20),
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "drums".to_string(),
            options: ClipOptions {
                bars: None,
                time_sig: None,
                scale: None,
            },
            body: ClipBody::Drum(crate::ast::clip::DrumClipBody {
                kit: "tr808".to_string(),
                resolution: 16,
                rows: vec![crate::ast::clip_drum::DrumRow {
                    instrument: "bd".to_string(),
                    hits: vec![
                        HitSymbol::Normal,
                        HitSymbol::Rest,
                        HitSymbol::Rest,
                        HitSymbol::Rest,
                        HitSymbol::Normal,
                        HitSymbol::Rest,
                        HitSymbol::Rest,
                        HitSymbol::Rest,
                    ],
                    probability: None,
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.events.len(), 4);
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn {
                channel: 10,
                note: 36,
                velocity: 100
            }
        ));
        // 2nd hit at step 4, 16th = 120 ticks → tick 480
        let second_on = compiled
            .events
            .iter()
            .find(|e| e.tick > 0 && matches!(e.message, MidiMessage::NoteOn { .. }));
        assert_eq!(second_on.unwrap().tick, 480);
    }

    #[test]
    fn drum_accent_velocity() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "kit".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "sn".to_string(),
                channel: 10,
                note: KitInstrumentNote {
                    name: NoteName::D,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "d".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Drum(crate::ast::clip::DrumClipBody {
                kit: "kit".to_string(),
                resolution: 16,
                rows: vec![crate::ast::clip_drum::DrumRow {
                    instrument: "sn".to_string(),
                    hits: vec![HitSymbol::Accent, HitSymbol::Ghost],
                    probability: None,
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn { velocity: 127, .. }
        ));
        let ghost_on = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOn { velocity: 40, .. }));
        assert!(ghost_on.is_some());
    }

    #[test]
    fn events_sorted_note_on_before_off() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "pad".to_string(),
            device: "dev".to_string(),
            channel: 3,
            note: None,
            gate_normal: Some(100),
            gate_staccato: Some(60),
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));

        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "pad".to_string(),
                    elements: vec![
                        single_note(NoteName::C, Some(4), Some(4), false),
                        single_note(NoteName::D, None, None, false),
                    ],
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &Clock::new(120.0), &registry).unwrap();
        let at_480: Vec<_> = compiled.events.iter().filter(|e| e.tick == 480).collect();
        if at_480.len() == 2 {
            assert!(matches!(at_480[0].message, MidiMessage::NoteOn { .. }));
            assert!(matches!(at_480[1].message, MidiMessage::NoteOff { .. }));
        }
    }

    #[test]
    fn bar_jump_sets_position() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            Some(4),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(1), false),
                    PitchedElement::BarJump(crate::parser::clip_bar_jump::BarJump {
                        bar_number: 3,
                    }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let e_on = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOn { note: 64, .. }));
        assert_eq!(e_on.unwrap().tick, 3840);
    }

    /// bars=4 で >5 がエラーになることを検証
    /// Verify that >5 with bars=4 returns an error
    #[test]
    fn bar_jump_out_of_range_error() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            Some(4),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    PitchedElement::BarJump(crate::parser::clip_bar_jump::BarJump {
                        bar_number: 5,
                    }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("範囲外"));
    }

    /// bars=4 で >4 が正常であることを検証
    /// Verify that >4 with bars=4 is valid
    #[test]
    fn bar_jump_at_boundary_ok() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            Some(4),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    PitchedElement::BarJump(crate::parser::clip_bar_jump::BarJump {
                        bar_number: 4,
                    }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(result.is_ok());
    }

    /// bars未指定で >N が正常であることを検証
    /// Verify that >N without bars is always valid
    #[test]
    fn bar_jump_no_bars_always_ok() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    PitchedElement::BarJump(crate::parser::clip_bar_jump::BarJump {
                        bar_number: 100,
                    }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_gate_percent_normal() {
        assert_eq!(resolve_gate_percent(&Articulation::Normal, 80, 40), 80);
    }

    #[test]
    fn resolve_gate_percent_staccato() {
        assert_eq!(resolve_gate_percent(&Articulation::Staccato, 80, 40), 40);
    }

    #[test]
    fn resolve_gate_percent_direct() {
        assert_eq!(
            resolve_gate_percent(&Articulation::GateDirect(95), 80, 40),
            95
        );
    }

    #[test]
    fn repetition_pitched_basic() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Repetition(
                    crate::parser::clip_repetition::Repetition {
                        content: "c:3:8 c eb".to_string(),
                        count: 4,
                    },
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // 3 notes * 4 reps = 12 notes = 24 events (NoteOn + NoteOff)
        let note_on_count = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .count();
        assert_eq!(note_on_count, 12);
    }

    #[test]
    fn repetition_carries_octave_and_duration() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // (c:3:8)*2 → 第2回もオクターブ3、8分音符を引き継ぐ
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Repetition(
                    crate::parser::clip_repetition::Repetition {
                        content: "c:3:8".to_string(),
                        count: 2,
                    },
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 2);
        // 両方 C3 = 48
        for ev in &note_ons {
            assert!(matches!(ev.message, MidiMessage::NoteOn { note: 48, .. }));
        }
        // 2nd note at tick 240 (8th note = 240 ticks)
        assert_eq!(note_ons[1].tick, 240);
    }

    // --- ChordName コンパイルテスト ---

    use crate::ast::clip_note::ChordSuffix;

    /// Cm7:4:2 → 4音(C4=60, Eb4=63, G4=67, Bb4=70)、gate80%
    /// Cm7:4:2 → 4 notes (C4=60, Eb4=63, G4=67, Bb4=70), gate 80%
    #[test]
    fn chord_name_cm7_basic() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Note(
                    NoteEvent::ChordName {
                        root: NoteName::C,
                        suffix: ChordSuffix::Min7,
                        octave: Some(4),
                        duration: Some(2),
                        dotted: false,
                    },
                    Articulation::Normal,
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // Cm7 = 4構成音
        assert_eq!(note_ons.len(), 4);
        let notes: Vec<u8> = note_ons
            .iter()
            .map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => note,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(notes, vec![60, 63, 67, 70]);

        // 全NoteOnは同一tick(0)
        assert!(note_ons.iter().all(|e| e.tick == 0));

        // gate 80%: 半音符=960ticks, 960*80%=768
        let note_offs: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOff { .. }))
            .collect();
        assert_eq!(note_offs.len(), 4);
        assert!(note_offs.iter().all(|e| e.tick == 768));
    }

    /// octave/duration の carry forward 検証
    /// Verify octave/duration carry forward
    #[test]
    fn chord_name_carry_forward() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // ChordName(oct=3, dur=8) → Single(oct=None, dur=None)
        // Singleは oct=3, dur=8 を引き継ぐべき
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    PitchedElement::Note(
                        NoteEvent::ChordName {
                            root: NoteName::C,
                            suffix: ChordSuffix::Maj,
                            octave: Some(3),
                            duration: Some(8),
                            dotted: false,
                        },
                        Articulation::Normal,
                    ),
                    single_note(NoteName::E, None, None, false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // Cmaj:3:8 = 3音 + 後続E = 計4 NoteOn
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 4);

        // 後続 E は oct=3 を引き継ぎ → E3=52
        let e_note = note_ons.last().unwrap();
        assert!(matches!(
            e_note.message,
            MidiMessage::NoteOn { note: 52, .. }
        ));
        // 8分音符=240ticks でのオフセット
        assert_eq!(e_note.tick, 240);
    }

    /// スタッカート時のgate40%検証
    /// Verify gate 40% with staccato articulation
    #[test]
    fn chord_name_staccato() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Note(
                    NoteEvent::ChordName {
                        root: NoteName::C,
                        suffix: ChordSuffix::Maj,
                        octave: Some(4),
                        duration: Some(4),
                        dotted: false,
                    },
                    Articulation::Staccato,
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // gate_staccato=40%, 480*40%=192
        let note_offs: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOff { .. }))
            .collect();
        assert!(note_offs.iter().all(|e| e.tick == 192));
    }

    /// 繰り返し内でのコード名使用検証
    /// Verify chord name usage inside repetition
    #[test]
    fn chord_name_in_repetition() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // (cm7:4:4)*2 → Cm7 4音 × 2回 = 8 NoteOn
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::Repetition(
                    crate::parser::clip_repetition::Repetition {
                        content: "cm7:4:4".to_string(),
                        count: 2,
                    },
                )],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_on_count = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .count();
        // Cm7=4音 × 2回 = 8
        assert_eq!(note_on_count, 8);

        // 2回目は tick=480 から開始
        let second_round: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| e.tick == 480 && matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(second_round.len(), 4);
    }

    // --- ChordBracket コンパイルテスト ---

    /// [c:4 eb g bb]:2 → 4音同時発音、gate80%
    /// [c:4 eb g bb]:2 → 4 simultaneous notes, gate 80%
    #[test]
    fn chord_bracket_basic() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::ChordBracket {
                    notes: vec![
                        (NoteName::C, Some(4)),
                        (NoteName::Eb, None),
                        (NoteName::G, None),
                        (NoteName::Bb, None),
                    ],
                    duration: Some(2),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: None,
                }],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 4);

        let notes: Vec<u8> = note_ons
            .iter()
            .map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => note,
                _ => unreachable!(),
            })
            .collect();
        // C4=60, Eb4=63, G4=67, Bb4=70
        assert_eq!(notes, vec![60, 63, 67, 70]);

        // 全NoteOnは同一tick(0)
        assert!(note_ons.iter().all(|e| e.tick == 0));

        // gate 80%: 半音符=960ticks, 960*80%=768
        let note_offs: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOff { .. }))
            .collect();
        assert_eq!(note_offs.len(), 4);
        assert!(note_offs.iter().all(|e| e.tick == 768));
    }

    /// スタッカート時のgate40%検証
    /// Verify gate 40% with staccato articulation on chord bracket
    #[test]
    fn chord_bracket_staccato() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::ChordBracket {
                    notes: vec![
                        (NoteName::C, Some(4)),
                        (NoteName::E, None),
                        (NoteName::G, None),
                    ],
                    duration: Some(4),
                    dotted: false,
                    articulation: Articulation::Staccato,
                    arpeggio: None,
                }],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // gate_staccato=40%, 480*40%=192
        let note_offs: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOff { .. }))
            .collect();
        assert!(note_offs.iter().all(|e| e.tick == 192));
    }

    /// duration引き継ぎ検証
    /// Verify duration carry forward from chord bracket
    #[test]
    fn chord_bracket_carry_forward() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // ChordBracket(dur=8) → Single(dur=None) → dur=8を引き継ぐべき
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    PitchedElement::ChordBracket {
                        notes: vec![(NoteName::C, Some(3)), (NoteName::E, None)],
                        duration: Some(8),
                        dotted: false,
                        articulation: Articulation::Normal,
                        arpeggio: None,
                    },
                    single_note(NoteName::G, None, None, false),
                ],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // ChordBracket: 2音 + Single: 1音 = 3 NoteOn
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 3);

        // 後続 G は8分音符=240ticks後に開始
        let g_note = note_ons.last().unwrap();
        assert_eq!(g_note.tick, 240);
    }

    /// 個別オクターブ指定検証
    /// Verify individual octave specification in chord bracket
    #[test]
    fn chord_bracket_individual_octave() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [c:3 e:5 g:4] — 各音が個別のオクターブ
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![PitchedElement::ChordBracket {
                    notes: vec![
                        (NoteName::C, Some(3)),
                        (NoteName::E, Some(5)),
                        (NoteName::G, Some(4)),
                    ],
                    duration: Some(4),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: None,
                }],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let notes: Vec<u8> = compiled
            .events
            .iter()
            .filter_map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => Some(note),
                _ => None,
            })
            .collect();
        // C3=48, E5=76, G4=67
        assert_eq!(notes, vec![48, 76, 67]);
    }

    // --- 最小Gate Off 5ms テスト ---

    /// apply_min_gate_off の単体テスト: gate100%はレガート（off=0）
    /// Unit test: gate 100% returns legato (off=0)
    #[test]
    fn min_gate_off_legato_unchanged() {
        let clock = Clock::new(120.0);
        // gate100% → レガート、note_ticksそのまま
        let result = apply_min_gate_off(480, 100, &clock);
        assert_eq!(result, 480);
    }

    /// apply_min_gate_off: 通常のgate比率では5ms保証に影響しない
    /// Normal gate ratio is not affected by 5ms guarantee
    #[test]
    fn min_gate_off_normal_unaffected() {
        let clock = Clock::new(120.0);
        // 120BPM, PPQ480: tick_duration_us = 1041us
        // 480ticks * 80% = 384 → off = 96 ticks ≈ 100ms >> 5ms → 変更なし
        let result = apply_min_gate_off(480, 80, &clock);
        assert_eq!(result, 384);
    }

    /// apply_min_gate_off: 極端なgate比率で5ms保証が効く
    /// Extreme gate ratio triggers 5ms guarantee
    #[test]
    fn min_gate_off_extreme_gate_clamped() {
        let clock = Clock::new(120.0);
        // 120BPM, PPQ480: tick_duration_us = 1041us
        // min_off_ticks = ceil(5000/1041) = 5
        // 10ticks * 99% = 9 → off = 1 tick < 5 → gate_ticks = 10 - 5 = 5
        let result = apply_min_gate_off(10, 99, &clock);
        assert_eq!(result, 5);
    }

    /// gate100%でも5ms保証がコンパイル結果に影響しないことを検証（統合テスト）
    /// Verify gate 100% (legato) produces full note_ticks as gate in compiled clip
    #[test]
    fn min_gate_off_legato_compile() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "pad".to_string(),
            device: "dev".to_string(),
            channel: 3,
            note: None,
            gate_normal: Some(100),
            gate_staccato: Some(60),
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));

        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "pad".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // gate100% → NoteOff at tick 480 (レガート)
        let note_off = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOff { .. }))
            .unwrap();
        assert_eq!(note_off.tick, 480);
    }
}
