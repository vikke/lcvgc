use crate::ast::clip_drum::HitSymbol;

/// Expand `|` shorthand in a drum pattern string.
///
/// `|` fills with `.` up to the next beat boundary (determined by `beats_per_step`).
/// For resolution 16 in 4/4 time, `beats_per_step` is 4.
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

/// Parse an expanded (no `|`) pattern string into a vector of `HitSymbol`.
pub fn parse_hit_symbols(input: &str) -> Vec<HitSymbol> {
    input
        .chars()
        .map(|ch| match ch {
            'x' => HitSymbol::Normal,
            'X' => HitSymbol::Accent,
            'o' => HitSymbol::Ghost,
            '.' => HitSymbol::Rest,
            other => panic!("unknown hit symbol: '{}'", other),
        })
        .collect()
}

/// Parse a probability row string into per-step probabilities (0-100).
///
/// - `.` → 100 (always fire)
/// - `0` → 0 (never fire)
/// - `1`-`9` → 10-90
pub fn parse_probability_row(input: &str) -> Vec<u8> {
    input
        .chars()
        .map(|ch| match ch {
            '.' => 100,
            '0' => 0,
            '1'..='9' => (ch as u8 - b'0') * 10,
            other => panic!("unknown probability symbol: '{}'", other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip_drum::HitSymbol::*;

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
            parse_hit_symbols("x...x..."),
            vec![Normal, Rest, Rest, Rest, Normal, Rest, Rest, Rest]
        );
    }

    #[test]
    fn hit_symbols_accent_ghost() {
        assert_eq!(
            parse_hit_symbols("x.o.X.o."),
            vec![Normal, Rest, Ghost, Rest, Accent, Rest, Ghost, Rest]
        );
    }

    // --- parse_probability_row tests ---

    #[test]
    fn probability_basic() {
        assert_eq!(
            parse_probability_row("..5...7."),
            vec![100, 100, 50, 100, 100, 100, 70, 100]
        );
    }

    #[test]
    fn probability_zero() {
        assert_eq!(parse_probability_row("0"), vec![0]);
    }

    #[test]
    fn probability_full_row() {
        assert_eq!(
            parse_probability_row("..5...7...3...5."),
            vec![100, 100, 50, 100, 100, 100, 70, 100, 100, 100, 30, 100, 100, 100, 50, 100]
        );
    }
}
