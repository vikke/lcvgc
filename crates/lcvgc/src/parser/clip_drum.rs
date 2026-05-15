use crate::ast::clip_drum::HitSymbol;
use crate::parser::cell_normalize::CellToken;

/// ドラムパターン文字列 (前段で `(...)*N` 等は展開済) を `CellToken<HitSymbol>` 列に
/// トークナイズする。
///
/// 認識するもの:
///   - `x` `X` `o` `.` → `CellToken::Cell(HitSymbol)`
///   - `|` → `CellToken::Pipe`
///   - `>N` → `CellToken::BarJump(N)` (1 文字以上の連続数字)
///   - 空白 (`' '` `'\t'`) → 無視
///
/// `(...)*N` 繰り返しは `expand_repetition` で **string 段で** 既に展開されている前提。
///
/// Tokenize a drum pattern string into a `CellToken<HitSymbol>` sequence.
/// Whitespace is ignored. `(...)*N` is expected to have been expanded by
/// `expand_repetition` at the string layer beforehand.
///
/// # Arguments
/// * `input` - 展開済みのパターン文字列 / pre-expanded pattern string
///
/// # Returns
/// `Ok(Vec<CellToken<HitSymbol>>)` — 成功時のトークン列 / token sequence on success
///
/// # Errors
/// 未知の文字が含まれる場合エラーを返す。
/// `>` の直後に数字が無い場合もエラー。
/// Returns an error on unknown characters, or when `>` is not followed by digits.
pub fn tokenize_drum_pattern(input: &str) -> Result<Vec<CellToken<HitSymbol>>, String> {
    let bytes = input.as_bytes();
    let mut out: Vec<CellToken<HitSymbol>> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            ' ' | '\t' => {
                i += 1;
            }
            'x' => {
                out.push(CellToken::Cell(HitSymbol::Normal));
                i += 1;
            }
            'X' => {
                out.push(CellToken::Cell(HitSymbol::Accent));
                i += 1;
            }
            'o' => {
                out.push(CellToken::Cell(HitSymbol::Ghost));
                i += 1;
            }
            '.' => {
                out.push(CellToken::Cell(HitSymbol::Rest));
                i += 1;
            }
            '|' => {
                out.push(CellToken::Pipe);
                i += 1;
            }
            '>' => {
                // `>` の後ろの空白を読み飛ばし、続く数字列を BarJump に変換する。
                // Skip whitespace after `>` and parse the following digits.
                let (n, consumed) = parse_bar_jump_digits(&bytes[i + 1..])?;
                out.push(CellToken::BarJump(n));
                i += 1 + consumed;
            }
            other => {
                return Err(format!("unknown hit symbol: '{}'", other));
            }
        }
    }
    Ok(out)
}

/// 確率行 (前段で `(...)*N` 等は展開済) を `CellToken<u8>` 列に
/// トークナイズする。
///
/// 認識するもの:
///   - `0` → `Cell(0)` (発音しない)
///   - `1`-`9` → `Cell(10)`〜`Cell(90)`
///   - `.` → `Cell(100)` (常に発音)
///   - `|` → `Pipe`
///   - `>N` → `BarJump(N)`
///   - 空白 → 無視
///
/// `.` の意味は「100 (常に発音)」なので drum 行とは異なる。padding 値も別に
/// 与える必要がある (= `expand_pipe_cells` の `skip_cell` には 100 を渡す)。
///
/// Tokenize a probability row into `CellToken<u8>`. Same meta-token grammar as
/// drums, but `.` means 100 (always-fire) and digits map `0` → 0, `1`-`9` → 10-90.
///
/// # Arguments
/// * `input` - 展開済みの確率行文字列 / pre-expanded probability row string
///
/// # Returns
/// `Ok(Vec<CellToken<u8>>)` — 成功時のトークン列 / token sequence on success
///
/// # Errors
/// 未知の文字が含まれる場合エラーを返す。
/// Returns an error on unknown characters.
pub fn tokenize_probability_pattern(input: &str) -> Result<Vec<CellToken<u8>>, String> {
    let bytes = input.as_bytes();
    let mut out: Vec<CellToken<u8>> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            ' ' | '\t' => {
                i += 1;
            }
            '.' => {
                out.push(CellToken::Cell(100));
                i += 1;
            }
            '0' => {
                out.push(CellToken::Cell(0));
                i += 1;
            }
            '1'..='9' => {
                out.push(CellToken::Cell((ch as u8 - b'0') * 10));
                i += 1;
            }
            '|' => {
                out.push(CellToken::Pipe);
                i += 1;
            }
            '>' => {
                let (n, consumed) = parse_bar_jump_digits(&bytes[i + 1..])?;
                out.push(CellToken::BarJump(n));
                i += 1 + consumed;
            }
            other => {
                return Err(format!("unknown probability symbol: '{}'", other));
            }
        }
    }
    Ok(out)
}

