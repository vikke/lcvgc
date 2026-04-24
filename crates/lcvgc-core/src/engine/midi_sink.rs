use crate::engine::error::EngineError;
use crate::midi::message::MidiMessage;
use crate::midi::port::PortManager;

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

/// 共有可能なテスト用モック
///
/// 内部で `Arc<Mutex<Vec<MidiMessage>>>` を保持するため、`Box<dyn MidiSink>`
/// として `PlaybackDriver` に渡した後も、clone したハンドルから送出内容を
/// 検証できる。Issue #49 の複数 device ルーティングテストで利用する。
///
/// A shareable mock sink backed by `Arc<Mutex<Vec<MidiMessage>>>`. Lets a
/// test hand the sink to `PlaybackDriver` (as `Box<dyn MidiSink>`) while
/// still being able to inspect the captured messages through a cloned
/// handle. Introduced for Issue #49 multi-device routing tests.
#[derive(Debug, Clone, Default)]
pub struct SharedMockSink {
    inner: std::sync::Arc<std::sync::Mutex<Vec<MidiMessage>>>,
}

impl SharedMockSink {
    /// 新しい `SharedMockSink` を生成する
    pub fn new() -> Self {
        Self::default()
    }

    /// これまでに送出されたメッセージのスナップショットを返す
    pub fn snapshot(&self) -> Vec<MidiMessage> {
        self.inner.lock().expect("mock sink poisoned").clone()
    }

    /// 内部バッファを空にする
    pub fn clear(&self) {
        self.inner.lock().expect("mock sink poisoned").clear();
    }
}

impl MidiSink for SharedMockSink {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), EngineError> {
        self.inner
            .lock()
            .expect("mock sink poisoned")
            .push(msg.clone());
        Ok(())
    }
}

/// midir経由の実MIDIポート送信
pub struct MidirSink {
    port_manager: PortManager,
    /// 送信先の論理名
    target: String,
}

impl MidirSink {
    /// 新しいMidirSinkを作成する
    ///
    /// port_manager: 接続済みのPortManager
    /// target: 送信先の論理名（PortManagerに登録済みであること）
    pub fn new(port_manager: PortManager, target: String) -> Self {
        MidirSink {
            port_manager,
            target,
        }
    }

    /// 内部のPortManagerへの参照を返す
    pub fn port_manager(&self) -> &PortManager {
        &self.port_manager
    }

    /// 内部のPortManagerへの可変参照を返す
    pub fn port_manager_mut(&mut self) -> &mut PortManager {
        &mut self.port_manager
    }
}

impl MidiSink for MidirSink {
    fn send(&mut self, msg: &MidiMessage) -> Result<(), EngineError> {
        let bytes = msg.to_bytes();
        self.port_manager
            .send(&self.target, &bytes)
            .map_err(|e| EngineError::Config(e.to_string()))
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
