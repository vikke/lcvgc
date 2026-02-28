mod cli;

use std::sync::Arc;

use clap::Parser;
use cli::{Cli, Commands};
use lcvgc::engine::evaluator::Evaluator;
use lcvgc::lsp::run_lsp;
use lcvgc::server::run_server;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Some(Commands::Lsp) = cli.command {
        run_lsp().await;
        return;
    }

    println!("lcvgc v{} 起動中...", env!("CARGO_PKG_VERSION"));
    println!("  ポート: {}", cli.port);
    println!("  ログレベル: {}", cli.log_level);
    if let Some(ref file) = cli.file {
        println!("  DSLファイル: {}", file.display());
    }
    if let Some(ref device) = cli.midi_device {
        println!("  MIDIデバイス: {}", device);
    }

    let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));

    if let Some(ref file) = cli.file {
        let mut ev = evaluator.lock().await;
        match ev.load_file(&file.to_string_lossy()) {
            Ok(results) => println!("  {} ブロックを評価しました", results.len()),
            Err(e) => eprintln!("  ファイル読み込みエラー: {}", e),
        }
    }

    println!("Ctrl+C で終了します");

    if let Err(e) = run_server(evaluator, cli.port).await {
        eprintln!("サーバーエラー: {}", e);
    }
}
