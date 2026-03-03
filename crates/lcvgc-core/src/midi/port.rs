use std::collections::HashMap;

use midir::{MidiInput, MidiOutput, MidiOutputConnection, MidiOutputPort};

use crate::midi::MidiError;

/// 利用可能なMIDI出力ポートを列挙する
pub fn list_ports() -> Result<Vec<String>, MidiError> {
    let output = MidiOutput::new("lcvgc-list")
        .map_err(|e| MidiError::ConnectionError(e.to_string()))?;
    let ports = output.ports();
    let mut names = Vec::with_capacity(ports.len());
    for port in &ports {
        let name = output
            .port_name(port)
            .map_err(|e| MidiError::ConnectionError(e.to_string()))?;
        names.push(name);
    }
    Ok(names)
}

/// 利用可能なMIDI入力ポートを列挙する
pub fn list_input_ports() -> Result<Vec<String>, MidiError> {
    let input = MidiInput::new("lcvgc-list")
        .map_err(|e| MidiError::ConnectionError(e.to_string()))?;
    let ports = input.ports();
    let mut names = Vec::with_capacity(ports.len());
    for port in &ports {
        let name = input
            .port_name(port)
            .map_err(|e| MidiError::ConnectionError(e.to_string()))?;
        names.push(name);
    }
    Ok(names)
}

/// 名前でMIDI出力ポートに接続する
pub fn connect(port_name: &str) -> Result<MidiOutputConnection, MidiError> {
    let output = MidiOutput::new("lcvgc")
        .map_err(|e| MidiError::ConnectionError(e.to_string()))?;
    let ports = output.ports();
    let port = find_port(&output, &ports, port_name)?;
    output
        .connect(&port, port_name)
        .map_err(|e| MidiError::ConnectionError(e.to_string()))
}

/// ポート一覧から名前に一致するポートを探す
fn find_port(
    output: &MidiOutput,
    ports: &[MidiOutputPort],
    name: &str,
) -> Result<MidiOutputPort, MidiError> {
    for port in ports {
        if let Ok(port_name) = output.port_name(port) {
            if port_name == name {
                return Ok(port.clone());
            }
        }
    }
    Err(MidiError::PortNotFound(name.to_string()))
}

/// MIDIポート接続を管理する構造体
pub struct PortManager {
    connections: HashMap<String, MidiOutputConnection>,
}

impl PortManager {
    pub fn new() -> Self {
        PortManager {
            connections: HashMap::new(),
        }
    }

    /// 論理名とポート名を指定して接続する
    pub fn connect(&mut self, name: &str, port_name: &str) -> Result<(), MidiError> {
        let conn = connect(port_name)?;
        self.connections.insert(name.to_string(), conn);
        Ok(())
    }

    /// 論理名の接続を切断する
    pub fn disconnect(&mut self, name: &str) {
        if let Some(conn) = self.connections.remove(name) {
            conn.close();
        }
    }

    /// 論理名の接続にMIDIバイト列を送信する
    pub fn send(&mut self, name: &str, msg: &[u8]) -> Result<(), MidiError> {
        let conn = self
            .connections
            .get_mut(name)
            .ok_or_else(|| MidiError::PortNotFound(name.to_string()))?;
        conn.send(msg)
            .map_err(|e| MidiError::SendError(e.to_string()))
    }

    /// 論理名が接続済みかどうかを返す
    pub fn is_connected(&self, name: &str) -> bool {
        self.connections.contains_key(name)
    }

    /// 接続済みの論理名一覧を返す
    pub fn connected_names(&self) -> Vec<&str> {
        self.connections.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for PortManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_manager_new_is_empty() {
        let pm = PortManager::new();
        assert!(pm.connected_names().is_empty());
    }

    #[test]
    fn port_manager_default_is_empty() {
        let pm = PortManager::default();
        assert!(pm.connected_names().is_empty());
    }

    #[test]
    fn port_manager_is_connected_false_when_empty() {
        let pm = PortManager::new();
        assert!(!pm.is_connected("synth1"));
    }

    #[test]
    fn port_manager_send_to_unknown_returns_error() {
        let mut pm = PortManager::new();
        let result = pm.send("nonexistent", &[0x90, 60, 100]);
        assert!(result.is_err());
    }

    #[test]
    fn port_manager_disconnect_unknown_is_noop() {
        let mut pm = PortManager::new();
        pm.disconnect("nonexistent"); // should not panic
    }

    #[test]
    #[ignore] // 実MIDIハードウェアが必要
    fn list_ports_returns_ok() {
        let result = list_ports();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore] // 実MIDIハードウェアが必要
    fn list_input_ports_returns_ok() {
        let result = list_input_ports();
        assert!(result.is_ok());
    }

    #[test]
    #[ignore] // 実MIDIハードウェアが必要
    fn connect_nonexistent_port_returns_error() {
        let result = connect("Nonexistent Port 12345");
        assert!(result.is_err());
    }
}
