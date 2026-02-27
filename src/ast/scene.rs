use crate::ast::tempo::Tempo;

#[derive(Debug, Clone, PartialEq)]
pub struct ShuffleCandidate {
    pub clip: String,
    pub weight: u32, // default 1
}

#[derive(Debug, Clone, PartialEq)]
pub enum SceneEntry {
    Clip {
        candidates: Vec<ShuffleCandidate>, // 1 = simple, >1 = shuffle
        probability: Option<u8>,           // 1-9
    },
    Tempo(Tempo),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneDef {
    pub name: String,
    pub entries: Vec<SceneEntry>,
}
