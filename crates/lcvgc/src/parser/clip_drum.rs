use crate::ast::clip_drum::HitSymbol;

/// ドラムパターン文字列中の `|` 省略記法を展開する。
///
/// `|` は次の拍境界まで `.` で埋める（拍境界は `beats_per_step` で決定）。
/// 4/4拍子・分解能16の場合、`beats_per_step` は 4。
///
/// Expand `|` shorthand in a drum pattern string.
///
/// `|` fills with `.` up to the next beat boundary (determined by `beats_per_step`).
/// For resolution 16 in 4/4 time, `beats_per_step` is 4.
///
/// # Arguments
/// * `pattern` - ドラムパターン文字列 / drum pattern string
/// * `beats_per_step` - 1拍あたりのステップ数 / number of steps per beat
///
/// # Returns
/// 展開済みのパターン文字列 / expanded pattern string
pub fn expand_pipe(pattern: &str, beats_per_step: usize) -> String {
    let mut result = String::new();
    for ch in pattern.chars() {
        if ch == '|' {
            let current_pos = result.len();
            let next_boundary = ((current_pos / beats_per_step) + 1) * beats_per_step;
            let fill = next_boundary - current_pos;
            for _ in 0..fill {
                result.push('.');
            }
        } else {
            result.push(ch);
        }
    }
    result
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

        // `)*N` 形式を検出（N は1桁以上の数値）
        // Detect `)*N` form (N is one or more digits)
        if let Some(stripped) = after.strip_prefix('*') {
            let digits_len = stripped
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(stripped.len());
            if digits_len > 0 {
                let n: usize = stripped[..digits_len].parse().unwrap_or(1);
                let repeated = inner.repeat(n);
                let rest = &stripped[digits_len..];
                s = format!("{}{}{}", &s[..open], repeated, rest);
                continue;
            }
        }

        // `(...)` だが `*N` がない場合、括弧を除去してスキップ
        // `(...)` without `*N` — remove parentheses and skip
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

    // --- expand_pipe tests ---

    #[test]
    fn expand_pipe_basic_four_beats() {
        assert_eq!(expand_pipe("x|x|x|x|", 4), "x...x...x...x...");
    }

    #[test]
    fn expand_pipe_leading_pipe() {
        // `|x` → 4 dots then x
        assert_eq!(expand_pipe("|x", 4), "....x");
    }

    #[test]
    fn expand_pipe_sparse() {
        assert_eq!(expand_pipe("|x||x|", 4), "....x.......x...");
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

    // --- expand_pipe: 確率行テスト / probability row tests ---

    /// 確率行の `|` ショートカットが正しく展開されることを検証する。
    /// `.5|.7|` (beats_per_step=4) → `.5..` で4文字境界、`.7..` で次の境界。
    ///
    /// Verify `|` shorthand expansion for probability rows.
    /// `.5|.7|` (beats_per_step=4) → `.5..` to 4-char boundary, `.7..` to next boundary.
    #[test]
    fn expand_pipe_probability_with_digits() {
        assert_eq!(expand_pipe(".5|.7|", 4), ".5...7..");
    }

    /// 先頭パイプ付き確率行の展開を検証する。
    /// `|5|7|` (beats_per_step=4) → 先頭 `|` で4文字分パディング。
    ///
    /// Verify expansion of probability row with leading pipe.
    /// `|5|7|` (beats_per_step=4) → leading `|` pads to 4 chars.
    #[test]
    fn expand_pipe_probability_leading_pipe() {
        assert_eq!(expand_pipe("|5|7|", 4), "....5...7...");
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

    /// 確率行の繰り返し展開後にパイプ展開をチェーンする。
    ///
    /// Chain repetition expansion then pipe expansion for probability rows.
    #[test]
    fn expand_repetition_probability_with_pipe() {
        let expanded = expand_repetition("(.5)*4");
        assert_eq!(expanded, ".5.5.5.5");
        let final_result = expand_pipe(&expanded, 4);
        assert_eq!(final_result, ".5.5.5.5");
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
