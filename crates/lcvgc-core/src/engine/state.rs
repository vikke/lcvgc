//! 再生状態管理モジュール
//! Playback state management module
//!
//! シーケンサーの再生状態遷移を管理する。
//! Manages playback state transitions of the sequencer.
//!
//! シーン再生・セッション再生・停止の3状態間を遷移し、
//! リピートモードに応じた小節進行制御を行う。
//! Transitions between three states: scene playback, session playback, and stopped,
//! and controls measure progression according to the repeat mode.

use crate::ast::playback::RepeatSpec;
use crate::ast::session::SessionDef;
use crate::engine::session_runner::{SessionAction, SessionRunner};

/// 再生状態
/// Playback state
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    /// 停止中
    /// Stopped
    Stopped,
    /// シーン再生中
    /// Playing a scene
    PlayingScene {
        /// シーン名
        /// Scene name
        name: String,
        /// リピートモード
        /// Repeat mode
        repeat: RepeatMode,
    },
    /// セッション再生中
    /// Playing a session
    PlayingSession {
        /// セッション名
        /// Session name
        name: String,
        /// セッション全体のリピートモード
        /// Repeat mode for the entire session
        repeat: RepeatMode,
        /// 現在のエントリインデックス
        /// Current entry index
        entry_index: usize,
        /// 現在のシーンのリピートモード
        /// Repeat mode for the current scene
        scene_repeat: RepeatMode,
    },
    /// ポーズ中（§10.4）
    /// Paused (§10.4)
    ///
    /// `prev` に pause 直前の状態を保持し、resume 時に復元する。
    /// `prev` には `Paused` 自身は入らない（pause の二重適用は no-op）。
    /// Holds the pre-pause state in `prev` and restores it on resume.
    /// `prev` never nests `Paused` itself (re-pause is a no-op).
    Paused {
        /// pause 直前の state（resume 時に復元）
        /// Pre-pause state (restored on resume)
        prev: Box<PlaybackState>,
    },
}

/// リピートモード（内部管理用）
/// Repeat mode (for internal management)
#[derive(Debug, Clone, PartialEq)]
pub enum RepeatMode {
    /// 1回のみ
    /// Once only
    Once,
    /// 残り回数
    /// Remaining count
    Count {
        /// 残りリピート回数
        /// Remaining repeat count
        remaining: u32,
    },
    /// 無限ループ
    /// Infinite loop
    Loop,
}

/// 再生コマンド
/// Playback command
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    /// シーン再生
    /// Play a scene
    PlayScene {
        /// シーン名
        /// Scene name
        name: String,
        /// リピート指定
        /// Repeat specification
        repeat: RepeatSpec,
    },
    /// セッション再生
    /// Play a session
    PlaySession {
        /// セッション名
        /// Session name
        name: String,
        /// リピート指定
        /// Repeat specification
        repeat: RepeatSpec,
    },
    /// 停止（対象指定なしで全停止）
    /// Stop (stops all if no target specified)
    Stop {
        /// 停止対象の名前（Noneで全停止）
        /// Name of the target to stop (None to stop all)
        target: Option<String>,
    },
    /// ポーズ（§10.4）
    /// Pause (§10.4)
    ///
    /// `target = None` で全体 pause、`Some(name)` で scene/session 名一致時のみ全体 pause。
    /// clip 名を指定した場合は StateManager レベルでは no-op となり、
    /// Evaluator 側で active_scene の該当 clip を pause する。
    ///
    /// `None` pauses the whole playback; `Some(name)` only pauses when the
    /// current scene/session name matches. Clip-name targets are no-ops at
    /// the StateManager level (the evaluator handles clip-level pausing).
    Pause {
        /// ポーズ対象名（Noneで全体） / Pause target (None = whole)
        target: Option<String>,
    },
    /// 再開（§10.4）
    /// Resume (§10.4)
    ///
    /// `target = None` で全体 resume、`Some(name)` で Paused の prev 名と一致時のみ全体 resume。
    /// clip 名を指定した場合は StateManager レベルでは no-op となり、
    /// Evaluator 側で active_scene の該当 clip を resume する。
    ///
    /// `None` resumes the whole playback; `Some(name)` only resumes when the
    /// paused scene/session name matches. Clip-name targets are no-ops at
    /// the StateManager level (the evaluator handles clip-level resuming).
    Resume {
        /// 再開対象名（Noneで全体） / Resume target (None = whole)
        target: Option<String>,
    },
}

