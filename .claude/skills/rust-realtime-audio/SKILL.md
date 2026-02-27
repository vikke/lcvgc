---
name: rust-realtime-audio
description: Rustでリアルタイムオーディオ・MIDIプログラミングを行うためのスキル。オーディオスレッドの制約（アロケーション禁止、ロック禁止）、ロックフリーキュー、リングバッファ、MIDIメッセージのパース/生成、cpal/midir連携、FM合成、wavetableオシレータ、エンベロープ生成器をカバー。「オーディオ」「音声処理」「MIDI」「リアルタイム」「レイテンシ」「バッファ」「サンプルレート」「オシレータ」「シンセサイザー」「FM合成」「cpal」「midir」「ロックフリー」「リングバッファ」「DAW」など音声/MIDI処理の話題が出たら必ずこのスキルを使うこと。lcvgcの音声エンジン実装にも必ず参照すること。
---

# Rust リアルタイムオーディオ・MIDI スキル

リアルタイムオーディオ処理とMIDIの制約を正しく理解し、安全かつ低レイテンシなコードを書くためのガイド。

## リアルタイムオーディオスレッドの絶対ルール

オーディオコールバック内で**絶対にやってはいけない**こと：

1. **ヒープアロケーション禁止** — `Vec::push`、`String::new`、`Box::new` すべてNG
2. **ロック禁止** — `Mutex::lock`、`RwLock` はデッドロックの危険
3. **I/O禁止** — ファイル読み書き、ネットワーク、`println!`
4. **システムコール最小化** — `sleep`、`yield` は論外
5. **パニック禁止** — `unwrap()` をオーディオスレッドで使わない

理由：オーディオコールバックは通常 1-10ms 以内に完了する必要がある。上記の操作は実行時間が不定で、バッファアンダーランを引き起こす。

## cpal（クロスプラットフォームオーディオ）

```toml
[dependencies]
cpal = "0.15"
```

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;

fn setup_audio() -> anyhow::Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_output_device()
        .ok_or_else(|| anyhow::anyhow!("出力デバイスが見つかりません"))?;

    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    // オーディオスレッドとの通信用（ロックフリー）
    let (tx, rx) = rtrb::RingBuffer::<MidiEvent>::new(256);

    let mut engine = AudioEngine::new(sample_rate, rx);

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // ここがオーディオコールバック — 上記のルールを厳守
            engine.process(data, channels);
        },
        |err| eprintln!("オーディオエラー: {err}"),
        None,
    )?;

    stream.play()?;
    Ok(stream)
}
```

## ロックフリー通信

### rtrb（リアルタイム安全リングバッファ）

```toml
[dependencies]
rtrb = "0.3"
```

```rust
use rtrb::{RingBuffer, Consumer, Producer};

// プロデューサー（メインスレッド）→ コンシューマー（オーディオスレッド）
let (mut producer, mut consumer) = RingBuffer::<MidiEvent>::new(256);

// メインスレッドからの送信（ノンブロッキング）
fn send_event(producer: &mut Producer<MidiEvent>, event: MidiEvent) {
    match producer.push(event) {
        Ok(()) => {}
        Err(_) => {
            // バッファフル — イベントをドロップ（ログはオーディオスレッド外で）
        }
    }
}

// オーディオコールバック内での受信
fn process_events(consumer: &mut Consumer<MidiEvent>, engine: &mut SynthEngine) {
    // すべての待機イベントを処理（アロケーションなし）
    while let Ok(event) = consumer.pop() {
        match event {
            MidiEvent::NoteOn { note, velocity } => engine.note_on(note, velocity),
            MidiEvent::NoteOff { note } => engine.note_off(note),
            MidiEvent::ControlChange { cc, value } => engine.control_change(cc, value),
        }
    }
}
```

### crossbeam-channel（バウンデッドチャネル）

オーディオスレッドからUIスレッドへの通知に（オーディオスレッドでは `try_send` のみ使う）：

```rust
use crossbeam_channel::{bounded, TrySendError};

let (tx, rx) = bounded::<MeterData>(16);

// オーディオコールバック内
let _ = tx.try_send(MeterData { peak_l, peak_r });  // フルなら捨てる
```

## MIDI メッセージ処理

### MIDI メッセージのパース

```rust
#[derive(Debug, Clone, Copy)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    PitchBend { channel: u8, value: u16 },
    // SysEx は別扱い
}

impl MidiMessage {
    /// 生のMIDIバイトからパース。アロケーションなし。
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let status = data[0];
        let kind = status & 0xF0;
        let channel = status & 0x0F;

