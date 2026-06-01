//! フォーマット別 reader 群。
//! Per-format readers.

/// Standard MIDI File reader
pub mod smf;

/// MDX (X68000 MXDRV, FM 部のみ) reader
pub mod mdx;

/// PDX (MXDRV ADPCM サンプルバンク) パーサ / ADPCM デコーダ
pub mod pdx;
