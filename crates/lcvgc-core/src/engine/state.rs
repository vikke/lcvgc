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
}

impl StateManager {
    /// 停止状態で初期化
    /// Initializes in the stopped state
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
        }
    }

    /// 現在の再生状態への参照を返す
    /// Returns a reference to the current playback state
    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// コマンドを適用して状態を遷移させる
    /// Applies a command and transitions the state
    pub fn apply_command(&mut self, cmd: PlaybackCommand) {
        match cmd {
            PlaybackCommand::PlayScene { name, repeat } => {
                self.state = PlaybackState::PlayingScene {
                    name,
                    repeat: Self::from_repeat_spec(&repeat),
                };
            }
            PlaybackCommand::PlaySession { name, repeat } => {
                self.state = PlaybackState::PlayingSession {
                    name,
                    repeat: Self::from_repeat_spec(&repeat),
                    entry_index: 0,
                    scene_repeat: RepeatMode::Once,
                };
            }
            PlaybackCommand::Stop { target } => match target {
                None => {
                    self.state = PlaybackState::Stopped;
                }
                Some(ref target_name) => {
                    let should_stop = match &self.state {
                        PlaybackState::PlayingScene { name, .. } => name == target_name,
                        PlaybackState::PlayingSession { name, .. } => name == target_name,
                        PlaybackState::Stopped => false,
                    };
                    if should_stop {
                        self.state = PlaybackState::Stopped;
                    }
                }
            },
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
            PlaybackState::PlayingSession {
                entry_index,
                scene_repeat,
                ..
            } => {
                // シーンリピートを消費
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

                // 次のエントリへ
                *entry_index += 1;
                *scene_repeat = RepeatMode::Once;

                NextAction::NextSessionEntry {
                    scene_name: String::new(),
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
    /// Returns the name of the currently playing scene
    pub fn current_scene_name(&self) -> Option<&str> {
        match &self.state {
            PlaybackState::PlayingScene { name, .. } => Some(name),
            PlaybackState::PlayingSession { name, .. } => Some(name),
            PlaybackState::Stopped => None,
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
}
