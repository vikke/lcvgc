/// 線形補間: from → to を steps ステップで補間
/// steps=0 → 空Vec
/// steps=1 → [to]
/// steps=2 → [from, to]
/// steps=3 → [from, midpoint, to]
/// 各値は0-127にクランプ
pub fn interpolate_linear(from: u8, to: u8, steps: usize) -> Vec<u8> {
    match steps {
        0 => vec![],
        1 => vec![clamp(to)],
        _ => {
            let from_f = from as f64;
            let to_f = to as f64;
            (0..steps)
                .map(|i| {
                    let t = i as f64 / (steps - 1) as f64;
                    let value = from_f + (to_f - from_f) * t;
                    clamp_f64(value)
                })
                .collect()
        }
    }
}

/// 指数補間: from → to を steps ステップで指数カーブで補間
/// 指数カーブ: value = from + (to - from) * (t^2)  (t = i / (steps-1))
/// steps=0 → 空Vec
/// steps=1 → [to]
/// 各値は0-127にクランプ
pub fn interpolate_exponential(from: u8, to: u8, steps: usize) -> Vec<u8> {
    match steps {
        0 => vec![],
        1 => vec![clamp(to)],
        _ => {
            let from_f = from as f64;
            let to_f = to as f64;
            (0..steps)
                .map(|i| {
                    let t = i as f64 / (steps - 1) as f64;
                    let value = from_f + (to_f - from_f) * t * t;
                    clamp_f64(value)
                })
                .collect()
        }
    }
}

fn clamp(v: u8) -> u8 {
    v.min(127)
}

fn clamp_f64(v: f64) -> u8 {
    (v.round() as i32).clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_steps_0() {
        assert_eq!(interpolate_linear(0, 127, 0), Vec::<u8>::new());
    }

    #[test]
    fn linear_steps_1() {
        assert_eq!(interpolate_linear(0, 127, 1), vec![127]);
    }

    #[test]
    fn linear_steps_2() {
        assert_eq!(interpolate_linear(0, 127, 2), vec![0, 127]);
    }

    #[test]
    fn linear_steps_3() {
        assert_eq!(interpolate_linear(0, 100, 3), vec![0, 50, 100]);
    }

    #[test]
    fn linear_steps_5() {
        assert_eq!(interpolate_linear(0, 100, 5), vec![0, 25, 50, 75, 100]);
    }

    #[test]
    fn linear_reverse() {
        assert_eq!(interpolate_linear(100, 0, 3), vec![100, 50, 0]);
    }

    #[test]
    fn linear_no_change() {
        assert_eq!(interpolate_linear(64, 64, 3), vec![64, 64, 64]);
    }

    #[test]
    fn exponential_steps_0() {
        assert_eq!(interpolate_exponential(0, 100, 0), Vec::<u8>::new());
    }

    #[test]
    fn exponential_steps_1() {
        assert_eq!(interpolate_exponential(0, 100, 1), vec![100]);
    }

    #[test]
    fn exponential_steps_2() {
        assert_eq!(interpolate_exponential(0, 100, 2), vec![0, 100]);
    }

    #[test]
    fn exponential_curve_below_linear() {
        let exp = interpolate_exponential(0, 100, 5);
        let lin = interpolate_linear(0, 100, 5);
        for i in 1..4 {
            assert!(
                exp[i] <= lin[i],
                "exp[{}]={} should be <= lin[{}]={}",
                i, exp[i], i, lin[i]
            );
        }
        assert_eq!(exp[0], 0);
        assert_eq!(exp[4], 100);
    }

    #[test]
    fn exponential_reverse() {
        let exp = interpolate_exponential(100, 0, 5);
        let lin = interpolate_linear(100, 0, 5);
        for i in 1..4 {
            assert!(
                exp[i] >= lin[i],
                "exp[{}]={} should be >= lin[{}]={}",
                i, exp[i], i, lin[i]
            );
        }
        assert_eq!(exp[0], 100);
        assert_eq!(exp[4], 0);
    }
}
