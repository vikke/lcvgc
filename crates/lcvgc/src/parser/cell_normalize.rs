//! セル列正規化モジュール
//! Cell-sequence normalization module.
//!
//! drum / CC step / pitched の 3 系統で共通の「セル列メタトークン」
//! (`.` `|` `>N` `(...)*N`) の意味を一元化するためのヘルパ群。
//!
//! 統一後セマンティクス:
//! - `.`         : セル単位の skip (drum=hit なし / CC=送らない)
//! - `|`         : 拍境界スナップ。直近 `|`/行頭から `|` 直前までの
//!                 セル数を `beats_per_step` で切り捨て、不足は
//!                 `skip_cell` で埋め、超過は拍境界まで切り落とす
//! - `>N`        : 小節 N へ絶対位置スナップ (1 始まり)。
//!                 pitched の bar_jump セマンティクスと一致。
//! - `(...)*N`   : 繰り返し (既存 `expand_repetition` が string 段で展開)
//!
//! Shared helpers used by drum / CC step (and indirectly pitched) so that
//! the meaning of cell-level meta tokens stays consistent across clip kinds.

/// セル列に現れるメタトークン込みの汎用要素。
///
/// `T` は cell の中身 (drum=`HitSymbol` や `char`、CC=`Option<u8>` 等)。
/// `(...)*N` は呼び出し側で `expand_repetition` により string 段で展開
/// される前提のため、このトークンには含めない。
///
/// Generic cell-token element including meta tokens. `T` is the cell
/// payload (e.g. `HitSymbol` for drums, `Option<u8>` for CC step).
/// `(...)*N` is expected to be expanded at the string layer beforehand,
/// so it is not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellToken<T> {
    /// 通常セル (drum=hit / CC=u8 値 or None)
    /// A regular cell payload.
    Cell(T),
    /// `|` 拍境界スナップ
    /// Bar/beat-boundary snap meta token.
    Pipe,
    /// `>N` 小節 N (1 始まり) へジャンプ
    /// Bar jump to bar N (1-based).
    BarJump(u32),
}

/// `|` を解決して flat なセル列に変換する。
///
/// 仕様:
///   - 直近の `|`/列頭から **当該 `|` の直前まで** のセル数を数える
///   - その長さを `beats_per_step` で切り捨てた次の拍境界に揃える
///     - 不足 (= 現在のセル数 < 次境界) → `skip_cell` で埋める
///     - 超過 (= 現在のセル数 > 次境界) → 拍境界まで **切り落とす**
///   - `BarJump` トークンは透過 (= 出力にそのまま残る)。後続の
///     `expand_bar_jump_cells` で解決する想定。
///
/// 例 (beats_per_step = 4):
///   `[c c c |]`          → `[c c c .]` (3 セル → 1 拍 = 4 セルまで埋める)
///   `[c c c c c |]`      → `[c c c c]` (5 セル → 1 拍 = 4 セルに切り落とす)
///   `[c c | c c c c c |]` → `[c c . . c c c c]`
///                          (前半: 2 → 4 埋め, 後半: 5 → 4 切り落とし)
///
/// Resolves `|` snap tokens. The number of cells since the previous
/// `|` (or sequence start) is rounded down to the next beat boundary;
/// shorts are padded with `skip_cell`, overruns are truncated.
/// `BarJump` tokens are passed through unchanged.
///
/// # Arguments
/// * `cells` - 入力セル列 / Input token sequence.
/// * `beats_per_step` - 1 拍あたりのセル数 / Cells per beat (must be > 0).
/// * `skip_cell` - 不足埋めに使う「何もしないセル」 /
///   The payload used for padding (e.g. `HitSymbol::Rest` or `None`).
///
/// # Returns
/// `|` を解決した正規化セル列 / Normalized token sequence with `Pipe` resolved.
pub fn expand_pipe_cells<T: Clone>(
    cells: &[CellToken<T>],
    beats_per_step: usize,
    skip_cell: &T,
) -> Vec<CellToken<T>> {
    assert!(beats_per_step > 0, "beats_per_step must be > 0");
    let mut out: Vec<CellToken<T>> = Vec::with_capacity(cells.len());
    // 最後の `|`/列頭以降に出力したセル (= Cell トークン) の個数。
    // BarJump は「セル数」に数えない (= 拍境界判定に使わない)。
    // Count of `Cell` tokens emitted since the most recent boundary.
    let mut cells_since_pipe: usize = 0;

    for tok in cells {
        match tok {
            CellToken::Cell(v) => {
                out.push(CellToken::Cell(v.clone()));
                cells_since_pipe += 1;
            }
            CellToken::BarJump(n) => {
                // `|` 判定に影響しないが、出力には残す。
                // Pass through, does not affect snap counting.
                out.push(CellToken::BarJump(*n));
            }
            CellToken::Pipe => {
                // 拍境界へのスナップ方針:
                //   - cells_since_pipe <= beats_per_step:
                //       不足 (= ちょうどなら no-op) → 次境界 beats_per_step まで埋め
                //   - cells_since_pipe > beats_per_step:
                //       超過 → 「直前の拍境界 (= cells_since_pipe を beats_per_step
                //       で割った商×beats_per_step)」まで切り落とし
                //
                // Snap policy at `|`:
                //   - if the count is at most one beat, pad up to one beat.
                //   - if the count exceeds one beat, truncate back to the
                //     previous beat boundary (multiple of `beats_per_step`).
                if cells_since_pipe <= beats_per_step {
                    let pad = beats_per_step - cells_since_pipe;
                    for _ in 0..pad {
                        out.push(CellToken::Cell(skip_cell.clone()));
                    }
                } else {
                    // このセグメント (= 直近 |/列頭以降) のセル数を
                    // 拍境界に合わせ、超過末尾セルを削る。
                    // drop_count = (超過したセル数) = cells_since_pipe を
                    // beats_per_step で割った剰余。
                    let drop_count = cells_since_pipe % beats_per_step;
                    truncate_trailing_cells_by(&mut out, drop_count);
                }
                cells_since_pipe = 0;
            }
        }
    }

    out
}

