use crate::engine::error::EngineError;
use crate::midi::message::MidiMessage;

/// MIDI送信の抽象trait（テスト時にモック差し替え可能）
pub trait MidiSink: Send {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), EngineError>;
}

/// テスト用モック
#[derive(Debug, Default)]
pub struct MockSink {
    pub sent: Vec<MidiMessage>,
}

impl MidiSink for MockSink {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), EngineError> {
        self.sent.push(msg.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_sink_default_has_empty_sent() {
        let sink = MockSink::default();
        assert!(sink.sent.is_empty());
    }

    #[test]
    fn mock_sink_send_note_on() {
        let mut sink = MockSink::default();
        let msg = MidiMessage::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        };
        sink.send(&msg).unwrap();
        assert_eq!(sink.sent.len(), 1);
        assert_eq!(sink.sent[0], msg);
    }

    #[test]
    fn mock_sink_send_multiple() {
        let mut sink = MockSink::default();
        let msgs = vec![
            MidiMessage::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            },
            MidiMessage::NoteOff {
                channel: 0,
                note: 60,
                velocity: 0,
            },
            MidiMessage::ControlChange {
                channel: 1,
                cc: 7,
                value: 127,
            },
        ];
        for msg in &msgs {
            sink.send(msg).unwrap();
        }
        assert_eq!(sink.sent.len(), 3);
        assert_eq!(sink.sent, msgs);
    }
}
