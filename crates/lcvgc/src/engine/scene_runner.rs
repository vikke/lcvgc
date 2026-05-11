use std::collections::HashSet;

use rand::Rng;

use crate::ast::scene::{SceneDef, SceneEntry};
use crate::ast::tempo::Tempo;
use crate::engine::shuffle;

/// シーンの1ループ分の解決結果
/// Resolved result for one loop iteration of a scene
#[derive(Debug, Clone, PartialEq)]
pub struct SceneInstance {
    /// 選択されたクリップ名のリスト
    /// List of selected clip names
    pub clips: Vec<String>,
    /// テンポ変更指示（あれば）
    /// Tempo change instruction (if any)
    pub tempo_change: Option<Tempo>,
}

/// シーン定義からクリップを選択する（シャッフル・確率判定付き）
/// Selects clips from a scene definition (with shuffle and probability evaluation)
///
/// ループごとに呼び出して毎回判定し直す。
/// Called per loop iteration to re-evaluate each time.
///
/// エントリごとに確率判定→候補から重み付き選択の順で処理する。
/// Processes each entry by probability check followed by weighted selection from candidates.
pub fn resolve_scene<R: Rng>(scene: &SceneDef, rng: &mut R) -> SceneInstance {
    let mut clips = Vec::new();
    let mut tempo_change = None;

    for entry in &scene.entries {
        match entry {
            SceneEntry::Clip {
                candidates,
                probability,
                muted: _, // muted は scene activate 時の初期状態用 (毎ループの解決には影響しない)
            } => {
                if !shuffle::probability_check(*probability, rng) {
                    continue;
                }
                if let Some(picked) = shuffle::weighted_pick(candidates, rng) {
                    clips.push(picked.to_string());
                }
            }
            SceneEntry::Tempo(tempo) => {
                tempo_change = Some(tempo.clone());
            }
        }
    }

    SceneInstance {
        clips,
        tempo_change,
    }
}

