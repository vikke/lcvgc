//! MIDIポート監視モジュール
//! MIDI port monitor module
//!
//! 起動時のポート一覧表示と、ポーリングによるホットプラグ検出を提供する。
//! Provides startup port listing and hot-plug detection via polling.
//! midir 0.10 にはホットプラグAPIがないため、定期ポーリング方式で実装している。
//! Since midir 0.10 lacks a hot-plug API, periodic polling is used instead.

use std::collections::HashSet;

use tracing::info;

use super::port::{list_input_ports, list_ports};

/// ポーリング間隔の設定
/// Configuration for polling interval
#[derive(Debug, Clone)]
pub struct PortMonitorConfig {
    /// ポーリング間隔（ミリ秒）
    /// Polling interval in milliseconds
    pub interval_ms: u64,
}

impl Default for PortMonitorConfig {
    /// デフォルトは2000ms間隔
    /// Default is 2000ms interval
    fn default() -> Self {
        Self { interval_ms: 2000 }
    }
}

/// ある時点のMIDIポート名スナップショット
/// Snapshot of MIDI port names at a given point in time
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortSnapshot {
    /// MIDI出力ポート名の集合
    /// Set of MIDI output port names
    output: HashSet<String>,
    /// MIDI入力ポート名の集合
    /// Set of MIDI input port names
    input: HashSet<String>,
}

/// 現在のMIDIポート一覧をスナップショットとして取得する。
/// いずれかの列挙でエラーが発生した場合はNoneを返す（誤った切断通知を防ぐ）。
/// Collects the current MIDI port list as a snapshot.
/// Returns None if any port enumeration fails (to prevent false disconnection notifications).
fn collect_ports() -> Option<PortSnapshot> {
    let output = list_ports().ok()?;
    let input = list_input_ports().ok()?;
    Some(PortSnapshot {
        output: output.into_iter().collect(),
        input: input.into_iter().collect(),
    })
}

/// 前回と今回のスナップショットを比較し、接続/切断をログ出力する。
/// 変更があった場合はtrueを返す。
/// Compares the previous and current snapshots, logging connections/disconnections.
/// Returns true if any changes were detected.
fn detect_changes(prev: &PortSnapshot, curr: &PortSnapshot) -> bool {
    let mut changed = false;

    // 出力ポートの接続検出
    for name in curr.output.difference(&prev.output) {
        info!("MIDI出力ポート 接続: {}", name);
        changed = true;
    }
    // 出力ポートの切断検出
    for name in prev.output.difference(&curr.output) {
        info!("MIDI出力ポート 切断: {}", name);
        changed = true;
    }
    // 入力ポートの接続検出
    for name in curr.input.difference(&prev.input) {
        info!("MIDI入力ポート 接続: {}", name);
        changed = true;
    }
    // 入力ポートの切断検出
    for name in prev.input.difference(&curr.input) {
        info!("MIDI入力ポート 切断: {}", name);
        changed = true;
    }

    changed
}

/// 起動時のMIDIポート一覧をログ出力する。
/// ポート列挙に失敗した場合はエラーではなく空表示にする。
/// Logs the MIDI port list at startup.
/// If port enumeration fails, displays empty instead of raising an error.
pub fn log_startup_ports() {
    let output_ports = list_ports().unwrap_or_default();
    let input_ports = list_input_ports().unwrap_or_default();

    info!("MIDI出力ポート ({}個):", output_ports.len());
    for name in &output_ports {
        info!("  out: {}", name);
    }
    info!("MIDI入力ポート ({}個):", input_ports.len());
    for name in &input_ports {
        info!("  in: {}", name);
    }
}

