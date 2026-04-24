/// MIDIメッセージ
/// MIDI message representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiMessage {
    /// ノートオン: 発音開始
    /// Note On: start sounding a note
    NoteOn {
        /// MIDIチャンネル (0-15)
        /// MIDI channel (0-15)
        channel: u8,
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
        /// MIDIチャンネル (0-15)
        /// MIDI channel (0-15)
        channel: u8,
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
        /// MIDIチャンネル (0-15)
        /// MIDI channel (0-15)
        channel: u8,
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
        /// MIDIチャンネル (0-15)
        /// MIDI channel (0-15)
        channel: u8,
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
                vec![0x90 | channel, *note, *velocity]
            }
            MidiMessage::NoteOff {
                channel,
                note,
                velocity,
            } => {
                vec![0x80 | channel, *note, *velocity]
            }
            MidiMessage::ControlChange { channel, cc, value } => {
                vec![0xB0 | channel, *cc, *value]
            }
            MidiMessage::ProgramChange { channel, program } => {
                vec![0xC0 | channel, *program]
            }
            MidiMessage::Start => vec![0xFA],
            MidiMessage::Stop => vec![0xFC],
            MidiMessage::Continue => vec![0xFB],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_ch0() {
        let msg = MidiMessage::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x90, 60, 100]);
    }

    #[test]
    fn note_off_ch0() {
        let msg = MidiMessage::NoteOff {
            channel: 0,
            note: 60,
            velocity: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0x80, 60, 0]);
    }

    #[test]
    fn note_on_drum_ch9() {
        let msg = MidiMessage::NoteOn {
            channel: 9,
            note: 36,
            velocity: 127,
        };
        assert_eq!(msg.to_bytes(), vec![0x99, 36, 127]);
    }

    #[test]
    fn control_change() {
        let msg = MidiMessage::ControlChange {
            channel: 0,
            cc: 74,
            value: 64,
        };
        assert_eq!(msg.to_bytes(), vec![0xB0, 74, 64]);
    }

    #[test]
    fn program_change() {
        let msg = MidiMessage::ProgramChange {
            channel: 0,
            program: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0xC0, 0]);
    }

    #[test]
    fn channel_15_boundary() {
        let msg = MidiMessage::NoteOn {
            channel: 15,
            note: 60,
            velocity: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x9F, 60, 100]);
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
}
