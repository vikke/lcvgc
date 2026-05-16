//! `lcvgc-gen` バイナリ: 外部音楽フォーマット → lcvgc DSL のジェネレーター。
//!
//! 使用例:
//! ```sh
//! lcvgc-gen song.mid    # 標準出力に .cvg を吐く (拡張子+magicで自動判定)
//! lcvgc-gen song.mdx
//! ```
//!
//! 設計: `crates/lcvgc/src/generator/` の Score IR + Reader trait + Emitter を
//! 駆動するだけの薄い CLI。format は `detect_format` がファイル内容と拡張子
//! から自動判定するため、ユーザーはパスを 1 つ渡すだけで動く。
//!
//! Thin CLI front-end that drives the `lcvgc::generator` library. The input
//! format is auto-detected from magic bytes and the file extension.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use lcvgc::generator::generate_from_path_auto;

/// `lcvgc-gen` の CLI 引数定義。
#[derive(Parser, Debug)]
#[command(
    name = "lcvgc-gen",
    about = "外部音楽フォーマット (SMF / MDX) を lcvgc DSL に変換する"
)]
struct Cli {
    /// 入力ファイルパス (拡張子と内容から SMF / MDX を自動判定)
    input: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match generate_from_path_auto(&cli.input) {
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