/// `>` 直後のバイト列から、空白を読み飛ばして連続する ASCII 数字を `u32` として
/// 解釈し、`(n, consumed)` を返す。
///
/// `consumed` は `>` の **直後から** 数字末尾までに進めたバイト数 (= 空白 + 数字)。
///
/// Parse digits after a `>` token (skipping leading whitespace). Returns the
/// parsed bar number and how many bytes were consumed after `>`.
fn parse_bar_jump_digits(after_gt: &[u8]) -> Result<(u32, usize), String> {
    let mut idx = 0;
    while idx < after_gt.len() && (after_gt[idx] == b' ' || after_gt[idx] == b'\t') {
        idx += 1;
    }
    let digits_start = idx;
    while idx < after_gt.len() && after_gt[idx].is_ascii_digit() {
        idx += 1;
    }
    if digits_start == idx {
        return Err("'>' の後ろに小節番号が必要です (例: `>3`)".to_string());
    }
    let digits = std::str::from_utf8(&after_gt[digits_start..idx])
        .map_err(|e| format!("'>N' の数字パースに失敗: {}", e))?;
    let n: u32 = digits
        .parse()
        .map_err(|e| format!("'>N' の数字パースに失敗: {}", e))?;
    Ok((n, idx))
}

/// 展開済み（`|` なし）のパターン文字列を `HitSymbol` のベクタにパースする。
///
/// Parse an expanded (no `|`) pattern string into a vector of `HitSymbol`.
///
/// # Arguments
/// * `input` - 展開済みパターン文字列 (`x`, `X`, `o`, `.` で構成) / expanded pattern string (composed of `x`, `X`, `o`, `.`)
///
/// # Returns
/// `Ok(Vec<HitSymbol>)` — パース成功時 / on success
///
/// # Errors
/// 未知のシンボル文字が含まれる場合エラーを返す。
/// Returns an error if an unknown symbol character is encountered.
pub fn parse_hit_symbols(input: &str) -> Result<Vec<HitSymbol>, String> {
    input
        .chars()
        .map(|ch| match ch {
            'x' => Ok(HitSymbol::Normal),
            'X' => Ok(HitSymbol::Accent),
            'o' => Ok(HitSymbol::Ghost),
            '.' => Ok(HitSymbol::Rest),
            other => Err(format!("unknown hit symbol: '{}'", other)),
        })
        .collect()
}

/// 確率行文字列をステップごとの確率値（0-100）にパースする。
///
/// - `.` → 100（常に発音）
/// - `0` → 0（発音しない）
/// - `1`〜`9` → 10〜90
///
/// Parse a probability row string into per-step probabilities (0-100).
///
/// - `.` → 100 (always fire)
/// - `0` → 0 (never fire)
/// - `1`-`9` → 10-90
///
/// # Arguments
/// * `input` - 確率行文字列 (`.`, `0`-`9` で構成) / probability row string (composed of `.`, `0`-`9`)
///
/// # Returns
/// `Ok(Vec<u8>)` — パース成功時 / on success
///
/// # Errors
/// 未知の確率シンボルが含まれる場合エラーを返す。
/// Returns an error if an unknown probability symbol is encountered.
pub fn parse_probability_row(input: &str) -> Result<Vec<u8>, String> {
    input
        .chars()
        .map(|ch| match ch {
            '.' => Ok(100),
            '0' => Ok(0),
            '1'..='9' => Ok((ch as u8 - b'0') * 10),
            other => Err(format!("unknown probability symbol: '{}'", other)),
        })
        .collect()
}

/// 文字列中の「最も内側」の `(...)` ペアの位置を返す。
///
/// 最初に見つかる `)` の位置と、その前方で対応する `(` の位置を返す。
/// 片方しか見つからない場合は `None` を返す。
///
/// Return the position of the innermost `(...)` pair in the string.
///
/// Returns the index of the first `)` and the last `(` before it.
/// Returns `None` if either side is missing.
///
/// # Arguments
/// * `s` - 探索対象の文字列 / string to search
///
/// # Returns
/// `Some((open, close))` — `(` と `)` のバイト位置 / byte indices of `(` and `)`
/// `None` — 対応するペアが見つからない場合 / no matching pair
fn find_innermost_paren(s: &str) -> Option<(usize, usize)> {
    let close = s.find(')')?;
    let open = s[..close].rfind('(')?;
    Some((open, close))
}

