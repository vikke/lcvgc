use std::collections::HashSet;

use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody, PitchedElement, PitchedLine};
use crate::ast::clip_arpeggio::ArpeggioDirection;
use crate::ast::clip_articulation::Articulation;
use crate::ast::clip_cc::{CcAutomation, Interpolation};
use crate::ast::clip_drum::HitSymbol;
use crate::ast::clip_note::NoteEvent;
use crate::domain::channel::MidiChannel;
use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::registry::Registry;
use crate::midi::chord::chord_notes;
use crate::midi::message::MidiMessage;
use crate::midi::note::note_number;
use crate::parser::clip_shorthand::CarryOverState;

/// メロディノートに `vN` 指定が無いときに用いる既定の MIDI velocity (Note On)。
/// Default MIDI velocity (Note On) for pitched notes without an explicit `vN` suffix.
const DEFAULT_NOTE_ON_VELOCITY: u8 = 100;

/// tickベースMIDIイベント
///
/// Issue #49 対応: `device` フィールドで送出先デバイスの論理名を保持する。
/// `PlaybackDriver` はこの値をキーに、対応する `MidiSink` に振り分ける。
/// 空文字列の場合は「未指定」を意味し、デフォルト sink へルーティングされる
/// （後方互換目的）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiEvent {
    pub tick: u64,
    pub message: MidiMessage,
    /// 送出先デバイスの論理名（`instrument.device` / `kit.device` 由来）
    /// Logical device name for routing, resolved from `instrument.device` or `kit.device`.
    pub device: String,
}

impl MidiEvent {
    /// `MidiEvent` を構築する。
    ///
    /// # Arguments
    /// * `tick` - イベント発生位置（tick 単位）
    /// * `message` - MIDI メッセージ
    /// * `device` - 送出先デバイスの論理名
    pub fn new(tick: u64, message: MidiMessage, device: impl Into<String>) -> Self {
        Self {
            tick,
            message,
            device: device.into(),
        }
    }
}

/// ドラム確率行で抽選対象となる 1 ステップ分のイベント群
///
/// `event_indices` には対応する NoteOn / NoteOff のような「同じ抽選結果を共有
/// すべき MIDI イベント」の `CompiledClip.events` 上の index を保持する。
/// ループ毎の再抽選時に、`probability` (0-100) を使って roll し、外れた場合は
/// これら index のイベントを丸ごと mute する。
///
/// A single step's worth of MIDI events that share one probability roll.
/// `event_indices` lists indices into `CompiledClip.events` for events
/// (typically a NoteOn/NoteOff pair) that must be triggered or muted together.
/// On every loop boundary the player rolls a new probability against
/// `probability` (0-100) and masks these indices out when the roll fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumProbabilityGroup {
    /// この group に属する MIDI イベントの `events` 配列上の index 群
    /// Indices into `events` that belong to this group.
    pub event_indices: Vec<usize>,
    /// 0-100 の発音確率（100 = 必ず発音、0 = 発音しない）
    /// Firing probability in 0-100 (100 = always, 0 = never).
    pub probability: u8,
}

/// `arp(random, ...)` のように「複数候補から1つだけを毎ループ抽選で選ぶ」
/// セマンティクスを表現する group。
///
/// 各 `candidates` 要素は、互いに排他で発音される候補ノートの MIDI イベント
/// index 集合（典型的には NoteOn と対応する NoteOff の 2 件）を保持する。
/// player はループ境界毎に 1 つだけ候補を選び、それ以外の候補に属する
/// イベントを `masked_events` に積む。
///
/// Group expressing "pick one candidate per loop iteration", used for
/// `arp(random, ...)`. Each `candidates` entry is a set of `events` indices
/// that fire together (e.g. a NoteOn/NoteOff pair). The player picks one
/// candidate at every loop boundary and masks all other candidates' events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RandomChoiceGroup {
    /// 候補リスト。各候補は同時に triggered/silenced される event index 群。
    /// Candidate sets, each holding the `events` indices to keep when chosen.
    pub candidates: Vec<Vec<usize>>,
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
    /// ドラム発音率行から生成された抽選グループ。空ならば確率ベースの抽選は無し。
    /// Probability groups produced from drum probability rows; empty means no
    /// per-loop probability gating is required.
    pub drum_mask_groups: Vec<DrumProbabilityGroup>,
    /// `arp(random, ...)` などの「複数候補から1つを毎ループ抽選」用 group。
    /// 空ならば抽選不要。
    ///
    /// Per-loop random-choice groups (e.g. produced by `arp(random, ...)`).
    /// Empty when no random selection is required.
    pub random_choice_groups: Vec<RandomChoiceGroup>,
}

/// ピッチド clip の MIDI イベント列に対し、全体オクターブシフトを適用する。
/// `octave_shift` が 0 のときは何もしない。各 Note On / Note Off の note
/// 番号に `octave_shift * 12` を加減算し、結果が MIDI 範囲 (0-127) を外れる
/// 場合はコンパイルエラーとする。
///
/// Apply a whole-clip octave shift to a pitched clip's MIDI events. A shift of
/// 0 is a no-op. Each Note On / Note Off note number is offset by
/// `octave_shift * 12`; if any result falls outside the MIDI range (0-127),
/// a compile error is returned.
///
/// # 引数 / Arguments
/// * `events` - 移調対象の MIDI イベント列（in-place で書き換える） / MIDI events to transpose in place
/// * `octave_shift` - シフトするオクターブ数（正で上、負で下） / Octave shift count (positive up, negative down)
///
/// # 戻り値 / Returns
/// 成功時 `Ok(())`。範囲外ノートが生じた場合 `Err(EngineError::CompileError)`。
/// `Ok(())` on success; `Err(EngineError::CompileError)` when a note goes out of range.
///
/// # エラー / Errors
/// シフト後のノート番号が 0 未満または 127 超になる場合に
/// `EngineError::CompileError` を返す。
/// Returns `EngineError::CompileError` if a shifted note number would be below
/// 0 or above 127.
fn apply_octave_shift(events: &mut [MidiEvent], octave_shift: i8) -> Result<(), EngineError> {
    if octave_shift == 0 {
        return Ok(());
    }
    let delta = octave_shift as i32 * 12;
    for event in events.iter_mut() {
        let note_ref: Option<&mut u8> = match &mut event.message {
            MidiMessage::NoteOn { note, .. } => Some(note),
            MidiMessage::NoteOff { note, .. } => Some(note),
            _ => None,
        };
        if let Some(note) = note_ref {
            let shifted = *note as i32 + delta;
            if !(0..=127).contains(&shifted) {
                return Err(EngineError::CompileError(format!(
                    "オクターブシフト [{}] によりノート番号が MIDI 範囲 (0-127) を外れました: {} -> {}",
                    octave_shift, note, shifted
                )));
            }
            *note = shifted as u8;
        }
    }
    Ok(())
}

/// クリップ定義をtickベースMIDIイベント列にコンパイルする
pub fn compile_clip(
    clip: &ClipDef,
    clock: &Clock,
    registry: &Registry,
) -> Result<CompiledClip, EngineError> {
    let (mut events, mut drum_mask_groups, mut random_choice_groups, logical_end_ticks) =
        match &clip.body {
            ClipBody::Pitched(body) => {
                let (mut evts, randoms, end) =
                    compile_pitched(body, clock, registry, clip.options.bars)?;
                // clip ヘッダの `[>>]` / `[<<]` による全体オクターブシフトを適用する。
                // ピッチド clip のみが対象 (drum clip は対象外)。
                // Apply the whole-clip octave shift from the `[>>]` / `[<<]`
                // header option. Pitched clips only (drum clips are excluded).
                apply_octave_shift(&mut evts, clip.options.octave_shift)?;
                (evts, Vec::new(), randoms, end)
            }
            ClipBody::Drum(body) => {
                let (evts, drums, end) = compile_drum(body, clock, registry, clip.options.bars)?;
                (evts, drums, Vec::new(), end)
            }
        };

    let mut warnings = Vec::new();

    // bars制約の適用
    let total_ticks = if let Some(bars) = clip.options.bars {
        let bar_ticks = clock.ticks_per_bar();
        let max_ticks = bar_ticks * bars as u64;

        // clip 内に Note On が存在する (channel, note, device) を集める。
        // この集合に属する Note Off で tick >= max_ticks のものはクランプ対象、
        // それ以外の超過イベントは切り捨て対象とする。
        //
        // Collect (channel, note, device) tuples whose NoteOn falls inside the
        // clip. A NoteOff for such a tuple that would land at/after max_ticks
        // gets clamped to max_ticks - 1, ensuring every sounded note is
        // closed and external MIDI gear never hangs.
        let inside_note_ons: HashSet<(MidiChannel, u8, String)> = events
            .iter()
            .filter_map(|e| {
                if e.tick >= max_ticks {
                    return None;
                }
                if let MidiMessage::NoteOn { channel, note, .. } = e.message {
                    Some((channel, note, e.device.clone()))
                } else {
                    None
                }
            })
            .collect();

        let clamp_target_tick = max_ticks.saturating_sub(1);
        let mut clamp_count = 0usize;
        let mut drop_count = 0usize;
        // 切り捨て対象判定: 該当イベントを削除する場合は true、保持 (クランプ含む) なら false。
        // Returns true when the event should be dropped; clamping is treated
        // as keeping the event (with its tick rewritten in place below).
        let kept_indices: Vec<bool> = events
            .iter_mut()
            .map(|e| {
                if e.tick < max_ticks {
                    return true;
                }
                // overflow: NoteOff で対応する NoteOn が clip 内ならクランプして残す
                if let MidiMessage::NoteOff { channel, note, .. } = e.message {
                    let key = (channel, note, e.device.clone());
                    if inside_note_ons.contains(&key) {
                        e.tick = clamp_target_tick;
                        clamp_count += 1;
                        return true;
                    }
                }
                drop_count += 1;
                false
            })
            .collect();

        if clamp_count > 0 {
            warnings.push(format!(
                "clip '{}': bars={} を超過する Note Off が {}個あり、clip 末尾へクランプしました",
                clip.name, bars, clamp_count
            ));
        }
        if drop_count > 0 {
            warnings.push(format!(
                "clip '{}': bars={} を超過するイベントが {}個あり、切り捨てられました",
                clip.name, bars, drop_count
            ));
        }

        // 削除対象を反映しつつ、drum_mask_groups / random_choice_groups の
        // index を整理する。
        // Drop deletion targets and prune matching index sets so remaining
        // indices stay consistent.
        let mut old_to_new: Vec<Option<usize>> = Vec::with_capacity(events.len());
        let mut new_idx: usize = 0;
        for keep in &kept_indices {
            if *keep {
                old_to_new.push(Some(new_idx));
                new_idx += 1;
            } else {
                old_to_new.push(None);
            }
        }
        let mut keep_iter = kept_indices.iter();
        events.retain(|_| *keep_iter.next().unwrap());
        for group in drum_mask_groups.iter_mut() {
            group.event_indices = group
                .event_indices
                .iter()
                .filter_map(|i| old_to_new.get(*i).copied().flatten())
                .collect();
        }
        drum_mask_groups.retain(|g| !g.event_indices.is_empty());
        for group in random_choice_groups.iter_mut() {
            for cand in group.candidates.iter_mut() {
                *cand = cand
                    .iter()
                    .filter_map(|i| old_to_new.get(*i).copied().flatten())
                    .collect();
            }
            group.candidates.retain(|c| !c.is_empty());
        }
        random_choice_groups.retain(|g| g.candidates.len() >= 2);
        max_ticks
    } else {
        // bars 未指定: 各ラインの論理終了 tick（音価ベース）の最大値を採用する。
        // gate 比率による NoteOff 早期化に依存しないため、scene 内で他 clip と
        // 小節長が揃いやすい。
        //
        // No `bars`: use the musical end tick (each line's `current_tick` after
        // its last element). This is independent of gate ratio so clips align
        // by musical bar length within a scene.
        logical_end_ticks
    };

    // tick順にソート（同一tickではNoteOnをNoteOffより先に）
    // ソートに伴うインデックス変動を drum_mask_groups / random_choice_groups にも
    // 反映するため、一時的に元 index を持たせてソートし、permutation を再構築する。
    //
    // Sort tick-ascending (NoteOn before NoteOff at the same tick). Because
    // the sort permutes events, also rebuild group event indices through the
    // permutation so they still point at the right events afterwards.
    let mut indexed: Vec<(usize, MidiEvent)> = events.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        a.1.tick.cmp(&b.1.tick).then_with(|| {
            let a_priority = event_sort_priority(&a.1.message);
            let b_priority = event_sort_priority(&b.1.message);
            a_priority.cmp(&b_priority)
        })
    });
    let mut old_to_sorted: Vec<usize> = vec![0; indexed.len()];
    let mut sorted_events: Vec<MidiEvent> = Vec::with_capacity(indexed.len());
    for (sorted_idx, (old_idx, ev)) in indexed.into_iter().enumerate() {
        old_to_sorted[old_idx] = sorted_idx;
        sorted_events.push(ev);
    }
    for group in drum_mask_groups.iter_mut() {
        for idx in group.event_indices.iter_mut() {
            *idx = old_to_sorted[*idx];
        }
        group.event_indices.sort_unstable();
    }
    for group in random_choice_groups.iter_mut() {
        for cand in group.candidates.iter_mut() {
            for idx in cand.iter_mut() {
                *idx = old_to_sorted[*idx];
            }
            cand.sort_unstable();
        }
    }

    Ok(CompiledClip {
        events: sorted_events,
        total_ticks,
        warnings,
        drum_mask_groups,
        random_choice_groups,
    })
}

