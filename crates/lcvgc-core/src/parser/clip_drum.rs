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
    loop {
        // 内側に `(` を含まない最も内側の `(...)` ペアを探す
        // Find the innermost `(...)` pair (no `(` inside)
        let Some(close) = s.find(')') else {
            break;
        };
        let prefix = &s[..close];
        let Some(open) = prefix.rfind('(') else {
            break;
        };
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
}
