use clap::Parser;
use std::path::PathBuf;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");

/// lcvgc - Live CV Gate Coder
///
/// テキストベースのDSLでMIDIシーケンスを記述し、リアルタイムに評価・再生するライブコーディングエンジン
#[derive(Parser, Debug)]
#[command(name = "lcvgc", version = VERSION, arg_required_else_help = true)]
pub struct Cli {
    /// 起動時に読み込むDSLファイル (.cvg)
    #[arg(long)]
    pub file: Option<PathBuf>,

    /// LSPサーバーのリッスンポート
    #[arg(long, default_value_t = 5555)]
    pub port: u16,

    /// MIDI出力デバイス名（省略でシステムデフォルト）
    #[arg(long)]
    pub midi_device: Option<String>,

    /// ログレベル [possible values: error, warn, info, debug]
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// 設定ファイルパス
    #[arg(long)]
    pub config: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_args_shows_help() {
        let result = Cli::try_parse_from(["lcvgc"]);
        assert!(result.is_err(), "引数なしではヘルプ表示のためエラーになるべき");
    }

    #[test]
    fn test_default_args() {
        let cli = Cli::parse_from(["lcvgc", "--file", "test.cvg"]);
        assert_eq!(cli.port, 5555);
        assert_eq!(cli.log_level, "info");
        assert_eq!(cli.file.unwrap().to_str().unwrap(), "test.cvg");
        assert!(cli.midi_device.is_none());
        assert!(cli.config.is_none());
    }

    #[test]
    fn test_file_option() {
        let cli = Cli::parse_from(["lcvgc", "--file", "song.cvg"]);
        assert_eq!(cli.file.unwrap().to_str().unwrap(), "song.cvg");
    }

    #[test]
    fn test_all_options() {
        let cli = Cli::parse_from([
            "lcvgc",
            "--file", "live.cvg",
            "--port", "7777",
            "--midi-device", "IAC Driver Bus 1",
            "--log-level", "debug",
            "--config", "/home/user/.config/lcvgc/config.toml",
        ]);
        assert_eq!(cli.file.unwrap().to_str().unwrap(), "live.cvg");
        assert_eq!(cli.port, 7777);
        assert_eq!(cli.midi_device.unwrap(), "IAC Driver Bus 1");
        assert_eq!(cli.log_level, "debug");
        assert!(cli.config.is_some());
    }

    #[test]
    fn test_version_contains_git_hash() {
        // VERSION should be like "0.1.0 (abc1234)"
        assert!(VERSION.contains('('));
        assert!(VERSION.contains(')'));
    }
}
