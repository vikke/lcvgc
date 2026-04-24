mod cli;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use clap::Parser;
use cli::Cli;
use lcvgc_core::engine::clock::Clock;
use lcvgc_core::engine::config::Config;
use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::midi_sink::MidirSink;
use lcvgc_core::engine::playback::{run_driver, BoxedSink};
use lcvgc_core::engine::watcher::{run_hot_reload, WatcherConfig};
use lcvgc_core::midi::monitor::{log_startup_ports, run_port_monitor, PortMonitorConfig};
use lcvgc_core::midi::port::PortManager;
use lcvgc_core::server::run_server;
use tokio::sync::Mutex;
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

    // MIDI 再生ドライバの起動（Issue #49: 複数 device 対応）
    //
    // 1. DSL の `device` ブロックから sink マップを構築（失敗した device は warn & skip）
    // 2. `--midi-device` 指定時は後方互換の "default" sink を追加
    // 3. sinks が空でなければ PlaybackDriver を起動
    //
    // Multi-device playback driver bootstrap (Issue #49):
    // - Build a per-device sink map from DSL `device` blocks
    // - Add a legacy `"default"` sink from `--midi-device` if provided
    // - Spawn the driver only when at least one sink was wired
    let mut sinks: HashMap<String, BoxedSink> = {
        let ev = evaluator.lock().await;
        build_sinks_from_registry(&ev)
    };

    if let Some(ref port_name) = cli.midi_device {
        match build_default_sink(port_name) {
            Ok(sink) => {
                info!("  MIDI default sink 接続: {}", port_name);
                sinks.insert("default".to_string(), Box::new(sink));
            }
            Err(e) => {
                warn!(
                    "  --midi-device の接続に失敗しました: {} ({}). default sink は登録しません。",
                    port_name, e
                );
            }
        }
    }

    if !sinks.is_empty() {
        info!(
            "  MIDI 再生ドライバを起動: sinks={:?}",
            sinks.keys().collect::<Vec<_>>()
        );
        let ev = evaluator.clone();
        let clock = Clock::new(default_bpm);
        tokio::spawn(async move {
            run_driver(ev, sinks, clock).await;
        });
    } else {
        info!("  MIDI sink 未登録のため、再生ドライバは起動しません");
    }

    info!("Ctrl+C で終了します");

    if let Err(e) = run_server(evaluator, cli.port).await {
        error!("サーバーエラー: {}", e);
    }
}
