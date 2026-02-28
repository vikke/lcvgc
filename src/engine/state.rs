/// 再生状態管理モジュール
///
/// シーケンサーの再生状態遷移を管理する。
/// シーン再生・セッション再生・停止の3状態間を遷移し、
/// リピートモードに応じた小節進行制御を行う。

use crate::ast::playback::RepeatSpec;

/// 再生状態
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    /// 停止中
    Stopped,
    /// シーン再生中
    PlayingScene {
        /// シーン名
        name: String,
        /// リピートモード
        repeat: RepeatMode,
    },
    /// セッション再生中
    PlayingSession {
        /// セッション名
        name: String,
        /// セッション全体のリピートモード
        repeat: RepeatMode,
        /// 現在のエントリインデックス
        entry_index: usize,
        /// 現在のシーンのリピートモード
        scene_repeat: RepeatMode,
    },
}

/// リピートモード（内部管理用）
#[derive(Debug, Clone, PartialEq)]
pub enum RepeatMode {
    /// 1回のみ
    Once,
    /// 残り回数
    Count { remaining: u32 },
    /// 無限ループ
    Loop,
}

/// 再生コマンド
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackCommand {
    /// シーン再生
    PlayScene { name: String, repeat: RepeatSpec },
    /// セッション再生
    PlaySession { name: String, repeat: RepeatSpec },
    /// 停止（対象指定なしで全停止）
    Stop { target: Option<String> },
}

/// 小節進行時のアクション
#[derive(Debug, Clone, PartialEq)]
pub enum NextAction {
    /// 同じシーンを続行
    ContinueScene,
    /// シーン完了、停止
    SceneComplete,
    /// セッション内の次シーンへ
    NextSessionEntry { scene_name: String },
    /// セッション完了、停止
    SessionComplete,
}

/// 再生状態マネージャ
///
/// 再生コマンドの適用と小節進行に伴う状態遷移を管理する。
#[derive(Debug)]
pub struct StateManager {
    state: PlaybackState,
}

impl StateManager {
    /// 停止状態で初期化
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
        }
    }

    /// 現在の再生状態への参照を返す
    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// コマンドを適用して状態を遷移させる
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
    ///
    /// リピートモードに応じて状態を更新し、適切なアクションを返す。
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
    pub fn from_repeat_spec(spec: &RepeatSpec) -> RepeatMode {
        match spec {
            RepeatSpec::Once => RepeatMode::Once,
            RepeatSpec::Count(n) => RepeatMode::Count { remaining: *n },
            RepeatSpec::Loop => RepeatMode::Loop,
        }
    }

    /// 現在再生中のシーン名を返す
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
