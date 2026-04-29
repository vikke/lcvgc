mod cli;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use clap::Parser;
use cli::Cli;
use lcvgc::{build_midir_sink, run_device_event_receiver_with_initial};
use lcvgc_core::engine::clock::Clock;
use lcvgc_core::engine::config::Config;
use lcvgc_core::engine::device_event::DeviceEvent;
use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::midi_sink::MidirSink;
use lcvgc_core::engine::playback::{run_driver_with_shared, BoxedSink, SharedSinks, SinksNotify};
use lcvgc_core::engine::watcher::{run_hot_reload, WatcherConfig};
use lcvgc_core::midi::monitor::{log_startup_ports, run_port_monitor, PortMonitorConfig};
use lcvgc_core::midi::port::PortManager;
use lcvgc_core::server::run_server;
use tokio::sync::{mpsc, Mutex, Notify};
use tracing::{error, info, warn};

/// 単一ポート名から `MidirSink` を構築するヘルパ
///
/// DSL に `device` ブロックが無い場合の後方互換経路として、論理名
/// `"default"` で CLI 引数 `--midi-device` に指定されたポートに接続する。
///
/// Builds a single-port `MidirSink` for the backward-compatibility path
/// when the DSL has no `device` block. Uses the logical name `"default"`.
///
/// # Arguments
/// * `port_name` - 接続先ポート名（`list_ports()` が返す文字列のいずれか）
///
/// # Errors
/// ポート接続に失敗した場合は `MidiError` を返す。
fn build_default_sink(port_name: &str) -> Result<MidirSink, lcvgc_core::midi::MidiError> {
    let mut pm = PortManager::new();
    pm.connect("default", port_name)?;
    Ok(MidirSink::new(pm, "default".to_string()))
}

/// DSL の `device <name> { port "..." }` ブロックから複数 sink マップを構築する
///
/// Issue #49: 各 `DeviceDef.port` を個別の `PortManager` に接続し、
/// 論理名 `DeviceDef.name` をキーとする `MidirSink` を sinks に詰める。
/// ポート接続に失敗した device は warn ログして当該 sink のみスキップし、
/// 他 device への振り分けは継続する。
///
/// Builds a per-device sink map from `DeviceDef` entries registered in the
/// evaluator's `Registry` (Issue #49). Connection failures for one device
/// are logged at `warn` and skipped; the remaining devices continue to be
/// wired so that routing for healthy devices is unaffected.
fn build_sinks_from_registry(evaluator: &Evaluator) -> HashMap<String, BoxedSink> {
    let mut sinks: HashMap<String, BoxedSink> = HashMap::new();
    let registry = evaluator.registry();
    for name in registry.device_names() {
        let Some(def) = registry.get_device(&name) else {
            continue;
        };
        let mut pm = PortManager::new();
        match pm.connect(&name, &def.port) {
            Ok(()) => {
                info!("  MIDI device 接続: {} -> {}", name, def.port);
                let sink: BoxedSink = Box::new(MidirSink::new(pm, name.clone()));
                sinks.insert(name.clone(), sink);
            }
            Err(e) => {
                warn!(
                    "  MIDI device 接続失敗: {} -> {} ({}). この device への送出はスキップします。",
                    name, def.port, e
                );
            }
        }
    }
    sinks
}

/// 設定ファイルパスを解決する。
/// --config 指定時はそのパスを返し、未指定時は ~/.config/lcvgc/config.toml を返す。
///
/// # Arguments
/// * `cli_config` - CLIで指定された設定ファイルパス
///
/// # Returns
/// `(PathBuf, bool)` - (解決済みパス, 明示指定されたかどうか)
fn resolve_config_path(cli_config: &Option<PathBuf>) -> (PathBuf, bool) {
    match cli_config {
        Some(p) => (p.clone(), true),
        None => {
            let default = dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("~/.config"))
                .join("lcvgc")
                .join("config.toml");
            (default, false)
        }
    }
}

/// 設定ファイルを読み込む。
/// 明示指定時はファイル不在でエラー終了、デフォルトパス時はサイレントにデフォルト値を使用。
///
/// # Arguments
/// * `config_path` - 設定ファイルのパス
/// * `explicit` - --config で明示指定されたかどうか
fn load_config(config_path: &Path, explicit: bool) -> Config {
    match Config::load(config_path) {
        Ok(config) => {
            if config_path.exists() {
                info!("  設定ファイル: {}", config_path.display());
            }
            config
        }
        Err(e) => {
            if explicit {
                error!(
                    "設定ファイル読み込みエラー: {} ({})",
                    config_path.display(),
                    e
                );
                process::exit(1);
            }
            warn!(
                "設定ファイル読み込みエラー: {} ({})",
                config_path.display(),
                e
            );
            Config::default()
        }
    }
}

