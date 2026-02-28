/// evalコマンドディスパッチャ
///
/// DSLのBlockをレジストリ・クロック・ステートに振り分けて評価する。

use crate::ast::playback::PlayTarget;
use crate::ast::Block;
use crate::engine::clock::Clock;
use crate::engine::error::EngineError;
use crate::engine::registry::Registry;
use crate::engine::state::{PlaybackCommand, StateManager};

/// eval結果
#[derive(Debug, Clone, PartialEq)]
pub enum EvalResult {
    /// ブロック登録成功
    Registered { kind: String, name: String },
    /// テンポ変更
    TempoChanged(f64),
    /// スケール変更
    ScaleChanged,
    /// 変数定義
    VarDefined { name: String },
    /// 再生開始
    PlayStarted,
    /// 停止
    Stopped,
    /// インクルード（未実装 - 呼び出し側が処理すべき）
    IncludeRequested(String),
}

/// evalコマンドディスパッチャ
#[derive(Debug)]
pub struct Evaluator {
    registry: Registry,
    state: StateManager,
    clock: Clock,
}

impl Evaluator {
    /// 指定BPMで初期化
    pub fn new(bpm: f64) -> Self {
        Self {
            registry: Registry::new(),
            state: StateManager::new(),
            clock: Clock::new(bpm),
        }
    }

