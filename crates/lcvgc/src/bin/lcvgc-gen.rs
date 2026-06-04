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
use lcvgc::generator::{generate_from_path_auto, GenOptions};

/// `lcvgc-gen` の CLI 引数定義。
#[derive(Parser, Debug)]
#[command(
    name = "lcvgc-gen",
    about = "外部音楽フォーマット (SMF / MDX) を lcvgc DSL に変換する"
)]
struct Cli {
    /// 入力ファイルパス (拡張子と内容から SMF / MDX を自動判定)
    input: PathBuf,

    /// 生成 DSL の音程ノートのオクターブを n だけ上下させる
    /// (正で上、負で下。例: `-o 1` で 1 上げ、`-o -1` で 1 下げ)。
    /// ドラムには適用しない。
    #[arg(
        short = 'o',
        long = "octave",
        value_name = "N",
        default_value_t = 0,
        allow_hyphen_values = true
    )]
    octave: i8,

    /// 何小節ごとに小節番号コメント行を出力するか。
    /// 各演奏行の直下に、対象小節の先頭トークンの桁位置へ揃えた小節番号を
    /// `// ...N...` 形式で出力する。先頭小節 (1) は省略する。
    /// 0 を指定するとコメント行を出力しない (既定 1: 毎小節)。
    #[arg(
        short = 'b',
        long = "bars-per-marker",
        value_name = "N",
        default_value_t = 1
    )]
    bars_per_marker: u32,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let opts = GenOptions {
        octave_shift: cli.octave,
        bars_per_marker: cli.bars_per_marker,
        ..Default::default()
    };
    match generate_from_path_auto(&cli.input, &opts) {
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
