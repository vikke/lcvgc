/// MIDIデバイス定義
/// MIDI device definition
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceDef {
    /// デバイス名
    /// Device name
    pub name: String,
    /// MIDIポート名
    /// MIDI port name
    pub port: String,
}
