//! evalコマンドディスパッチャ
//!
//! DSLのBlockをレジストリ・クロック・ステートに振り分けて評価する。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// インクルード処理済み / Include processed
    IncludeProcessed {
        /// インクルード先ファイルパス / Path of the included file
        path: String,
        /// 展開されたブロック数 / Number of expanded blocks
        results_count: usize,
    },
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
            Block::Include(ref inc) => Ok(EvalResult::IncludeProcessed {
                path: inc.path.clone(),
                results_count: 0,
            }),
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

    /// ファイルパスを指定して全ブロックを評価する（include展開付き）
    /// Evaluates all blocks from a file path with include expansion
    ///
    /// # Arguments
    /// * `path` - 評価するファイルのパス / Path to the file to evaluate
    ///
    /// # Returns
    /// 評価結果のベクター / Vector of evaluation results
    ///
    /// # Errors
    /// - `EngineError::IncludeNotFound` - ファイルが見つからない / File not found
    /// - `EngineError::IncludeReadError` - ファイル読み込みエラー / File read error
    /// - `EngineError::ParseError` - パースエラー / Parse error
    /// - `EngineError::CircularInclude` - 循環インクルード / Circular include
    pub fn eval_file(&mut self, path: &Path) -> Result<Vec<EvalResult>, EngineError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| EngineError::IncludeNotFound(path.display().to_string()))?;
        let mut include_stack = HashSet::new();
        include_stack.insert(canonical.clone());
        self.eval_file_recursive(&canonical, &mut include_stack)
    }

    /// 再帰的にファイルを評価する（内部メソッド）
    /// Recursively evaluates a file (internal method)
    ///
    /// # Arguments
    /// * `path` - 正規化済みのファイルパス / Canonicalized file path
    /// * `include_stack` - 循環検出用のインクルードスタック / Include stack for cycle detection
    ///
    /// # Returns
    /// 評価結果のベクター / Vector of evaluation results
    ///
    /// # Errors
    /// - `EngineError::CircularInclude` - 循環インクルード / Circular include
    /// - `EngineError::IncludeNotFound` - インクルードファイル未検出 / Include file not found
    /// - `EngineError::IncludeReadError` - ファイル読み込みエラー / File read error
    fn eval_file_recursive(
        &mut self,
        path: &Path,
        include_stack: &mut HashSet<PathBuf>,
    ) -> Result<Vec<EvalResult>, EngineError> {
        let source = std::fs::read_to_string(path).map_err(|e| EngineError::IncludeReadError {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let (_, blocks) = crate::parser::parse_source(&source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;

        let mut results = Vec::new();
        for block in blocks {
            match block {
                Block::Include(ref inc) => {
                    let base_dir = path.parent().unwrap_or(Path::new("."));
                    let include_path = base_dir.join(&inc.path);
                    let canonical = include_path
                        .canonicalize()
                        .map_err(|_| EngineError::IncludeNotFound(inc.path.clone()))?;

                    if !include_stack.insert(canonical.clone()) {
                        let chain: Vec<String> = include_stack
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect();
                        return Err(EngineError::CircularInclude(format!(
                            "{} -> {}",
                            chain.join(" -> "),
                            canonical.display()
                        )));
                    }

                    let sub_results = self.eval_file_recursive(&canonical, include_stack)?;
                    let count = sub_results.len();
                    results.extend(sub_results);
                    results.push(EvalResult::IncludeProcessed {
                        path: inc.path.clone(),
                        results_count: count,
                    });

                    include_stack.remove(&canonical);
                }
                _ => {
                    results.push(self.eval_block(block)?);
                }
            }
        }
        Ok(results)
    }

    /// ソースコード文字列をプリロード評価する（play/stopをスキップ）
    /// Preload-evaluates DSL source code, skipping play/stop blocks
    ///
    /// # Arguments
    /// * `source` - 評価するDSLソース文字列 / DSL source string to evaluate
    ///
    /// # Returns
    /// 評価結果のベクター（play/stopを除く） / Vector of evaluation results (excluding play/stop)
    ///
    /// # Errors
    /// - `EngineError::ParseError` - パースエラー / Parse error
    pub fn eval_source_preload(&mut self, source: &str) -> Result<Vec<EvalResult>, EngineError> {
        let (_, blocks) = crate::parser::parse_source(source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;
        let mut results = Vec::new();
        for block in blocks {
            match block {
                Block::Play(_) | Block::Stop(_) => {
                    // preloadモードではplay/stopをスキップ
                    // Skip play/stop blocks in preload mode
                    continue;
                }
                _ => {
                    results.push(self.eval_block(block)?);
                }
            }
        }
        Ok(results)
    }

    /// ソースコード文字列を全ブロック評価する
    pub fn eval_source(&mut self, source: &str) -> Result<Vec<EvalResult>, EngineError> {
        let (_, blocks) = crate::parser::parse_source(source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;
        let mut results = Vec::new();
        for block in blocks {
            results.push(self.eval_block(block)?);
        }
        Ok(results)
    }

    /// ファイルを読み込んで全ブロックを評価する
    pub fn load_file(&mut self, path: &str) -> Result<Vec<EvalResult>, EngineError> {
        let source = std::fs::read_to_string(path)?;
        self.eval_source(&source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
    use crate::ast::common::NoteName;
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
        assert_eq!(result, EvalResult::VarDefined { name: "key".into() });
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
        assert!(matches!(
            ev.state().state(),
            PlaybackState::PlayingScene { .. }
        ));
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
    fn eval_include_processed() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Include(IncludeDef {
                path: "other.lcvgc".into(),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::IncludeProcessed {
                path: "other.lcvgc".into(),
                results_count: 0,
            }
        );
    }

    #[test]
    fn eval_file_single_include() {
        let dir = tempfile::tempdir().unwrap();
        let sub_file = dir.path().join("sub.cvg");
        std::fs::write(&sub_file, "tempo 140\n").unwrap();

        let main_file = dir.path().join("main.cvg");
        std::fs::write(&main_file, format!("include {}\n", sub_file.display())).unwrap();

        let mut ev = Evaluator::new(120.0);
        let results = ev.eval_file(&main_file).unwrap();
        // tempo 140 が評価され、IncludeProcessed が返る
        assert!(results.iter().any(
            |r| matches!(r, EvalResult::TempoChanged(t) if (*t - 140.0).abs() < f64::EPSILON)
        ));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::IncludeProcessed { .. })));
    }

    #[test]
    fn eval_file_nested_include() {
        let dir = tempfile::tempdir().unwrap();
        let leaf_file = dir.path().join("leaf.cvg");
        std::fs::write(&leaf_file, "tempo 160\n").unwrap();

        let mid_file = dir.path().join("mid.cvg");
        std::fs::write(&mid_file, format!("include {}\n", leaf_file.display())).unwrap();

        let main_file = dir.path().join("main.cvg");
        std::fs::write(&main_file, format!("include {}\n", mid_file.display())).unwrap();

        let mut ev = Evaluator::new(120.0);
        let results = ev.eval_file(&main_file).unwrap();
        assert!(results.iter().any(
            |r| matches!(r, EvalResult::TempoChanged(t) if (*t - 160.0).abs() < f64::EPSILON)
        ));
    }

    #[test]
    fn eval_file_circular_include() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.cvg");
        let file_b = dir.path().join("b.cvg");
        std::fs::write(&file_a, format!("include {}\n", file_b.display())).unwrap();
        std::fs::write(&file_b, format!("include {}\n", file_a.display())).unwrap();

        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_file(&file_a);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::CircularInclude(_)));
    }

    #[test]
    fn eval_file_not_found() {
        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_file(Path::new("/nonexistent/file.cvg"));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::IncludeNotFound(_)
        ));
    }

    #[test]
    fn eval_source_multiple_blocks() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
