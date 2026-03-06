use rand::Rng;

use crate::ast::scene::ShuffleCandidate;

/// 重み付きランダム選択。candidates から weight 比率で1つ選ぶ。
/// candidates が空の場合は None を返す。
pub fn weighted_pick<'a, R: Rng>(
    candidates: &'a [ShuffleCandidate],
    rng: &mut R,
) -> Option<&'a str> {
    if candidates.is_empty() {
        return None;
    }

    let total_weight: u32 = candidates.iter().map(|c| c.weight).sum();
    if total_weight == 0 {
        return None;
    }

    let mut roll = rng.gen_range(0..total_weight);
    for candidate in candidates {
        if roll < candidate.weight {
            return Some(&candidate.clip);
        }
        roll -= candidate.weight;
    }

    candidates.last().map(|c| c.clip.as_str())
}

/// 確率判定。probability=None → 常にtrue (100%)
/// probability=Some(n) where n=1..9 → n*10 % の確率でtrue
/// n=0 → 常にfalse
pub fn probability_check<R: Rng>(probability: Option<u8>, rng: &mut R) -> bool {
    match probability {
        None => true,
        Some(0) => false,
        Some(n) => {
            let threshold = (n as u32) * 10;
            let roll: u32 = rng.gen_range(0..100);
            roll < threshold
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn candidate(clip: &str, weight: u32) -> ShuffleCandidate {
        ShuffleCandidate {
            clip: clip.to_string(),
            weight,
        }
    }

    #[test]
    fn weighted_pick_single_candidate_always_returns_it() {
        let candidates = vec![candidate("a", 1)];
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            assert_eq!(weighted_pick(&candidates, &mut rng), Some("a"));
        }
    }

    #[test]
    fn weighted_pick_empty_returns_none() {
        let candidates: Vec<ShuffleCandidate> = vec![];
        let mut rng = StdRng::seed_from_u64(42);
        assert_eq!(weighted_pick(&candidates, &mut rng), None);
    }

    #[test]
    fn weighted_pick_equal_weight_roughly_even() {
        let candidates = vec![candidate("a", 1), candidate("b", 1)];
        let mut rng = StdRng::seed_from_u64(42);
        let mut count_a = 0u32;
        let mut count_b = 0u32;
        for _ in 0..1000 {
            match weighted_pick(&candidates, &mut rng) {
                Some("a") => count_a += 1,
                Some("b") => count_b += 1,
                _ => panic!("unexpected result"),
            }
        }
        assert!(count_a > 400 && count_a < 600, "count_a={count_a}");
        assert!(count_b > 400 && count_b < 600, "count_b={count_b}");
    }

    #[test]
    fn weighted_pick_3_to_1_ratio() {
        let candidates = vec![candidate("a", 3), candidate("b", 1)];
        let mut rng = StdRng::seed_from_u64(42);
        let mut count_a = 0u32;
        for _ in 0..1000 {
            if weighted_pick(&candidates, &mut rng) == Some("a") {
                count_a += 1;
            }
        }
        assert!(count_a > 650 && count_a < 850, "count_a={count_a}");
    }

    #[test]
    fn probability_check_none_always_true() {
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            assert!(probability_check(None, &mut rng));
        }
    }

    #[test]
    fn probability_check_zero_always_false() {
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            assert!(!probability_check(Some(0), &mut rng));
        }
    }

    #[test]
    fn probability_check_nine_about_90_percent() {
        let mut rng = StdRng::seed_from_u64(42);
        let count = (0..1000)
            .filter(|_| probability_check(Some(9), &mut rng))
            .count();
        assert!(count > 850 && count < 950, "count={count}");
    }

    #[test]
    fn probability_check_five_about_50_percent() {
        let mut rng = StdRng::seed_from_u64(42);
        let count = (0..1000)
            .filter(|_| probability_check(Some(5), &mut rng))
            .count();
        assert!(count > 400 && count < 600, "count={count}");
    }
}
