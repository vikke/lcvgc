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

/// ADPCM 波形からドラム楽器を推定する分類器
/// Drum-voice classifier from ADPCM waveforms
pub mod drum_classify;

use std::path::Path;

/// ジェネレーターの出力挙動を制御するオプション群。
///
/// CLI から渡され、reader → Score 構築後の正規化や emitter での出力に影響する。
/// 後方互換のため `Default` を持ち、既存呼び出しは `GenOptions::default()` で済む。
///
/// Options controlling generator output. Passed from the CLI and applied during
/// score normalization and emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenOptions {
    /// 生成 DSL の音程ノートに適用するオクターブシフト量。
    /// 正で上、負で下。ドラムには適用しない。既定 0。
    ///
    /// Octave shift applied to pitched notes (positive up, negative down).
    /// Drums are unaffected. Defaults to 0.
    pub octave_shift: i8,

    /// ベースライン判定のしきい値 (MIDI ノート番号)。
    /// 音程トラックの平均ノートがこの値未満なら `bass` 系、以上なら `fm` 系の
    /// instrument 名を割り当てる。既定 48 (= C3)。
    ///
    /// Threshold (MIDI note) for bass-line detection. A melodic track whose mean
    /// note is below this gets a `bass` name; otherwise `fm`. Defaults to 48 (C3).
    pub bass_max_avg_note: u8,

    /// 何小節ごとに小節番号コメント行を出力するか。
    /// 各演奏行の直下に、対象小節の先頭トークンの桁位置へ揃えた小節番号を
    /// `// ...N...` 形式で出力する。先頭小節 (1) は省略する。
    /// 0 を指定するとコメント行を一切出力しない。既定 1 (毎小節)。
    ///
    /// Emit a bar-number comment line every `bars_per_marker` bars, aligned to
    /// the column of each bar's first token. Bar 1 is omitted. 0 disables the
    /// comment lines entirely. Defaults to 1 (every bar).
    pub bars_per_marker: u32,
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions {
            octave_shift: 0,
            bass_max_avg_note: 48,
            bars_per_marker: 1,
        }
    }
}

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
    opts: &GenOptions,
) -> Result<String, GeneratorError> {
    generate_with_aux(format, bytes, None, source_name, opts)
}

/// `generate` の拡張版。MDX の ADPCM ドラム解析用に、付随ファイル (PDX) の
/// バイト列を任意で渡せる。SMF では `aux_bytes` は無視される。
///
/// Extended `generate` that accepts optional auxiliary bytes (a PDX bank for
/// MDX). Ignored for SMF.
///
/// # Arguments
/// * `format` - 入力フォーマット
/// * `bytes` - 入力ファイルのバイト列
/// * `aux_bytes` - MDX の場合の PDX バイト列 (無ければ `None`)
/// * `source_name` - 表示用入力名
/// * `opts` - 生成オプション
///
/// # Errors
/// reader か emitter のいずれかが失敗した場合に `GeneratorError`。
pub fn generate_with_aux(
    format: InputFormat,
    bytes: &[u8],
    aux_bytes: Option<&[u8]>,
    source_name: &str,
    opts: &GenOptions,
) -> Result<String, GeneratorError> {
    let score = match format {
        InputFormat::Smf => readers::smf::SmfReader.read(bytes, source_name)?,
        InputFormat::Mdx => readers::mdx::MdxReader.read_with_pdx(bytes, aux_bytes, source_name)?,
    };
    emitter::emit(&score, opts)
}

/// MDX ヘッダから参照される PDX ファイル名を返す (無ければ `None`)。
///
/// Returns the PDX filename referenced by an MDX header, if any.
pub fn mdx_pdx_filename(bytes: &[u8]) -> Option<String> {
    readers::mdx::pdx_filename(bytes)
}

/// ファイルパスから `generate` を呼び出すユーティリティ。
///
/// Convenience wrapper around `generate` that reads a file.
pub fn generate_from_path(
    format: InputFormat,
    path: &Path,
    opts: &GenOptions,
) -> Result<String, GeneratorError> {
    let bytes = std::fs::read(path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    let aux = if format == InputFormat::Mdx {
        resolve_pdx_bytes(path, &bytes)
    } else {
        None
    };
    generate_with_aux(format, &bytes, aux.as_deref(), &name, opts)
}

/// MDX に対応する PDX のバイト列を解決して返す。
///
/// 探索順:
/// 1. MDX ヘッダが参照する PDX ファイル名 (同階層)
/// 2. MDX と同じ stem の `.pdx` / `.PDX` (同階層)
///
/// 見つからなければ `None`。大文字小文字の揺れも順に試す。
///
/// Resolves the PDX bytes for an MDX file: first the filename referenced by the
/// MDX header, then `<stem>.pdx` next to the MDX. Tries case variants. Returns
/// `None` if nothing is found.
fn resolve_pdx_bytes(mdx_path: &Path, mdx_bytes: &[u8]) -> Option<Vec<u8>> {
    let dir = mdx_path.parent().unwrap_or_else(|| Path::new("."));

    // 候補ファイル名を集める。
    let mut candidates: Vec<String> = Vec::new();
    if let Some(name) = mdx_pdx_filename(mdx_bytes) {
        if !name.is_empty() {
            candidates.push(name.clone());
            // 拡張子が無ければ .pdx を補う。
            if !name.to_ascii_lowercase().ends_with(".pdx") {
                candidates.push(format!("{}.pdx", name));
            }
        }
    }
    if let Some(stem) = mdx_path.file_stem().and_then(|s| s.to_str()) {
        candidates.push(format!("{}.pdx", stem));
        candidates.push(format!("{}.PDX", stem));
    }

    for cand in candidates {
        // そのままのパスと、大文字小文字を変えたパスを試す。
        for variant in [
            cand.clone(),
            cand.to_ascii_uppercase(),
            cand.to_ascii_lowercase(),
        ] {
            let p = dir.join(&variant);
            if let Ok(bytes) = std::fs::read(&p) {
                return Some(bytes);
            }
        }
    }
    None
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
pub fn generate_from_path_auto(path: &Path, opts: &GenOptions) -> Result<String, GeneratorError> {
    let bytes = std::fs::read(path)?;
    let format = detect_format(&bytes, path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input")
        .to_string();
    let aux = if format == InputFormat::Mdx {
        resolve_pdx_bytes(path, &bytes)
    } else {
        None
    };
    generate_with_aux(format, &bytes, aux.as_deref(), &name, opts)
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