/// MIDIポートの変更を定期的にポーリングして監視する非同期ループ。
/// tokio::spawnで起動する想定。キャンセル（abort）で停止する。
/// Async loop that periodically polls for MIDI port changes.
/// Intended to be launched via tokio::spawn. Stopped by cancellation (abort).
pub async fn run_port_monitor(config: PortMonitorConfig) {
    let interval = tokio::time::Duration::from_millis(config.interval_ms);

    // 初回スナップショット取得（失敗時は空で開始）
    let mut prev = collect_ports().unwrap_or(PortSnapshot {
        output: HashSet::new(),
        input: HashSet::new(),
    });

    info!("MIDIポート監視開始 (間隔: {}ms)", config.interval_ms);

    loop {
        tokio::time::sleep(interval).await;

        // ポート取得失敗時はスキップしてprevを維持
        let Some(curr) = collect_ports() else {
            continue;
        };

        detect_changes(&prev, &curr);
        prev = curr;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// デフォルトのポーリング間隔が2000msであることを検証
    /// Verifies that the default polling interval is 2000ms
    #[test]
    fn default_config_interval() {
        let config = PortMonitorConfig::default();
        assert_eq!(config.interval_ms, 2000);
    }

    /// 変化なしの場合、detect_changesがfalseを返すことを検証
    /// Verifies that detect_changes returns false when there are no changes
    #[test]
    fn detect_changes_no_change() {
        let snapshot = PortSnapshot {
            output: HashSet::from(["Port A".to_string(), "Port B".to_string()]),
            input: HashSet::from(["Port C".to_string()]),
        };
        assert!(!detect_changes(&snapshot, &snapshot.clone()));
    }

    /// 出力ポートが接続された場合、detect_changesがtrueを返すことを検証
    /// Verifies that detect_changes returns true when an output port is connected
    #[test]
    fn detect_changes_output_connected() {
        let prev = PortSnapshot {
            output: HashSet::from(["Port A".to_string()]),
            input: HashSet::new(),
        };
        let curr = PortSnapshot {
            output: HashSet::from(["Port A".to_string(), "Port B".to_string()]),
            input: HashSet::new(),
        };
        assert!(detect_changes(&prev, &curr));
    }

    /// 出力ポートが切断された場合、detect_changesがtrueを返すことを検証
    /// Verifies that detect_changes returns true when an output port is disconnected
    #[test]
    fn detect_changes_output_disconnected() {
        let prev = PortSnapshot {
            output: HashSet::from(["Port A".to_string(), "Port B".to_string()]),
            input: HashSet::new(),
        };
        let curr = PortSnapshot {
            output: HashSet::from(["Port A".to_string()]),
            input: HashSet::new(),
        };
        assert!(detect_changes(&prev, &curr));
    }

    /// 入力ポートが接続された場合、detect_changesがtrueを返すことを検証
    /// Verifies that detect_changes returns true when an input port is connected
    #[test]
    fn detect_changes_input_connected() {
        let prev = PortSnapshot {
            output: HashSet::new(),
            input: HashSet::new(),
        };
        let curr = PortSnapshot {
            output: HashSet::new(),
            input: HashSet::from(["Port X".to_string()]),
        };
        assert!(detect_changes(&prev, &curr));
    }

    /// 入力ポートが切断された場合、detect_changesがtrueを返すことを検証
    /// Verifies that detect_changes returns true when an input port is disconnected
    #[test]
    fn detect_changes_input_disconnected() {
        let prev = PortSnapshot {
            output: HashSet::new(),
            input: HashSet::from(["Port X".to_string()]),
        };
        let curr = PortSnapshot {
            output: HashSet::new(),
            input: HashSet::new(),
        };
        assert!(detect_changes(&prev, &curr));
    }

    /// run_port_monitorが起動後にabortで正常停止できることを検証
    /// Verifies that run_port_monitor can be cleanly stopped via abort after startup
    #[tokio::test]
    async fn run_port_monitor_abort() {
        let config = PortMonitorConfig { interval_ms: 50 };
        let handle = tokio::spawn(run_port_monitor(config));

        // 少し待ってからabort
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        handle.abort();

        // abortされたタスクはJoinError(Cancelled)を返す
        let result = handle.await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_cancelled());
    }
}