/// 末尾側から Cell トークン (BarJump は無視) を `drop_count` 個だけ削る。
///
/// 直近 `|`/列頭以降の超過セルを切り落とすために使う。前 segment で
/// 確定したセルには触れないよう、「末尾から数えて drop_count 個」のみ
/// 削除する。
///
/// Drop the most-recent `drop_count` `Cell` tokens from `out`, leaving
/// any `BarJump` tokens untouched. Used to truncate cells overrunning
/// the previous beat boundary at a `|` token.
fn truncate_trailing_cells_by<T>(out: &mut Vec<CellToken<T>>, drop_count: usize) {
    if drop_count == 0 {
        return;
    }
    let mut idx = out.len();
    let mut removed = 0usize;
    let mut to_remove: Vec<usize> = Vec::new();
    while idx > 0 && removed < drop_count {
        idx -= 1;
        if matches!(out[idx], CellToken::Cell(_)) {
            to_remove.push(idx);
            removed += 1;
        }
    }
    // 大きい index から削る (= 詰まりで他の index がずれない)
    for i in to_remove {
        out.remove(i);
    }
}

/// `>N` を解決して、長さ揃えの flat なペイロード列を返す。
///
/// 仕様:
///   - `Cell` は時系列ポインタを 1 進めて当該位置に書き込む
///   - `BarJump(N)` は時系列ポインタを `(N - 1) * steps_per_bar` に
///     **絶対位置スナップ** する (pitched bar_jump と同じ)
///   - 既に書き込み済の位置に再度書き込もうとした場合は **上書き**
///     (= 後勝ち)
///   - `total_steps` を渡した場合: 結果長を `total_steps` に揃える
///     - 不足: 末尾を `skip_cell` で埋め
///     - 超過: 末尾を切り落とし
///   - `total_steps = None`: 自動算出 (= 末尾セル位置 + 1 と
///     `steps_per_bar` の倍数の大きい方)
///
/// Resolves `BarJump(N)` tokens by snapping a write pointer to the
/// absolute step at the start of bar `N` (1-based). Cells write to the
/// current pointer position, overwriting on collision. The result is
/// optionally padded/truncated to `total_steps`.
///
/// # Arguments
/// * `cells` - 入力セル列 (Pipe は事前に `expand_pipe_cells` で解決済が前提)
/// * `steps_per_bar` - 1 小節あたりのセル数 (= resolution × beats_per_bar / 4)
/// * `total_steps` - 最終出力長 / Final length to pad-or-truncate to.
/// * `skip_cell` - 歯抜けや埋めに使う「何もしないセル」 / Padding payload.
///
/// # Returns
/// `>N` を解決した平坦なペイロード列 / Flat payload vector with bar jumps resolved.
///
/// # Errors
/// `BarJump(N)` の N が 1 未満の場合エラー (pitched と同じ制約)。
pub fn expand_bar_jump_cells<T: Clone>(
    cells: &[CellToken<T>],
    steps_per_bar: usize,
    total_steps: Option<usize>,
    skip_cell: &T,
) -> Result<Vec<T>, String> {
    assert!(steps_per_bar > 0, "steps_per_bar must be > 0");
    let mut buf: Vec<T> = Vec::new();
    let mut cursor: usize = 0;

    for tok in cells {
        match tok {
            CellToken::Pipe => {
                // この段階で Pipe が残っているのは前段呼び出し漏れ。
                // パイプは前段で解決済が前提のため呼び出し側のミス。
                return Err(
                    "expand_bar_jump_cells: Pipe トークンが未解決のまま残っている (expand_pipe_cells を先に呼び出してください)".to_string(),
                );
            }
            CellToken::BarJump(n) => {
                if *n < 1 {
                    return Err(format!(">{}は無効です (1 始まり)", n));
                }
                cursor = ((*n as usize) - 1) * steps_per_bar;
            }
            CellToken::Cell(v) => {
                if cursor >= buf.len() {
                    // 歯抜けを skip_cell で埋めて、新しい cell を末尾に push
                    buf.resize(cursor, skip_cell.clone());
                    buf.push(v.clone());
                } else {
                    // 既存位置を上書き
                    buf[cursor] = v.clone();
                }
                cursor += 1;
            }
        }
    }

    if let Some(total) = total_steps {
        if buf.len() < total {
            buf.resize(total, skip_cell.clone());
        } else if buf.len() > total {
            buf.truncate(total);
        }
    } else {
        // 自動: steps_per_bar の倍数に切り上げ
        let rounded = buf.len().div_ceil(steps_per_bar) * steps_per_bar;
        if rounded > buf.len() {
            buf.resize(rounded, skip_cell.clone());
        }
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // expand_pipe_cells
    // ============================================================

    /// 不足ケース: 3 セル + `|` (拍 = 4 セル) → 4 セルに `.` で埋める
    /// Short case: 3 cells + `|` (beat = 4 cells) is padded with `.`.
    #[test]
    fn pipe_pads_short_to_next_beat_boundary() {
        // beats_per_step = 4, skip_cell = '.'
        let cells = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Pipe,
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        let expected: Vec<CellToken<char>> = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('.'),
        ];
        assert_eq!(out, expected);
    }

    /// 超過ケース: 5 セル + `|` (拍 = 4) → 4 セルに切り落とす
    /// Overrun case: 5 cells + `|` (beat = 4) truncated to 4 cells.
    #[test]
    fn pipe_truncates_overrun_to_previous_beat_boundary() {
        let cells = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Pipe,
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        let expected: Vec<CellToken<char>> = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
        ];
        assert_eq!(out, expected);
    }

    /// 拍境界ぴったりは変更なし
    /// Exact beat boundary leaves cells untouched.
    #[test]
    fn pipe_at_exact_boundary_is_noop() {
        let cells = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Pipe,
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        let expected: Vec<CellToken<char>> = vec![
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
            CellToken::Cell('c'),
        ];
        assert_eq!(out, expected);
    }

    /// 複数の `|` が連続するケース。前後別カウントで処理される。
    /// Multiple `|` segments are counted independently.
    #[test]
    fn pipe_multiple_segments_are_independent() {
        // 2 セル + | (padding 2) + 5 セル + | (truncate to 4)
        let cells = vec![
            CellToken::Cell('a'),
            CellToken::Cell('b'),
            CellToken::Pipe,
            CellToken::Cell('c'),
            CellToken::Cell('d'),
            CellToken::Cell('e'),
            CellToken::Cell('f'),
            CellToken::Cell('g'),
            CellToken::Pipe,
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        let expected: Vec<CellToken<char>> = vec![
            CellToken::Cell('a'),
            CellToken::Cell('b'),
            CellToken::Cell('.'),
            CellToken::Cell('.'),
            CellToken::Cell('c'),
            CellToken::Cell('d'),
            CellToken::Cell('e'),
            CellToken::Cell('f'),
        ];
        assert_eq!(out, expected);
    }

    /// `BarJump` は `|` 判定で無視される (= セル数に数えない) が、
    /// 出力には残る。
    /// `BarJump` is not counted toward the snap but is preserved in output.
    #[test]
    fn pipe_passes_through_bar_jump_tokens() {
        let cells = vec![
            CellToken::Cell('c'),
            CellToken::BarJump(3),
            CellToken::Cell('c'),
            CellToken::Pipe,
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        // セル数は 2 (BarJump はノーカン)。次境界 4 まで 2 セル `.` で埋め。
        // BarJump は途中位置に残る。
        let expected: Vec<CellToken<char>> = vec![
            CellToken::Cell('c'),
            CellToken::BarJump(3),
            CellToken::Cell('c'),
            CellToken::Cell('.'),
            CellToken::Cell('.'),
        ];
        assert_eq!(out, expected);
    }

    /// `|` が無いときはそのまま素通し
    /// Without any `|`, input is returned unchanged.
    #[test]
    fn pipe_absent_is_identity() {
        let cells = vec![
            CellToken::Cell('a'),
            CellToken::Cell('b'),
            CellToken::Cell('c'),
        ];
        let out = expand_pipe_cells(&cells, 4, &'.');
        assert_eq!(out, cells);
    }

    // ============================================================
    // expand_bar_jump_cells
    // ============================================================

    /// `>N` 無しの基本動作: cells をそのまま flat 列にする
    /// Without `>N`, cells are flattened in order.
    #[test]
    fn bar_jump_absent_is_identity() {
        let cells = vec![
            CellToken::Cell(1u8),
            CellToken::Cell(2),
            CellToken::Cell(3),
            CellToken::Cell(4),
        ];
        let out = expand_bar_jump_cells(&cells, 4, Some(4), &0u8).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4]);
    }

    /// `>2` で 2 小節目の頭にジャンプ。手前の歯抜けは skip_cell で埋め。
    /// `>2` jumps the write pointer to bar 2 head; the prefix is padded
    /// with `skip_cell`.
    #[test]
    fn bar_jump_to_bar2_pads_prefix() {
        let cells = vec![
            CellToken::Cell(1u8),
            CellToken::BarJump(2),
            CellToken::Cell(2),
        ];
        // steps_per_bar = 4, total = 8
        let out = expand_bar_jump_cells(&cells, 4, Some(8), &0u8).unwrap();
        // bar1: [1, 0, 0, 0], bar2: [2, 0, 0, 0]
        assert_eq!(out, vec![1, 0, 0, 0, 2, 0, 0, 0]);
    }

    /// `>1` は 1 小節目頭 (= step 0) に戻り、後勝ちで上書きする
    /// `>1` rewinds to step 0 and the subsequent cell overwrites.
    #[test]
    fn bar_jump_to_bar1_rewinds_and_overwrites() {
        let cells = vec![
            CellToken::Cell(1u8),
            CellToken::Cell(2),
            CellToken::BarJump(1),
            CellToken::Cell(99),
        ];
        let out = expand_bar_jump_cells(&cells, 4, Some(4), &0u8).unwrap();
        // step0 が 1 → 99 に上書き、step1 は 2 のまま、step2-3 は 0 埋め
        assert_eq!(out, vec![99, 2, 0, 0]);
    }

    /// total_steps 超過時は末尾を切り落とす
    /// Truncates trailing cells when total_steps is shorter than data.
    #[test]
    fn bar_jump_total_truncates_excess() {
        let cells: Vec<CellToken<u8>> = (1..=10).map(CellToken::Cell).collect();
        let out = expand_bar_jump_cells(&cells, 4, Some(4), &0u8).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4]);
    }

    /// total_steps 不足時は末尾を skip_cell で埋める
    /// Pads trailing cells with skip_cell when total_steps is longer.
    #[test]
    fn bar_jump_total_pads_trailing_with_skip() {
        let cells = vec![CellToken::Cell(1u8), CellToken::Cell(2)];
        let out = expand_bar_jump_cells(&cells, 4, Some(4), &0u8).unwrap();
        assert_eq!(out, vec![1, 2, 0, 0]);
    }

    /// `>N` で N=0 はエラー
    /// `BarJump(0)` returns an error (bar numbers are 1-based).
    #[test]
    fn bar_jump_zero_is_error() {
        let cells: Vec<CellToken<u8>> = vec![CellToken::BarJump(0)];
        let err = expand_bar_jump_cells(&cells, 4, None, &0u8);
        assert!(err.is_err());
    }

    /// Pipe が残っている入力はエラー (呼び出し漏れ検出)
    /// Leftover Pipe tokens are an error (caller forgot expand_pipe_cells).
    #[test]
    fn bar_jump_with_pipe_left_is_error() {
        let cells: Vec<CellToken<u8>> = vec![CellToken::Pipe];
        let err = expand_bar_jump_cells(&cells, 4, None, &0u8);
        assert!(err.is_err());
    }

    /// total_steps = None は自動算出 (steps_per_bar の倍数に切り上げ)
    /// total_steps = None auto-rounds the length up to a multiple of
    /// steps_per_bar.
    #[test]
    fn bar_jump_auto_total_rounds_up_to_bar() {
        let cells: Vec<CellToken<u8>> = (1..=5).map(CellToken::Cell).collect();
        let out = expand_bar_jump_cells(&cells, 4, None, &0u8).unwrap();
        // 5 セル → 8 (= 2 bar) まで埋め
        assert_eq!(out, vec![1, 2, 3, 4, 5, 0, 0, 0]);
    }

    // ============================================================
    // パイプライン全体: expand_pipe_cells → expand_bar_jump_cells
    // ============================================================

    /// ユーザ提案の例 (drum): 1 拍分書いたら `|`、`>3` で 3 小節目から再開
    /// End-to-end pipeline example combining `|` and `>N`.
    #[test]
    fn pipeline_pipe_then_bar_jump() {
        // 入力: a a | >3 b b
        //   beats_per_step = 4, steps_per_bar = 16, bars = 4
        // 期待:
        //   pipe 後: [a, a, ., ., (BarJump 3), b, b]
        //   bar_jump 後: [a, a, ., ., 0...0 (32 個埋め), b, b, ., ., ...] → total 64 に切り詰め
        let cells = vec![
            CellToken::Cell('a'),
            CellToken::Cell('a'),
            CellToken::Pipe,
            CellToken::BarJump(3),
            CellToken::Cell('b'),
            CellToken::Cell('b'),
        ];
        let piped = expand_pipe_cells(&cells, 4, &'.');
        let expected_piped: Vec<CellToken<char>> = vec![
            CellToken::Cell('a'),
            CellToken::Cell('a'),
            CellToken::Cell('.'),
            CellToken::Cell('.'),
            CellToken::BarJump(3),
            CellToken::Cell('b'),
            CellToken::Cell('b'),
        ];
        assert_eq!(piped, expected_piped);

        let final_out = expand_bar_jump_cells(&piped, 16, Some(64), &'.').unwrap();
        // bar1 (step0-15): a a . . . . . . . . . . . . . .
        // bar2 (step16-31): all .
        // bar3 (step32-): b b . ... (32-step total padded)
        assert_eq!(final_out.len(), 64);
        assert_eq!(&final_out[..4], &['a', 'a', '.', '.']);
        for i in 4..32 {
            assert_eq!(final_out[i], '.', "bar1 末尾と bar2 は全 . のはず (idx={i})");
        }
        assert_eq!(&final_out[32..34], &['b', 'b']);
        for i in 34..64 {
            assert_eq!(final_out[i], '.', "bar3 残りと bar4 は全 . のはず (idx={i})");
        }
    }
}
