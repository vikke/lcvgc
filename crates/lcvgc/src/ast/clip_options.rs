//! クリップオプションの AST 型。
//! Clip-option AST type.

use crate::ast::scale::ScaleDef;

/// クリップに付与できるオプション群を保持する構造体。
/// `[bars N]`、`[time N/N]`、`[scale ROOT TYPE]` の各指定を格納する。
///
/// A struct that holds clip-level options.
/// Stores `[bars N]`, `[time N/N]`, and `[scale ROOT TYPE]` specifications.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipOptions {
    /// 小節数の指定（例: `[bars 4]`）。
    ///
    /// Number of bars (e.g. `[bars 4]`).
    pub bars: Option<u32>,
    /// 拍子の指定（分子, 分母）（例: `[time 3/4]`）。
    ///
    /// Time signature as (numerator, denominator) (e.g. `[time 3/4]`).
    pub time_sig: Option<(u8, u8)>,
    /// スケールの指定（例: `[scale c minor]`）。
    ///
    /// Scale specification (e.g. `[scale c minor]`).
    pub scale: Option<ScaleDef>,
    /// オクターブシフト量（例: `[>>]` で +1、`[<<]` で -1、`[>> >>]` で +2）。
    /// clip 全体のピッチを 12 半音単位で上下させる。`>>` と `<<` は合算される。
    /// 既定値 0（シフトなし）。ピッチド clip にのみ効果がある。
    ///
    /// Octave shift amount (e.g. `[>>]` is +1, `[<<]` is -1, `[>> >>]` is +2).
    /// Transposes the whole clip in 12-semitone steps. `>>` and `<<` accumulate.
    /// Defaults to 0 (no shift). Only affects pitched clips.
    pub octave_shift: i8,
}
