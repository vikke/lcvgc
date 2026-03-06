//! MIDI関連モジュール
//! MIDI-related modules

pub mod cc;
pub mod chord;
pub mod gate;
pub mod message;
pub mod monitor;
pub mod note;
pub mod port;
pub mod probability;
pub mod velocity;

/// MIDI操作に関するエラー型
/// Error type for MIDI operations
#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    /// 指定されたMIDIポートが見つからない
    /// The specified MIDI port was not found
    #[error("MIDIポートが見つかりません: {0}")]
    PortNotFound(String),
    /// MIDI接続時のエラー
    /// Error during MIDI connection
    #[error("MIDI接続エラー: {0}")]
    ConnectionError(String),
    /// MIDIメッセージ送信時のエラー
    /// Error during MIDI message transmission
    #[error("MIDI送信エラー: {0}")]
    SendError(String),
}