/// 小節進行時のアクション
/// Action upon measure progression
#[derive(Debug, Clone, PartialEq)]
pub enum NextAction {
    /// 同じシーンを続行
    /// Continue the same scene
    ContinueScene,
    /// シーン完了、停止
    /// Scene complete, stop
    SceneComplete,
    /// セッション内の次シーンへ
    /// Advance to the next scene in the session
    NextSessionEntry {
        /// 次のシーン名
        /// Next scene name
        scene_name: String,
    },
    /// セッション完了、停止
    /// Session complete, stop
    SessionComplete,
}

/// 再生状態マネージャ
/// Playback state manager
///
/// 再生コマンドの適用と小節進行に伴う状態遷移を管理する。
/// Manages state transitions from playback command application and measure progression.
#[derive(Debug)]
pub struct StateManager {
    /// 現在の再生状態
    /// Current playback state
    state: PlaybackState,
    /// セッション再生中の進行管理ランナー
    /// Runner that manages progression during session playback
    session_runner: Option<SessionRunner>,
    /// 次のエントリ遷移時に差し替える新しい session 定義（§12 session上書き）
    /// Session definition pending swap at the next entry transition (§12 session overwrite)
    pending_session: Option<SessionDef>,
}

impl StateManager {
    /// 停止状態で初期化
    /// Initializes in the stopped state
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            session_runner: None,
            pending_session: None,
        }
    }

    /// 現在の再生状態への参照を返す
    /// Returns a reference to the current playback state
    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// コマンドを適用して状態を遷移させる
    /// Applies a command and transitions the state
    ///
    /// 注意: `PlaySession` の場合は SessionRunner が初期化されないため、
    /// 次エントリ名の解決が必要な用途では [`apply_play_session`] を使うこと。
    /// Note: For `PlaySession`, the SessionRunner is not initialized, so use
    /// [`apply_play_session`] when next-entry resolution is required.
    pub fn apply_command(&mut self, cmd: PlaybackCommand) {
        match cmd {
            PlaybackCommand::PlayScene { name, repeat } => {
                self.state = PlaybackState::PlayingScene {
                    name,
                    repeat: Self::from_repeat_spec(&repeat),
                };
                self.session_runner = None;
                self.pending_session = None;
            }
            PlaybackCommand::PlaySession { name, repeat } => {
                self.state = PlaybackState::PlayingSession {
                    name,
                    repeat: Self::from_repeat_spec(&repeat),
                    entry_index: 0,
                    scene_repeat: RepeatMode::Once,
                };
                self.session_runner = None;
                self.pending_session = None;
            }
            PlaybackCommand::Stop { target } => match target {
                None => {
                    self.state = PlaybackState::Stopped;
                    self.session_runner = None;
                    self.pending_session = None;
                }
                Some(ref target_name) => {
                    let should_stop = match &self.state {
                        PlaybackState::PlayingScene { name, .. } => name == target_name,
                        PlaybackState::PlayingSession { name, .. } => name == target_name,
                        PlaybackState::Stopped => false,
                        // Paused 中の stop <name>: prev の名前と一致するなら停止する（§10.4 D7）
                        // stop <name> while paused: stop if `prev` matches (§10.4 D7)
                        PlaybackState::Paused { prev } => match prev.as_ref() {
                            PlaybackState::PlayingScene { name, .. } => name == target_name,
                            PlaybackState::PlayingSession { name, .. } => name == target_name,
                            _ => false,
                        },
                    };
                    if should_stop {
                        self.state = PlaybackState::Stopped;
                        self.session_runner = None;
                        self.pending_session = None;
                    }
                }
            },
            // §10.4 Pause: Paused バリアントへ遷移する
            // §10.4 Pause: transition to the Paused variant
            PlaybackCommand::Pause { target } => {
                self.apply_pause(target);
            }
            // §10.4 Resume: Paused から prev へ復元する
            // §10.4 Resume: restore `prev` from Paused
            PlaybackCommand::Resume { target } => {
                self.apply_resume(target);
            }
        }
    }

    /// §10.4: Pause コマンドの state 遷移を適用する
    ///
    /// * `target = None`: 再生中なら Paused { prev } に遷移
    /// * `target = Some(name)`: 現在再生中の scene/session 名と一致なら Paused に遷移、
    ///   それ以外（clip 名・不一致）は state 不変（Evaluator 側で clip 個別 pause）
    ///
    /// 既に Stopped または Paused の場合は state 不変。
    ///
    /// Applies the state transition for a Pause command.
    /// * `None`: if playing, move to `Paused { prev }`.
    /// * `Some(name)`: only transitions when the current scene/session name matches.
    ///   Clip names and mismatches leave the state unchanged (the evaluator
    ///   handles clip-level pausing separately).
    ///
    /// Already Stopped or Paused states are left unchanged.
    fn apply_pause(&mut self, target: Option<String>) {
        // 既に Paused または Stopped の場合は何もしない
        // No-op when already paused or stopped
        let is_playing = matches!(
            &self.state,
            PlaybackState::PlayingScene { .. } | PlaybackState::PlayingSession { .. }
        );
        if !is_playing {
            return;
        }
        let should_pause = match &target {
            None => true,
            Some(name) => match &self.state {
                PlaybackState::PlayingScene { name: n, .. } => n == name,
                PlaybackState::PlayingSession { name: n, .. } => n == name,
                _ => false,
            },
        };
        if should_pause {
            let prev = std::mem::replace(&mut self.state, PlaybackState::Stopped);
            self.state = PlaybackState::Paused {
                prev: Box::new(prev),
            };
        }
    }

    /// §10.4: Resume コマンドの state 遷移を適用する
    ///
    /// * `target = None`: Paused なら prev へ復元
    /// * `target = Some(name)`: Paused かつ prev の scene/session 名と一致なら復元、
    ///   それ以外（clip 名・不一致）は state 不変（Evaluator 側で clip 個別 resume）
    ///
    /// Paused でない場合は state 不変。
    ///
    /// Applies the state transition for a Resume command.
    /// * `None`: if Paused, restore `prev`.
    /// * `Some(name)`: only restores when the `prev` scene/session name matches.
    ///   Clip names and mismatches leave the state unchanged.
    fn apply_resume(&mut self, target: Option<String>) {
        let PlaybackState::Paused { prev } = &self.state else {
            return;
        };
        let should_resume = match &target {
            None => true,
            Some(name) => match prev.as_ref() {
                PlaybackState::PlayingScene { name: n, .. } => n == name,
                PlaybackState::PlayingSession { name: n, .. } => n == name,
                _ => false,
            },
        };
        if should_resume {
            // Paused の prev を取り出して state に戻す
            // Extract `prev` from Paused and restore it as the current state
            if let PlaybackState::Paused { prev } =
                std::mem::replace(&mut self.state, PlaybackState::Stopped)
            {
                self.state = *prev;
            }
        }
    }

    /// 現在の state が Paused か
    /// Whether the current state is Paused
    pub fn is_paused(&self) -> bool {
        matches!(&self.state, PlaybackState::Paused { .. })
    }

    /// セッション再生を SessionDef 付きで開始する（§9/§10.2）
    /// Starts session playback with a SessionDef (§9/§10.2)
    ///
    /// SessionRunner を内部で構築し、以降の `scene_loop_complete` で
    /// 次シーン名を正しく返せるようにする。
    /// Constructs the SessionRunner internally so subsequent
    /// `scene_loop_complete` calls can return correct next scene names.
    pub fn apply_play_session(&mut self, session: &SessionDef, repeat: RepeatSpec) {
        let runner = match repeat {
            RepeatSpec::Loop => SessionRunner::new_looping(session),
            _ => SessionRunner::new(session),
        };
        self.state = PlaybackState::PlayingSession {
            name: session.name.clone(),
            repeat: Self::from_repeat_spec(&repeat),
            entry_index: 0,
            scene_repeat: RepeatMode::Once,
        };
        self.session_runner = Some(runner);
        self.pending_session = None;
    }

    /// session 定義が更新されたことを通知する（§12 session 上書き）
    /// Notifies that a session definition has been updated (§12 session overwrite)
    ///
    /// 現在再生中のセッションと同名なら、次のエントリ遷移時に新定義へ差し替える。
    /// If the name matches the currently playing session, the new definition
    /// will be swapped in at the next entry transition.
    pub fn notify_session_updated(&mut self, session: &SessionDef) {
        if let PlaybackState::PlayingSession { name, .. } = &self.state {
            if name == &session.name {
                self.pending_session = Some(session.clone());
            }
        }
    }

    /// シーンの1ループ完了時に呼び出し、次のアクションを返す
    /// Called when one scene loop completes, returns the next action
    ///
    /// リピートモードに応じて状態を更新し、適切なアクションを返す。
    /// Updates the state according to the repeat mode and returns the appropriate action.
    pub fn scene_loop_complete(&mut self) -> NextAction {
        match &mut self.state {
            PlaybackState::Stopped => NextAction::SceneComplete,
            // §10.4: Paused 中は時間が止まっているため scene_loop_complete は基本呼ばれない。
            // 念のため呼ばれても state を維持し、シーンを継続扱いにする。
            // §10.4: scene_loop_complete should not fire while paused (tick is frozen),
            // but if it does, keep the state and treat it as scene continuation.
            PlaybackState::Paused { .. } => NextAction::ContinueScene,
            PlaybackState::PlayingScene { repeat, .. } => match repeat {
                RepeatMode::Once => {
                    self.state = PlaybackState::Stopped;
                    NextAction::SceneComplete
                }
                RepeatMode::Count { remaining } => {
                    *remaining -= 1;
                    if *remaining == 0 {
                        self.state = PlaybackState::Stopped;
                        NextAction::SceneComplete
                    } else {
                        NextAction::ContinueScene
                    }
                }
                RepeatMode::Loop => NextAction::ContinueScene,
            },
            PlaybackState::PlayingSession { .. } => self.session_loop_complete(),
        }
    }

    /// セッション再生中の 1 シーンループ完了処理
    /// Handles completion of one scene loop during session playback
    ///
    /// SessionRunner が存在する場合は runner.advance() で次シーンを解決する。
    /// pending_session がある場合はエントリ遷移時に runner を差し替える（§12）。
    /// runner が無い場合は後方互換のため従来の entry_index インクリメントのみ行う。
    /// If SessionRunner exists, uses runner.advance() to resolve the next scene.
    /// If pending_session exists, swaps the runner at entry transition (§12).
    /// If no runner exists, falls back to the legacy entry_index increment for compatibility.
    fn session_loop_complete(&mut self) -> NextAction {
        // SessionRunner が未設定（レガシー apply_command 経由）の場合は従来動作
        // Legacy behavior when no SessionRunner is set (via legacy apply_command path)
        if self.session_runner.is_none() {
            if let PlaybackState::PlayingSession {
                entry_index,
                scene_repeat,
                ..
            } = &mut self.state
            {
                let scene_exhausted = match scene_repeat {
                    RepeatMode::Once => true,
                    RepeatMode::Count { remaining } => {
                        *remaining -= 1;
                        *remaining == 0
                    }
                    RepeatMode::Loop => false,
                };
                if !scene_exhausted {
                    return NextAction::ContinueScene;
                }
                *entry_index += 1;
                *scene_repeat = RepeatMode::Once;
                return NextAction::NextSessionEntry {
                    scene_name: String::new(),
                };
            }
            return NextAction::SceneComplete;
        }

        // runner 経由でエントリ遷移を解決
        // Resolve entry transition via runner
        let runner = self.session_runner.as_mut().unwrap();
        let action = runner.advance();
        let crossed = runner.last_advance_crossed_entry();

        // エントリ境界を越えたら、pending_session があれば差し替える（§12）
        // When crossing an entry boundary, swap in pending_session if present (§12)
        if crossed {
            if let Some(new_def) = self.pending_session.take() {
                // 現在のセッション全体リピート状態を維持する
                // Preserve the current session-wide repeat state
                let was_looping = matches!(
                    &self.state,
                    PlaybackState::PlayingSession {
                        repeat: RepeatMode::Loop,
                        ..
                    }
                );
                let new_runner = if was_looping {
                    SessionRunner::new_looping(&new_def)
                } else {
                    SessionRunner::new(&new_def)
                };
                self.session_runner = Some(new_runner);
                // 新 runner で即時に advance し直し、新定義の先頭エントリを得る
                // Re-advance with the new runner to obtain the first entry of the new definition
                let runner = self.session_runner.as_mut().unwrap();
                let new_action = runner.advance();
                return self.finalize_session_action(new_action);
            }
        }

        self.finalize_session_action(action)
    }

    /// SessionAction を NextAction に変換し、state 側の entry_index を更新する
    /// Converts SessionAction to NextAction and updates the state's entry_index
    fn finalize_session_action(&mut self, action: SessionAction) -> NextAction {
        match action {
            SessionAction::PlayScene(scene_name) => {
                if let (
                    PlaybackState::PlayingSession {
                        entry_index,
                        repeat: session_repeat,
                        ..
                    },
                    Some(runner),
                ) = (&mut self.state, &self.session_runner)
                {
                    *entry_index = runner.current_index();
                    // session_looping ではない Count/Once で runner が Done に達したら
                    // セッション全体リピートを消費する
                    // If the runner is Done and not session-looping, consume the
                    // session-wide repeat count (Count/Once).
                    let _ = session_repeat;
                }
                NextAction::NextSessionEntry { scene_name }
            }
            SessionAction::Done => {
                // セッション全体のリピートを消費する
                // Consume the session-wide repeat count
                let mut should_stop = true;
                if let PlaybackState::PlayingSession {
                    repeat: session_repeat,
                    ..
                } = &mut self.state
                {
                    match session_repeat {
                        RepeatMode::Once => {
                            should_stop = true;
                        }
                        RepeatMode::Count { remaining } => {
                            if *remaining > 1 {
                                *remaining -= 1;
                                should_stop = false;
                            } else {
                                should_stop = true;
                            }
                        }
                        RepeatMode::Loop => {
                            // new_looping で作られた runner なら Done にならないはずだが
                            // 念のため stop しない扱いとし runner をリセット
                            // A runner built with new_looping should not return Done,
                            // but as a safeguard, do not stop and reset the runner.
                            should_stop = false;
                        }
                    }
                }
                if should_stop {
                    self.state = PlaybackState::Stopped;
                    self.session_runner = None;
                    self.pending_session = None;
                    NextAction::SessionComplete
                } else {
                    // セッション先頭から再開
                    // Restart from the beginning of the session
                    if let Some(runner) = self.session_runner.as_mut() {
                        runner.reset();
                        let action = runner.advance();
                        if let SessionAction::PlayScene(scene_name) = action {
                            if let PlaybackState::PlayingSession { entry_index, .. } =
                                &mut self.state
                            {
                                *entry_index = 0;
                            }
                            return NextAction::NextSessionEntry { scene_name };
                        }
                    }
                    NextAction::SessionComplete
                }
            }
        }
    }

    /// RepeatSpecからRepeatModeへ変換する
    /// Converts RepeatSpec to RepeatMode
    pub fn from_repeat_spec(spec: &RepeatSpec) -> RepeatMode {
        match spec {
            RepeatSpec::Once => RepeatMode::Once,
            RepeatSpec::Count(n) => RepeatMode::Count { remaining: *n },
            RepeatSpec::Loop => RepeatMode::Loop,
        }
    }

    /// 現在再生中のシーン名を返す
    ///
    /// Paused 中も prev の名前を返す（§10.4 stop <name> 等の名前一致判定で使用）。
    /// Returns the name of the currently playing scene.
    /// Also returns the `prev` name while Paused (used by stop <name> matching in §10.4).
    pub fn current_scene_name(&self) -> Option<&str> {
        match &self.state {
            PlaybackState::PlayingScene { name, .. } => Some(name),
            PlaybackState::PlayingSession { name, .. } => Some(name),
            PlaybackState::Stopped => None,
            PlaybackState::Paused { prev } => match prev.as_ref() {
                PlaybackState::PlayingScene { name, .. } => Some(name),
                PlaybackState::PlayingSession { name, .. } => Some(name),
                _ => None,
            },
        }
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_is_stopped() {
        let sm = StateManager::new();
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_play_scene_once() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Once,
        });
        assert_eq!(
            *sm.state(),
            PlaybackState::PlayingScene {
                name: "intro".to_string(),
                repeat: RepeatMode::Once,
            }
        );
    }

    #[test]
    fn test_play_scene_loop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        assert_eq!(
            *sm.state(),
            PlaybackState::PlayingScene {
                name: "verse".to_string(),
                repeat: RepeatMode::Loop,
            }
        );
    }

    #[test]
    fn test_play_session() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlaySession {
            name: "song".to_string(),
            repeat: RepeatSpec::Count(2),
        });
        assert_eq!(
            *sm.state(),
            PlaybackState::PlayingSession {
                name: "song".to_string(),
                repeat: RepeatMode::Count { remaining: 2 },
                entry_index: 0,
                scene_repeat: RepeatMode::Once,
            }
        );
    }

    #[test]
    fn test_stop_all() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Stop { target: None });
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_stop_matching_target() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Stop {
            target: Some("intro".to_string()),
        });
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_stop_non_matching_target() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Stop {
            target: Some("outro".to_string()),
        });
        assert!(matches!(sm.state(), PlaybackState::PlayingScene { .. }));
    }

    #[test]
    fn test_scene_loop_complete_once() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Once,
        });
        let action = sm.scene_loop_complete();
        assert_eq!(action, NextAction::SceneComplete);
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_scene_loop_complete_count() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Count(3),
        });
        assert_eq!(sm.scene_loop_complete(), NextAction::ContinueScene);
        assert_eq!(sm.scene_loop_complete(), NextAction::ContinueScene);
        assert_eq!(sm.scene_loop_complete(), NextAction::SceneComplete);
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    #[test]
    fn test_scene_loop_complete_loop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "groove".to_string(),
            repeat: RepeatSpec::Loop,
        });
        for _ in 0..100 {
            assert_eq!(sm.scene_loop_complete(), NextAction::ContinueScene);
        }
        assert!(matches!(sm.state(), PlaybackState::PlayingScene { .. }));
    }

    #[test]
    fn test_scene_loop_complete_when_stopped() {
        let mut sm = StateManager::new();
        let action = sm.scene_loop_complete();
        assert_eq!(action, NextAction::SceneComplete);
    }

    #[test]
    fn test_session_scene_advance() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlaySession {
            name: "song".to_string(),
            repeat: RepeatSpec::Once,
        });
        let action = sm.scene_loop_complete();
        assert_eq!(
            action,
            NextAction::NextSessionEntry {
                scene_name: String::new()
            }
        );
        if let PlaybackState::PlayingSession { entry_index, .. } = sm.state() {
            assert_eq!(*entry_index, 1);
        } else {
            panic!("expected PlayingSession");
        }
    }

    #[test]
    fn test_from_repeat_spec() {
        assert_eq!(
            StateManager::from_repeat_spec(&RepeatSpec::Once),
            RepeatMode::Once
        );
        assert_eq!(
            StateManager::from_repeat_spec(&RepeatSpec::Count(5)),
            RepeatMode::Count { remaining: 5 }
        );
        assert_eq!(
            StateManager::from_repeat_spec(&RepeatSpec::Loop),
            RepeatMode::Loop
        );
    }

    #[test]
    fn test_current_scene_name() {
        let mut sm = StateManager::new();
        assert_eq!(sm.current_scene_name(), None);

        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Once,
        });
        assert_eq!(sm.current_scene_name(), Some("intro"));

        sm.apply_command(PlaybackCommand::PlaySession {
            name: "song".to_string(),
            repeat: RepeatSpec::Loop,
        });
        assert_eq!(sm.current_scene_name(), Some("song"));
    }

    #[test]
    fn test_play_scene_replaces_current() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "intro".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Once,
        });
        assert_eq!(sm.current_scene_name(), Some("verse"));
    }

    #[test]
    fn test_stop_session_by_name() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlaySession {
            name: "song".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Stop {
            target: Some("song".to_string()),
        });
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    // --- SessionRunner 統合テスト ---
    // --- SessionRunner integration tests ---

    use crate::ast::session::{SessionEntry, SessionRepeat};

    fn session_def(name: &str, entries: Vec<(&str, SessionRepeat)>) -> SessionDef {
        SessionDef {
            name: name.to_string(),
            entries: entries
                .into_iter()
                .map(|(scene, repeat)| SessionEntry {
                    scene: scene.to_string(),
                    repeat,
                })
                .collect(),
        }
    }

    /// apply_play_session 後、scene_loop_complete で実際のシーン名が返る
    /// After apply_play_session, scene_loop_complete returns the actual scene name
    #[test]
    fn session_next_action_contains_scene_name() {
        let mut sm = StateManager::new();
        let def = session_def(
            "song",
            vec![
                ("intro", SessionRepeat::Once),
                ("verse", SessionRepeat::Count(2)),
                ("outro", SessionRepeat::Once),
            ],
        );
        sm.apply_play_session(&def, RepeatSpec::Once);

        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "intro".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "verse".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "verse".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "outro".to_string()
            }
        );
        assert_eq!(sm.scene_loop_complete(), NextAction::SessionComplete);
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    /// §10.2 play session [repeat N]: セッション全体をN回繰り返す
    /// §10.2 play session [repeat N]: repeat the entire session N times
    #[test]
    fn session_repeat_count_loops_entire_session() {
        let mut sm = StateManager::new();
        let def = session_def("song", vec![("a", SessionRepeat::Once)]);
        sm.apply_play_session(&def, RepeatSpec::Count(3));

        // 1周目
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "a".to_string()
            }
        );
        // 2周目（内部で Done → 先頭から再開）
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "a".to_string()
            }
        );
        // 3周目
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "a".to_string()
            }
        );
        // 3周完了 → SessionComplete
        assert_eq!(sm.scene_loop_complete(), NextAction::SessionComplete);
    }

    /// §10.2 play session [loop]: セッション全体を無限ループ
    /// §10.2 play session [loop]: loop the entire session infinitely
    #[test]
    fn session_loop_never_completes() {
        let mut sm = StateManager::new();
        let def = session_def(
            "song",
            vec![("a", SessionRepeat::Once), ("b", SessionRepeat::Once)],
        );
        sm.apply_play_session(&def, RepeatSpec::Loop);

        for _ in 0..10 {
            let action = sm.scene_loop_complete();
            assert!(matches!(action, NextAction::NextSessionEntry { .. }));
        }
        assert!(matches!(sm.state(), PlaybackState::PlayingSession { .. }));
    }

    /// §9: session 内の [loop] エントリはそこで無限ループ
    /// §9: a [loop] entry in a session loops infinitely at that point
    #[test]
    fn session_entry_loop_stays_on_same_scene() {
        let mut sm = StateManager::new();
        let def = session_def(
            "jam",
            vec![
                ("intro", SessionRepeat::Once),
                ("verse", SessionRepeat::Loop),
                ("outro", SessionRepeat::Once),
            ],
        );
        sm.apply_play_session(&def, RepeatSpec::Once);

        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "intro".to_string()
            }
        );
        for _ in 0..20 {
            assert_eq!(
                sm.scene_loop_complete(),
                NextAction::NextSessionEntry {
                    scene_name: "verse".to_string()
                }
            );
        }
        assert!(matches!(sm.state(), PlaybackState::PlayingSession { .. }));
    }

    /// §12: session を eval で上書きすると、次のシーン切り替え時から新構成になる
    /// §12: overwriting a session via eval applies the new composition at the next scene transition
    #[test]
    fn session_overwrite_swaps_at_next_entry() {
        let mut sm = StateManager::new();
        let def_old = session_def(
            "song",
            vec![
                ("intro", SessionRepeat::Once),
                ("old_verse", SessionRepeat::Once),
                ("old_outro", SessionRepeat::Once),
            ],
        );
        sm.apply_play_session(&def_old, RepeatSpec::Once);

        // intro 再生
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "intro".to_string()
            }
        );

        // intro 再生中に session を上書き
        // Overwrite the session while intro is playing
        let def_new = session_def(
            "song",
            vec![
                ("new_a", SessionRepeat::Once),
                ("new_b", SessionRepeat::Once),
            ],
        );
        sm.notify_session_updated(&def_new);

        // 次のエントリ遷移時に新構成の先頭から再生される
        // At the next entry transition, playback restarts from the head of the new composition
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "new_a".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "new_b".to_string()
            }
        );
        assert_eq!(sm.scene_loop_complete(), NextAction::SessionComplete);
    }

    /// §12: 別名 session の eval は現在の session に影響しない
    /// §12: evaluating a session with a different name does not affect the current session
    #[test]
    fn session_overwrite_different_name_ignored() {
        let mut sm = StateManager::new();
        let def = session_def(
            "song",
            vec![("a", SessionRepeat::Once), ("b", SessionRepeat::Once)],
        );
        sm.apply_play_session(&def, RepeatSpec::Once);

        let unrelated = session_def("other", vec![("x", SessionRepeat::Once)]);
        sm.notify_session_updated(&unrelated);

        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "a".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "b".to_string()
            }
        );
        assert_eq!(sm.scene_loop_complete(), NextAction::SessionComplete);
    }

    /// §12: session ループ中の上書きもループ属性を維持
    /// §12: overwriting during a session loop preserves the loop attribute
    #[test]
    fn session_overwrite_preserves_looping() {
        let mut sm = StateManager::new();
        let def_old = session_def("song", vec![("a", SessionRepeat::Once)]);
        sm.apply_play_session(&def_old, RepeatSpec::Loop);

        assert!(matches!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry { .. }
        ));

        let def_new = session_def(
            "song",
            vec![("x", SessionRepeat::Once), ("y", SessionRepeat::Once)],
        );
        sm.notify_session_updated(&def_new);

        // 新構成で再生
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "x".to_string()
            }
        );
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "y".to_string()
            }
        );
        // ループ属性が維持されているので Done にならず先頭に戻る
        assert_eq!(
            sm.scene_loop_complete(),
            NextAction::NextSessionEntry {
                scene_name: "x".to_string()
            }
        );
    }

    // --- §10.4 pause / resume tests ---

    /// 全体 pause → Paused { prev: PlayingScene }
    /// Full pause transitions to `Paused { prev: PlayingScene }`
    #[test]
    fn pause_all_while_playing_scene_wraps_state() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        assert!(sm.is_paused());
        if let PlaybackState::Paused { prev } = sm.state() {
            assert!(matches!(prev.as_ref(), PlaybackState::PlayingScene { .. }));
        } else {
            panic!("expected Paused");
        }
    }

    /// Paused から resume で元の state に戻る
    /// Resume restores the original state from Paused
    #[test]
    fn resume_all_restores_prev_state() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Resume { target: None });
        assert!(!sm.is_paused());
        assert!(matches!(sm.state(), PlaybackState::PlayingScene { .. }));
    }

    /// 名前指定 pause: 一致時のみ Paused に遷移
    /// Named pause: transitions to Paused only on name match
    #[test]
    fn pause_named_matches_current_scene() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause {
            target: Some("verse".to_string()),
        });
        assert!(sm.is_paused());
    }

    /// 名前不一致 pause は state 不変（§10.4 D2）
    /// Mismatched name pause leaves state unchanged (§10.4 D2)
    #[test]
    fn pause_named_mismatch_is_noop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause {
            target: Some("chorus".to_string()),
        });
        assert!(!sm.is_paused());
        assert!(matches!(sm.state(), PlaybackState::PlayingScene { .. }));
    }

    /// 名前指定 resume: prev と一致時のみ復元
    /// Named resume: restores only when prev name matches
    #[test]
    fn resume_named_matches_prev_scene() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Resume {
            target: Some("verse".to_string()),
        });
        assert!(!sm.is_paused());
    }

    /// 名前不一致 resume は state 不変（§10.4 D8）
    /// Mismatched name resume leaves state unchanged (§10.4 D8)
    #[test]
    fn resume_named_mismatch_is_noop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Resume {
            target: Some("chorus".to_string()),
        });
        assert!(sm.is_paused());
    }

    /// Stopped に対する pause は no-op
    /// Pause against Stopped is a no-op
    #[test]
    fn pause_on_stopped_is_noop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::Pause { target: None });
        assert!(!sm.is_paused());
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    /// Paused に対する二重 pause は no-op（prev が二重 Paused にならない）
    /// Double pause is a no-op (prev never wraps Paused)
    #[test]
    fn pause_on_paused_is_noop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        if let PlaybackState::Paused { prev } = sm.state() {
            // prev は PlayingScene のまま（二重 Paused になっていない）
            assert!(matches!(prev.as_ref(), PlaybackState::PlayingScene { .. }));
        } else {
            panic!("expected Paused");
        }
    }

    /// Paused でないときの resume は no-op
    /// Resume on non-paused state is a no-op
    #[test]
    fn resume_on_non_paused_is_noop() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Resume { target: None });
        assert!(!sm.is_paused());
        assert!(matches!(sm.state(), PlaybackState::PlayingScene { .. }));
    }

    /// Paused 中の stop <name>: prev の名前と一致すれば Stopped に遷移（§10.4 D7）
    /// stop <name> while paused: transitions to Stopped if prev matches (§10.4 D7)
    #[test]
    fn stop_named_while_paused_matches_prev() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Stop {
            target: Some("verse".to_string()),
        });
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    /// Paused 中の stop（全停止）は Stopped に遷移（§10.4 D7）
    /// stop (all) while paused transitions to Stopped (§10.4 D7)
    #[test]
    fn stop_all_while_paused_resets_state() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::Stop { target: None });
        assert_eq!(*sm.state(), PlaybackState::Stopped);
    }

    /// Paused 中の play <scene> は Paused を解除して新 scene を再生（§10.4 D6）
    /// play <scene> while paused clears Paused and starts new scene (§10.4 D6)
    #[test]
    fn play_scene_while_paused_clears_paused() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "chorus".to_string(),
            repeat: RepeatSpec::Once,
        });
        assert!(!sm.is_paused());
        if let PlaybackState::PlayingScene { name, .. } = sm.state() {
            assert_eq!(name, "chorus");
        } else {
            panic!("expected PlayingScene");
        }
    }

    /// current_scene_name は Paused でも prev の名前を返す
    /// current_scene_name returns the prev name while paused
    #[test]
    fn current_scene_name_returns_prev_while_paused() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        assert_eq!(sm.current_scene_name(), Some("verse"));
    }

    /// scene_loop_complete は Paused 中に ContinueScene を返す（tick 凍結想定）
    /// scene_loop_complete returns ContinueScene while paused (tick is frozen)
    #[test]
    fn scene_loop_complete_while_paused_continues() {
        let mut sm = StateManager::new();
        sm.apply_command(PlaybackCommand::PlayScene {
            name: "verse".to_string(),
            repeat: RepeatSpec::Loop,
        });
        sm.apply_command(PlaybackCommand::Pause { target: None });
        assert_eq!(sm.scene_loop_complete(), NextAction::ContinueScene);
        assert!(sm.is_paused());
    }
}