tempo 140

device mb {
  port Mutant Brain
}
"#;
        let results = ev.eval_source(source).unwrap();
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], EvalResult::TempoChanged(140.0)));
        assert!(matches!(results[1], EvalResult::Registered { .. }));
    }

    #[test]
    fn eval_source_empty() {
        let mut ev = Evaluator::new(120.0);
        let results = ev.eval_source("").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn eval_source_parse_error() {
        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_source("invalid !@# syntax");
        assert!(result.is_err());
    }

    #[test]
    fn load_file_not_found() {
        let mut ev = Evaluator::new(120.0);
        let result = ev.load_file("/nonexistent/path.cvg");
        assert!(result.is_err());
    }

    /// play/stopがスキップされ、それ以外のブロックは評価されることを検証する
    /// Verifies that play/stop are skipped while other blocks are evaluated
    #[test]
    fn eval_source_preload_skips_play_and_stop() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
tempo 140

device mb {
  port Mutant Brain
}

instrument bass {
  device mb
  channel 1
}

clip intro [bars 1] {
  bass C3 _ _ _
}

scene verse {
  intro
}

session main {
  verse
}

scale c major

var key = cm

play verse

stop
"#;
        let results = ev.eval_source_preload(source).unwrap();

        // Device, Instrument, Clip, Scene, Session, Tempo, Scale, Var はevalされる
        // Device, Instrument, Clip, Scene, Session, Tempo, Scale, Var are evaluated
        assert!(results.iter().any(
            |r| matches!(r, EvalResult::TempoChanged(t) if (*t - 140.0).abs() < f64::EPSILON)
        ));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Device")));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Instrument")));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Clip")));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Scene")));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::Registered { kind, .. } if kind == "Session")));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::ScaleChanged)));
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::VarDefined { .. })));

        // Play, Stop はスキップされる（結果に含まれない）
        // Play and Stop are skipped (not included in results)
        assert!(!results.iter().any(|r| matches!(r, EvalResult::PlayStarted)));
        assert!(!results.iter().any(|r| matches!(r, EvalResult::Stopped)));
    }
}
