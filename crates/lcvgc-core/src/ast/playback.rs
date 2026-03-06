/// 再生のリピート指定
/// Repeat specification for playback
#[derive(Debug, Clone, PartialEq)]
pub enum RepeatSpec {
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

/// 再生対象（シーンまたはセッション）
/// Play target (scene or session)
#[derive(Debug, Clone, PartialEq)]
pub enum PlayTarget {
    /// シーンを再生対象とする
    /// Target a scene for playback
    Scene(String),
    /// セッションを再生対象とする
    /// Target a session for playback
    Session(String),
}

/// 再生コマンド
/// Play command
#[derive(Debug, Clone, PartialEq)]
pub struct PlayCommand {
    /// 再生対象
    /// Play target
    pub target: PlayTarget,
    /// リピート指定
    /// Repeat specification
    pub repeat: RepeatSpec,
}

/// 停止コマンド
/// Stop command
#[derive(Debug, Clone, PartialEq)]
pub struct StopCommand {
    /// 停止対象名（`None`の場合は全停止）
    /// Target name to stop (`None` means stop all)
    pub target: Option<String>,
}