/// ソート優先度: NoteOn(0) < CC(1) < NoteOff(2)
/// System Real-Time (Start/Stop/Continue) は clip にコンパイルされないため到達しない。
/// System Real-Time messages never appear in compiled clip events.
fn event_sort_priority(msg: &MidiMessage) -> u8 {
    match msg {
        MidiMessage::NoteOn { .. } => 0,
        MidiMessage::ControlChange { .. } => 1,
        MidiMessage::NoteOff { .. } => 2,
        MidiMessage::ProgramChange { .. } => 1,
        MidiMessage::Start | MidiMessage::Stop | MidiMessage::Continue | MidiMessage::Clock => {
            unreachable!("System Real-Time messages are not part of compiled clip events")
        }
    }
}

/// ピッチドクリップのコンパイル
///
/// 戻り値の `u64` は **論理終了 tick**（各 line の `current_tick` 終端の最大値）。
/// これは「最後のノートの音価が終わる位置」を意味し、gate 比率による NoteOff
/// 早期化の影響を受けない。bars 未指定時の `total_ticks` 算出に使われる。
///
/// The returned `u64` is the musical end tick (max of each line's final
/// `current_tick`). It represents where the last note's musical duration
/// ends, independent of gate-driven NoteOff shortening, and is consumed by
/// `compile_clip` to compute `total_ticks` when `bars` is omitted.
fn compile_pitched(
    body: &PitchedClipBody,
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<(Vec<MidiEvent>, Vec<RandomChoiceGroup>, u64), EngineError> {
    let mut events = Vec::new();
    let mut random_choice_groups: Vec<RandomChoiceGroup> = Vec::new();
    let mut logical_end_ticks: u64 = 0;

    // 連続する PitchedLine を「レイヤー」単位にグルーピングしてコンパイルする。
    // `is_layer_start = true` のラインから次の `is_layer_start = true` の手前までが
    // 1 つのレイヤー。 1 レイヤー内は単一の CarryOverState と current_tick を共有
    // して時系列に連結される。レイヤー境界では tick / carry-over が 0 リセット。
    //
    // Group consecutive PitchedLines into layers. A layer spans from one
    // `is_layer_start = true` line up to (but not including) the next
    // `is_layer_start = true`. Within a layer, a single CarryOverState and
    // current_tick is threaded across all lines, producing a merged timeline.
    // At each layer boundary the tick and carry-over are reset to 0.
    let mut i = 0;
    while i < body.lines.len() {
        // レイヤー範囲を決定: i から次の is_layer_start まで
        // Find the layer range: from i to the next is_layer_start
        let layer_start = i;
        let mut layer_end = i + 1;
        while layer_end < body.lines.len() && !body.lines[layer_end].is_layer_start {
            layer_end += 1;
        }

        let (layer_events, layer_groups, layer_logical_end) =
            compile_pitched_layer(&body.lines[layer_start..layer_end], clock, registry, bars)?;

        let offset = events.len();
        for mut group in layer_groups {
            for cand in group.candidates.iter_mut() {
                for idx in cand.iter_mut() {
                    *idx += offset;
                }
            }
            random_choice_groups.push(group);
        }
        events.extend(layer_events);
        if layer_logical_end > logical_end_ticks {
            logical_end_ticks = layer_logical_end;
        }

        i = layer_end;
    }

    // CCオートメーションのコンパイル
    let cc_events = compile_cc_automations(&body.cc_automations, clock, registry, bars)?;
    events.extend(cc_events);
    Ok((events, random_choice_groups, logical_end_ticks))
}

/// 1 つのレイヤー (連続する同 instrument の PitchedLine 群) をコンパイルする。
/// レイヤー内では `CarryOverState` と `current_tick` を共有し、各 line を時系列に
/// 連結する。
///
/// Compile a single layer: a slice of PitchedLines that share a CarryOverState
/// and a continuously advancing `current_tick`. The first line in the slice
/// must have `is_layer_start = true`; subsequent lines merge onto the same
/// timeline.
fn compile_pitched_layer(
    layer_lines: &[PitchedLine],
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<(Vec<MidiEvent>, Vec<RandomChoiceGroup>, u64), EngineError> {
    debug_assert!(
        !layer_lines.is_empty(),
        "compile_pitched_layer requires at least one line"
    );

    // レイヤー内の全行は同じ instrument のはず (parser 側でその不変条件を保証)
    // All lines within a layer share the same instrument (parser invariant).
    let inst = registry
        .get_instrument(&layer_lines[0].instrument)
        .ok_or_else(|| EngineError::UnknownInstrument(layer_lines[0].instrument.clone()))?;

    let channel = inst.channel;
    let gate_normal = inst.gate_normal.unwrap_or(80);
    let gate_staccato = inst.gate_staccato.unwrap_or(40);
    // 音程楽器の `vN` 未指定ノートに用いる既定 velocity。velocity_normal 未指定時は従来の固定値。
    // Default velocity for pitched notes without a `vN` suffix; falls back to the legacy constant.
    let velocity_normal = inst.velocity_normal.unwrap_or(DEFAULT_NOTE_ON_VELOCITY);
    let device = inst.device.clone();

    let mut events = Vec::new();
    let mut random_choice_groups: Vec<RandomChoiceGroup> = Vec::new();
    let mut current_tick: u64 = 0;
    let mut carry = CarryOverState::new();

    for line in layer_lines {
        compile_elements(
            &line.elements,
            clock,
            channel,
            &device,
            gate_normal,
            gate_staccato,
            velocity_normal,
            &mut current_tick,
            &mut carry,
            &mut events,
            &mut random_choice_groups,
            bars,
        )?;
    }

    Ok((events, random_choice_groups, current_tick))
}

/// ピッチド要素列をMIDIイベントにコンパイルする（再帰対応）。
/// Repetition の展開時に再帰呼び出しされる。
///
/// Compile a slice of pitched elements into MIDI events (supports recursion for Repetition).
#[allow(clippy::too_many_arguments)]
fn compile_elements(
    elements: &[PitchedElement],
    clock: &Clock,
    channel: MidiChannel,
    device: &str,
    gate_normal: u8,
    gate_staccato: u8,
    velocity_normal: u8,
    current_tick: &mut u64,
    carry: &mut CarryOverState,
    events: &mut Vec<MidiEvent>,
    random_choice_groups: &mut Vec<RandomChoiceGroup>,
    bars: Option<u32>,
) -> Result<(), EngineError> {
    // PipeSnap (`|`) のための直近アンカー追跡。
    //   - `pipe_anchor_tick`: 直近 `|`/行頭の絶対 tick 位置。
    //   - `pipe_anchor_event_count`: そのときの `events.len()`。 truncate 時
    //     「このセグメントで追加された events のみ」を対象に絞るために使う。
    //
    // Anchor state for `|` snap:
    //   - `pipe_anchor_tick`: absolute tick at the most recent `|` / start
    //     of this element list.
    //   - `pipe_anchor_event_count`: `events.len()` at that anchor. Used to
    //     limit truncation to events emitted in the current segment so that
    //     events emitted by previous segments stay intact.
    let mut pipe_anchor_tick: u64 = *current_tick;
    let mut pipe_anchor_event_count: usize = events.len();
    for element in elements {
        match element {
            PitchedElement::Note(note_event, articulation, velocity_override) => match note_event {
                NoteEvent::Single {
                    name,
                    octave,
                    duration,
                    dotted,
                } => {
                    let resolved = carry.resolve(*octave, *duration, *dotted);

                    let note = note_number(*name, resolved.octave);
                    let note_ticks = clock.duration_to_ticks(resolved.duration, resolved.dotted);
                    let gate_percent =
                        resolve_gate_percent(articulation, gate_normal, gate_staccato);
                    let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);
                    let velocity = velocity_override.unwrap_or(velocity_normal);

                    events.push(MidiEvent::new(
                        *current_tick,
                        MidiMessage::NoteOn {
                            channel,
                            note,
                            velocity,
                        },
                        device,
                    ));
                    events.push(MidiEvent::new(
                        *current_tick + gate_ticks,
                        MidiMessage::NoteOff {
                            channel,
                            note,
                            velocity: 0,
                        },
                        device,
                    ));

                    *current_tick += note_ticks;
                }
                NoteEvent::Rest { duration, dotted } => {
                    let resolved = carry.resolve_duration_only(*duration, *dotted);
                    let note_ticks = clock.duration_to_ticks(resolved.duration, resolved.dotted);
                    *current_tick += note_ticks;
                }
                NoteEvent::ChordName {
                    root,
                    suffix,
                    octave,
                    duration,
                    dotted,
                    arpeggio,
                } => {
                    // コード名→MIDIノート群に展開
                    // Expand chord name to MIDI notes
                    let resolved = carry.resolve(*octave, *duration, *dotted);
                    let notes = chord_notes(*root, resolved.octave, suffix);
                    let gate_percent =
                        resolve_gate_percent(articulation, gate_normal, gate_staccato);
                    let velocity = velocity_override.unwrap_or(velocity_normal);

                    if let Some(arp) = arpeggio {
                        // --- アルペジオ展開 ---
                        // Per-step duration: resolution > duration(明示) > carry-over
                        let step_duration_value = match arp.resolution {
                            Some(r) => {
                                let resolved_step = carry.resolve_duration_only(Some(r), *dotted);
                                resolved_step.duration
                            }
                            None => resolved.duration,
                        };
                        let step_ticks = clock.duration_to_ticks(step_duration_value, *dotted);
                        let gate_ticks = apply_min_gate_off(step_ticks, gate_percent, clock);

                        emit_arpeggio_cycle(
                            &notes,
                            arp.direction,
                            step_ticks,
                            gate_ticks,
                            channel,
                            device,
                            current_tick,
                            events,
                            random_choice_groups,
                            velocity,
                        );
                    } else {
                        // --- 同時発音（既存挙動）---
                        let note_ticks =
                            clock.duration_to_ticks(resolved.duration, resolved.dotted);
                        let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);

                        for &note in &notes {
                            events.push(MidiEvent::new(
                                *current_tick,
                                MidiMessage::NoteOn {
                                    channel,
                                    note,
                                    velocity,
                                },
                                device,
                            ));
                            events.push(MidiEvent::new(
                                *current_tick + gate_ticks,
                                MidiMessage::NoteOff {
                                    channel,
                                    note,
                                    velocity: 0,
                                },
                                device,
                            ));
                        }

                        *current_tick += note_ticks;
                    }
                }
            },
            PitchedElement::ChordBracket {
                notes,
                duration,
                dotted,
                articulation,
                arpeggio,
                velocity: velocity_override,
            } => {
                // 構成音を MIDI ノート番号列に解決（記譜順を保持）
                // Resolve chord tones to MIDI note numbers, preserving notation order.
                let resolved_notes: Vec<u8> = notes
                    .iter()
                    .map(|&(name, oct_opt)| {
                        let oct = oct_opt.unwrap_or(carry.octave);
                        note_number(name, oct)
                    })
                    .collect();

                let gate_percent = resolve_gate_percent(articulation, gate_normal, gate_staccato);
                let velocity = velocity_override.unwrap_or(velocity_normal);

                if let Some(arp) = arpeggio {
                    // --- アルペジオ展開 ---
                    // Arpeggio expansion: chord tones are sequenced one at a time.

                    // 1音あたりの音価を決定する。
                    // resolution > duration(明示) > carry-over(前回 duration) の順で優先。
                    //
                    // Determine per-step duration: prefer arpeggio.resolution, then the
                    // explicit chord duration, then the carry-over duration.
                    let step_duration_value = match arp.resolution {
                        Some(r) => {
                            // resolution があれば carry-over も上書きする（次の要素にも引き継ぐため）
                            // Update carry-over so subsequent elements inherit this value.
                            let resolved = carry.resolve_duration_only(Some(r), *dotted);
                            resolved.duration
                        }
                        None => {
                            let resolved = carry.resolve_duration_only(*duration, *dotted);
                            resolved.duration
                        }
                    };
                    let step_ticks = clock.duration_to_ticks(step_duration_value, *dotted);
                    let gate_ticks = apply_min_gate_off(step_ticks, gate_percent, clock);

                    emit_arpeggio_cycle(
                        &resolved_notes,
                        arp.direction,
                        step_ticks,
                        gate_ticks,
                        channel,
                        device,
                        current_tick,
                        events,
                        random_choice_groups,
                        velocity,
                    );
                } else {
                    // --- 同時発音（既存挙動）---
                    // Simultaneous chord (legacy behavior).
                    let resolved = carry.resolve_duration_only(*duration, *dotted);

                    let note_ticks = clock.duration_to_ticks(resolved.duration, resolved.dotted);
                    let gate_ticks = apply_min_gate_off(note_ticks, gate_percent, clock);

                    for &note in &resolved_notes {
                        events.push(MidiEvent::new(
                            *current_tick,
                            MidiMessage::NoteOn {
                                channel,
                                note,
                                velocity,
                            },
                            device,
                        ));
                        events.push(MidiEvent::new(
                            *current_tick + gate_ticks,
                            MidiMessage::NoteOff {
                                channel,
                                note,
                                velocity: 0,
                            },
                            device,
                        ));
                    }

                    *current_tick += note_ticks;
                }
            }
            PitchedElement::Repetition(rep) => {
                let inner_elements = crate::parser::clip::parse_repetition_content(&rep.content)
                    .map_err(EngineError::CompileError)?;
                for _ in 0..rep.count {
                    compile_elements(
                        &inner_elements,
                        clock,
                        channel,
                        device,
                        gate_normal,
                        gate_staccato,
                        velocity_normal,
                        current_tick,
                        carry,
                        events,
                        random_choice_groups,
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
                // BarJump で絶対位置にスナップした後は、 `|` のアンカーも
                // 新位置にリセットする (= 新セグメント開始扱い)。
                // After absolute snap, reset the `|` anchor to start a new segment.
                pipe_anchor_tick = *current_tick;
                pipe_anchor_event_count = events.len();
            }
            PitchedElement::PipeSnap => {
                // `|` 拍境界スナップ。
                //   - ticks_since_pipe <= ticks_per_beat: 不足 → 次拍境界 (= anchor + tpb) まで進める
                //   - ticks_since_pipe > ticks_per_beat: 超過 → 直前拍境界 (= anchor + floor(ts/tpb)*tpb) に戻し、
                //     anchor 以降に追加された events のうちオンセットが境界以降のものを削除する。
                //
                // Snap to a beat boundary. Pad short segments by advancing
                // `current_tick`, truncate overruns by rewinding `current_tick`
                // and dropping events from this segment whose onset moved past
                // the boundary.
                let tpb = clock.ticks_per_beat();
                let ticks_since_pipe = current_tick.saturating_sub(pipe_anchor_tick);
                if ticks_since_pipe <= tpb {
                    // 不足ケース。 次拍境界まで進めるだけ (events に変更なし)。
                    // Short: advance to the next beat boundary, no events removed.
                    *current_tick = pipe_anchor_tick + tpb;
                } else {
                    // 超過ケース。
                    // Overrun: rewind to previous beat boundary and drop events.
                    let beats = ticks_since_pipe / tpb;
                    let target_tick = pipe_anchor_tick + beats * tpb;
                    // このセグメントで追加された events (index >= pipe_anchor_event_count)
                    // のうち、 tick >= target_tick の物を削除する。
                    // それより前 (= 別セグメント) の events や、まだ境界内に収まる
                    // events は触らない。
                    //
                    // Remove events added in this segment whose tick falls
                    // at or past the truncation target. Earlier-segment events
                    // and events still within the previous beat boundary stay.
                    let mut i = pipe_anchor_event_count;
                    while i < events.len() {
                        if events[i].tick >= target_tick {
                            events.remove(i);
                        } else {
                            i += 1;
                        }
                    }
                    *current_tick = target_tick;
                }
                // 次のセグメントのために anchor を更新。
                // Update anchors for the next segment.
                pipe_anchor_tick = *current_tick;
                pipe_anchor_event_count = events.len();
            }
        }
    }

    Ok(())
}

/// 1 サイクル分のアルペジオを `events` / `random_choice_groups` に書き出す共通ヘルパー。
///
/// ChordBracket と ChordName の両者から呼び出される。`resolved_notes` は構成音の
/// MIDI ノート番号列（記譜順）。`step_ticks` は1音あたりの長さ、`gate_ticks` は
/// 1音あたりの NoteOn → NoteOff の長さ。`current_tick` は呼び出し前の発音開始
/// 位置で、関数内で 1 サイクル分（= 構成音数 ステップ）だけ進められる。
///
/// Shared arpeggio emitter used by both ChordBracket and ChordName.
/// `resolved_notes` are the chord tones in notation order. `step_ticks` is the
/// per-step length, `gate_ticks` the per-step NoteOn→NoteOff distance.
/// `current_tick` is advanced by one full cycle (notes.len() steps).
#[allow(clippy::too_many_arguments)]
fn emit_arpeggio_cycle(
    resolved_notes: &[u8],
    direction: ArpeggioDirection,
    step_ticks: u64,
    gate_ticks: u64,
    channel: MidiChannel,
    device: &str,
    current_tick: &mut u64,
    events: &mut Vec<MidiEvent>,
    random_choice_groups: &mut Vec<RandomChoiceGroup>,
    velocity: u8,
) {
    if resolved_notes.is_empty() {
        return;
    }

    if direction == ArpeggioDirection::Random {
        // Random: 各ステップに全候補ノートを重ねて emit し、ループ毎に
        // 1 候補だけ残るよう RandomChoiceGroup を作る。
        // For Random, lay all candidates per step; player keeps one per loop.
        let step_count = resolved_notes.len();
        for _ in 0..step_count {
            let mut candidates: Vec<Vec<usize>> = Vec::with_capacity(resolved_notes.len());
            for &note in resolved_notes {
                let note_on_idx = events.len();
                events.push(MidiEvent::new(
                    *current_tick,
                    MidiMessage::NoteOn {
                        channel,
                        note,
                        velocity,
                    },
                    device,
                ));
                let note_off_idx = events.len();
                events.push(MidiEvent::new(
                    *current_tick + gate_ticks,
                    MidiMessage::NoteOff {
                        channel,
                        note,
                        velocity: 0,
                    },
                    device,
                ));
                candidates.push(vec![note_on_idx, note_off_idx]);
            }
            if candidates.len() >= 2 {
                random_choice_groups.push(RandomChoiceGroup { candidates });
            }
            *current_tick += step_ticks;
        }
    } else {
        let sequence = build_arpeggio_sequence(resolved_notes, direction);
        for note in sequence {
            events.push(MidiEvent::new(
                *current_tick,
                MidiMessage::NoteOn {
                    channel,
                    note,
                    velocity,
                },
                device,
            ));
            events.push(MidiEvent::new(
                *current_tick + gate_ticks,
                MidiMessage::NoteOff {
                    channel,
                    note,
                    velocity: 0,
                },
                device,
            ));
            *current_tick += step_ticks;
        }
    }
}

/// アルペジオの方向に従って構成音を1サイクル分のシーケンスへ並べ替える。
///
/// - `Up`: 音高昇順
/// - `Down`: 音高降順
/// - `UpDown`: 音高昇順 → 降順の往復（両端は二度鳴らさない、ping-pong）
/// - `Random`: 記譜順をそのまま返す（ループ毎の抽選は player 側で適用するため、
///   ここでは候補列としての並びを保持する）
///
/// Reorder chord tones into one arpeggio cycle according to the direction.
/// `Random` keeps notation order; per-loop randomization is performed in the player.
fn build_arpeggio_sequence(notes: &[u8], direction: ArpeggioDirection) -> Vec<u8> {
    if notes.is_empty() {
        return Vec::new();
    }
    match direction {
        ArpeggioDirection::Up => {
            let mut sorted = notes.to_vec();
            sorted.sort_unstable();
            sorted
        }
        ArpeggioDirection::Down => {
            let mut sorted = notes.to_vec();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted
        }
        ArpeggioDirection::UpDown => {
            let mut sorted = notes.to_vec();
            sorted.sort_unstable();
            // ping-pong: 末尾と先頭の重複を避けるため、降順側は端点を除いて連結
            let len = sorted.len();
            let mut sequence = sorted.clone();
            if len >= 2 {
                for &n in sorted[1..len.saturating_sub(1)].iter().rev() {
                    sequence.push(n);
                }
            }
            sequence
        }
        ArpeggioDirection::Random => notes.to_vec(),
    }
}

/// アーティキュレーションからゲート比率を解決
fn resolve_gate_percent(art: &Articulation, gate_normal: u8, gate_staccato: u8) -> u8 {
    match art {
        Articulation::Normal => gate_normal,
        Articulation::Staccato => gate_staccato,
        Articulation::GateDirect(pct) => *pct,
    }
}

/// ドラムヒットシンボルから MIDI velocity を解決する。
///
/// kit インストゥルメントに `velocity_normal` / `velocity_accent` / `velocity_ghost`
/// が設定されていればそれを優先し、未設定の場合は `HitSymbol` の既定値
/// （Normal=100 / Accent=127 / Ghost=40）にフォールバックする。休符は `None`。
///
/// Resolves the MIDI velocity for a drum hit symbol. Per-instrument
/// `velocity_normal` / `velocity_accent` / `velocity_ghost` overrides take
/// precedence; otherwise the `HitSymbol` defaults apply. A rest yields `None`.
///
/// # 引数 / Arguments
/// * `hit` - ヒットシンボル / Hit symbol
/// * `kit_inst` - 対象 kit インストゥルメント定義 / The kit instrument definition
///
/// # 戻り値 / Returns
/// MIDI velocity (0-127)。休符の場合は `None`。/ MIDI velocity, or `None` for a rest.
fn resolve_drum_velocity(hit: &HitSymbol, kit_inst: &crate::ast::kit::KitInstrument) -> Option<u8> {
    match hit {
        HitSymbol::Normal => Some(kit_inst.velocity_normal.unwrap_or(100)),
        HitSymbol::Accent => Some(kit_inst.velocity_accent.unwrap_or(127)),
        HitSymbol::Ghost => Some(kit_inst.velocity_ghost.unwrap_or(40)),
        HitSymbol::Rest => None,
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

/// CCオートメーション列をMIDI CCイベントにコンパイルする
///
/// `bars` には clip の `[bars N]` 指定があれば値を渡す。step 方式の
/// `>N` / `|` 解決と最終長は `bars` を踏まえて行われる。
///
/// Compile CC automations into MIDI ControlChange events. `bars` carries
/// the clip's `[bars N]` constraint (if any) and drives the final
/// step-length resolution for `>N` / `|` meta tokens.
fn compile_cc_automations(
    automations: &[CcAutomation],
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<Vec<MidiEvent>, EngineError> {
    let mut events = Vec::new();

    for automation in automations {
        match automation {
            CcAutomation::Step(step) => {
                let inst = registry
                    .get_instrument(&step.target.instrument)
                    .ok_or_else(|| {
                        EngineError::UnknownInstrument(step.target.instrument.clone())
                    })?;
                let channel = inst.channel;
                let device = inst.device.clone();
                let cc_number = inst
                    .cc_mappings
                    .iter()
                    .find(|m| m.alias == step.target.param)
                    .map(|m| m.cc_number)
                    .ok_or_else(|| {
                        EngineError::CompileError(format!(
                            "CC mapping '{}' not found in instrument '{}'",
                            step.target.param, step.target.instrument
                        ))
                    })?;

                // ステップ方式: resolutionベースのticks_per_step を使う
                // Step mode: use resolution-based ticks_per_step
                // ステップ方式は仕様上 resolution=16 のドラム解像度を共有
                // デフォルト16分音符
                let ticks_per_step = clock.duration_to_ticks(16, false);
                let bar_ticks = clock.ticks_per_bar();
                // 1 step = 16 分音符。1 小節あたりのセル数 (= steps_per_bar) は
                // bar_ticks / ticks_per_step で求まる。
                // Cells per bar at this resolution.
                let steps_per_bar = (bar_ticks / ticks_per_step.max(1)).max(1) as usize;
                // `|` 拍境界は 4 セル (= 1 拍 = 16 分音符 4 つ) 単位。
                // `|` snaps to a beat boundary; 1 beat == 4 sixteenths.
                let beats_per_step = 4usize;
                // bars 指定があれば total_steps = bars * steps_per_bar、
                // 無ければ None (= 自動 = steps_per_bar の倍数に切り上げ)。
                // total_steps from `bars` if present, else auto rounding.
                let total_steps = bars.map(|n| (n as usize).saturating_mul(steps_per_bar));
                // `|` / `>N` を解決し、Option<u8> の平坦な列に展開する。
                // Resolve `|` then `>N` into a flat `Option<u8>` sequence.
                let piped = crate::parser::cell_normalize::expand_pipe_cells(
                    &step.cells,
                    beats_per_step,
                    &None,
                );
                let resolved = crate::parser::cell_normalize::expand_bar_jump_cells(
                    &piped,
                    steps_per_bar,
                    total_steps,
                    &None,
                )
                .map_err(EngineError::CompileError)?;
                for (i, cell) in resolved.iter().enumerate() {
                    // `None` (= `.`) はこの step では CC を送出しない
                    // `None` cells emit nothing for this step.
                    let Some(value) = *cell else {
                        continue;
                    };
                    events.push(MidiEvent::new(
                        i as u64 * ticks_per_step,
                        MidiMessage::ControlChange {
                            channel,
                            cc: cc_number,
                            value,
                        },
                        &device,
                    ));
                }
            }
            CcAutomation::Time(time) => {
                let inst = registry
                    .get_instrument(&time.target.instrument)
                    .ok_or_else(|| {
                        EngineError::UnknownInstrument(time.target.instrument.clone())
                    })?;
                let channel = inst.channel;
                let device = inst.device.clone();
                let cc_number = inst
                    .cc_mappings
                    .iter()
                    .find(|m| m.alias == time.target.param)
                    .map(|m| m.cc_number)
                    .ok_or_else(|| {
                        EngineError::CompileError(format!(
                            "CC mapping '{}' not found in instrument '{}'",
                            time.target.param, time.target.instrument
                        ))
                    })?;

                let bar_ticks = clock.ticks_per_bar();
                let beat_ticks = bar_ticks / u64::from(clock.time_sig().numerator);

                for segment in &time.segments {
                    let from_tick = (segment.from.bar as u64 - 1) * bar_ticks
                        + (segment.from.beat as u64 - 1) * beat_ticks;

                    events.push(MidiEvent::new(
                        from_tick,
                        MidiMessage::ControlChange {
                            channel,
                            cc: cc_number,
                            value: segment.from.value,
                        },
                        &device,
                    ));

                    // 補間処理
                    // Interpolation processing
                    if let Some((interp, to_point)) = &segment.to {
                        let to_tick = (to_point.bar as u64 - 1) * bar_ticks
                            + (to_point.beat as u64 - 1) * beat_ticks;
                        if to_tick > from_tick {
                            let tick_span = to_tick - from_tick;
                            // 補間ステップ数: 概ね1ステップ = 16分音符相当
                            let step_ticks = clock.duration_to_ticks(16, false);
                            let num_steps = (tick_span / step_ticks).max(1);

                            for s in 1..=num_steps {
                                let t = s as f64 / num_steps as f64;
                                let value = match interp {
                                    Interpolation::None => to_point.value,
                                    Interpolation::Linear => {
                                        let v = segment.from.value as f64
                                            + (to_point.value as f64 - segment.from.value as f64)
                                                * t;
                                        v.round() as u8
                                    }
                                    Interpolation::Exponential => {
                                        // 指数カーブ: t^2 で近似
                                        let t_exp = t * t;
                                        let v = segment.from.value as f64
                                            + (to_point.value as f64 - segment.from.value as f64)
                                                * t_exp;
                                        v.round() as u8
                                    }
                                };
                                let tick = from_tick + s * step_ticks;
                                if tick <= to_tick {
                                    events.push(MidiEvent::new(
                                        tick,
                                        MidiMessage::ControlChange {
                                            channel,
                                            cc: cc_number,
                                            value,
                                        },
                                        &device,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(events)
}

/// ドラムクリップのコンパイル
///
/// 戻り値の `Vec<DrumProbabilityGroup>` は probability 行が指定されており、
/// かつ 100% 未満の確率を持つステップだけを抽選対象として保持する。
/// 100% (= `.`) と確率行未指定のステップは抽選不要なので group には含めない。
/// `u64` は **論理終了 tick** で、最長 row の文字数 × `ticks_per_step` を返す。
/// gate 比率による NoteOff 早期化の影響を受けず、bars 未指定 clip の `total_ticks`
/// を決定するために使われる。
///
/// Returns the MIDI events, probability groups (steps with sub-100% odds),
/// and the musical end tick (= longest row length × `ticks_per_step`),
/// independent of gate-driven NoteOff shortening.
fn compile_drum(
    body: &crate::ast::clip::DrumClipBody,
    clock: &Clock,
    registry: &Registry,
    bars: Option<u32>,
) -> Result<(Vec<MidiEvent>, Vec<DrumProbabilityGroup>, u64), EngineError> {
    let kit = registry
        .get_kit(&body.kit)
        .ok_or_else(|| EngineError::UnknownKit(body.kit.clone()))?;

    let device = kit.device.clone();
    let ticks_per_step = clock.duration_to_ticks(body.resolution, false);

    let mut events = Vec::new();
    let mut groups: Vec<DrumProbabilityGroup> = Vec::new();
    // 最長 row の文字数 × ticks_per_step を論理終了 tick とする。
    // 行が無いクリップは 0 を返す（既存挙動を踏襲）。
    // The longest row's step count × ticks_per_step is the musical end tick.
    let max_steps = body.rows.iter().map(|r| r.hits.len()).max().unwrap_or(0) as u64;
    let logical_end_ticks = max_steps * ticks_per_step;

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

            let velocity = resolve_drum_velocity(hit, kit_inst).unwrap_or(0);
            if velocity == 0 {
                continue;
            }

            let tick = i as u64 * ticks_per_step;
            let gate_ticks = apply_min_gate_off(ticks_per_step, gate_percent, clock);

            // 同一ステップの NoteOn / NoteOff は同じ抽選結果を共有させるため、
            // 後で group に登録する index を覚えておく。
            // Capture the indices of this step's NoteOn/NoteOff so they share
            // a probability roll if the row has a probability mask.
            let note_on_idx = events.len();
            events.push(MidiEvent::new(
                tick,
                MidiMessage::NoteOn {
                    channel,
                    note,
                    velocity,
                },
                &device,
            ));
            let note_off_idx = events.len();
            events.push(MidiEvent::new(
                tick + gate_ticks,
                MidiMessage::NoteOff {
                    channel,
                    note,
                    velocity: 0,
                },
                &device,
            ));

            if let Some(probs) = &row.probability {
                if let Some(p) = probs.get(i).copied() {
                    // `.` は 100 として扱われるため抽選不要。0-99 のみを group 化する。
                    // `.` is encoded as 100 and never gated; only 0-99 needs a group.
                    if p < 100 {
                        groups.push(DrumProbabilityGroup {
                            event_indices: vec![note_on_idx, note_off_idx],
                            probability: p,
                        });
                    }
                }
            }
        }
    }

    // ドラムクリップのCCオートメーションコンパイル
    let cc_events = compile_cc_automations(&body.cc_automations, clock, registry, bars)?;
    events.extend(cc_events);
    Ok((events, groups, logical_end_ticks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipDef, PitchedClipBody, PitchedLine};
    use crate::ast::clip_note::NoteEvent;
    use crate::ast::clip_options::ClipOptions;
    use crate::ast::instrument::InstrumentDef;
    use crate::ast::kit::{KitDef, KitInstrument, KitInstrumentNote};
    use crate::domain::pitch::NoteName;

    fn make_registry_with_bass() -> Registry {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "bass".to_string(),
            device: "dev".to_string(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
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
                octave_shift: 0,
            },
            body: ClipBody::Pitched(PitchedClipBody {
                lines,
                cc_automations: vec![],
            }),
        }
    }

    /// オクターブシフト付きのピッチド clip を作るテスト用ヘルパー。
    /// Build a pitched clip with an octave shift, for tests.
    fn make_pitched_clip_with_shift(
        name: &str,
        octave_shift: i8,
        lines: Vec<PitchedLine>,
    ) -> ClipDef {
        ClipDef {
            name: name.to_string(),
            options: ClipOptions {
                bars: None,
                time_sig: None,
                scale: None,
                octave_shift,
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
            None,
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
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.events.len(), 2);
        assert_eq!(compiled.events[0].tick, 0);
        if let MidiMessage::NoteOn {
            channel,
            note,
            velocity,
        } = compiled.events[0].message
        {
            assert_eq!(channel, MidiChannel::from_zero_based(0).unwrap());
            assert_eq!(note, 60);
            assert_eq!(velocity, 100);
        } else {
            panic!("expected NoteOn");
        }
        // gate_normal=80%, 480ticks * 80% = 384ticks
        assert_eq!(compiled.events[1].tick, 384);
        if let MidiMessage::NoteOff { channel, note, .. } = compiled.events[1].message {
            assert_eq!(channel, MidiChannel::from_zero_based(0).unwrap());
            assert_eq!(note, 60);
        } else {
            panic!("expected NoteOff");
        }
    }

    #[test]
    fn octave_shift_up_raises_note_by_12() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [>>] = +1 オクターブ。C4(60) -> C5(72)
        let clip = make_pitched_clip_with_shift(
            "test",
            1,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // NoteOn / NoteOff いずれも +12 されている
        for e in &compiled.events {
            match e.message {
                MidiMessage::NoteOn { note, .. } => assert_eq!(note, 72),
                MidiMessage::NoteOff { note, .. } => assert_eq!(note, 72),
                _ => {}
            }
        }
    }

    #[test]
    fn octave_shift_down_lowers_note_by_12() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [<<] = -1 オクターブ。C4(60) -> C3(48)
        let clip = make_pitched_clip_with_shift(
            "test",
            -1,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        if let MidiMessage::NoteOn { note, .. } = compiled.events[0].message {
            assert_eq!(note, 48);
        } else {
            panic!("expected NoteOn");
        }
    }

    #[test]
    fn octave_shift_up_two_raises_note_by_24() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [>> >>] = +2 オクターブ。C4(60) -> C6(84)
        let clip = make_pitched_clip_with_shift(
            "test",
            2,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        if let MidiMessage::NoteOn { note, .. } = compiled.events[0].message {
            assert_eq!(note, 84);
        } else {
            panic!("expected NoteOn");
        }
    }

    #[test]
    fn octave_shift_zero_is_noop() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip_with_shift(
            "test",
            0,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        if let MidiMessage::NoteOn { note, .. } = compiled.events[0].message {
            assert_eq!(note, 60);
        } else {
            panic!("expected NoteOn");
        }
    }

    #[test]
    fn octave_shift_out_of_range_is_compile_error() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // C9(120) を +1 オクターブすると 132 となり 127 を超えるためエラー。
        let clip = make_pitched_clip_with_shift(
            "test",
            1,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(9), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(matches!(result, Err(EngineError::CompileError(_))));
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
                is_layer_start: true,
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
                        None,
                    ),
                    single_note(NoteName::C, Some(4), Some(4), false),
                ],
                is_layer_start: true,
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
                    None,
                )],
                is_layer_start: true,
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
                    None,
                )],
                is_layer_start: true,
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
                is_layer_start: true,
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

    /// bars 未指定の clip では、total_ticks は最後のノートの **音価終了 tick**
    /// (gate 早期化を含まない) であるべき。
    /// gate 80% で NoteOff が早期化されても total_ticks は影響を受けない。
    ///
    /// When `bars` is unspecified, `total_ticks` must equal the last note's
    /// musical-duration end (not the last NoteOff tick + 1). This way a clip
    /// of `c:4:4 d e f` (= one bar) ends at exactly 1920 ticks regardless of
    /// gate ratio (which only affects NoteOff position, not musical length).
    #[test]
    fn bars_unspecified_total_ticks_is_logical_end() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // 4分音符 × 4 = 1920 ticks (= 1 小節 @120BPM/PPQ480)
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    single_note(NoteName::D, None, None, false),
                    single_note(NoteName::E, None, None, false),
                    single_note(NoteName::F, None, None, false),
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // gate 80% でも total_ticks は音価ベース = 1920 であるべき
        assert_eq!(
            compiled.total_ticks, 1920,
            "total_ticks should be musical duration end (1920), not gate-affected NoteOff+1"
        );
    }

    /// bars 未指定 + 複数 line（ポリリズム想定）では、各 line の最終音価終了の最大値を採用する。
    /// With multiple lines and no bars, total_ticks must be the max of each
    /// line's musical end.
    #[test]
    fn bars_unspecified_multi_line_takes_max_logical_end() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // line1: 4分音符1個 = 480 ticks
        // line2: 4分音符4個 = 1920 ticks
        // → total_ticks = 1920
        let clip = make_pitched_clip(
            "test",
            None,
            vec![
                PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                },
                PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![
                        single_note(NoteName::E, Some(4), Some(4), false),
                        single_note(NoteName::G, None, None, false),
                        single_note(NoteName::B, None, None, false),
                        single_note(NoteName::D, None, None, false),
                    ],
                    is_layer_start: true,
                },
            ],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 1920);
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
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 3840);
        // bars未超過時はワーニングなし
        assert!(compiled.warnings.is_empty());
    }

    /// bars 超過時、Note On が clip 内で対応する Note Off が clip 外のペアは、
    /// Note Off の tick を max_ticks - 1 にクランプして clip 内に残す。
    /// 外部 MIDI 機器のハングノート (CV 開きっぱなし) を防ぐための挙動。
    ///
    /// When a NoteOn falls inside the clip but its NoteOff would be after
    /// `max_ticks`, the NoteOff is clamped to `max_ticks - 1` so the played
    /// note is always closed. Prevents stuck notes on external MIDI gear.
    #[test]
    fn bars_overflow_clamps_note_off_when_note_on_inside_clip() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bars=1 (= 1920 tick). 4分 + 2分 + 2分 = 5 拍 = 2400 tick で超過する。
        // C : On=0,    Off=384  (gate80% × 480 = 384)  - clip 内
        // D : On=480,  Off=1248 (480 + 960*0.8)        - clip 内
        // E : On=1440, Off=2208 (1440 + 960*0.8)       - On 内 / Off 外 → クランプ対象
        let clip = make_pitched_clip(
            "test",
            Some(1),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    single_note(NoteName::D, None, Some(2), false),
                    single_note(NoteName::E, None, Some(2), false),
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 1920);

        // E (note 64) の NoteOn / NoteOff が共に残っていることを確認
        let e_on = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOn { note: 64, .. }))
            .expect("E NoteOn は clip 内なので残るはず");
        assert_eq!(e_on.tick, 1440);

        let e_off = compiled
            .events
            .iter()
            .find(|e| matches!(e.message, MidiMessage::NoteOff { note: 64, .. }))
            .expect("E NoteOff はクランプされて残るはず");
        assert_eq!(
            e_off.tick, 1919,
            "NoteOff は max_ticks - 1 にクランプされる"
        );

        // クランプを示す warning が出る
        assert!(
            compiled.warnings.iter().any(|w| w.contains("クランプ")),
            "クランプ警告が含まれるべき: {:?}",
            compiled.warnings
        );
    }

    /// bars 超過時、Note On 自体が clip 外のペアは丸ごと切り捨てる。
    /// 「鳴らさない音は鳴らさないまま」が原則。
    ///
    /// When a NoteOn falls outside `max_ticks`, both NoteOn and its NoteOff
    /// are dropped — the note never sounds in the first place.
    #[test]
    fn bars_overflow_drops_pair_when_note_on_outside_clip() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bars=1 (= 1920 tick). 全音符 + 全音符 で 2 小節分。
        // C : On=0,    Off=1536 - clip 内
        // D : On=1920, Off=3456 - On も Off も clip 外 → ペアごと削除
        let clip = make_pitched_clip(
            "test",
            Some(1),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(1), false),
                    single_note(NoteName::D, None, None, false),
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert_eq!(compiled.total_ticks, 1920);

        let d_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e.message,
                    MidiMessage::NoteOn { note: 62, .. } | MidiMessage::NoteOff { note: 62, .. }
                )
            })
            .collect();
        assert!(
            d_events.is_empty(),
            "D の NoteOn / NoteOff は両方削除される"
        );

        // 切り捨てを示す warning が出る
        assert!(
            compiled.warnings.iter().any(|w| w.contains("切り捨て")),
            "切り捨て警告が含まれるべき: {:?}",
            compiled.warnings
        );
    }

    /// クランプ対象と切り捨て対象が混在する場合、warning が 2 種類とも出る。
    /// When both clamp and drop happen, both warnings are emitted separately.
    #[test]
    fn bars_overflow_emits_both_warnings_when_mixed() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bars=1 (= 1920 tick).
        // C 4分:  On=0,    Off=384  - 内
        // D 2分:  On=480,  Off=1248 - 内
        // E 2分:  On=1440, Off=2208 - On 内 / Off 外 → クランプ
        // F 4分:  On=2400, Off=2784 - On も Off も外 → 削除
        let clip = make_pitched_clip(
            "test",
            Some(1),
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(4), Some(4), false),
                    single_note(NoteName::D, None, Some(2), false),
                    single_note(NoteName::E, None, Some(2), false),
                    single_note(NoteName::F, None, Some(4), false),
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();

        let clamp_warn = compiled.warnings.iter().find(|w| w.contains("クランプ"));
        let drop_warn = compiled.warnings.iter().find(|w| w.contains("切り捨て"));
        assert!(
            clamp_warn.is_some() && drop_warn.is_some(),
            "クランプと切り捨ての warning が両方含まれるべき: {:?}",
            compiled.warnings
        );
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
                is_layer_start: true,
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
                is_layer_start: true,
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
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: Some(20),
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
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
                octave_shift: 0,
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
        if let MidiMessage::NoteOn {
            channel,
            note,
            velocity,
        } = compiled.events[0].message
        {
            assert_eq!(channel, MidiChannel::from_zero_based(9).unwrap());
            assert_eq!(note, 36);
            assert_eq!(velocity, 100);
        } else {
            panic!("expected NoteOn");
        }
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
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::D,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
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

    /// kit インストゥルメントに velocity_normal/accent/ghost を設定すると、
    /// HitSymbol の既定 velocity を上書きすること（ドラム）。
    /// Per-instrument velocity_normal/accent/ghost override the HitSymbol defaults (drums).
    #[test]
    fn drum_velocity_overrides_hit_symbol_defaults() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "kit".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "sn".to_string(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::D,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                velocity_normal: Some(90),
                velocity_accent: Some(120),
                velocity_ghost: Some(30),
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
                    hits: vec![HitSymbol::Normal, HitSymbol::Accent, HitSymbol::Ghost],
                    probability: None,
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // Normal=90, Accent=120, Ghost=30 の NoteOn がそれぞれ存在すること
        for expected in [90u8, 120, 30] {
            assert!(
                compiled.events.iter().any(|e| matches!(
                    e.message,
                    MidiMessage::NoteOn { velocity, .. } if velocity == expected
                )),
                "velocity {} の NoteOn が見つからない",
                expected
            );
        }
    }

    /// instrument に velocity_normal を設定すると、`vN` 未指定ノートの
    /// デフォルト velocity がその値になること（音程楽器）。
    /// velocity_normal sets the default velocity for pitched notes without a `vN` suffix.
    #[test]
    fn pitched_velocity_normal_sets_default() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "bass".to_string(),
            device: "dev".to_string(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: Some(70),
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));

        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // velocity_override 未指定なので velocity_normal=70 が使われる
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn { velocity: 70, .. }
        ));
    }

    /// instrument の velocity_normal 未設定時は従来どおり 100 がデフォルトになること。
    /// When velocity_normal is unset, the legacy default of 100 still applies.
    #[test]
    fn pitched_velocity_normal_defaults_to_100() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn { velocity: 100, .. }
        ));
    }

    /// `vN` 明示指定は velocity_normal より優先されること（音程楽器）。
    /// An explicit `vN` suffix takes precedence over velocity_normal.
    #[test]
    fn pitched_vn_overrides_velocity_normal() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "bass".to_string(),
            device: "dev".to_string(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: Some(70),
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));

        let clock = Clock::new(120.0);
        // velocity_override = Some(127)（`c4v127` 相当）
        let note = PitchedElement::Note(
            NoteEvent::Single {
                name: NoteName::C,
                octave: Some(4),
                duration: Some(4),
                dotted: false,
            },
            Articulation::Normal,
            Some(127),
        );
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![note],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(matches!(
            compiled.events[0].message,
            MidiMessage::NoteOn { velocity: 127, .. }
        ));
    }

    /// 確率行を含むドラムクリップは drum_mask_groups を生成する
    /// Drum clips with a probability row produce drum_mask_groups.
    #[test]
    fn compile_drum_builds_probability_groups() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "kit".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "hh".to_string(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::Fs,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "drums".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Drum(crate::ast::clip::DrumClipBody {
                kit: "kit".to_string(),
                resolution: 16,
                rows: vec![crate::ast::clip_drum::DrumRow {
                    instrument: "hh".to_string(),
                    // step0: `.` (=100), step1: `5` (=50), step2: `0` (=0), step3: `9` (=90)
                    hits: vec![
                        HitSymbol::Normal,
                        HitSymbol::Normal,
                        HitSymbol::Normal,
                        HitSymbol::Normal,
                    ],
                    probability: Some(vec![100, 50, 0, 90]),
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // 4 hits → 8 events (NoteOn + NoteOff each)
        assert_eq!(compiled.events.len(), 8);
        // 100% step is excluded, so 3 groups
        assert_eq!(compiled.drum_mask_groups.len(), 3);

        let probs: Vec<u8> = compiled
            .drum_mask_groups
            .iter()
            .map(|g| g.probability)
            .collect();
        assert_eq!(probs, vec![50, 0, 90]);

        // Each group must reference exactly two events (NoteOn + NoteOff)
        for g in &compiled.drum_mask_groups {
            assert_eq!(g.event_indices.len(), 2);
        }
    }

    /// probability 行が無いドラムクリップは drum_mask_groups が空のまま
    /// Drum clips without a probability row produce no probability groups.
    #[test]
    fn compile_drum_no_probability_no_groups() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "kit".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "bd".to_string(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "drums".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Drum(crate::ast::clip::DrumClipBody {
                kit: "kit".to_string(),
                resolution: 16,
                rows: vec![crate::ast::clip_drum::DrumRow {
                    instrument: "bd".to_string(),
                    hits: vec![HitSymbol::Normal, HitSymbol::Normal],
                    probability: None,
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(compiled.drum_mask_groups.is_empty());
    }

    #[test]
    fn events_sorted_note_on_before_off() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "pad".to_string(),
            device: "dev".to_string(),
            channel: MidiChannel::from_one_based(3).unwrap(),
            note: None,
            gate_normal: Some(100),
            gate_staccato: Some(60),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
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
                    is_layer_start: true,
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
                    PitchedElement::BarJump(crate::ast::clip_bar_jump::BarJump { bar_number: 3 }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
                is_layer_start: true,
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
                    PitchedElement::BarJump(crate::ast::clip_bar_jump::BarJump { bar_number: 5 }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
                is_layer_start: true,
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
                    PitchedElement::BarJump(crate::ast::clip_bar_jump::BarJump { bar_number: 4 }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
                is_layer_start: true,
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
                    PitchedElement::BarJump(crate::ast::clip_bar_jump::BarJump { bar_number: 100 }),
                    single_note(NoteName::E, None, Some(4), false),
                ],
                is_layer_start: true,
            }],
        );

        let result = compile_clip(&clip, &clock, &registry);
        assert!(result.is_ok());
    }

    // ============================================================
    // PipeSnap (`|` 拍境界スナップ) のテスト
    // PipeSnap (`|` beat-boundary snap) tests.
    // ============================================================

    /// 拍ぴったり (= 8分2個) の後に `|` が来てもイベントに変化が無い。
    /// `|` after exactly one beat is a no-op.
    #[test]
    fn pipe_snap_exact_beat_is_noop() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bass c:3:8 c | → 8 分 × 2 = 1 拍 = 480 tick ピッタリ
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    single_note(NoteName::C, None, None, false),
                    PitchedElement::PipeSnap,
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // 2 つのノートが残り、 truncate されない
        assert_eq!(note_ons.len(), 2);
        assert_eq!(note_ons[0].tick, 0);
        assert_eq!(note_ons[1].tick, 240);
    }

    /// 不足ケース (= 8分1個 = 半拍) の後の `|` で次拍境界まで進む。
    /// `|` after only half a beat pads to the next beat boundary.
    #[test]
    fn pipe_snap_pads_short_to_next_beat() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bass c:3:8 | c → 1 個目 8 分 (240 tick) の後 `|` で 480 まで進める
        //   → 2 個目 c のオンセットは tick 480
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    PitchedElement::PipeSnap,
                    single_note(NoteName::C, None, None, false),
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 2);
        assert_eq!(note_ons[0].tick, 0);
        // `|` で次拍境界 (480) に揃える
        assert_eq!(note_ons[1].tick, 480);
    }

    /// 超過ケース (= 8分5個 = 1.25 拍) の後の `|` で前拍境界まで戻り、
    /// 5 個目の音を削除する。
    /// `|` after 5 eighth notes (1.25 beats) truncates the 5th note.
    #[test]
    fn pipe_snap_truncates_overrun_to_previous_beat() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bass c:3:8 c c c c c | → 5 個 (= 1200 tick) → 拍境界 (= 480) に戻して
        //   音 1-4 だけ残す (= 4 個)
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    PitchedElement::PipeSnap,
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // 5 個目はカットされて 4 個残る
        // 5th note is truncated, 4 notes remain
        assert_eq!(note_ons.len(), 4);
    }

    /// 上のテストの正確な期待値を確認するための tick 並び。
    /// (テストファースト用に separate な確認テスト)
    /// Verifies exact tick ordering after pipe-snap truncate.
    #[test]
    fn pipe_snap_truncate_keeps_first_beat_notes() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    PitchedElement::PipeSnap,
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let mut note_on_ticks: Vec<u64> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .map(|e| e.tick)
            .collect();
        note_on_ticks.sort();
        // 0, 240, 480, 720 が残り (1.5 拍まで)、ではなく
        // 0, 240 (1拍 = 480 ticks), then... ?
        //
        // 8 分音符 (= 240 tick) × 4 個 = 960 tick = 2 拍。 これは拍境界。
        // 5 個目 (tick 960) → ticks_since_pipe = 1200 > 480。 直前拍境界 = 1200 / 480 * 480 = 960。
        // 5 個目のオンセットが 960 だが、これは 960 >= 960 なので削除対象。
        // 結果、 0, 240, 480, 720 の 4 個。
        assert_eq!(note_on_ticks, vec![0, 240, 480, 720]);
    }

    /// 複数の `|` が連続するときに、各セグメントが独立に評価される。
    /// Multiple `|` segments are evaluated independently.
    #[test]
    fn pipe_snap_multiple_segments_are_independent() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // bass c:3:8 | c c c c c | → 前半: 不足 1個 → 480 まで埋め
        //  後半: 5 個 (1200 tick) → 拍境界 480 に切り落とし → 後半は 4 個
        // 合計 NoteOn = 1 + 4 = 5
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    PitchedElement::PipeSnap,
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    single_note(NoteName::C, None, None, false),
                    PitchedElement::PipeSnap,
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let mut note_on_ticks: Vec<u64> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .map(|e| e.tick)
            .collect();
        note_on_ticks.sort();
        // 1 個目: tick 0
        // `|` で 480 まで pad
        // 2-5 個目: tick 480, 720, 960, 1200 (5 個目までは pad しない)
        // 6 個目: tick 1440 (= 480 + 240*4 = 1440, これは 480+960 = 1440, 拍境界では無い)
        //   → ticks_since_pipe2 = 1440 - 480 = 960 (= 2 拍)。 5個入れたら 1200。
        // 待った: 5 個入れたら累積 ticks_since_pipe = 5 * 240 = 1200。 1200 > 480 なので truncate。
        // 直前拍境界 = 1200 / 480 * 480 = 960。
        // 5 個目 (= 6 番目のグローバルノート) は anchor_tick + 960 = 480 + 960 = 1440 がオンセット。
        // この 5 個目を削る (= 4 個残る)。
        // 結果: 1 + 4 = 5 NoteOn
        assert_eq!(note_on_ticks, vec![0, 480, 720, 960, 1200]);
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
                    crate::ast::clip_repetition::Repetition {
                        content: "c:3:8 c eb".to_string(),
                        count: 4,
                    },
                )],
                is_layer_start: true,
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
                    crate::ast::clip_repetition::Repetition {
                        content: "c:3:8".to_string(),
                        count: 2,
                    },
                )],
                is_layer_start: true,
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

    use crate::domain::chord::ChordSuffix;

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
                        arpeggio: None,
                    },
                    Articulation::Normal,
                    None,
                )],
                is_layer_start: true,
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
                            arpeggio: None,
                        },
                        Articulation::Normal,
                        None,
                    ),
                    single_note(NoteName::E, None, None, false),
                ],
                is_layer_start: true,
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
                        arpeggio: None,
                    },
                    Articulation::Staccato,
                    None,
                )],
                is_layer_start: true,
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
                    crate::ast::clip_repetition::Repetition {
                        content: "cm7:4:4".to_string(),
                        count: 2,
                    },
                )],
                is_layer_start: true,
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
                    velocity: None,
                }],
                is_layer_start: true,
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
                    velocity: None,
                }],
                is_layer_start: true,
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
                        velocity: None,
                    },
                    single_note(NoteName::G, None, None, false),
                ],
                is_layer_start: true,
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
                    velocity: None,
                }],
                is_layer_start: true,
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

    // --- ChordBracket アルペジオ テスト ---

    /// `arp(up, 16)` で構成音が音高昇順に16分音符間隔で発音されること。
    /// `arp(up, 16)` should sequence chord tones in ascending pitch order at 16th-note intervals.
    #[test]
    fn chord_bracket_arpeggio_up_uses_resolution() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [c:4 eb:4 g:4 bb:4]:1 arp(up, 16)
        // 1音=16分音符=120ticks、4音 → 計480ticks、duration(:1)は無視されること
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
                    duration: Some(1),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: Some(Arpeggio {
                        direction: ArpeggioDirection::Up,
                        resolution: Some(16),
                    }),
                    velocity: None,
                }],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // 4音が順次（同時ではなく）発音される
        assert_eq!(note_ons.len(), 4);
        let ticks: Vec<u64> = note_ons.iter().map(|e| e.tick).collect();
        assert_eq!(ticks, vec![0, 120, 240, 360]);
        let notes: Vec<u8> = note_ons
            .iter()
            .map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => note,
                _ => unreachable!(),
            })
            .collect();
        // C4=60, Eb4=63, G4=67, Bb4=70 (昇順)
        assert_eq!(notes, vec![60, 63, 67, 70]);
    }

    /// `arp(down, 16)` で構成音が音高降順に発音されること。
    /// `arp(down, 16)` should sequence chord tones in descending pitch order.
    #[test]
    fn chord_bracket_arpeggio_down() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
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
                    duration: Some(1),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: Some(Arpeggio {
                        direction: ArpeggioDirection::Down,
                        resolution: Some(16),
                    }),
                    velocity: None,
                }],
                is_layer_start: true,
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
        // G4=67, E4=64, C4=60 (降順)
        assert_eq!(notes, vec![67, 64, 60]);
    }

    /// `arp(updown, 16)` は up→down の往復で、両端を二度鳴らさない（ping-pong）。
    /// `arp(updown, 16)` ping-pongs without repeating the endpoints.
    #[test]
    fn chord_bracket_arpeggio_updown_pingpong() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // 3音 [c, e, g] の updown は c, e, g, e の 4 ステップ（c と g は端点なので二度鳴らさない）
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
                    duration: Some(1),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: Some(Arpeggio {
                        direction: ArpeggioDirection::UpDown,
                        resolution: Some(16),
                    }),
                    velocity: None,
                }],
                is_layer_start: true,
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
        // C4=60, E4=64, G4=67, E4=64
        assert_eq!(notes, vec![60, 64, 67, 64]);
    }

    /// resolution 省略時は和音 duration を1音あたりの長さとして採用する。
    /// When resolution is omitted, the chord duration is used as the per-step length.
    #[test]
    fn chord_bracket_arpeggio_falls_back_to_duration() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // [c:4 e:4 g:4]:8 arp(up) → 各音=8分音符=240ticks
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
                    duration: Some(8),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: Some(Arpeggio {
                        direction: ArpeggioDirection::Up,
                        resolution: None,
                    }),
                    velocity: None,
                }],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        let ticks: Vec<u64> = note_ons.iter().map(|e| e.tick).collect();
        assert_eq!(ticks, vec![0, 240, 480]);
    }

    /// resolution / duration が共に明示指定無しでも carry-over された duration を採用。
    /// (LSP では明示記述レベルで両方無しを警告として扱うが、コンパイラは carry-over を尊重する)
    /// When both resolution and duration are unspecified, the compiler still
    /// compiles using the carry-over duration. (LSP separately warns when the
    /// source omits both — handled at the diagnostics layer.)
    #[test]
    fn chord_bracket_arpeggio_uses_carry_over_when_unspecified() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // 前のノートで duration=8 を確定させ、その後 ChordBracket は両方未指定 → 8 を引き継ぐ
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![
                    single_note(NoteName::C, Some(3), Some(8), false),
                    PitchedElement::ChordBracket {
                        notes: vec![(NoteName::C, Some(4)), (NoteName::E, None)],
                        duration: None,
                        dotted: false,
                        articulation: Articulation::Normal,
                        arpeggio: Some(Arpeggio {
                            direction: ArpeggioDirection::Up,
                            resolution: None,
                        }),
                        velocity: None,
                    },
                ],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // 1つ目: 単音 C3@tick0 (8分音符=240ticks)
        // 2つ目以降: アルペジオ C4@240, E4@480 (各8分音符=240ticks)
        assert_eq!(note_ons.len(), 3);
        let ticks: Vec<u64> = note_ons.iter().map(|e| e.tick).collect();
        assert_eq!(ticks, vec![0, 240, 480]);
    }

    /// `arp(random, ...)` は各ステップに全候補を重ねて emit し、
    /// random_choice_groups を構成音数分作成する。
    /// `arp(random, ...)` lays all candidates per step and emits one
    /// random_choice_group per step.
    #[test]
    fn chord_bracket_arpeggio_random_emits_choice_groups() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        // 3音 [c, e, g]:1 arp(random, 8) → 各ステップ=8分音符=240ticks
        // ステップ数=3、各ステップに3候補 (NoteOn+NoteOff = 6 events) を重ねる
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
                    duration: Some(1),
                    dotted: false,
                    articulation: Articulation::Normal,
                    arpeggio: Some(Arpeggio {
                        direction: ArpeggioDirection::Random,
                        resolution: Some(8),
                    }),
                    velocity: None,
                }],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // 構成音 3 × ステップ数 3 = 9 NoteOn (ただし候補として並ぶ)
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        assert_eq!(note_ons.len(), 9);
        // 3 ステップそれぞれで group が 1 つ、各 group は 3 候補
        assert_eq!(compiled.random_choice_groups.len(), 3);
        for g in &compiled.random_choice_groups {
            assert_eq!(g.candidates.len(), 3);
            // 各候補は NoteOn/NoteOff の 2 件
            for c in &g.candidates {
                assert_eq!(c.len(), 2);
            }
        }
    }

    // --- ChordName + arp テスト ---

    /// `cm:4:1 arp(up, 8)` でコード構成音 [C4, Eb4, G4] が昇順に8分音符間隔で発音される。
    /// `cm:4:1 arp(up, 8)` should arpeggiate chord tones [C4, Eb4, G4] ascending.
    #[test]
    fn chord_name_arpeggio_up_uses_resolution() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
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
                        suffix: ChordSuffix::Min,
                        octave: Some(4),
                        duration: Some(1),
                        dotted: false,
                        arpeggio: Some(Arpeggio {
                            direction: ArpeggioDirection::Up,
                            resolution: Some(8),
                        }),
                    },
                    Articulation::Normal,
                    None,
                )],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // cm = [C4=60, Eb4=63, G4=67] が 8分音符=240ticks 間隔で発音される
        assert_eq!(note_ons.len(), 3);
        let ticks: Vec<u64> = note_ons.iter().map(|e| e.tick).collect();
        assert_eq!(ticks, vec![0, 240, 480]);
        let notes: Vec<u8> = note_ons
            .iter()
            .map(|e| match e.message {
                MidiMessage::NoteOn { note, .. } => note,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(notes, vec![60, 63, 67]);
    }

    /// `cm arp(random, 4)` で ChordName からも RandomChoiceGroup が生成されること。
    /// `cm arp(random, 4)` should produce RandomChoiceGroups for chord tones.
    #[test]
    fn chord_name_arpeggio_random_emits_choice_groups() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
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
                        suffix: ChordSuffix::Min,
                        octave: Some(4),
                        duration: Some(1),
                        dotted: false,
                        arpeggio: Some(Arpeggio {
                            direction: ArpeggioDirection::Random,
                            resolution: Some(4),
                        }),
                    },
                    Articulation::Normal,
                    None,
                )],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        // cm = 3 構成音 → 3 ステップ × 各 3 候補（NoteOn+NoteOff）
        assert_eq!(compiled.random_choice_groups.len(), 3);
        for g in &compiled.random_choice_groups {
            assert_eq!(g.candidates.len(), 3);
            for c in &g.candidates {
                assert_eq!(c.len(), 2);
            }
        }
    }

    /// `cm:4:8 arp(up)`（resolution 省略）は和音 duration を1音あたりの長さに採用する。
    /// `cm:4:8 arp(up)` falls back to the chord duration as per-step length.
    #[test]
    fn chord_name_arpeggio_falls_back_to_duration() {
        use crate::ast::clip_arpeggio::{Arpeggio, ArpeggioDirection};
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
                        duration: Some(8),
                        dotted: false,
                        arpeggio: Some(Arpeggio {
                            direction: ArpeggioDirection::Up,
                            resolution: None,
                        }),
                    },
                    Articulation::Normal,
                    None,
                )],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let note_ons: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::NoteOn { .. }))
            .collect();
        // C major = [C4, E4, G4]、各音=8分音符=240ticks
        let ticks: Vec<u64> = note_ons.iter().map(|e| e.tick).collect();
        assert_eq!(ticks, vec![0, 240, 480]);
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
            channel: MidiChannel::from_one_based(3).unwrap(),
            note: None,
            gate_normal: Some(100),
            gate_staccato: Some(60),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
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
                is_layer_start: true,
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

    // --- CCオートメーション テスト ---

    use crate::ast::clip_cc::{
        CcAutomation, CcStepValues, CcTarget, CcTimePoint, CcTimeSegment, CcTimeValues,
    };
    use crate::ast::instrument::CcMapping;

    /// cc_mappings付きのregistryを作成するヘルパー
    fn make_registry_with_bass_cc() -> Registry {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "bass".to_string(),
            device: "dev".to_string(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![CcMapping {
                alias: "cutoff".to_string(),
                cc_number: 74,
                cc_number_ref: None,
            }],
            local_vars: vec![],
            unresolved: Default::default(),
        }));
        registry
    }

    /// ステップ方式のCCオートメーションがMIDI CCイベントに変換されることを検証
    /// Verify step-mode CC automation produces MIDI CC events
    #[test]
    fn cc_automation_step_basic() {
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    cells: vec![
                        crate::parser::cell_normalize::CellToken::Cell(Some(0)),
                        crate::parser::cell_normalize::CellToken::Cell(Some(32)),
                        crate::parser::cell_normalize::CellToken::Cell(Some(64)),
                        crate::parser::cell_normalize::CellToken::Cell(Some(127)),
                    ],
                })],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        assert_eq!(cc_events.len(), 4);

        // 最初のCC: tick=0, cc=74, value=0
        if let MidiMessage::ControlChange { channel, cc, value } = cc_events[0].message {
            assert_eq!(channel, MidiChannel::from_zero_based(0).unwrap());
            assert_eq!(cc, 74);
            assert_eq!(value, 0);
        } else {
            panic!("expected ControlChange");
        }
        // 最後のCC: tick=360(16分音符*3=120*3), cc=74, value=127
        if let MidiMessage::ControlChange { channel, cc, value } = cc_events[3].message {
            assert_eq!(channel, MidiChannel::from_zero_based(0).unwrap());
            assert_eq!(cc, 74);
            assert_eq!(value, 127);
        } else {
            panic!("expected ControlChange");
        }
    }

    /// 時間指定方式のCCオートメーション（ポイント指定のみ）
    /// Time-specified CC automation with point values only
    #[test]
    fn cc_automation_time_point() {
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Time(CcTimeValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    segments: vec![
                        CcTimeSegment {
                            from: CcTimePoint {
                                value: 0,
                                bar: 1,
                                beat: 1,
                            },
                            to: None,
                        },
                        CcTimeSegment {
                            from: CcTimePoint {
                                value: 127,
                                bar: 2,
                                beat: 1,
                            },
                            to: None,
                        },
                    ],
                })],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        assert_eq!(cc_events.len(), 2);
        // 1:1 → tick=0, value=0
        assert_eq!(cc_events[0].tick, 0);
        assert!(matches!(
            cc_events[0].message,
            MidiMessage::ControlChange { value: 0, .. }
        ));
        // 2:1 → tick=1920 (1小節=1920 at 4/4 120BPM), value=127
        assert_eq!(cc_events[1].tick, 1920);
        assert!(matches!(
            cc_events[1].message,
            MidiMessage::ControlChange { value: 127, .. }
        ));
    }

    /// ドラムクリップでのCCオートメーション
    /// CC automation in drum clip
    #[test]
    fn cc_automation_in_drum_clip() {
        let mut registry = make_registry_with_bass_cc();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "kit".to_string(),
            device: "dev".to_string(),
            instruments: vec![KitInstrument {
                name: "bd".to_string(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: Some(50),
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "drums".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Drum(crate::ast::clip::DrumClipBody {
                kit: "kit".to_string(),
                resolution: 16,
                rows: vec![crate::ast::clip_drum::DrumRow {
                    instrument: "bd".to_string(),
                    hits: vec![HitSymbol::Normal, HitSymbol::Rest],
                    probability: None,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    cells: vec![
                        crate::parser::cell_normalize::CellToken::Cell(Some(64)),
                        crate::parser::cell_normalize::CellToken::Cell(Some(127)),
                    ],
                })],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        assert_eq!(cc_events.len(), 2);
        if let MidiMessage::ControlChange { channel, cc, value } = cc_events[0].message {
            assert_eq!(channel, MidiChannel::from_zero_based(0).unwrap());
            assert_eq!(cc, 74);
            assert_eq!(value, 64);
        } else {
            panic!("expected ControlChange");
        }
    }

    // ---------------------------------------------------------------------
    // CC step メタトークン (`.` `|` `>N`) 統合テスト
    // CC-step meta-token integration tests.
    // ---------------------------------------------------------------------

    /// `.` (None) セルは MIDI イベントを生成しない
    /// `None` cells produce no MIDI event.
    #[test]
    fn cc_step_dot_skips_emission() {
        use crate::parser::cell_normalize::CellToken;
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    // 4 step: Some(0), None, Some(64), None
                    cells: vec![
                        CellToken::Cell(Some(0)),
                        CellToken::Cell(None),
                        CellToken::Cell(Some(64)),
                        CellToken::Cell(None),
                    ],
                })],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        // None 2 つはスキップされ、CC イベントは 2 つだけ
        assert_eq!(cc_events.len(), 2);
        // 1 つ目は step 0 (tick 0) の 0、2 つ目は step 2 (tick = 2 * 16分音符)
        let ticks_per_step = clock.duration_to_ticks(16, false);
        assert_eq!(cc_events[0].tick, 0);
        assert_eq!(cc_events[1].tick, 2 * ticks_per_step);
        if let MidiMessage::ControlChange { value, .. } = cc_events[1].message {
            assert_eq!(value, 64);
        } else {
            panic!("expected ControlChange");
        }
    }

    /// `|` で 1 拍 (= 4 step) 境界に揃える: 不足は `.` で埋め
    /// `|` snaps to a beat boundary (= 4 steps); shorts are padded.
    #[test]
    fn cc_step_pipe_pads_to_beat_boundary() {
        use crate::parser::cell_normalize::CellToken;
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        // 2 step + | (拍 = 4 step) → padding 2 step → 計 4 step
        // 続けて 0, 127 を追加して、bar 1 の 5,6 step 目になる
        let cells = vec![
            CellToken::Cell(Some(10)),
            CellToken::Cell(Some(20)),
            CellToken::Pipe,
            CellToken::Cell(Some(64)),
            CellToken::Cell(Some(127)),
        ];
        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    cells,
                })],
            }),
        };
        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        // padding は None なので発火しない: 4 イベント (step 0, 1, 4, 5)
        assert_eq!(cc_events.len(), 4);
        let ticks_per_step = clock.duration_to_ticks(16, false);
        assert_eq!(cc_events[0].tick, 0);
        assert_eq!(cc_events[1].tick, ticks_per_step);
        assert_eq!(cc_events[2].tick, 4 * ticks_per_step);
        assert_eq!(cc_events[3].tick, 5 * ticks_per_step);
    }

    /// `|` 超過時は前境界まで切り落とし
    /// `|` truncates overruns back to the previous beat boundary.
    #[test]
    fn cc_step_pipe_truncates_overrun() {
        use crate::parser::cell_normalize::CellToken;
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        // 5 step + | (拍 = 4 step) → 4 step に切り落とし
        let cells = vec![
            CellToken::Cell(Some(1)),
            CellToken::Cell(Some(2)),
            CellToken::Cell(Some(3)),
            CellToken::Cell(Some(4)),
            CellToken::Cell(Some(5)),
            CellToken::Pipe,
        ];
        let clip = ClipDef {
            name: "test".to_string(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    cells,
                })],
            }),
        };
        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        // 5 番目 (value=5) は切り落とされる
        assert_eq!(cc_events.len(), 4);
        for (i, ev) in cc_events.iter().enumerate() {
            if let MidiMessage::ControlChange { value, .. } = ev.message {
                assert_eq!(value, (i + 1) as u8);
            }
        }
    }

    /// `>N` で小節 N の頭にジャンプ
    /// `>N` snaps the cursor to bar N's start.
    #[test]
    fn cc_step_bar_jump_jumps_to_bar() {
        use crate::parser::cell_normalize::CellToken;
        let registry = make_registry_with_bass_cc();
        let clock = Clock::new(120.0);
        // 1 step + >3 + 1 step  (steps_per_bar = 16 in 4/4)
        // 期待: step 0 と step 32 (= bar3 頭) に CC が出る
        let cells = vec![
            CellToken::Cell(Some(10)),
            CellToken::BarJump(3),
            CellToken::Cell(Some(99)),
        ];
        let clip = ClipDef {
            name: "test".to_string(),
            // bars=4 にすると total_steps=64 で十分
            options: ClipOptions {
                bars: Some(4),
                time_sig: None,
                scale: None,
                octave_shift: 0,
            },
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![PitchedLine {
                    instrument: "bass".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                }],
                cc_automations: vec![CcAutomation::Step(CcStepValues {
                    target: CcTarget {
                        instrument: "bass".to_string(),
                        param: "cutoff".to_string(),
                    },
                    cells,
                })],
            }),
        };
        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        let cc_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| matches!(e.message, MidiMessage::ControlChange { .. }))
            .collect();
        assert_eq!(cc_events.len(), 2);
        let ticks_per_step = clock.duration_to_ticks(16, false);
        assert_eq!(cc_events[0].tick, 0);
        // bar3 頭 = step 32
        assert_eq!(cc_events[1].tick, 32 * ticks_per_step);
        if let MidiMessage::ControlChange { value, .. } = cc_events[1].message {
            assert_eq!(value, 99);
        }
    }

    // ---------------------------------------------------------------------
    // Issue #49: 複数 device へのルーティングのため、
    // compile 時に MidiEvent.device が正しく埋まることを検証する。
    // ---------------------------------------------------------------------

    /// Issue #49: pitched clip の全イベントに `instrument.device` が埋まる
    #[test]
    fn pitched_events_carry_instrument_device() {
        let registry = make_registry_with_bass();
        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "test",
            None,
            vec![PitchedLine {
                instrument: "bass".to_string(),
                elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                is_layer_start: true,
            }],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(!compiled.events.is_empty());
        // make_registry_with_bass で device="dev" に設定されている
        for ev in &compiled.events {
            assert_eq!(ev.device, "dev", "NoteOn/NoteOff should carry device");
        }
    }

    /// Issue #49: 異なる device を持つ 2 つの instrument を同じ clip に並べた
    /// 場合、各 line の events に該当 instrument の device が割り振られる
    #[test]
    fn pitched_multi_line_uses_per_instrument_device() {
        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "lead".to_string(),
            device: "synth_a".to_string(),
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));
        registry.register_block(crate::ast::Block::Instrument(InstrumentDef {
            name: "pad".to_string(),
            device: "synth_b".to_string(),
            channel: MidiChannel::from_one_based(2).unwrap(),
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
            cc_mappings: vec![],
            local_vars: vec![],
            unresolved: Default::default(),
        }));

        let clock = Clock::new(120.0);
        let clip = make_pitched_clip(
            "dual",
            None,
            vec![
                PitchedLine {
                    instrument: "lead".to_string(),
                    elements: vec![single_note(NoteName::C, Some(4), Some(4), false)],
                    is_layer_start: true,
                },
                PitchedLine {
                    instrument: "pad".to_string(),
                    elements: vec![single_note(NoteName::E, Some(4), Some(4), false)],
                    is_layer_start: true,
                },
            ],
        );

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();

        let lead_ch = MidiChannel::from_zero_based(0).unwrap();
        let pad_ch = MidiChannel::from_zero_based(1).unwrap();
        let lead_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| match e.message {
                MidiMessage::NoteOn { channel, .. } | MidiMessage::NoteOff { channel, .. } => {
                    channel == lead_ch
                }
                _ => false,
            })
            .collect();
        let pad_events: Vec<_> = compiled
            .events
            .iter()
            .filter(|e| match e.message {
                MidiMessage::NoteOn { channel, .. } | MidiMessage::NoteOff { channel, .. } => {
                    channel == pad_ch
                }
                _ => false,
            })
            .collect();

        assert!(!lead_events.is_empty());
        assert!(!pad_events.is_empty());
        for ev in lead_events {
            assert_eq!(ev.device, "synth_a");
        }
        for ev in pad_events {
            assert_eq!(ev.device, "synth_b");
        }
    }

    /// Issue #49: drum clip の全イベントに `kit.device` が埋まる
    #[test]
    fn drum_events_carry_kit_device() {
        use crate::ast::clip::DrumClipBody;
        use crate::ast::clip_drum::{DrumRow, HitSymbol};

        let mut registry = Registry::default();
        registry.register_block(crate::ast::Block::Kit(KitDef {
            name: "mykit".to_string(),
            device: "drum_device".to_string(),
            instruments: vec![KitInstrument {
                name: "kick".to_string(),
                channel: MidiChannel::from_one_based(10).unwrap(),
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: Some(80),
                gate_staccato: Some(40),
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
                unresolved: Default::default(),
            }],
        }));

        let clock = Clock::new(120.0);
        let clip = ClipDef {
            name: "beat".to_string(),
            options: ClipOptions {
                bars: None,
                time_sig: None,
                scale: None,
                octave_shift: 0,
            },
            body: ClipBody::Drum(DrumClipBody {
                kit: "mykit".to_string(),
                resolution: 16,
                rows: vec![DrumRow {
                    instrument: "kick".to_string(),
                    hits: vec![
                        HitSymbol::Accent,
                        HitSymbol::Rest,
                        HitSymbol::Normal,
                        HitSymbol::Rest,
                    ],
                    probability: None,
                }],
                cc_automations: vec![],
            }),
        };

        let compiled = compile_clip(&clip, &clock, &registry).unwrap();
        assert!(!compiled.events.is_empty());
        for ev in &compiled.events {
            assert_eq!(ev.device, "drum_device");
        }
    }
}