/// tracing初期化
///
/// # Arguments
/// * `log_level` - ログレベル文字列 (e.g. "info", "debug")
fn init_tracing(log_level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    init_tracing(&cli.log_level);

    info!("lcvgc v{} 起動中...", env!("CARGO_PKG_VERSION"));
    info!("  ポート: {}", cli.port);
    info!("  ログレベル: {}", cli.log_level);
    if let Some(ref file) = cli.file {
        info!("  DSLファイル: {}", file.display());
    }
    if let Some(ref device) = cli.midi_device {
        info!("  MIDIデバイス: {}", device);
    }

    // 設定ファイル読み込み
    let (config_path, explicit) = resolve_config_path(&cli.config);
    let config = load_config(&config_path, explicit);
    let default_bpm = config.default_bpm.unwrap_or(120.0);

    // 起動時MIDIポート一覧表示
    log_startup_ports();

    // MIDIポートホットプラグ監視
    tokio::spawn(async {
        run_port_monitor(PortMonitorConfig::default()).await;
    });

    let evaluator = Arc::new(Mutex::new(Evaluator::new(default_bpm)));

    // PR #54: device の動的登録経路を準備する
    //
    // - `DeviceEvent` の mpsc を作り、tx を Evaluator に配線
    // - 共有 sink マップ (`SharedSinks`) と `SinksNotify` を生成し、
    //   PlaybackDriver / receiver loop / 起動時 sink builder の三者で共有
    //
    // PR #54: wire up dynamic device registration. Create the `DeviceEvent`
    // mpsc, hand its tx to the Evaluator, and share the sink map / notifier
    // between the playback driver, the receiver task, and the startup sink
    // builder.
    let (device_event_tx, device_event_rx) = mpsc::unbounded_channel::<DeviceEvent>();
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(device_event_tx);
    }
    let shared_sinks: SharedSinks = Arc::new(Mutex::new(HashMap::new()));
    let sinks_notify: SinksNotify = Arc::new(Notify::new());

    if let Some(ref file) = cli.file {
        let mut ev = evaluator.lock().await;
        match ev.eval_file(file) {
            Ok(results) => info!("  {} ブロックを評価しました", results.len()),
            Err(e) => error!("  ファイル読み込みエラー: {}", e),
        }
    }

    // ホットリロード
    if let Some(ref watch_path) = cli.watch {
        info!("  ホットリロード: {}", watch_path.display());
        let ev = evaluator.clone();
        let path = watch_path.clone();
        tokio::spawn(async move {
            run_hot_reload(ev, path, WatcherConfig::default()).await;
        });
    }

    // 起動時の MIDI sink マップ初期化（Issue #49 + PR #54）
    //
    // file 経由で評価された device ブロックは、上記の `set_device_event_tx`
    // 配線後に評価されているため、receiver 経由でも共有 sink マップに到着する。
    // ただし receiver タスクはまだ spawn していないので、ここでは file 経由で
    // 既に registry に入った device を従来同様 `build_sinks_from_registry` で
    // 取り込み、同じ device 名の Upsert イベントは receiver 側で同 port 判定
    // により no-op となるよう、receiver 側の `current_ports` 初期値も用意する。
    //
    // Initialize the sink map at startup. Devices already evaluated from `--file`
    // are pulled in via `build_sinks_from_registry`; the matching `Upsert`
    // events that were queued during eval_file will be deduplicated by the
    // receiver loop using the `initial_ports` snapshot we hand to it below.
    let initial_ports: HashMap<String, String> = {
        let ev = evaluator.lock().await;
        let initial = build_sinks_from_registry(&ev);
        let port_map: HashMap<String, String> = ev
            .registry()
            .device_names()
            .into_iter()
            .filter_map(|name| {
                ev.registry()
                    .get_device(&name)
                    .map(|d| (name, d.port.clone()))
            })
            .collect();
        // 共有 sink マップに初期 sink を移し替える
        // Move the initial sinks into the shared map.
        let mut map = shared_sinks.lock().await;
        for (name, sink) in initial {
            map.insert(name, sink);
        }
        port_map
    };

    if let Some(ref port_name) = cli.midi_device {
        match build_default_sink(port_name) {
            Ok(sink) => {
                info!("  MIDI default sink 接続: {}", port_name);
                let mut map = shared_sinks.lock().await;
                map.insert("default".to_string(), Box::new(sink));
            }
            Err(e) => {
                warn!(
                    "  --midi-device の接続に失敗しました: {} ({}). default sink は登録しません。",
                    port_name, e
                );
            }
        }
    }

    // PR #54/#55: device の動的登録 receiver タスクを spawn
    //
    // PR #54/#55: spawn the dynamic device registration receiver task.
    {
        let sinks_for_rx = shared_sinks.clone();
        let notify_for_rx = sinks_notify.clone();
        let evaluator_for_rx = evaluator.clone();
        tokio::spawn(async move {
            run_device_event_receiver_with_initial(
                device_event_rx,
                sinks_for_rx,
                notify_for_rx,
                initial_ports,
                build_midir_sink,
                evaluator_for_rx,
            )
            .await;
        });
    }

    // PR #54: PlaybackDriver は常時 spawn する。sinks が空なら
    // `run_driver_with_shared` 内部で notify を待機し、device の動的登録で
    // 起こされたタイミングから tick ループに入る。
    //
    // PR #54: always spawn the playback driver. With no sinks, it parks on
    // `notify.notified()` inside `run_driver_with_shared` and starts ticking
    // once a device has been registered dynamically.
    {
        let sink_count = { shared_sinks.lock().await.len() };
        if sink_count == 0 {
            info!("  MIDI sink 未登録: device の動的登録待機中（device ブロックを eval すると駆動を開始します）");
        } else {
            let names: Vec<String> = shared_sinks.lock().await.keys().cloned().collect();
            info!("  MIDI 再生ドライバを起動: sinks={:?}", names);
        }
        let ev = evaluator.clone();
        let sinks_for_driver = shared_sinks.clone();
        let notify_for_driver = sinks_notify.clone();
        let clock = Clock::new(default_bpm);
        tokio::spawn(async move {
            run_driver_with_shared(ev, sinks_for_driver, notify_for_driver, clock).await;
        });
    }

    info!("Ctrl+C で終了します");

    if let Err(e) = run_server(evaluator, cli.port).await {
        error!("サーバーエラー: {}", e);
    }
}
