use rand::Rng;

/// ドラムの確率行に基づく発音判定
///
/// probability: 0-9 (0=0%, 1=10%, ..., 9=90%), None=100%（必ず発音）
pub fn should_trigger<R: Rng>(probability: Option<u8>, rng: &mut R) -> bool {
    match probability {
        None => true,
        Some(p) => {
            if p == 0 {
                return false;
            }
            let threshold = p as f64 * 10.0;
            let roll: f64 = rng.gen_range(0.0..100.0);
            roll < threshold
        }
    }
}

/// ステップ列に確率を適用し、各ステップの発音可否を返す
///
/// hits_len: ステップ数
/// probability: 各ステップの確率値（0-9）のベクタ、Noneなら全ステップ100%
/// 戻り値: true = 発音, false = ミュート
pub fn apply_probability_mask<R: Rng>(
    hits_len: usize,
    probability: &Option<Vec<u8>>,
    rng: &mut R,
) -> Vec<bool> {
    match probability {
        None => vec![true; hits_len],
        Some(probs) => (0..hits_len)
            .map(|i| {
                let p = probs.get(i).copied();
                should_trigger(p, rng)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn fixed_rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    #[test]
    fn none_always_triggers() {
        let mut rng = fixed_rng();
        for _ in 0..100 {
            assert!(should_trigger(None, &mut rng));
        }
    }

    #[test]
    fn zero_never_triggers() {
        let mut rng = fixed_rng();
        for _ in 0..100 {
            assert!(!should_trigger(Some(0), &mut rng));
        }
    }

    #[test]
    fn nine_triggers_about_90_percent() {
        let mut rng = fixed_rng();
        let count = (0..1000)
            .filter(|_| should_trigger(Some(9), &mut rng))
            .count();
        // 90% +/- 5%
        assert!(count > 850 && count < 950, "count was {count}");
    }

    #[test]
    fn one_triggers_about_10_percent() {
        let mut rng = fixed_rng();
        let count = (0..1000)
            .filter(|_| should_trigger(Some(1), &mut rng))
            .count();
        // 10% +/- 5%
        assert!(count > 50 && count < 150, "count was {count}");
    }

    #[test]
    fn five_triggers_about_50_percent() {
        let mut rng = fixed_rng();
        let count = (0..1000)
            .filter(|_| should_trigger(Some(5), &mut rng))
            .count();
        // 50% +/- 7%
        assert!(count > 430 && count < 570, "count was {count}");
    }

    #[test]
    fn apply_mask_none_all_true() {
        let mut rng = fixed_rng();
        let mask = apply_probability_mask(4, &None, &mut rng);
        assert_eq!(mask, vec![true, true, true, true]);
    }

    #[test]
    fn apply_mask_all_zero_all_false() {
        let mut rng = fixed_rng();
        let mask = apply_probability_mask(3, &Some(vec![0, 0, 0]), &mut rng);
        assert_eq!(mask, vec![false, false, false]);
    }

    #[test]
    fn apply_mask_shorter_probs_uses_none_for_missing() {
        let mut rng = fixed_rng();
        // probs has 2 entries but hits_len is 4; indices 2,3 get None -> true
        let mask = apply_probability_mask(4, &Some(vec![0, 0]), &mut rng);
        assert!(!mask[0]);
        assert!(!mask[1]);
        assert!(mask[2]);
        assert!(mask[3]);
    }

    #[test]
    fn apply_mask_length_matches_hits_len() {
        let mut rng = fixed_rng();
        let mask = apply_probability_mask(8, &Some(vec![5; 8]), &mut rng);
        assert_eq!(mask.len(), 8);
    }

    #[test]
    fn apply_mask_empty() {
        let mut rng = fixed_rng();
        let mask = apply_probability_mask(0, &None, &mut rng);
        assert!(mask.is_empty());
    }
}