/// シーン定義から「activate 時に初期 mute 状態でロードすべき clip 名」の集合を返す（§8.6）。
/// シャッフル候補に `mute` 前置が付いている場合は、候補リスト内の全 clip 名を muted 集合に含める。
/// resolve_scene が実際にどの clip を選ぶかとは独立で、scene activate 時に1度だけ参照する。
///
/// Return the set of clip names that must be loaded in muted state when the scene
/// becomes active (§8.6). When a shuffle entry carries the `mute` prefix, every
/// candidate name in that entry is included so that whichever candidate is picked
/// is muted from the start. This is independent of the per-loop result of
/// `resolve_scene` and is consulted only once per scene activation.
pub fn initial_muted_clips(scene: &SceneDef) -> HashSet<String> {
    let mut set = HashSet::new();
    for entry in &scene.entries {
        if let SceneEntry::Clip {
            candidates,
            probability: _,
            muted: true,
        } = entry
        {
            for cand in candidates {
                set.insert(cand.clip.clone());
            }
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::scene::ShuffleCandidate;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn candidate(clip: &str, weight: u32) -> ShuffleCandidate {
        ShuffleCandidate {
            clip: clip.to_string(),
            weight,
        }
    }

    fn scene(entries: Vec<SceneEntry>) -> SceneDef {
        SceneDef {
            name: "test".to_string(),
            entries,
        }
    }

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    // 1. 空のシーン → 空のクリップリスト
    #[test]
    fn empty_scene_returns_empty_clips() {
        let s = scene(vec![]);
        let result = resolve_scene(&s, &mut rng());
        assert!(result.clips.is_empty());
        assert!(result.tempo_change.is_none());
    }

    // 2. クリップ1つ、確率なし → 常に選択される
    #[test]
    fn single_clip_no_probability_always_selected() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("bass", 1)],
            probability: None,
            muted: false,
        }]);
        let mut r = rng();
        for _ in 0..100 {
            let result = resolve_scene(&s, &mut r);
            assert_eq!(result.clips, vec!["bass"]);
        }
    }

    // 3. 確率0 → 決して選択されない
    #[test]
    fn probability_zero_never_selected() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("bass", 1)],
            probability: Some(0),
            muted: false,
        }]);
        let mut r = rng();
        for _ in 0..100 {
            let result = resolve_scene(&s, &mut r);
            assert!(result.clips.is_empty());
        }
    }

    // 4. 確率9 → ほぼ常に選択される（90%）
    #[test]
    fn probability_nine_almost_always_selected() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("bass", 1)],
            probability: Some(9),
            muted: false,
        }]);
        let mut r = rng();
        let count = (0..1000)
            .filter(|_| !resolve_scene(&s, &mut r).clips.is_empty())
            .count();
        assert!(count > 850 && count < 950, "count={count}");
    }

    // 5. シャッフル候補 → いずれか1つが選ばれる
    #[test]
    fn shuffle_candidates_one_is_picked() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("a", 1), candidate("b", 1)],
            probability: None,
            muted: false,
        }]);
        let mut r = rng();
        let result = resolve_scene(&s, &mut r);
        assert!(result.clips.len() == 1);
        assert!(result.clips[0] == "a" || result.clips[0] == "b");
    }

    // 6. テンポエントリ → tempo_changeがSome
    #[test]
    fn tempo_entry_sets_tempo_change() {
        let s = scene(vec![SceneEntry::Tempo(Tempo::Absolute(140))]);
        let result = resolve_scene(&s, &mut rng());
        assert_eq!(result.tempo_change, Some(Tempo::Absolute(140)));
        assert!(result.clips.is_empty());
    }

    // 7. クリップとテンポの混在
    #[test]
    fn mixed_clips_and_tempo() {
        let s = scene(vec![
            SceneEntry::Clip {
                candidates: vec![candidate("bass", 1)],
                probability: None,
                muted: false,
            },
            SceneEntry::Tempo(Tempo::Relative(10)),
            SceneEntry::Clip {
                candidates: vec![candidate("lead", 1)],
                probability: None,
                muted: false,
            },
        ]);
        let result = resolve_scene(&s, &mut rng());
        assert_eq!(result.clips, vec!["bass", "lead"]);
        assert_eq!(result.tempo_change, Some(Tempo::Relative(10)));
    }

    // 8. 複数クリップ → 各々独立に判定される
    #[test]
    fn multiple_clips_independent() {
        let s = scene(vec![
            SceneEntry::Clip {
                candidates: vec![candidate("a", 1)],
                probability: None,
                muted: false,
            },
            SceneEntry::Clip {
                candidates: vec![candidate("b", 1)],
                probability: None,
                muted: false,
            },
            SceneEntry::Clip {
                candidates: vec![candidate("c", 1)],
                probability: None,
                muted: false,
            },
        ]);
        let result = resolve_scene(&s, &mut rng());
        assert_eq!(result.clips, vec!["a", "b", "c"]);
    }

    // 9. 重み付きシャッフル → 重い方がより多く選ばれる（統計的検証）
    #[test]
    fn weighted_shuffle_higher_weight_picked_more() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("heavy", 9), candidate("light", 1)],
            probability: None,
            muted: false,
        }]);
        let mut r = rng();
        let heavy_count = (0..1000)
            .filter(|_| resolve_scene(&s, &mut r).clips == vec!["heavy"])
            .count();
        assert!(
            heavy_count > 850 && heavy_count < 950,
            "heavy_count={heavy_count}"
        );
    }

    // 10. 空の候補リスト → クリップ追加なし
    #[test]
    fn empty_candidates_no_clip_added() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![],
            probability: None,
            muted: false,
        }]);
        let result = resolve_scene(&s, &mut rng());
        assert!(result.clips.is_empty());
    }

    // 11. テンポが複数回出現 → 最後のものが採用される
    #[test]
    fn multiple_tempo_entries_last_wins() {
        let s = scene(vec![
            SceneEntry::Tempo(Tempo::Absolute(120)),
            SceneEntry::Tempo(Tempo::Absolute(140)),
        ]);
        let result = resolve_scene(&s, &mut rng());
        assert_eq!(result.tempo_change, Some(Tempo::Absolute(140)));
    }

    // 12. 相対テンポ
    #[test]
    fn relative_tempo_preserved() {
        let s = scene(vec![SceneEntry::Tempo(Tempo::Relative(-10))]);
        let result = resolve_scene(&s, &mut rng());
        assert_eq!(result.tempo_change, Some(Tempo::Relative(-10)));
    }

    // §8.6: initial_muted_clips のテスト
    // §8.6: tests for initial_muted_clips

    /// muted: false のみなら空集合
    /// Empty set when no entry is marked muted.
    #[test]
    fn initial_muted_clips_empty_when_no_muted_entries() {
        let s = scene(vec![
            SceneEntry::Clip {
                candidates: vec![candidate("bass", 1)],
                probability: None,
                muted: false,
            },
            SceneEntry::Clip {
                candidates: vec![candidate("drums", 1)],
                probability: Some(7),
                muted: false,
            },
        ]);
        assert!(initial_muted_clips(&s).is_empty());
    }

    /// muted: true のエントリの clip 名が集合に含まれる
    /// Clip names from muted entries are collected.
    #[test]
    fn initial_muted_clips_collects_muted_entries() {
        let s = scene(vec![
            SceneEntry::Clip {
                candidates: vec![candidate("bass", 1)],
                probability: None,
                muted: false,
            },
            SceneEntry::Clip {
                candidates: vec![candidate("drums", 1)],
                probability: None,
                muted: true,
            },
        ]);
        let muted = initial_muted_clips(&s);
        assert_eq!(muted.len(), 1);
        assert!(muted.contains("drums"));
    }

    /// muted: true でシャッフル候補がある場合、全候補が集合に含まれる
    /// All shuffle candidates are included when the muted prefix is on a shuffle entry.
    #[test]
    fn initial_muted_clips_includes_all_shuffle_candidates() {
        let s = scene(vec![SceneEntry::Clip {
            candidates: vec![candidate("drums_a", 1), candidate("drums_funk", 1)],
            probability: None,
            muted: true,
        }]);
        let muted = initial_muted_clips(&s);
        assert_eq!(muted.len(), 2);
        assert!(muted.contains("drums_a"));
        assert!(muted.contains("drums_funk"));
    }

    /// Tempo エントリは集合に影響しない
    /// Tempo entries do not affect the muted set.
    #[test]
    fn initial_muted_clips_ignores_tempo() {
        let s = scene(vec![
            SceneEntry::Tempo(Tempo::Absolute(120)),
            SceneEntry::Clip {
                candidates: vec![candidate("bass", 1)],
                probability: None,
                muted: true,
            },
        ]);
        let muted = initial_muted_clips(&s);
        assert_eq!(muted.len(), 1);
        assert!(muted.contains("bass"));
    }
}
