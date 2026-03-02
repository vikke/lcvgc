#[derive(Debug, Clone, PartialEq)]
pub enum RepeatSpec {
    Once,
    Count(u32),
    Loop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayTarget {
    Scene(String),
    Session(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayCommand {
    pub target: PlayTarget,
    pub repeat: RepeatSpec,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopCommand {
    pub target: Option<String>,
}
