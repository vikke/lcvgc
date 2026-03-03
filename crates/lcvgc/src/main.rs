mod cli;

use std::sync::Arc;

use clap::Parser;
use cli::Cli;
use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::watcher::{run_hot_reload, WatcherConfig};
use lcvgc_core::midi::monitor::{log_startup_ports, run_port_monitor, PortMonitorConfig};
use lcvgc_core::server::run_server;
use tokio::sync::Mutex;
use tracing::{error, info};

/// tracing初期化
///
/// # Arguments
/// * `log_level` - ログレベル文字列 (e.g. "info", "debug")
fn init_tracing(log_level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_new(log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();
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

    // 起動時MIDIポート一覧表示
    log_startup_ports();

    // MIDIポートホットプラグ監視
    tokio::spawn(async {
        run_port_monitor(PortMonitorConfig::default()).await;
    });

    let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));

    if let Some(ref file) = cli.file {
        let mut ev = evaluator.lock().await;
        match ev.load_file(&file.to_string_lossy()) {
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
