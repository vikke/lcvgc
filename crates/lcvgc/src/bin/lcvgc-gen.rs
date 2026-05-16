//! `lcvgc-gen` バイナリ: 外部音楽フォーマット → lcvgc DSL のジェネレーター。
//!
//! 使用例:
//! ```sh
//! lcvgc-gen --format smf --input song.mid    # 標準出力に .cvg を吐く
//! lcvgc-gen --format mdx --input song.mdx
//! ```
//!
//! 設計: `crates/lcvgc/src/generator/` の Score IR + Reader trait + Emitter を
//! 駆動するだけの薄い CLI。新フォーマット追加時はライブラリ側の
//! `readers::*` と `InputFormat` に 1 件追加するだけで本 bin は変更不要に近い。
//!
//! Thin CLI front-end that drives the `lcvgc::generator` library.

use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use clap::Parser;
use lcvgc::generator::{generate_from_path, InputFormat};

/// `lcvgc-gen` の CLI 引数定義。
#[derive(Parser, Debug)]
#[command(
    name = "lcvgc-gen",
    about = "外部音楽フォーマット (SMF / MDX) を lcvgc DSL に変換する"
)]
struct Cli {
    /// 入力フォーマット (`smf` または `mdx`)
    #[arg(long, short)]
    format: String,

    /// 入力ファイルパス
    #[arg(long, short)]
    input: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = match InputFormat::from_str(&cli.format) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(2);
        }
    };
    match generate_from_path(format, &cli.input) {
        Ok(dsl) => {
            print!("{}", dsl);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {}", e);
            ExitCode::FAILURE
        }
    }
}
