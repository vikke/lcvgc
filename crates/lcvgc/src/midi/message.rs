use crate::midi::channel::MidiChannel;

/// MIDIメッセージ
/// MIDI message representation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMessage {
    /// ノートオン: 発音開始
    /// Note On: start sounding a note
    NoteOn {
        /// MIDIチャンネル
        /// MIDI channel
        channel: MidiChannel,
        /// ノート番号 (0-127)
        /// Note number (0-127)
        note: u8,
        /// ベロシティ (0-127)
        /// Velocity (0-127)
        velocity: u8,
    },
    /// ノートオフ: 発音停止
    /// Note Off: stop sounding a note
    NoteOff {
        /// MIDIチャンネル
        /// MIDI channel
        channel: MidiChannel,
        /// ノート番号 (0-127)
        /// Note number (0-127)
        note: u8,
        /// ベロシティ (0-127)
        /// Velocity (0-127)
        velocity: u8,
    },
    /// コントロールチェンジ
    /// Control Change
    ControlChange {
        /// MIDIチャンネル
        /// MIDI channel
        channel: MidiChannel,
        /// CC番号 (0-127)
        /// CC number (0-127)
        cc: u8,
        /// CC値 (0-127)
        /// CC value (0-127)
        value: u8,
    },
    /// プログラムチェンジ: 音色変更
    /// Program Change: change instrument
    ProgramChange {
        /// MIDIチャンネル
        /// MIDI channel
        channel: MidiChannel,
        /// プログラム番号 (0-127)
        /// Program number (0-127)
        program: u8,
    },
    /// System Real-Time: Start (0xFA) — 外部 device に再生開始を伝える
    /// System Real-Time: Start (0xFA) — tells external devices to begin playback
    Start,
    /// System Real-Time: Stop (0xFC) — 外部 device に再生停止を伝える
    /// System Real-Time: Stop (0xFC) — tells external devices to stop playback
    Stop,
    /// System Real-Time: Continue (0xFB) — 外部 device に再生再開を伝える
    /// System Real-Time: Continue (0xFB) — tells external devices to resume playback
    Continue,
    /// System Real-Time: Timing Clock (0xF8) — 24 PPQN で外部 device にテンポを伝える
    ///
    /// 1 四分音符あたり 24 個の Timing Clock が送出される (MIDI 1.0 標準)。
    /// `play` 後に `stop` までの間、`transport = true` な device に対し
    /// `Clock::clock_period_ticks()` 周期で送出される。Start (0xFA) と
    /// 同時に最初の 1 個も送出される (MIDI 仕様: 最初の Clock が beat 0)。
    ///
    /// System Real-Time: Timing Clock (0xF8). MIDI 1.0 mandates 24 pulses per
    /// quarter note. While playing, this is emitted at `Clock::clock_period_ticks()`
    /// intervals to every `transport = true` device. The first Clock is sent
    /// together with Start (per MIDI spec: the first Clock marks beat 0).
    Clock,
}

