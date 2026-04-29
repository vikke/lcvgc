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

/// 指定 sink へ MIDI Channel 0..15 すべての AllNotesOff (CC#123 value=0) を
/// 送出する。Device の port 張り替えや緊急停止で「現在鳴っているノート全て」
/// を強制的に止める用途を想定。
///
/// 16 個目で失敗した場合などは fail-fast で `Err` を返し、後続の送出は
/// 試みない。AllNotesOff は idempotent な操作のため、再試行は呼び出し側で
/// 行えばよい。
///
/// Sends AllNotesOff (CC#123 value=0) on every MIDI channel (0..15) to the
/// given sink. Used when swapping a device's underlying port or doing a
/// hard stop, to silence all currently sounding notes. Fails fast on the
/// first send error and does not retry; AllNotesOff is idempotent so the
/// caller can re-invoke if needed.
///
/// # Arguments
/// * `sink` - 対象 MidiSink への可変参照 / mutable reference to the target sink
///
/// # Errors
/// `MidiSink::send` が返した最初のエラーを伝播する。
pub fn send_all_notes_off_all_channels(
    sink: &mut dyn MidiSink,
) -> Result<(), crate::engine::error::EngineError> {
    use crate::midi::message::MidiMessage;
    for channel in 0u8..16 {
        let msg = MidiMessage::ControlChange {
            channel,
            cc: 123,
            value: 0,
        };
        sink.send(&msg)?;
    }
    Ok(())
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

    /// `send_all_notes_off_all_channels` が 16 channel 全てに
    /// AllNotesOff (CC#123 value=0) を順番に送出することを検証する。
    /// Verifies the helper emits AllNotesOff on every channel 0..15.
    #[test]
    fn send_all_notes_off_all_channels_emits_16_messages() {
        let mut sink = MockSink::default();
        send_all_notes_off_all_channels(&mut sink).expect("送出は成功するはず");

        assert_eq!(sink.sent.len(), 16, "16 channel 分送出されるはず");
        for (idx, msg) in sink.sent.iter().enumerate() {
            match msg {
                MidiMessage::ControlChange { channel, cc, value } => {
                    assert_eq!(
                        *channel as usize, idx,
                        "channel は 0..15 を順に網羅するはず"
                    );
                    assert_eq!(*cc, 123, "AllNotesOff の CC 番号は 123");
                    assert_eq!(*value, 0, "AllNotesOff の value は 0");
                }
                other => panic!("ControlChange 以外が送られた: {other:?}"),
            }
        }
    }

    /// `MidiSink::send` がエラーを返したとき、ヘルパーがそのエラーを
    /// fail-fast で伝播し、以降の channel に送出を試みないことを検証する。
    /// Verifies the helper bails out on the first send error.
    #[test]
    fn send_all_notes_off_returns_err_if_sink_fails() {
        /// 最初の送出で常に失敗するテスト専用 sink。
        /// Test-only sink that fails on the first send.
        struct FailingSink {
            /// これまでに send 試行された回数
            /// Number of send attempts so far
            attempts: usize,
        }

        impl MidiSink for FailingSink {
            fn send(&mut self, _msg: &MidiMessage) -> Result<(), EngineError> {
                self.attempts += 1;
                Err(EngineError::Config("forced failure".to_string()))
            }
        }

        let mut sink = FailingSink { attempts: 0 };
        let result = send_all_notes_off_all_channels(&mut sink);

        assert!(result.is_err(), "ヘルパーはエラーを伝播するはず");
        assert_eq!(
            sink.attempts, 1,
            "fail-fast: 最初の失敗で打ち切るはず（試行回数は 1）"
        );
        match result.unwrap_err() {
            EngineError::Config(msg) => assert_eq!(msg, "forced failure"),
            other => panic!("予期しないエラー: {other:?}"),
        }
    }
}
