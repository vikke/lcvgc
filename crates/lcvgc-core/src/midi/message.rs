/// MIDIメッセージ
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    ControlChange { channel: u8, cc: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
}

impl MidiMessage {
    /// MIDIバイト列にシリアライズ
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
}