        match kind {
            0x90 if data.len() >= 3 => {
                let velocity = data[2];
                if velocity == 0 {
                    // velocity 0 の NoteOn は NoteOff として扱う
                    Some(MidiMessage::NoteOff { channel, note: data[1], velocity: 0 })
                } else {
                    Some(MidiMessage::NoteOn { channel, note: data[1], velocity })
                }
            }
            0x80 if data.len() >= 3 => {
                Some(MidiMessage::NoteOff { channel, note: data[1], velocity: data[2] })
            }
            0xB0 if data.len() >= 3 => {
                Some(MidiMessage::ControlChange {
                    channel,
                    controller: data[1],
                    value: data[2],
                })
            }
            0xC0 if data.len() >= 2 => {
                Some(MidiMessage::ProgramChange { channel, program: data[1] })
            }
            0xE0 if data.len() >= 3 => {
                let value = (data[2] as u16) << 7 | data[1] as u16;
                Some(MidiMessage::PitchBend { channel, value })
            }
            _ => None,
        }
    }

    /// MIDI バイト列に変換。固定サイズ配列を返す（アロケーションなし）。
    pub fn to_bytes(&self) -> ([u8; 3], usize) {
        match *self {
            MidiMessage::NoteOn { channel, note, velocity } => {
                ([0x90 | channel, note, velocity], 3)
            }
            MidiMessage::NoteOff { channel, note, velocity } => {
                ([0x80 | channel, note, velocity], 3)
            }
            MidiMessage::ControlChange { channel, controller, value } => {
                ([0xB0 | channel, controller, value], 3)
            }
            MidiMessage::ProgramChange { channel, program } => {
                ([0xC0 | channel, program, 0], 2)
            }
            MidiMessage::PitchBend { channel, value } => {
                let lsb = (value & 0x7F) as u8;
                let msb = ((value >> 7) & 0x7F) as u8;
                ([0xE0 | channel, lsb, msb], 3)
            }
        }
    }
}
```

### midir（MIDI I/O）

```toml
[dependencies]
midir = "0.10"
```

```rust
use midir::{MidiInput, MidiOutput, MidiInputConnection};

fn setup_midi_input(
    producer: rtrb::Producer<MidiEvent>,
) -> anyhow::Result<MidiInputConnection<()>> {
    let midi_in = MidiInput::new("lcvgc input")?;
    let ports = midi_in.ports();

    // ポート一覧を表示して選択
    for (i, port) in ports.iter().enumerate() {
        println!("{i}: {}", midi_in.port_name(port)?);
    }

    let port = &ports[0];  // 実際にはユーザーに選択させる
    let mut producer = producer;

    let conn = midi_in.connect(
        port,
        "lcvgc-in",
        move |_timestamp, data, _| {
            // このコールバックは MIDI I/O スレッドで実行される
            if let Some(msg) = MidiMessage::from_bytes(data) {
                let event = MidiEvent::from(msg);
                let _ = producer.push(event);  // フルなら捨てる
            }
        },
        (),
    )?;

    Ok(conn)
}
```

## シンセサイザー基本コンポーネント

### オシレータ（アロケーションなし）

```rust
pub struct Oscillator {
    phase: f32,
    frequency: f32,
    sample_rate: f32,
}

impl Oscillator {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            phase: 0.0,
            frequency: 440.0,
            sample_rate: sample_rate as f32,
        }
    }

    pub fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq;
    }

    /// サイン波 — 1サンプル生成
    #[inline]
    pub fn next_sine(&mut self) -> f32 {
        let sample = (self.phase * std::f32::consts::TAU).sin();
        self.advance_phase();
        sample
    }

    /// 矩形波
    #[inline]
    pub fn next_square(&mut self) -> f32 {
        let sample = if self.phase < 0.5 { 1.0 } else { -1.0 };
        self.advance_phase();
        sample
    }

    /// ノコギリ波
    #[inline]
    pub fn next_saw(&mut self) -> f32 {
        let sample = 2.0 * self.phase - 1.0;
        self.advance_phase();
        sample
    }

    /// 三角波
    #[inline]
    pub fn next_triangle(&mut self) -> f32 {
        let sample = if self.phase < 0.5 {
            4.0 * self.phase - 1.0
        } else {
            3.0 - 4.0 * self.phase
        };
        self.advance_phase();
        sample
    }

    #[inline]
    fn advance_phase(&mut self) {
        self.phase += self.frequency / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }
}
```

### FM 合成

クラシックなゲーム音楽に必須：

```rust
/// 2オペレータFM合成器
pub struct FmSynth {
    carrier: Oscillator,
    modulator: Oscillator,
    mod_index: f32,      // 変調指数（音色の明るさ）
    mod_ratio: f32,      // キャリア周波数に対するモジュレータの比率
    sample_rate: f32,
}

