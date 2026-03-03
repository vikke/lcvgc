pub mod cc;
pub mod chord;
pub mod gate;
pub mod message;
pub mod note;
pub mod monitor;
pub mod port;
pub mod probability;
pub mod velocity;

#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    #[error("MIDIポートが見つかりません: {0}")]
    PortNotFound(String),
    #[error("MIDI接続エラー: {0}")]
    ConnectionError(String),
    #[error("MIDI送信エラー: {0}")]
    SendError(String),
}