impl MidiMessage {
    /// MIDIバイト列にシリアライズ
    /// Serialize to MIDI byte sequence
    ///
    /// # 戻り値 / Returns
    /// `Vec<u8>` - MIDIプロトコルに準拠したバイト列 / MIDI protocol-compliant byte sequence
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } => {
                vec![0x90 | channel.as_zero_based(), *note, *velocity]
            }
            MidiMessage::NoteOff {
                channel,
                note,
                velocity,
            } => {
                vec![0x80 | channel.as_zero_based(), *note, *velocity]
            }
            MidiMessage::ControlChange { channel, cc, value } => {
                vec![0xB0 | channel.as_zero_based(), *cc, *value]
            }
            MidiMessage::ProgramChange { channel, program } => {
                vec![0xC0 | channel.as_zero_based(), *program]
            }
            MidiMessage::Start => vec![0xFA],
            MidiMessage::Stop => vec![0xFC],
            MidiMessage::Continue => vec![0xFB],
            MidiMessage::Clock => vec![0xF8],
        }
    }

    /// MIDIバイト列から `MidiMessage` をパースする（ワイヤ受信用）。
    ///
    /// MIDI 入力ポートから届いた生バイト列を構造化メッセージへ変換する。
    /// 対応するのは Note On / Note Off / Control Change / Program Change の
    /// チャンネルボイスメッセージと、System Real-Time (Start/Stop/Continue/
    /// Clock) の単一バイトメッセージ。長さ不足や未対応のステータスバイトは
    /// `None` を返す。ランニングステータスや SysEx は未対応。
    ///
    /// Parses a `MidiMessage` from raw wire bytes received on a MIDI input
    /// port. Supports Note On/Off, Control Change, Program Change channel-voice
    /// messages plus single-byte System Real-Time messages. Returns `None` for
    /// truncated input or unsupported status bytes. Running status and SysEx are
    /// not supported.
    ///
    /// # 引数 / Arguments
    /// * `bytes` - MIDIワイヤバイト列 / MIDI wire byte sequence
    ///
    /// # 戻り値 / Returns
    /// パースできれば `Some(MidiMessage)`、できなければ `None`
    /// `Some(MidiMessage)` if parseable, otherwise `None`
    pub fn from_bytes(bytes: &[u8]) -> Option<MidiMessage> {
        let status = *bytes.first()?;

        // System Real-Time（単一バイト、チャンネルを含まない）。
        // System Real-Time (single byte, channel-less).
        match status {
            0xF8 => return Some(MidiMessage::Clock),
            0xFA => return Some(MidiMessage::Start),
            0xFB => return Some(MidiMessage::Continue),
            0xFC => return Some(MidiMessage::Stop),
            _ => {}
        }

        // チャンネルボイスメッセージ: 上位ニブルが種別、下位ニブルが 0-based ch。
        // Channel-voice messages: high nibble = kind, low nibble = 0-based ch.
        let kind = status & 0xF0;
        let channel = MidiChannel::from_zero_based(status & 0x0F).ok()?;
        match kind {
            0x80 => {
                let note = *bytes.get(1)?;
                let velocity = *bytes.get(2)?;
                Some(MidiMessage::NoteOff {
                    channel,
                    note,
                    velocity,
                })
            }
            0x90 => {
                let note = *bytes.get(1)?;
                let velocity = *bytes.get(2)?;
                Some(MidiMessage::NoteOn {
                    channel,
                    note,
                    velocity,
                })
            }
            0xB0 => {
                let cc = *bytes.get(1)?;
                let value = *bytes.get(2)?;
                Some(MidiMessage::ControlChange { channel, cc, value })
            }
            0xC0 => {
                let program = *bytes.get(1)?;
                Some(MidiMessage::ProgramChange { channel, program })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch0() -> MidiChannel {
        MidiChannel::from_zero_based(0).unwrap()
    }

    #[test]
    fn note_on_ch0() {
        let msg = MidiMessage::NoteOn {
            channel: ch0(),
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x90, 60, 100]);
    }

    #[test]
    fn note_off_ch0() {
        let msg = MidiMessage::NoteOff {
            channel: ch0(),
            note: 60,
            velocity: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0x80, 60, 0]);
    }

    #[test]
    fn note_on_drum_ch9() {
        let msg = MidiMessage::NoteOn {
            channel: MidiChannel::from_zero_based(9).unwrap(),
            note: 36,
            velocity: 127,
        };
        assert_eq!(msg.to_bytes(), vec![0x99, 36, 127]);
    }

    #[test]
    fn control_change() {
        let msg = MidiMessage::ControlChange {
            channel: ch0(),
            cc: 74,
            value: 64,
        };
        assert_eq!(msg.to_bytes(), vec![0xB0, 74, 64]);
    }

    #[test]
    fn program_change() {
        let msg = MidiMessage::ProgramChange {
            channel: ch0(),
            program: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0xC0, 0]);
    }

    #[test]
    fn channel_15_boundary() {
        let msg = MidiMessage::NoteOn {
            channel: MidiChannel::from_zero_based(15).unwrap(),
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x9F, 60, 100]);
    }

    /// 「DSL の `channel 1` (1-based) → MIDI バイト列の status バイトは
    /// `0x90`」という end-to-end の保証。本 PR のバグ修正の核となるテスト。
    /// End-to-end guarantee: DSL `channel 1` (1-based) → MIDI status byte
    /// `0x90`. The core test of the bug fix in this PR.
    #[test]
    fn one_based_channel_1_yields_status_0x90() {
        let msg = MidiMessage::NoteOn {
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x90, 60, 100]);
    }

    /// DSL の `channel 10` (GM ドラム) は status バイト `0x99` を生む
    /// DSL `channel 10` (GM drum) yields status byte `0x99`
    #[test]
    fn one_based_channel_10_yields_status_0x99() {
        let msg = MidiMessage::NoteOn {
            channel: MidiChannel::from_one_based(10).unwrap(),
            note: 36,
            velocity: 127,
        };
        assert_eq!(msg.to_bytes(), vec![0x99, 36, 127]);
    }

    #[test]
    fn system_realtime_start() {
        assert_eq!(MidiMessage::Start.to_bytes(), vec![0xFA]);
    }

    #[test]
    fn system_realtime_stop() {
        assert_eq!(MidiMessage::Stop.to_bytes(), vec![0xFC]);
    }

    #[test]
    fn system_realtime_continue() {
        assert_eq!(MidiMessage::Continue.to_bytes(), vec![0xFB]);
    }

    /// MIDI System Real-Time Timing Clock (0xF8) は単一バイトの
    /// `vec![0xF8]` にシリアライズされる。再生中に 24 PPQN で送出される。
    ///
    /// MIDI System Real-Time Timing Clock (0xF8) serializes to a single
    /// byte `vec![0xF8]`. Sent at 24 PPQN while playing.
    #[test]
    fn system_realtime_clock() {
        assert_eq!(MidiMessage::Clock.to_bytes(), vec![0xF8]);
    }

    // --- from_bytes（ワイヤ受信パース）---
    // --- from_bytes (wire-receive parsing) ---

    #[test]
    fn from_bytes_note_on() {
        let msg = MidiMessage::from_bytes(&[0x90, 60, 100]).unwrap();
        assert_eq!(
            msg,
            MidiMessage::NoteOn {
                channel: ch0(),
                note: 60,
                velocity: 100,
            }
        );
    }

    #[test]
    fn from_bytes_note_off() {
        let msg = MidiMessage::from_bytes(&[0x80, 60, 0]).unwrap();
        assert_eq!(
            msg,
            MidiMessage::NoteOff {
                channel: ch0(),
                note: 60,
                velocity: 0,
            }
        );
    }

    #[test]
    fn from_bytes_note_on_drum_ch9() {
        let msg = MidiMessage::from_bytes(&[0x99, 36, 127]).unwrap();
        assert_eq!(
            msg,
            MidiMessage::NoteOn {
                channel: MidiChannel::from_zero_based(9).unwrap(),
                note: 36,
                velocity: 127,
            }
        );
    }

    #[test]
    fn from_bytes_control_change() {
        let msg = MidiMessage::from_bytes(&[0xB0, 74, 64]).unwrap();
        assert_eq!(
            msg,
            MidiMessage::ControlChange {
                channel: ch0(),
                cc: 74,
                value: 64,
            }
        );
    }

    #[test]
    fn from_bytes_program_change() {
        let msg = MidiMessage::from_bytes(&[0xC0, 5]).unwrap();
        assert_eq!(
            msg,
            MidiMessage::ProgramChange {
                channel: ch0(),
                program: 5,
            }
        );
    }

    #[test]
    fn from_bytes_system_realtime() {
        assert_eq!(
            MidiMessage::from_bytes(&[0xFA]).unwrap(),
            MidiMessage::Start
        );
        assert_eq!(MidiMessage::from_bytes(&[0xFC]).unwrap(), MidiMessage::Stop);
        assert_eq!(
            MidiMessage::from_bytes(&[0xFB]).unwrap(),
            MidiMessage::Continue
        );
        assert_eq!(
            MidiMessage::from_bytes(&[0xF8]).unwrap(),
            MidiMessage::Clock
        );
    }

    /// 空バイト列・長さ不足は None を返す。
    /// Empty or truncated byte sequences yield None.
    #[test]
    fn from_bytes_too_short_is_none() {
        assert!(MidiMessage::from_bytes(&[]).is_none());
        assert!(MidiMessage::from_bytes(&[0x90]).is_none()); // note 欠落
        assert!(MidiMessage::from_bytes(&[0x90, 60]).is_none()); // velocity 欠落
        assert!(MidiMessage::from_bytes(&[0xC0]).is_none()); // program 欠落
    }

    /// 未対応のステータスバイト（例: ピッチベンド 0xE0、SysEx 0xF0）は None。
    /// Unsupported status bytes (e.g. pitch bend, SysEx) yield None.
    #[test]
    fn from_bytes_unsupported_status_is_none() {
        assert!(MidiMessage::from_bytes(&[0xE0, 0, 64]).is_none()); // pitch bend
        assert!(MidiMessage::from_bytes(&[0xF0, 0x7E]).is_none()); // SysEx
    }

    /// to_bytes → from_bytes のラウンドトリップが一致する（対応メッセージ）。
    /// Round-trip to_bytes → from_bytes is identity for supported messages.
    #[test]
    fn round_trip_note_on() {
        let msg = MidiMessage::NoteOn {
            channel: MidiChannel::from_zero_based(3).unwrap(),
            note: 64,
            velocity: 90,
        };
        assert_eq!(MidiMessage::from_bytes(&msg.to_bytes()).unwrap(), msg);
    }
}