impl FmSynth {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            carrier: Oscillator::new(sample_rate),
            modulator: Oscillator::new(sample_rate),
            mod_index: 2.0,
            mod_ratio: 2.0,
            sample_rate: sample_rate as f32,
        }
    }

    pub fn set_note(&mut self, frequency: f32) {
        self.carrier.set_frequency(frequency);
        self.modulator.set_frequency(frequency * self.mod_ratio);
    }

    /// クラシックゲーム音色プリセット
    pub fn set_preset(&mut self, preset: FmPreset) {
        match preset {
            FmPreset::ElectricPiano => {
                self.mod_ratio = 1.0;
                self.mod_index = 1.5;
            }
            FmPreset::Bass => {
                self.mod_ratio = 1.0;
                self.mod_index = 3.0;
            }
            FmPreset::Bell => {
                self.mod_ratio = 3.5;
                self.mod_index = 5.0;
            }
            FmPreset::Brass => {
                self.mod_ratio = 1.0;
                self.mod_index = 5.0;
            }
        }
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let mod_output = self.modulator.next_sine();
        let mod_amount = mod_output * self.mod_index * self.carrier.frequency;

        // キャリアの位相をモジュレータで変調
        let phase = self.carrier.phase * std::f32::consts::TAU
            + mod_amount / self.sample_rate * std::f32::consts::TAU;
        let sample = phase.sin();

        self.carrier.advance_phase();
        sample
    }
}
```

### ADSR エンベロープ

```rust
#[derive(Debug, Clone, Copy)]
pub struct AdsrParams {
    pub attack: f32,   // 秒
    pub decay: f32,    // 秒
    pub sustain: f32,  // 0.0-1.0 レベル
    pub release: f32,  // 秒
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EnvelopeStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Envelope {
    stage: EnvelopeStage,
    level: f32,
    params: AdsrParams,
    sample_rate: f32,
}

impl Envelope {
    pub fn new(sample_rate: u32, params: AdsrParams) -> Self {
        Self {
            stage: EnvelopeStage::Idle,
            level: 0.0,
            params,
            sample_rate: sample_rate as f32,
        }
    }

    pub fn trigger(&mut self) {
        self.stage = EnvelopeStage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != EnvelopeStage::Idle
    }

    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        match self.stage {
            EnvelopeStage::Idle => 0.0,
            EnvelopeStage::Attack => {
                self.level += 1.0 / (self.params.attack * self.sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
                self.level
            }
            EnvelopeStage::Decay => {
                self.level -= (1.0 - self.params.sustain)
                    / (self.params.decay * self.sample_rate);
                if self.level <= self.params.sustain {
                    self.level = self.params.sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
                self.level
            }
            EnvelopeStage::Sustain => self.level,
            EnvelopeStage::Release => {
                self.level -= self.params.sustain
                    / (self.params.release * self.sample_rate);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
                self.level
            }
        }
    }
}
```

## ボイス管理（ポリフォニー）

```rust
const MAX_VOICES: usize = 16;

struct Voice {
    active: bool,
    note: u8,
    oscillator: Oscillator,
    envelope: Envelope,
    fm_synth: FmSynth,
}

pub struct VoiceManager {
    voices: [Voice; MAX_VOICES],  // 固定サイズ配列（ヒープアロケーションなし）
}

impl VoiceManager {
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        // 空きボイスを探す
        let voice = self.voices.iter_mut()
            .find(|v| !v.active)
            .or_else(|| {
                // 全ボイス使用中 → 最も古いボイスをスチール
                self.voices.iter_mut().min_by_key(|v| v.age)
            });

        if let Some(voice) = voice {
            let freq = midi_to_freq(note);
            voice.active = true;
            voice.note = note;
            voice.fm_synth.set_note(freq);
            voice.envelope.trigger();
        }
    }

    pub fn note_off(&mut self, note: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.note == note {
                voice.envelope.release();
            }
        }
    }

    /// オーディオバッファを埋める（アロケーションなし）
    #[inline]
    pub fn process(&mut self, buffer: &mut [f32], channels: usize) {
        // バッファをゼロクリア
        for sample in buffer.iter_mut() {
            *sample = 0.0;
        }

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }

            for frame in buffer.chunks_exact_mut(channels) {
                let env = voice.envelope.next_sample();
                let osc = voice.fm_synth.next_sample();
                let sample = osc * env * 0.2;  // マスターボリューム

                // すべてのチャンネルに同じ値（モノラル）
                for ch in frame.iter_mut() {
                    *ch += sample;
                }
            }

            if !voice.envelope.is_active() {
                voice.active = false;
            }
        }
    }
}

/// MIDI ノート番号を周波数に変換
#[inline]
fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}
```

## パフォーマンスのヒント

- `#[inline]` をオーディオパスのホット関数に付ける
- SIMD 最適化が必要な場合は `std::simd`（nightly）または `packed_simd2`
- バッファサイズの選択：128 サンプル ≈ 2.9ms @44100Hz（低レイテンシ）、512 ≈ 11.6ms（安定）
- プロファイリングには `cargo flamegraph` が有効
