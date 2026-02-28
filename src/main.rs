mod cli;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    let running = Arc::new(AtomicBool::new(true));
    let pair = Arc::new((Mutex::new(false), Condvar::new()));

    let running_clone = Arc::clone(&running);
    let pair_clone = Arc::clone(&pair);
    ctrlc::set_handler(move || {
        running_clone.store(false, Ordering::SeqCst);
        let (lock, cvar) = &*pair_clone;
        let mut shutdown = lock.lock().unwrap();
        *shutdown = true;
        cvar.notify_one();
    })
    .expect("Ctrl+Cハンドラの設定に失敗");

    println!("lcvgc v{} 起動中...", env!("CARGO_PKG_VERSION"));
    println!("  ポート: {}", cli.port);
    println!("  ログレベル: {}", cli.log_level);
    if let Some(ref file) = cli.file {
        println!("  DSLファイル: {}", file.display());
    }
    if let Some(ref device) = cli.midi_device {
        println!("  MIDIデバイス: {}", device);
    }
    println!("Ctrl+C で終了します");

    let (lock, cvar) = &*pair;
    let mut shutdown = lock.lock().unwrap();
    while !*shutdown {
        shutdown = cvar.wait(shutdown).unwrap();
    }

    println!("\nlcvgc を終了します");
}
