mod cli;

use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use clap::Parser;
use cli::Cli;
use lcvgc_core::engine::config::Config;
use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::watcher::{run_hot_reload, WatcherConfig};
use lcvgc_core::midi::monitor::{log_startup_ports, run_port_monitor, PortMonitorConfig};
use lcvgc_core::server::run_server;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

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
fn load_config(config_path: &PathBuf, explicit: bool) -> Config {
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

    info!("Ctrl+C で終了します");

    if let Err(e) = run_server(evaluator, cli.port).await {
        error!("サーバーエラー: {}", e);
    }
}
