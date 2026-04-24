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
    /// MIDI System Real-Time (Start/Stop/Continue) をこの device に送出するか。
    /// DSL で省略された場合は `true`（既定値）。
    /// Whether to emit MIDI System Real-Time messages (Start/Stop/Continue)
    /// to this device. Defaults to `true` when omitted in the DSL.
    pub transport: bool,
}
