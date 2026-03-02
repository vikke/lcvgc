#[derive(Debug, Clone, PartialEq)]
pub enum SessionRepeat {
    Once,
    Count(u32),
    Loop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionEntry {
    pub scene: String,
    pub repeat: SessionRepeat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionDef {
    pub name: String,
    pub entries: Vec<SessionEntry>,
}