/// ドラムパターン文字列中の `(pattern)*N` 繰り返し記法を展開する。
/// ネストした繰り返し（内側から順に展開）に対応。
///
/// Expand `(pattern)*N` repetition notation in drum pattern strings.
/// Handles nested repetitions (expands inner ones first).
///
/// # Arguments
/// * `input` - 繰り返し記法を含むドラムパターン文字列 / drum pattern string with repetition notation
///
/// # Returns
/// 展開済みのパターン文字列 / expanded pattern string
pub fn expand_repetition(input: &str) -> String {
    let mut s = input.to_string();
    while let Some((open, close)) = find_innermost_paren(&s) {
        let inner = &s[open + 1..close];
        let after = &s[close + 1..];

        // `) [ws] * [ws] N` 形式を検出 (空白は任意個数許容、改行も可)
        // Detect `) [ws] * [ws] N` form, allowing any whitespace (incl. newlines)
        // around the `*` and the digits.
        let after_ws1 = after.trim_start();
        if let Some(after_star) = after_ws1.strip_prefix('*') {
            let after_ws2 = after_star.trim_start();
            let digits_len = after_ws2
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_ws2.len());
            if digits_len > 0 {
                let n: usize = after_ws2[..digits_len].parse().unwrap_or(1);
                let repeated = inner.repeat(n);
                let rest = &after_ws2[digits_len..];
                s = format!("{}{}{}", &s[..open], repeated, rest);
                continue;
            }
        }

        // `(...)` だが `*N` がない場合、括弧を除去してスキップ (count=1 相当)。
        // 括弧の前後にあった空白はそのまま保持する。
        //
        // `(...)` without `*N` — strip parens and keep going (equivalent to
        // count=1). Whitespace surrounding the parens is left untouched.
        s = format!("{}{}{}", &s[..open], inner, after);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip_drum::HitSymbol::*;

    // --- find_innermost_paren tests ---

    /// 単純な `(...)` の `(`/`)` 位置を返すことを確認する。
    ///
    /// Verify that `find_innermost_paren` returns `(` / `)` indices for simple `(...)`.
    #[test]
    fn find_innermost_paren_simple() {
        assert_eq!(find_innermost_paren("x.(ab)*2"), Some((2, 5)));
    }

    /// ネストしている場合は最も内側の `(...)` を返すことを確認する。
    ///
    /// Verify that for nested `(...)`, the innermost pair is returned.
    #[test]
    fn find_innermost_paren_nested() {
        // "((ab)*2c)*3" → 最内は "(ab)" の 1..4
        assert_eq!(find_innermost_paren("((ab)*2c)*3"), Some((1, 4)));
    }

    /// `(` のみで `)` が無ければ None を返すことを確認する。
    ///
    /// Verify None is returned when `)` is missing.
    #[test]
    fn find_innermost_paren_no_close() {
        assert_eq!(find_innermost_paren("(abc"), None);
    }

    /// `)` のみで対応する `(` が無ければ None を返すことを確認する。
    ///
    /// Verify None is returned when matching `(` is missing.
    #[test]
    fn find_innermost_paren_no_open() {
        assert_eq!(find_innermost_paren("abc)"), None);
    }

    /// 括弧が全く無ければ None を返すことを確認する。
    ///
    /// Verify None is returned when there are no parentheses.
    #[test]
    fn find_innermost_paren_none() {
        assert_eq!(find_innermost_paren("x.x.x.x."), None);
    }

    // --- expand_repetition tests ---

    #[test]
    fn expand_repetition_basic() {
        assert_eq!(expand_repetition("(x.x.)*4"), "x.x.x.x.x.x.x.x.");
    }

    #[test]
    fn expand_repetition_with_surrounding() {
        assert_eq!(expand_repetition("x.(x.)*2.x"), "x.x.x..x");
    }

    #[test]
    fn expand_repetition_nested() {
        // 内側: (ab)*2 → abab → (ababc)*3 → ababcababcababc
        assert_eq!(expand_repetition("((ab)*2c)*3"), "ababcababcababc");
    }

    #[test]
    fn expand_repetition_no_repetition() {
        assert_eq!(expand_repetition("x.x.x.x."), "x.x.x.x.");
    }

    #[test]
    fn expand_repetition_multiple() {
        assert_eq!(expand_repetition("(x.)*2(X.)*2"), "x.x.X.X.");
    }

    // --- 空白許容 / グルーピング (PR #79) ---

    /// `)` と `*` の間にスペースがあっても展開される
    #[test]
    fn expand_repetition_space_between_close_paren_and_star() {
        assert_eq!(expand_repetition("(x.) *4"), "x.x.x.x.");
    }

    /// `*` と数字の間にスペースがあっても展開される
    #[test]
    fn expand_repetition_space_between_star_and_number() {
        assert_eq!(expand_repetition("(x.)* 4"), "x.x.x.x.");
    }

    /// `*` 前後に改行が混在しても展開される
    #[test]
    fn expand_repetition_multiline_around_star() {
        assert_eq!(expand_repetition("(x.)\n*\n4"), "x.x.x.x.");
    }

    /// `*N` を省略した `(...)` は中身をそのまま残す (count=1 のグルーピング)
    #[test]
    fn expand_repetition_grouping_no_count() {
        assert_eq!(expand_repetition("(x.x.)"), "x.x.");
    }

    // --- tokenize_drum_pattern tests ---

    /// 通常セルだけのトークナイズ。
    /// Tokenize plain cells (no meta tokens).
    #[test]
    fn tokenize_drum_plain_cells() {
        let out = tokenize_drum_pattern("x.x.").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(HitSymbol::Normal),
                CellToken::Cell(HitSymbol::Rest),
                CellToken::Cell(HitSymbol::Normal),
                CellToken::Cell(HitSymbol::Rest),
            ]
        );
    }

    /// 空白は無視される (任意の数 OK)。
    /// Whitespace is ignored.
    #[test]
    fn tokenize_drum_ignores_whitespace() {
        let out = tokenize_drum_pattern("x. x .").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(HitSymbol::Normal),
                CellToken::Cell(HitSymbol::Rest),
                CellToken::Cell(HitSymbol::Normal),
                CellToken::Cell(HitSymbol::Rest),
            ]
        );
    }

    /// `|` は `Pipe` トークンになる。
    /// `|` becomes a `Pipe` token.
    #[test]
    fn tokenize_drum_pipe() {
        let out = tokenize_drum_pattern("x|x").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(HitSymbol::Normal),
                CellToken::Pipe,
                CellToken::Cell(HitSymbol::Normal),
            ]
        );
    }

    /// `>N` は `BarJump(N)` トークンになる。
    /// `>N` becomes a `BarJump(N)` token.
    #[test]
    fn tokenize_drum_bar_jump_single_digit() {
        let out = tokenize_drum_pattern("x>3x").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(HitSymbol::Normal),
                CellToken::BarJump(3),
                CellToken::Cell(HitSymbol::Normal),
            ]
        );
    }

    /// 複数桁 + 空白を挟んだ `>N` も受け付ける。
    /// Multi-digit `>N` and surrounding whitespace are accepted.
    #[test]
    fn tokenize_drum_bar_jump_multi_digit_with_spaces() {
        let out = tokenize_drum_pattern("x> 12 x").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(HitSymbol::Normal),
                CellToken::BarJump(12),
                CellToken::Cell(HitSymbol::Normal),
            ]
        );
    }

    /// 未知文字はエラー。
    /// Unknown chars return an error.
    #[test]
    fn tokenize_drum_unknown_char_errors() {
        let err = tokenize_drum_pattern("xyx").unwrap_err();
        assert!(err.contains("unknown hit symbol: 'y'"));
    }

    /// 空入力は空 Vec。
    /// Empty input returns an empty Vec.
    #[test]
    fn tokenize_drum_empty_input() {
        let out = tokenize_drum_pattern("").unwrap();
        assert!(out.is_empty());
    }

    /// `>` の直後に数字が無いとエラー。
    /// `>` without digits returns an error.
    #[test]
    fn tokenize_drum_bare_gt_errors() {
        let err = tokenize_drum_pattern("x>x").unwrap_err();
        assert!(err.contains("小節番号"));
    }

    // --- tokenize_probability_pattern tests ---

    /// 確率行のセルトークナイズ基本ケース。
    /// Basic probability row tokenization.
    #[test]
    fn tokenize_probability_basic() {
        let out = tokenize_probability_pattern("..5...7.").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(100),
                CellToken::Cell(100),
                CellToken::Cell(50),
                CellToken::Cell(100),
                CellToken::Cell(100),
                CellToken::Cell(100),
                CellToken::Cell(70),
                CellToken::Cell(100),
            ]
        );
    }

    /// 確率行で `|` と `>N` が認識される。
    /// Probability row recognises `|` and `>N`.
    #[test]
    fn tokenize_probability_with_pipe_and_bar_jump() {
        let out = tokenize_probability_pattern("5|>2 7").unwrap();
        assert_eq!(
            out,
            vec![
                CellToken::Cell(50),
                CellToken::Pipe,
                CellToken::BarJump(2),
                CellToken::Cell(70),
            ]
        );
    }

    /// 確率行の未知文字はエラー。
    /// Unknown chars in probability row return an error.
    #[test]
    fn tokenize_probability_unknown_char_errors() {
        let err = tokenize_probability_pattern("..a..").unwrap_err();
        assert!(err.contains("unknown probability symbol: 'a'"));
    }

    // --- parse_hit_symbols tests ---

    #[test]
    fn hit_symbols_basic() {
        assert_eq!(
            parse_hit_symbols("x...x...").unwrap(),
            vec![Normal, Rest, Rest, Rest, Normal, Rest, Rest, Rest]
        );
    }

    #[test]
    fn hit_symbols_accent_ghost() {
        assert_eq!(
            parse_hit_symbols("x.o.X.o.").unwrap(),
            vec![Normal, Rest, Ghost, Rest, Accent, Rest, Ghost, Rest]
        );
    }

    #[test]
    fn hit_symbols_unknown_char_returns_error() {
        let err = parse_hit_symbols("x.?.x").unwrap_err();
        assert!(err.contains("unknown hit symbol: '?'"));
    }

    // --- parse_probability_row tests ---

    #[test]
    fn probability_basic() {
        assert_eq!(
            parse_probability_row("..5...7.").unwrap(),
            vec![100, 100, 50, 100, 100, 100, 70, 100]
        );
    }

    #[test]
    fn probability_zero() {
        assert_eq!(parse_probability_row("0").unwrap(), vec![0]);
    }

    #[test]
    fn probability_full_row() {
        assert_eq!(
            parse_probability_row("..5...7...3...5.").unwrap(),
            vec![100, 100, 50, 100, 100, 100, 70, 100, 100, 100, 30, 100, 100, 100, 50, 100]
        );
    }

    #[test]
    fn probability_unknown_char_returns_error() {
        let err = parse_probability_row("..a..").unwrap_err();
        assert!(err.contains("unknown probability symbol: 'a'"));
    }

    // --- expand_repetition: 確率行テスト / probability row tests ---

    /// 確率行の `()*N` 繰り返しが正しく展開されることを検証する。
    /// `(..5.)*4` → `..5.` を4回繰り返し → `..5...5...5...5.` (16文字)。
    ///
    /// Verify `()*N` repetition expansion for probability rows.
    /// `(..5.)*4` → repeat `..5.` 4 times → `..5...5...5...5.` (16 chars).
    #[test]
    fn expand_repetition_probability() {
        assert_eq!(expand_repetition("(..5.)*4"), "..5...5...5...5.");
    }

    // --- スペース除去後のパーステスト ---
    // --- Tests for parsing after space stripping ---

    #[test]
    fn hit_symbols_spaces_stripped_before_parse() {
        // スペース除去後のパターンが正しくパースされることを確認
        // Verify that pattern with spaces stripped is parsed correctly
        let with_spaces = "x.   x.  x.   x.";
        let stripped: String = with_spaces.chars().filter(|c| *c != ' ').collect();
        let result = parse_hit_symbols(&stripped).unwrap();
        assert_eq!(result.len(), 8);
        assert_eq!(result[0], Normal);
        assert_eq!(result[1], Rest);
    }

    #[test]
    fn probability_spaces_stripped_before_parse() {
        // スペース除去後の確率行が正しくパースされることを確認
        // Verify that probability row with spaces stripped is parsed correctly
        let with_spaces = ". .  5 . . . 7 .";
        let stripped: String = with_spaces.chars().filter(|c| *c != ' ').collect();
        let result = parse_probability_row(&stripped).unwrap();
        assert_eq!(result, vec![100, 100, 50, 100, 100, 100, 70, 100]);
    }
}
