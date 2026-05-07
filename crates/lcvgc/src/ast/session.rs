/// セッション内シーンのリピート指定
/// Repeat specification for a scene within a session
#[derive(Debug, Clone, PartialEq)]
pub enum SessionRepeat {
    /// 1回のみ再生
    /// Play once
    Once,
    /// 指定回数リピート
    /// Repeat a specified number of times
    Count(u32),
    /// 無限ループ
    /// Infinite loop
    Loop,
}

/// セッション内のエントリ（シーンとリピート指定）
/// An entry within a session (scene with repeat specification)
#[derive(Debug, Clone, PartialEq)]
pub struct SessionEntry {
    /// シーン名
    /// Scene name
    pub scene: String,
    /// リピート指定
    /// Repeat specification
    pub repeat: SessionRepeat,
}

/// セッション定義
/// Session definition
#[derive(Debug, Clone, PartialEq)]
pub struct SessionDef {
    /// セッション名
    /// Session name
    pub name: String,
    /// セッション内のエントリリスト
    /// List of entries within the session
    pub entries: Vec<SessionEntry>,
}