    /// 単一ブロックを評価
    pub fn eval_block(&mut self, block: Block) -> Result<EvalResult, EngineError> {
        match block {
            Block::Device(ref d) => {
                let name = d.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Device".into(),
                    name,
                })
            }
            Block::Instrument(ref i) => {
                let name = i.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Instrument".into(),
                    name,
                })
            }
            Block::Kit(ref k) => {
                let name = k.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Kit".into(),
                    name,
                })
            }
            Block::Clip(ref c) => {
                let name = c.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Clip".into(),
                    name,
                })
            }
            Block::Scene(ref s) => {
                let name = s.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Scene".into(),
                    name,
                })
            }
            Block::Session(ref s) => {
                let name = s.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Session".into(),
                    name,
                })
            }
            Block::Tempo(ref t) => {
                self.clock.apply_tempo(t);
                let new_bpm = self.clock.bpm();
                self.registry.register_block(block);
                Ok(EvalResult::TempoChanged(new_bpm))
            }
            Block::Scale(_) => {
                self.registry.register_block(block);
                Ok(EvalResult::ScaleChanged)
            }
            Block::Var(ref v) => {
                let name = v.name.clone();
                self.registry.register_block(block);
                Ok(EvalResult::VarDefined { name })
            }
            Block::Play(cmd) => {
                let playback_cmd = match cmd.target {
                    PlayTarget::Scene(name) => PlaybackCommand::PlayScene {
                        name,
                        repeat: cmd.repeat,
                    },
                    PlayTarget::Session(name) => PlaybackCommand::PlaySession {
                        name,
                        repeat: cmd.repeat,
                    },
                };
                self.state.apply_command(playback_cmd);
                Ok(EvalResult::PlayStarted)
            }
            Block::Stop(cmd) => {
                self.state
                    .apply_command(PlaybackCommand::Stop { target: cmd.target });
                Ok(EvalResult::Stopped)
            }
            Block::Include(inc) => Ok(EvalResult::IncludeRequested(inc.path)),
        }
    }

    /// Registry参照
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Clock参照
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// State参照
    pub fn state(&self) -> &StateManager {
        &self.state
    }

    /// 現在のBPM
    pub fn bpm(&self) -> f64 {
        self.clock.bpm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
    use crate::ast::device::DeviceDef;
    use crate::ast::include::IncludeDef;
    use crate::ast::instrument::InstrumentDef;
    use crate::ast::kit::KitDef;
    use crate::ast::playback::{PlayCommand, PlayTarget, RepeatSpec, StopCommand};
    use crate::ast::scale::{ScaleDef, ScaleType};
    use crate::ast::scene::SceneDef;
    use crate::ast::session::SessionDef;
    use crate::ast::tempo::Tempo;
    use crate::ast::var::VarDef;
    use crate::ast::common::NoteName;
    use crate::engine::state::PlaybackState;
    use crate::parser::clip_options::ClipOptions;

    #[test]
    fn eval_device_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Device(DeviceDef {
                name: "synth".into(),
                port: "IAC Bus 1".into(),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Device".into(),
                name: "synth".into(),
            }
        );
        assert!(ev.registry().get_device("synth").is_some());
    }

    #[test]
    fn eval_instrument_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Instrument(InstrumentDef {
                name: "piano".into(),
                device: "synth".into(),
                channel: 1,
                note: None,
                gate_normal: None,
                gate_staccato: None,
                cc_mappings: vec![],
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Instrument".into(),
                name: "piano".into(),
            }
        );
        let inst = ev.registry().get_instrument("piano").unwrap();
        assert_eq!(inst.channel, 1);
    }

    #[test]
    fn eval_kit_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Kit(KitDef {
                name: "drums".into(),
                device: "synth".into(),
                instruments: vec![],
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Kit".into(),
                name: "drums".into(),
            }
        );
        assert!(ev.registry().get_kit("drums").is_some());
    }

    #[test]
    fn eval_clip_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Clip(ClipDef {
                name: "intro".into(),
                options: ClipOptions::default(),
                body: ClipBody::Pitched(PitchedClipBody {
                    lines: vec![],
                    cc_automations: vec![],
                }),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Clip".into(),
                name: "intro".into(),
            }
        );
        assert!(ev.registry().get_clip("intro").is_some());
    }

    #[test]
    fn eval_scene_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Scene(SceneDef {
                name: "verse".into(),
                entries: vec![],
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Scene".into(),
                name: "verse".into(),
            }
        );
        assert!(ev.registry().get_scene("verse").is_some());
    }

    #[test]
    fn eval_session_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Session(SessionDef {
                name: "main".into(),
                entries: vec![],
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Registered {
                kind: "Session".into(),
                name: "main".into(),
            }
        );
        assert!(ev.registry().get_session("main").is_some());
    }

    #[test]
    fn eval_tempo_absolute() {
        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_block(Block::Tempo(Tempo::Absolute(140))).unwrap();
        assert_eq!(result, EvalResult::TempoChanged(140.0));
        assert!((ev.bpm() - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn eval_tempo_relative() {
        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_block(Block::Tempo(Tempo::Relative(10))).unwrap();
        assert_eq!(result, EvalResult::TempoChanged(130.0));
        assert!((ev.bpm() - 130.0).abs() < f64::EPSILON);
    }

    #[test]
    fn eval_scale_changed() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Scale(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Major,
            }))
            .unwrap();
        assert_eq!(result, EvalResult::ScaleChanged);
        assert!(ev.registry().scale().is_some());
    }

    #[test]
    fn eval_var_defined() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Var(VarDef {
                name: "key".into(),
                value: "Cm".into(),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::VarDefined {
                name: "key".into(),
            }
        );
        assert_eq!(ev.registry().get_var("key"), Some("Cm"));
    }

    #[test]
    fn eval_play_scene() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Play(PlayCommand {
                target: PlayTarget::Scene("verse".into()),
                repeat: RepeatSpec::Loop,
            }))
            .unwrap();
        assert_eq!(result, EvalResult::PlayStarted);
        assert!(matches!(ev.state().state(), PlaybackState::PlayingScene { .. }));
    }

    #[test]
    fn eval_play_session() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Play(PlayCommand {
                target: PlayTarget::Session("song".into()),
                repeat: RepeatSpec::Count(2),
            }))
            .unwrap();
        assert_eq!(result, EvalResult::PlayStarted);
        assert!(matches!(
            ev.state().state(),
            PlaybackState::PlayingSession { .. }
        ));
    }

    #[test]
    fn eval_stop() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let result = ev
            .eval_block(Block::Stop(StopCommand { target: None }))
            .unwrap();
        assert_eq!(result, EvalResult::Stopped);
        assert_eq!(*ev.state().state(), PlaybackState::Stopped);
    }

    #[test]
    fn eval_include_requested() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Include(IncludeDef {
                path: "other.lcvgc".into(),
            }))
            .unwrap();
        assert_eq!(result, EvalResult::IncludeRequested("other.lcvgc".into()));
    }
}
