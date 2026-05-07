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

/// ポーズコマンド（§10.4）
/// Pause command (§10.4)
///
/// `None` = 全体 pause / scene/session/clip 名 = 名前指定 pause。
/// 名前不一致（tick 凍結対象なし）は Evaluator 側で no-op 扱いとなる。
///
/// `None` pauses globally. A name targets a scene/session (if matching the
/// currently playing one) or a clip (if present in `active_scene`).
/// Name mismatches are treated as a no-op by the evaluator.
#[derive(Debug, Clone, PartialEq)]
pub struct PauseCommand {
    /// ポーズ対象名（`None` の場合は全体 pause）
    /// Target name to pause (`None` means pause all)
    pub target: Option<String>,
}

/// 再開コマンド（§10.4）
/// Resume command (§10.4)
///
/// `None` = 全体 resume / scene/session/clip 名 = 名前指定 resume。
/// 名前不一致（Paused 中でない scene/session 名、active_scene に無い clip 名）は
/// Evaluator 側で no-op 扱いとなる。
///
/// `None` resumes globally. A name targets the Paused scene/session (if
/// matching) or a clip in `active_scene`. Name mismatches are no-ops.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeCommand {
    /// 再開対象名（`None` の場合は全体 resume）
    /// Target name to resume (`None` means resume all)
    pub target: Option<String>,
}

/// クリップ・ミュートコマンド（§10.4）
/// Clip mute command (§10.4)
///
/// `active_scene` 内の指定 clip を mute する。tick は継続し、位相は維持されるが、
/// 発音は停止し AllNotesOff が送出される。`stop <clip>` のリネームで追加された
/// 構文で、scene/session 全体停止の `stop` とは挙動が明確に異なる。
///
/// Mutes the named clip inside `active_scene`. Tick continues and the phase is
/// preserved, but note output stops and AllNotesOff is emitted. This replaces
/// the previous `stop <clip>` form to distinguish it from the hard stop applied
/// to scenes and sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct MuteCommand {
    /// ミュート対象の clip 名
    /// Target clip name to mute
    pub target: String,
}

/// クリップ・アンミュートコマンド（§10.4）
/// Clip unmute command (§10.4)
///
/// `MuteCommand` で mute された clip のミュートを解除する。対応する clip が
/// active_scene に存在しない、または mute されていない場合は Evaluator 側で
/// no-op 扱いとなる。
///
/// Releases the mute applied by `MuteCommand`. If the clip is missing from
/// `active_scene` or not muted, the evaluator treats it as a no-op.
#[derive(Debug, Clone, PartialEq)]
pub struct UnmuteCommand {
    /// アンミュート対象の clip 名
    /// Target clip name to unmute
    pub target: String,
}
