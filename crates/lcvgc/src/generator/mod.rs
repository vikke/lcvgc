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

/// ファイル内容と拡張子から入力フォーマットを自動判定する。
///
/// 判定優先順位:
/// 1. 先頭 4 バイトが `MThd` (SMF magic) → `Smf`
/// 2. 拡張子 `.mid` / `.midi` → `Smf` (ただし magic と不一致ならエラー)
/// 3. 拡張子 `.mdx` → `Mdx`
/// 4. それ以外 → `UnsupportedFormat`
///
/// Detect the input format from file contents and extension.
///
/// # Arguments
/// * `bytes` - ファイル先頭部分 (最低 4 バイト推奨)
/// * `path` - ファイルパス (拡張子参照用)
pub fn detect_format(bytes: &[u8], path: &Path) -> Result<InputFormat, GeneratorError> {
    const SMF_MAGIC: &[u8; 4] = b"MThd";

    let has_smf_magic = bytes.len() >= 4 && &bytes[..4] == SMF_MAGIC;
    if has_smf_magic {
        return Ok(InputFormat::Smf);
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    match ext.as_deref() {
        Some("mid") | Some("midi") => {
            // 拡張子は SMF だが magic が無い → ファイル破損か別形式
            Err(GeneratorError::UnsupportedFormat(format!(
                "{} は .mid/.midi だが SMF magic (MThd) が見つからない",
                path.display()
            )))
        }
        Some("mdx") => Ok(InputFormat::Mdx),
        Some(other) => Err(GeneratorError::UnsupportedFormat(format!(
            "未対応の拡張子: .{}",
            other
        ))),
        None => Err(GeneratorError::UnsupportedFormat(format!(
            "拡張子なしのファイル: {}",
            path.display()
        ))),
    }
}

/// パスを与え、フォーマットを自動判定して DSL 文字列を返すワンショット関数。
///
/// CLI の位置引数 1 個用エントリポイント。
///
/// Auto-detect entry point. Reads `path`, sniffs its format, and emits DSL.
pub fn generate_from_path_auto(path: &Path) -> Result<String, GeneratorError> {
    let bytes = std::fs::read(path)?;
    let format = detect_format(&bytes, path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    generate(format, &bytes, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_smf_by_magic() {
        let bytes = b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x00\x60";
        let path = Path::new("anything.bin");
        assert_eq!(detect_format(bytes, path).unwrap(), InputFormat::Smf);
    }

    #[test]
    fn detect_smf_magic_wins_over_mdx_extension() {
        // 拡張子が .mdx でも先頭が MThd なら SMF として扱う (magic 優先)
        let bytes = b"MThd\x00\x00\x00\x06\x00\x00\x00\x01\x00\x60";
        let path = Path::new("weird.mdx");
        assert_eq!(detect_format(bytes, path).unwrap(), InputFormat::Smf);
    }

    #[test]
    fn detect_mdx_by_extension() {
        // MDX には固定 magic がないので拡張子で判定する
        let bytes = b"Some Shift-JIS title \x0d\x0a\x1a...";
        let path = Path::new("song.mdx");
        assert_eq!(detect_format(bytes, path).unwrap(), InputFormat::Mdx);
    }

    #[test]
    fn detect_smf_by_extension_only_fails_without_magic() {
        // .mid 拡張子だが magic が無い → 破損扱いでエラー
        let bytes = b"\x00\x00\x00\x00";
        let path = Path::new("broken.mid");
        let err = detect_format(bytes, path).unwrap_err();
        assert!(matches!(err, GeneratorError::UnsupportedFormat(_)));
    }

    #[test]
    fn detect_unknown_extension_fails() {
        let bytes = b"random";
        let path = Path::new("data.txt");
        let err = detect_format(bytes, path).unwrap_err();
        assert!(matches!(err, GeneratorError::UnsupportedFormat(_)));
    }

    #[test]
    fn detect_no_extension_fails() {
        let bytes = b"random";
        let path = Path::new("noextension");
        let err = detect_format(bytes, path).unwrap_err();
        assert!(matches!(err, GeneratorError::UnsupportedFormat(_)));
    }

    #[test]
    fn detect_extension_is_case_insensitive() {
        let bytes = b"x";
        let path = Path::new("Song.MDX");
        assert_eq!(detect_format(bytes, path).unwrap(), InputFormat::Mdx);
    }

    #[test]
    fn detect_handles_short_file() {
        // 4 バイト未満でも panic しないこと
        let bytes = b"MT";
        let path = Path::new("song.mdx");
        assert_eq!(detect_format(bytes, path).unwrap(), InputFormat::Mdx);
    }
}
