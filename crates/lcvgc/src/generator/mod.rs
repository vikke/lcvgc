//! 外部音楽フォーマット → lcvgc DSL ジェネレーターのトップモジュール。
//!
//! 設計は「reader → 共通中間表現 (Score) → emitter」の3層構造。
//! - `score`: 共通中間表現 (IR)。Track と Event の並び、テンポ、拍子等を保持。
//! - `quantize`: 物理時間 (tick) を lcvgc DSL の音価表現へ量子化する。
//! - `emitter`: Score から lcvgc DSL 文字列を生成する。
//! - `readers`: 各フォーマットのパーサ。`ScoreReader` を実装し Score を返す。
//!
//! Top module of the external music format → lcvgc DSL generator.
//!
//! Three-layer design: `reader → Score IR → emitter`. New formats are added
//! by implementing `ScoreReader` and registering them in the CLI front-end.

/// 共通中間表現
/// Common intermediate representation
pub mod score;

/// tick → lcvgc Duration 量子化
/// tick → lcvgc Duration quantizer
pub mod quantize;

/// Score → lcvgc DSL 文字列の出力器
/// Score → lcvgc DSL string emitter
pub mod emitter;

/// フォーマット別 reader 群
/// Per-format readers
pub mod readers;

use std::path::Path;

/// 外部フォーマット → Score IR の reader が満たすトレイト。
///
/// 新フォーマットを追加する場合は本トレイトを実装し、CLI から呼び出せるよう
/// `readers::mod` で再エクスポートする。
///
/// Trait that every format reader implements. To add a new format, implement
/// this trait and re-export the reader from `readers::mod`.
pub trait ScoreReader {
    /// バイナリ列 (またはテキスト) から Score を構築する。
    ///
    /// Parses the given byte slice into a [`score::Score`].
    ///
    /// # Arguments
    /// * `bytes` - 入力ファイルのバイト列
    /// * `source_name` - 表示用の入力名（ファイル名等）
    ///
    /// # Errors
    /// パース失敗時は `GeneratorError` を返す。
    fn read(&self, bytes: &[u8], source_name: &str) -> Result<score::Score, GeneratorError>;
}

/// ジェネレーター層のエラー型。
///
/// reader / emitter のいずれで発生したかをバリアントで区別する。
/// Error type for the generator layer.
#[derive(Debug, thiserror::Error)]
pub enum GeneratorError {
    /// I/O エラー
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// reader 内部のパース失敗
    /// Parse error inside a reader
    #[error("parse error ({format}): {message}")]
    Parse {
        /// フォーマット名 (例: "smf", "mdx")
        /// Format name
        format: &'static str,
        /// 失敗内容
        /// Human-readable message
        message: String,
    },

    /// emitter で表現できない構造（量子化不能・未対応イベント等）
    /// Structure that the emitter cannot represent
    #[error("emit error: {0}")]
    Emit(String),

    /// 未対応フォーマット
    /// Unsupported format
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
}

/// 入力フォーマットの識別子。
///
/// 新フォーマット追加時は本 enum と `from_str` を更新する。
/// Identifier of an input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    /// Standard MIDI File (.mid)
    Smf,
    /// MDX (X68000 MXDRV, FM 部のみ)
    /// MDX (X68000 MXDRV, FM section only)
    Mdx,
}

impl std::str::FromStr for InputFormat {
    type Err = GeneratorError;

    /// 文字列から `InputFormat` を解決する (大文字小文字無視)。
    ///
    /// Parses a format name (case-insensitive).
    fn from_str(s: &str) -> Result<Self, GeneratorError> {
        match s.to_ascii_lowercase().as_str() {
            "smf" | "mid" | "midi" => Ok(InputFormat::Smf),
            "mdx" => Ok(InputFormat::Mdx),
            other => Err(GeneratorError::UnsupportedFormat(other.to_string())),
        }
    }
}

/// フォーマットを指定して `Score` を読み出し、DSL 文字列を返すワンショット関数。
///
/// One-shot helper: read `bytes` as `format` and emit the DSL string.
///
/// # Arguments
/// * `format` - 入力フォーマット
/// * `bytes` - 入力ファイルのバイト列
/// * `source_name` - 表示用入力名 (ファイル名や stdin など)
///
/// # Errors
/// reader か emitter のいずれかが失敗した場合に `GeneratorError`。
pub fn generate(
    format: InputFormat,
    bytes: &[u8],
    source_name: &str,
) -> Result<String, GeneratorError> {
    let score = match format {
        InputFormat::Smf => readers::smf::SmfReader.read(bytes, source_name)?,
        InputFormat::Mdx => readers::mdx::MdxReader.read(bytes, source_name)?,
    };
    emitter::emit(&score)
}

/// ファイルパスから `generate` を呼び出すユーティリティ。
///
/// Convenience wrapper around `generate` that reads a file.
pub fn generate_from_path(format: InputFormat, path: &Path) -> Result<String, GeneratorError> {
    let bytes = std::fs::read(path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    generate(format, &bytes, &name)
}
