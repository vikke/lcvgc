//! `lcvgc` バイナリの内部 API を結合テストから利用可能にするためのライブラリ層。
//!
//! main.rs から `device` の動的登録に関するロジック (`apply_device_event`、
//! `run_device_event_receiver_with_initial`) を切り出し、`MidirSink` 構築を
//! sink builder closure として注入できる形にしておくことで、`MockSink` ベースの
//! テストが書ける。
//!
//! Library layer that exposes binary internals to integration tests. Hosts the
//! dynamic-device registration handlers (`apply_device_event`,
//! `run_device_event_receiver_with_initial`) with the `MidirSink` build step
//! abstracted as a sink-builder closure, so tests can drive the same code with
//! `MockSink`.

use std::collections::HashMap;

use lcvgc_core::engine::device_event::DeviceEvent;
use lcvgc_core::engine::midi_sink::{send_all_notes_off_all_channels, MidirSink};
use lcvgc_core::engine::playback::{BoxedSink, SharedSinks, SinksNotify};
use lcvgc_core::midi::port::PortManager;
use lcvgc_core::midi::MidiError;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// 本番用の sink builder（`MidirSink` を実 MIDI ポートに接続して構築する）
///
/// `apply_device_event` に渡されるとこの closure が `Upsert` 毎に呼ばれ、
/// `PortManager::connect` の失敗は `MidiError` として伝播する（呼び出し側で warn
/// ログにする想定）。
///
/// Production sink builder that connects a `MidirSink` to a real MIDI port.
/// `apply_device_event` invokes it on every `Upsert`; connection failures are
/// returned as `MidiError` so the caller can log a warning.
pub fn build_midir_sink(name: &str, port: &str) -> Result<BoxedSink, MidiError> {
    let mut pm = PortManager::new();
    pm.connect(name, port)?;
    Ok(Box::new(MidirSink::new(pm, name.to_string())))
}

/// 単一の `DeviceEvent` を共有 sink マップに反映する（builder 注入形）
///
/// PR #54: 同名 + 同 port の no-op 判定は呼び出し側 (receiver loop) の責務。
/// 本関数は受け取った `Upsert` を必ず「旧 sink を AllNotesOff 送出後に drop し、
/// `build_sink` を呼んで新 sink を insert」する流れで処理する。
/// 接続失敗は warn ログを出して当該 device のみスキップし、既存 sink には触れない。
/// 反映が成功した場合のみ `notify.notify_one()` で driver を起こす。
///
/// Applies one `DeviceEvent` to the shared sink map. The receiver loop is
/// responsible for deduplicating same-name + same-port events; this function
/// always drops the old sink (after sending AllNotesOff on every channel) and
/// invokes `build_sink` to create the replacement. Build failures are logged at
/// `warn` and the existing sink is left intact. `notify.notify_one()` runs only
/// when the map was successfully mutated.
///
/// # Type parameters
/// * `F` - sink builder closure。引数 `(name, port)` を受け取り `BoxedSink` を返す
/// * `E` - builder が返すエラー型（`std::fmt::Display` を要求するのは warn ログ用）
pub async fn apply_device_event<F, E>(
    event: DeviceEvent,
    sinks: &SharedSinks,
    notify: &SinksNotify,
    build_sink: &F,
) where
    F: Fn(&str, &str) -> Result<BoxedSink, E>,
    E: std::fmt::Display,
{
    match event {
        DeviceEvent::Upsert { name, port } => {
            // 旧 sink を取り出して AllNotesOff を送出してから drop（port 張り替え時の
            // 安全停止）。新規追加の場合は旧 sink は None なのでスキップされる。
            // Drop the old sink—if any—after sending AllNotesOff on all channels.
            {
                let mut map = sinks.lock().await;
                if let Some(mut old) = map.remove(&name) {
                    if let Err(e) = send_all_notes_off_all_channels(old.as_mut()) {
                        warn!(
                            "  旧 sink への AllNotesOff 送出に失敗: device={} ({})",
                            name, e
                        );
                    }
                    // old はスコープ末で drop され、内部の sink 接続が閉じる
                    drop(old);
                }
            }

            match build_sink(&name, &port) {
                Ok(sink) => {
                    {
                        let mut map = sinks.lock().await;
                        map.insert(name.clone(), sink);
                    }
                    info!("  MIDI device 動的登録: {} -> {}", name, port);
                    notify.notify_one();
                }
                Err(e) => {
                    warn!(
                        "  MIDI device 接続失敗 (動的登録): {} -> {} ({}). この device への送出はスキップします。",
                        name, port, e
                    );
                }
            }
        }
    }
}

/// `DeviceEvent` の receiver loop（builder 注入形）
///
/// PR #54: Evaluator から流れてくる `DeviceEvent` を消費し、各イベントを
/// `apply_device_event` で共有 sink マップに反映する。同名 + 同 port の重複登録を
/// 避けるため、device 名 → 現 port の対応を内部 `HashMap` で記憶し、変化が
/// 無いイベントは skip する。`initial_ports` で起動時 sink の重複弾きを行う。
///
/// Consumes `DeviceEvent`s emitted by the Evaluator and applies them to the
/// shared sink map via the injected `build_sink`. The loop deduplicates
/// `Upsert`s with no port change against `current_ports` (seeded from
/// `initial_ports` for startup parity).
pub async fn run_device_event_receiver_with_initial<F, E>(
    mut rx: mpsc::UnboundedReceiver<DeviceEvent>,
    sinks: SharedSinks,
    notify: SinksNotify,
    initial_ports: HashMap<String, String>,
    build_sink: F,
) where
    F: Fn(&str, &str) -> Result<BoxedSink, E>,
    E: std::fmt::Display,
{
    let mut current_ports: HashMap<String, String> = initial_ports;
    while let Some(event) = rx.recv().await {
        match &event {
            DeviceEvent::Upsert { name, port } => {
                if current_ports.get(name) == Some(port) {
                    // 同名 + 同 port は no-op（sink を張り替えない）
                    continue;
                }
                current_ports.insert(name.clone(), port.clone());
            }
        }
        apply_device_event(event, &sinks, &notify, &build_sink).await;
    }
}
