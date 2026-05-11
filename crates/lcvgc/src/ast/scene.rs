use crate::ast::tempo::Tempo;

/// シャッフル再生の候補クリップ
/// A candidate clip for shuffle playback
#[derive(Debug, Clone, PartialEq)]
pub struct ShuffleCandidate {
    /// クリップ名
    /// Clip name
    pub clip: String,
    /// 選択重み（デフォルト1）
    /// Selection weight (default 1)
    pub weight: u32, // default 1
}

/// シーン内のエントリ（クリップまたはテンポ変更）
/// An entry within a scene (clip or tempo change)
#[derive(Debug, Clone, PartialEq)]
pub enum SceneEntry {
    /// クリップエントリ（シャッフル候補と発音確率付き）
    /// Clip entry (with shuffle candidates and firing probability)
    Clip {
        /// シャッフル候補のリスト（1件=単純指定、2件以上=シャッフル）
        /// List of shuffle candidates (1 = simple, >1 = shuffle)
        candidates: Vec<ShuffleCandidate>, // 1 = simple, >1 = shuffle
        /// 発音確率 (1-9、オプション)
        /// Firing probability (1-9, optional)
        probability: Option<u8>, // 1-9
        /// 初期 mute フラグ（§8.6, scene 内 `mute` 前置で true）
        /// `mute` を前置すると true。scene activate 時に該当 clip を初期 mute 状態でロードする。
        /// Initial-mute flag (§8.6). True when the entry was prefixed with `mute` in the
        /// scene block; the clip is loaded in muted state when the scene becomes active.
        muted: bool,
    },
    /// テンポ変更エントリ
    /// Tempo change entry
    Tempo(Tempo),
}

/// シーン定義
/// Scene definition
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDef {
    /// シーン名
    /// Scene name
    pub name: String,
    /// シーン内のエントリリスト
    /// List of entries within the scene
    pub entries: Vec<SceneEntry>,
}
