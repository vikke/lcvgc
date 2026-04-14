//! evalコマンドディスパッチャ
//!
//! DSLのBlockをレジストリ・クロック・ステートに振り分けて評価する。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::playback::PlayTarget;
use crate::ast::scene::SceneDef;
use crate::ast::Block;
use crate::engine::clock::Clock;
use crate::engine::compiler::compile_clip;
use crate::engine::error::EngineError;
use crate::engine::player::ScenePlayer;
use crate::engine::registry::Registry;
use crate::engine::resolver;
use crate::engine::scene_runner::resolve_scene;
use crate::engine::scope::ScopeChain;
use crate::engine::state::{NextAction, PlaybackCommand, StateManager};

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
    /// インクルード重複スキップ / Include duplicate skipped
    IncludeSkipped {
        /// スキップされたファイルパス / Path of the skipped file
        path: String,
    },
}

/// シーンループ完了通知 (`on_scene_loop_complete`) の結果
/// Outcome returned by `on_scene_loop_complete`
#[derive(Debug, Clone, PartialEq)]
pub enum SceneTransitionOutcome {
    /// 同じシーンを継続再生
    /// Keep playing the same scene
    Continue,
    /// 次のシーンへ遷移（new active_scene が構築済み）
    /// Transitioned to the next scene (new active_scene has been built)
    NextScene {
        /// 次のシーン名 / Name of the next scene
        scene_name: String,
    },
    /// シーン完了（停止、active_scene は解放）
    /// Scene completed — playback stopped, active_scene cleared
    SceneComplete,
    /// セッション完了（停止、active_scene は解放）
    /// Session completed — playback stopped, active_scene cleared
    SessionComplete,
}

/// evalコマンドディスパッチャ
#[derive(Debug)]
pub struct Evaluator {
    registry: Registry,
    state: StateManager,
    clock: Clock,
    /// 変数スコープチェーン（§6.1 ブロックスコープ対応）
    /// Variable scope chain (§6.1 block scope support)
    scope: ScopeChain,
    /// 現在 play 中の ScenePlayer（Phase 3: PlayScene でコンパイル・構築）
    /// Currently active ScenePlayer (Phase 3: built when PlayScene is evaluated)
    active_scene: Option<ScenePlayer>,
}

impl Evaluator {
    /// 指定BPMで初期化
    pub fn new(bpm: f64) -> Self {
        Self {
            registry: Registry::new(),
            state: StateManager::new(),
            clock: Clock::new(bpm),
            scope: ScopeChain::new(),
            active_scene: None,
        }
    }

    /// 現在 play 中の ScenePlayer への不変参照
    /// Immutable reference to the currently active ScenePlayer (if any)
    pub fn active_scene(&self) -> Option<&ScenePlayer> {
        self.active_scene.as_ref()
    }

    /// 現在 play 中の ScenePlayer への可変参照（ミュート・差し替え用途）
    /// Mutable reference to the currently active ScenePlayer
    pub fn active_scene_mut(&mut self) -> Option<&mut ScenePlayer> {
        self.active_scene.as_mut()
    }

    /// ScenePlayer を取り出す（Evaluator 側は None に戻る）
    /// Takes the ScenePlayer out, leaving None in the Evaluator
    pub fn take_active_scene(&mut self) -> Option<ScenePlayer> {
        self.active_scene.take()
    }

    /// シーンの1ループ完了を通知し、状態遷移と active_scene の差し替えを行う
    ///
    /// tick 境界検出は呼び出し側（driver/daemon）の責務。
    /// 呼び出し側は `active_scene().scene_tick_length()` で1ループ長を取得し、
    /// 境界越えを検出するたびに本メソッドを呼ぶ。
    ///
    /// Notifies that one scene loop has completed; advances state and swaps
    /// `active_scene` as required. Tick-boundary detection is the caller's
    /// responsibility (e.g. compare the driver's tick counter to
    /// `scene_tick_length()`).
    ///
    /// # Errors
    /// - `EngineError::UnknownScene` - 次シーンが registry に未登録
    /// - `EngineError::UnknownClip` - 次シーン内の clip が未登録
    pub fn on_scene_loop_complete(&mut self) -> Result<SceneTransitionOutcome, EngineError> {
        let action = self.state.scene_loop_complete();
        match action {
            NextAction::ContinueScene => Ok(SceneTransitionOutcome::Continue),
            NextAction::SceneComplete => {
                self.active_scene = None;
                Ok(SceneTransitionOutcome::SceneComplete)
            }
            NextAction::SessionComplete => {
                self.active_scene = None;
                Ok(SceneTransitionOutcome::SessionComplete)
            }
            NextAction::NextSessionEntry { scene_name } => {
                let scene_def = self
                    .registry
                    .get_scene(&scene_name)
                    .ok_or_else(|| EngineError::UnknownScene(scene_name.clone()))?
                    .clone();
                let player = self.build_scene_player(&scene_def)?;
                self.active_scene = Some(player);
                Ok(SceneTransitionOutcome::NextScene { scene_name })
            }
        }
    }

    /// scene 定義と registry/clock からコンパイル済み ScenePlayer を構築する
    ///
    /// `resolve_scene` で 1 ループ分の clip 列を確定し、各 clip を
    /// `compile_clip` で MIDI イベント列に変換して ScenePlayer に積む。
    ///
    /// Builds a ScenePlayer from a scene definition using the registry and clock.
    /// `resolve_scene` picks the clips for one loop iteration, then each clip is
    /// compiled and added to the ScenePlayer.
    ///
    /// # Errors
    /// - `EngineError::UnknownClip` - scene 内で参照された clip が registry に未登録
    fn build_scene_player(&self, scene_def: &SceneDef) -> Result<ScenePlayer, EngineError> {
        let mut rng = rand::thread_rng();
        let instance = resolve_scene(scene_def, &mut rng);
        let mut player = ScenePlayer::new();
        for clip_name in &instance.clips {
            let clip_def = self
                .registry
                .get_clip(clip_name)
                .ok_or_else(|| EngineError::UnknownClip(clip_name.clone()))?;
            let compiled = compile_clip(clip_def, &self.clock, &self.registry)?;
            // Phase 3 では scene 内の全 clip を looping=true として扱う
            // Phase 3 treats all clips in a scene as looping=true
            player.add_clip(clip_name.clone(), compiled, true);
        }
        Ok(player)
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
            Block::Instrument(mut inst) => {
                let name = inst.name.clone();
                // ブロックスコープをプッシュしてローカル変数を定義（§6.1）
                // Push block scope and define local variables (§6.1)
                self.scope.push_scope();
                for var in &inst.local_vars {
                    self.scope.define(var.name.clone(), var.value.clone());
                }
                // device フィールドの変数解決（String なので scope.resolve() で直接）
                // Resolve device field variable reference (String, resolve directly)
                if let Some(resolved) = self.scope.resolve(&inst.device) {
                    inst.device = resolved.to_string();
                }
                // 未解決変数参照を resolver で解決（§6 変数展開）
                // Resolve unresolved variable references via resolver (§6 variable expansion)
                resolver::resolve_instrument(&mut inst, &self.scope)?;
                self.scope.pop_scope();
                self.registry.register_block(Block::Instrument(inst));
                Ok(EvalResult::Registered {
                    kind: "Instrument".into(),
                    name,
                })
            }
            Block::Kit(mut kit) => {
                let name = kit.name.clone();
                // device フィールドの変数解決（§6 変数展開）
                // Resolve device field variable reference (§6 variable expansion)
                if let Some(resolved) = self.scope.resolve(&kit.device) {
                    kit.device = resolved.to_string();
                }
                // 未解決変数参照を resolver で解決（§6 変数展開）
                // Resolve unresolved variable references via resolver (§6 variable expansion)
                resolver::resolve_kit(&mut kit, &self.scope)?;
                self.registry.register_block(Block::Kit(kit));
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
                // §12: 再生中の同名セッションなら次エントリ遷移時に差し替える
                // §12: If a session with the same name is currently playing,
                // queue it to swap in at the next entry transition.
                self.state.notify_session_updated(s);
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
                // グローバルスコープに変数を定義（§6 変数）
                // Define variable in global scope (§6 variables)
                self.scope.define_global(v.name.clone(), v.value.clone());
                self.registry.register_block(block);
                Ok(EvalResult::VarDefined { name })
            }
            Block::Play(cmd) => {
                match cmd.target {
                    PlayTarget::Scene(name) => {
                        // Phase 3: scene 定義を取り出して ScenePlayer を構築する
                        // Phase 3: resolve the scene definition and build a ScenePlayer
                        let scene_def = self
                            .registry
                            .get_scene(&name)
                            .ok_or_else(|| EngineError::UnknownScene(name.clone()))?
                            .clone();
                        let player = self.build_scene_player(&scene_def)?;
                        self.active_scene = Some(player);
                        self.state.apply_command(PlaybackCommand::PlayScene {
                            name,
                            repeat: cmd.repeat,
                        });
                    }
                    PlayTarget::Session(name) => {
                        // registry から SessionDef を取得して SessionRunner を構築する
                        // Fetch SessionDef from registry to construct a SessionRunner
                        match self.registry.get_session(&name) {
                            Some(session_def) => {
                                let def = session_def.clone();
                                // Phase 4: 最初のエントリの scene を build して active_scene にセット
                                // Phase 4: build the first entry's scene and set it as active
                                if let Some(first) = def.entries.first() {
                                    let scene_def = self
                                        .registry
                                        .get_scene(&first.scene)
                                        .ok_or_else(|| {
                                            EngineError::UnknownScene(first.scene.clone())
                                        })?
                                        .clone();
                                    let player = self.build_scene_player(&scene_def)?;
                                    self.active_scene = Some(player);
                                } else {
                                    self.active_scene = None;
                                }
                                self.state.apply_play_session(&def, cmd.repeat);
                            }
                            None => return Err(EngineError::UnknownSession(name)),
                        }
                    }
                }
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

    /// ScopeChain参照（§6.1 ブロックスコープ）
    /// Reference to the scope chain (§6.1 block scope)
    pub fn scope(&self) -> &ScopeChain {
        &self.scope
    }

    /// ScopeChain可変参照
    /// Mutable reference to the scope chain
    pub fn scope_mut(&mut self) -> &mut ScopeChain {
        &mut self.scope
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
        // 重複インクルード検出用セット（単調増加、removeしない）
        // Set for duplicate include detection (monotonically increasing, never removed)
        let mut included_files = HashSet::new();
        included_files.insert(canonical.clone());
        self.eval_file_recursive(&canonical, &mut include_stack, &mut included_files)
    }

    /// 再帰的にファイルを評価する（内部メソッド）
    /// Recursively evaluates a file (internal method)
    ///
    /// includeはファイル先頭にのみ許可される。非includeブロックの後に
    /// includeが出現した場合はエラーとなる。
    /// Includes are only allowed at the top of the file. An include appearing
    /// after a non-include block will result in an error.
    ///
    /// 同一ファイルを複数回インクルードした場合は `IncludeSkipped` を返し、
    /// 再評価はスキップされる。
    /// If the same file is included more than once, `IncludeSkipped` is returned
    /// and re-evaluation is skipped.
    ///
    /// # Arguments
    /// * `path` - 正規化済みのファイルパス / Canonicalized file path
    /// * `include_stack` - 循環検出用のインクルードスタック（push/popする） / Include stack for cycle detection (push/pop)
    /// * `included_files` - 重複インクルード検出用セット（単調増加、removeしない） / Set for duplicate include detection (monotonically increasing, never removed)
    ///
    /// # Returns
    /// 評価結果のベクター / Vector of evaluation results
    ///
    /// # Errors
    /// - `EngineError::CircularInclude` - 循環インクルード / Circular include
    /// - `EngineError::IncludeNotFound` - インクルードファイル未検出 / Include file not found
    /// - `EngineError::IncludeReadError` - ファイル読み込みエラー / File read error
    /// - `EngineError::IncludeNotAtTop` - includeがファイル先頭にない / Include not at top of file
    fn eval_file_recursive(
        &mut self,
        path: &Path,
        include_stack: &mut HashSet<PathBuf>,
        included_files: &mut HashSet<PathBuf>,
    ) -> Result<Vec<EvalResult>, EngineError> {
        let source = std::fs::read_to_string(path).map_err(|e| EngineError::IncludeReadError {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        let (_, blocks) = crate::parser::parse_source(&source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;

        let mut results = Vec::new();
        // includeフェーズが終了したかどうかを追跡
        // Track whether the include phase has ended
        let mut include_phase_ended = false;

        for block in blocks {
            match block {
                Block::Include(ref inc) => {
                    // 非includeブロックの後にincludeがある場合はエラー
                    // Error if include appears after a non-include block
                    if include_phase_ended {
                        return Err(EngineError::IncludeNotAtTop(inc.path.clone()));
                    }

                    let base_dir = path.parent().unwrap_or(Path::new("."));
                    let include_path = base_dir.join(&inc.path);
                    let canonical = include_path
                        .canonicalize()
                        .map_err(|_| EngineError::IncludeNotFound(inc.path.clone()))?;

                    // 重複チェック（循環チェックの前に行う）
                    // Duplicate check (before cycle detection)
                    if !included_files.insert(canonical.clone()) {
                        results.push(EvalResult::IncludeSkipped {
                            path: inc.path.clone(),
                        });
                        continue;
                    }

                    // 循環チェック
                    // Cycle detection
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

                    let sub_results =
                        self.eval_file_recursive(&canonical, include_stack, included_files)?;
                    let count = sub_results.len();
                    results.extend(sub_results);
                    results.push(EvalResult::IncludeProcessed {
                        path: inc.path.clone(),
                        results_count: count,
                    });

                    // include_stackはpush/popする（循環検出用）
                    // Pop from include_stack (used for cycle detection)
                    include_stack.remove(&canonical);
                }
                _ => {
                    include_phase_ended = true;
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

    /// registryが空の場合にソースからregistryを自動構築する
    /// Auto-populates registry from source when registry is empty
    ///
    /// # Arguments
    /// * `source` - メインのDSLソース文字列 / Main DSL source string
    /// * `additional_sources` - include由来の追加ソース / Additional sources from includes
    ///
    /// # Returns
    /// `true` if registry was populated, `false` if skipped (registry already has data)
    pub fn preload_from_source(&mut self, source: &str, additional_sources: &[&str]) -> bool {
        if !self.registry.is_empty() {
            return false;
        }
        // メインソースをプリロード評価
        // Preload-evaluate main source
        if self.eval_source_preload(source).is_err() {
            return false;
        }
        // 追加ソース（include分）をプリロード評価
        // Preload-evaluate additional sources (from includes)
        for additional in additional_sources {
            if self.eval_source_preload(additional).is_err() {
                return false;
            }
        }
        true
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
                local_vars: vec![],
                unresolved: Default::default(),
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

    /// グローバル変数が ScopeChain に登録されることを検証（§6）
    /// Verify global variables are registered in ScopeChain (§6)
    #[test]
    fn eval_var_registered_in_scope() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Var(VarDef {
            name: "dev".into(),
            value: "mutant_brain".into(),
        }))
        .unwrap();
        assert_eq!(ev.scope().resolve("dev"), Some("mutant_brain"));
    }

    /// グローバル変数の再定義で値が更新されること（§6.2）
    /// Verify global variable redefinition updates value (§6.2)
    #[test]
    fn eval_var_redefinition_updates_scope() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Var(VarDef {
            name: "dev".into(),
            value: "mutant_brain".into(),
        }))
        .unwrap();
        ev.eval_block(Block::Var(VarDef {
            name: "dev".into(),
            value: "keystep".into(),
        }))
        .unwrap();
        assert_eq!(ev.scope().resolve("dev"), Some("keystep"));
    }

    /// instrument ブロック内の local_vars がスコープ管理されること（§6.1）
    /// Verify instrument block local_vars are scope-managed (§6.1)
    #[test]
    fn eval_instrument_with_local_vars() {
        let mut ev = Evaluator::new(120.0);
        // グローバル変数を定義
        ev.eval_block(Block::Var(VarDef {
            name: "ch".into(),
            value: "1".into(),
        }))
        .unwrap();

        // ブロック内 local_vars 付きのインストゥルメントを登録
        ev.eval_block(Block::Instrument(InstrumentDef {
            name: "bass".into(),
            device: "mb".into(),
            channel: 3,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
            local_vars: vec![VarDef {
                name: "ch".into(),
                value: "3".into(),
            }],
            unresolved: Default::default(),
        }))
        .unwrap();

        // ブロック評価後はグローバルスコープに戻っていること
        assert_eq!(ev.scope().resolve("ch"), Some("1"));
    }

    #[test]
    fn eval_play_scene() {
        let mut ev = Evaluator::new(120.0);
        // clip と scene を事前登録
        // Register clip and scene beforehand
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
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
        // Phase 3: ScenePlayer が構築されている
        // Phase 3: ScenePlayer has been built
        assert!(ev.active_scene().is_some());
        assert_eq!(ev.active_scene().unwrap().clip_count(), 1);
    }

    /// 未登録シーン名を play した場合は UnknownScene エラー
    /// Playing an unregistered scene returns UnknownScene
    #[test]
    fn eval_play_scene_unknown_errors() {
        let mut ev = Evaluator::new(120.0);
        let err = ev
            .eval_block(Block::Play(PlayCommand {
                target: PlayTarget::Scene("missing".into()),
                repeat: RepeatSpec::Loop,
            }))
            .unwrap_err();
        assert!(matches!(err, EngineError::UnknownScene(ref n) if n == "missing"));
    }

    /// scene 内の clip が未登録の場合は UnknownClip エラー
    /// Playing a scene whose clip is unregistered returns UnknownClip
    #[test]
    fn eval_play_scene_unknown_clip_errors() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "ghost".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
        let err = ev
            .eval_block(Block::Play(PlayCommand {
                target: PlayTarget::Scene("verse".into()),
                repeat: RepeatSpec::Loop,
            }))
            .unwrap_err();
        assert!(matches!(err, EngineError::UnknownClip(ref n) if n == "ghost"));
    }

    /// Phase 4: session 内の最初の scene を build して active_scene にセット
    /// Play(Session) builds the first entry's ScenePlayer as active_scene (Phase 4)
    #[test]
    fn eval_play_session_builds_first_scene() {
        let mut ev = Evaluator::new(120.0);
        // clip/scene/session を順番に登録
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "s1".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![crate::ast::session::SessionEntry {
                scene: "s1".into(),
                repeat: crate::ast::session::SessionRepeat::Once,
            }],
        }))
        .unwrap();

        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Session("song".into()),
            repeat: RepeatSpec::Once,
        }))
        .unwrap();
        assert!(ev.active_scene().is_some());
        assert_eq!(ev.active_scene().unwrap().clip_count(), 1);
    }

    /// Phase 4: on_scene_loop_complete が NextScene で active_scene を差し替える
    /// on_scene_loop_complete swaps active_scene on NextScene (Phase 4)
    #[test]
    fn on_scene_loop_complete_transitions_to_next_scene() {
        let mut ev = Evaluator::new(120.0);
        // 2 clip + 2 scene + 2-entry session
        for name in ["a", "b"] {
            ev.eval_block(Block::Clip(ClipDef {
                name: name.into(),
                options: ClipOptions::default(),
                body: ClipBody::Pitched(PitchedClipBody {
                    lines: vec![],
                    cc_automations: vec![],
                }),
            }))
            .unwrap();
        }
        for (scene, clip) in [("s1", "a"), ("s2", "b")] {
            ev.eval_block(Block::Scene(SceneDef {
                name: scene.into(),
                entries: vec![crate::ast::scene::SceneEntry::Clip {
                    candidates: vec![crate::ast::scene::ShuffleCandidate {
                        clip: clip.into(),
                        weight: 1,
                    }],
                    probability: None,
                }],
            }))
            .unwrap();
        }
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![
                crate::ast::session::SessionEntry {
                    scene: "s1".into(),
                    repeat: crate::ast::session::SessionRepeat::Once,
                },
                crate::ast::session::SessionEntry {
                    scene: "s2".into(),
                    repeat: crate::ast::session::SessionRepeat::Once,
                },
            ],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Session("song".into()),
            repeat: RepeatSpec::Once,
        }))
        .unwrap();

        // 1ループ完了: SessionRunner.advance() が1回目に entries[0]=s1 を返すため
        // NextScene{s1}（既に Play 時に build 済みだが再 build される）
        // First loop complete: SessionRunner.advance() returns entries[0]=s1 first,
        // so NextScene{s1} (already built at Play, but rebuilt here)
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(
            outcome,
            SceneTransitionOutcome::NextScene {
                scene_name: "s1".into()
            }
        );

        // 2ループ目 → NextScene{s2}
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(
            outcome,
            SceneTransitionOutcome::NextScene {
                scene_name: "s2".into()
            }
        );
        assert!(ev.active_scene().is_some());

        // 3ループ目 → SessionComplete、active_scene が解放される
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(outcome, SceneTransitionOutcome::SessionComplete);
        assert!(ev.active_scene().is_none());
    }

    /// Phase 4: PlayScene(Loop) 下の on_scene_loop_complete は Continue を返す
    /// For PlayScene(Loop), on_scene_loop_complete returns Continue (Phase 4)
    #[test]
    fn on_scene_loop_complete_loop_returns_continue() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(outcome, SceneTransitionOutcome::Continue);
        assert!(ev.active_scene().is_some());
    }

    /// Phase 4: PlayScene(Once) で on_scene_loop_complete は SceneComplete
    /// For PlayScene(Once), returns SceneComplete and clears active_scene
    #[test]
    fn on_scene_loop_complete_once_returns_scene_complete() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Once,
        }))
        .unwrap();

        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(outcome, SceneTransitionOutcome::SceneComplete);
        assert!(ev.active_scene().is_none());
    }

    /// take_active_scene は ScenePlayer を奪い取り、Evaluator 側は None になる
    /// take_active_scene transfers the ScenePlayer out and leaves Evaluator with None
    #[test]
    fn take_active_scene_transfers_ownership() {
        let mut ev = Evaluator::new(120.0);
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let taken = ev.take_active_scene();
        assert!(taken.is_some());
        assert!(ev.active_scene().is_none());
    }

    #[test]
    fn eval_play_session() {
        let mut ev = Evaluator::new(120.0);
        // session を事前登録しておく
        // Register the session beforehand
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![],
        }))
        .unwrap();
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
    fn eval_play_session_unknown_errors() {
        let mut ev = Evaluator::new(120.0);
        let err = ev
            .eval_block(Block::Play(PlayCommand {
                target: PlayTarget::Session("missing".into()),
                repeat: RepeatSpec::Once,
            }))
            .unwrap_err();
        assert!(matches!(err, EngineError::UnknownSession(ref n) if n == "missing"));
    }

    #[test]
    fn eval_stop() {
        let mut ev = Evaluator::new(120.0);
        // Phase 3: play には登録済みの scene が必要
        // Phase 3: a registered scene is required to play
        ev.eval_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![],
        }))
        .unwrap();
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

    /// 循環インクルード（a→b→a）は重複スキップとして処理されエラーにならないことを検証
    /// Verifies that circular includes (a→b→a) are treated as duplicate skips and do not cause an error
    ///
    /// 重複チェックが循環チェックより先に行われるため、同一ファイルへの再インクルードは
    /// IncludeSkipped として処理される。
    /// Because duplicate check is performed before cycle detection, re-including the same
    /// file results in IncludeSkipped rather than CircularInclude.
    #[test]
    fn eval_file_circular_include() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.cvg");
        let file_b = dir.path().join("b.cvg");
        std::fs::write(&file_a, format!("include {}\n", file_b.display())).unwrap();
        std::fs::write(&file_b, format!("include {}\n", file_a.display())).unwrap();

        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_file(&file_a);
        // 循環は重複スキップとして処理され、エラーにならない
        // Circular include is treated as duplicate skip, not an error
        assert!(result.is_ok());
        let results = result.unwrap();
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::IncludeSkipped { .. })));
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

    /// includeがファイル先頭以外にある場合にエラーになることを検証
    /// Verifies that include not at the top of the file causes an error
    #[test]
    fn eval_file_include_not_at_top() {
        let dir = tempfile::tempdir().unwrap();
        let inc_file = dir.path().join("inc.cvg");
        std::fs::write(&inc_file, "tempo 120\n").unwrap();

        let main_file = dir.path().join("main.cvg");
        std::fs::write(
            &main_file,
            format!("tempo 120\ninclude {}\n", inc_file.display()),
        )
        .unwrap();

        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_file(&main_file);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::IncludeNotAtTop(_)
        ));
    }

    /// includeがファイル先頭にある場合は正常に動作することを検証
    /// Verifies that include at the top of the file works correctly
    #[test]
    fn eval_file_include_at_top_ok() {
        let dir = tempfile::tempdir().unwrap();
        let inc_file = dir.path().join("inc.cvg");
        std::fs::write(&inc_file, "tempo 120\n").unwrap();

        let main_file = dir.path().join("main.cvg");
        std::fs::write(
            &main_file,
            format!("include {}\nvar x = 42\n", inc_file.display()),
        )
        .unwrap();

        let mut ev = Evaluator::new(120.0);
        let result = ev.eval_file(&main_file);
        assert!(result.is_ok());
    }

    /// 同じファイルを複数回インクルードした場合に IncludeSkipped が返ることを検証
    /// Verifies that IncludeSkipped is returned when the same file is included more than once
    #[test]
    fn eval_file_duplicate_include_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let common_file = dir.path().join("common.cvg");
        std::fs::write(&common_file, "tempo 140\n").unwrap();

        // shared.cvg は common.cvg を一度インクルード
        // shared.cvg includes common.cvg once
        let shared_file = dir.path().join("shared.cvg");
        std::fs::write(&shared_file, format!("include {}\n", common_file.display())).unwrap();

        // main.cvg は shared.cvg と common.cvg の両方をインクルード（common は重複）
        // main.cvg includes both shared.cvg and common.cvg (common is duplicate)
        let main_file = dir.path().join("main.cvg");
        std::fs::write(
            &main_file,
            format!(
                "include {}\ninclude {}\n",
                shared_file.display(),
                common_file.display()
            ),
        )
        .unwrap();

        let mut ev = Evaluator::new(120.0);
        let results = ev.eval_file(&main_file).unwrap();

        // TempoChanged は1回だけ（重複スキップにより2回目は評価されない）
        // TempoChanged appears only once (second evaluation is skipped by dedup)
        let tempo_count = results
            .iter()
            .filter(|r| matches!(r, EvalResult::TempoChanged(_)))
            .count();
        assert_eq!(tempo_count, 1);

        // IncludeSkipped が含まれること
        // IncludeSkipped must be present
        assert!(results
            .iter()
            .any(|r| matches!(r, EvalResult::IncludeSkipped { .. })));
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

    /// 空registryの場合にpreload_from_sourceが成功することを検証
    /// Verifies preload_from_source succeeds when registry is empty
    #[test]
    fn preload_from_source_populates_empty_registry() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
device mb {
  port Mutant Brain
}

instrument bass {
  device mb
  channel 1
}
"#;
        assert!(ev.registry().is_empty());
        let result = ev.preload_from_source(source, &[]);
        assert!(result);
        assert!(!ev.registry().is_empty());
        assert!(ev.registry().get_device("mb").is_some());
        assert!(ev.registry().get_instrument("bass").is_some());
    }

    /// 非空registryの場合にpreload_from_sourceがスキップされることを検証
    /// Verifies preload_from_source skips when registry already has data
    #[test]
    fn preload_from_source_skips_non_empty_registry() {
        let mut ev = Evaluator::new(120.0);
        // 先にデバイスを登録
        // Register a device first
        ev.eval_source_preload("device d1 { port P1 }").unwrap();
        assert!(!ev.registry().is_empty());

        let result = ev.preload_from_source("device d2 { port P2 }", &[]);
        assert!(!result);
        // d2は登録されない
        // d2 should not be registered
        assert!(ev.registry().get_device("d2").is_none());
    }

    /// additional_sourcesが正しく登録されることを検証
    /// Verifies additional_sources are properly registered
    #[test]
    fn preload_from_source_with_additional_sources() {
        let mut ev = Evaluator::new(120.0);
        let main_source = r#"
device mb {
  port Mutant Brain
}
"#;
        let additional = r#"
instrument bass {
  device mb
  channel 1
}
"#;
        let result = ev.preload_from_source(main_source, &[additional]);
        assert!(result);
        assert!(ev.registry().get_device("mb").is_some());
        assert!(ev.registry().get_instrument("bass").is_some());
    }

    /// preload_from_sourceでPlay/Stopがスキップされることを検証
    /// Verifies preload_from_source skips Play/Stop blocks
    #[test]
    fn preload_from_source_skips_play_stop() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
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

play verse

stop
"#;
        let result = ev.preload_from_source(source, &[]);
        assert!(result);
        assert!(ev.registry().get_device("mb").is_some());
        // play/stopがスキップされても他は登録される
        // Other blocks are registered even though play/stop are skipped
        assert!(ev.registry().get_instrument("bass").is_some());
        assert!(ev.registry().get_clip("intro").is_some());
        assert!(ev.registry().get_scene("verse").is_some());
    }

    // === Phase 4: 変数展開 統合テスト（§6） ===
    // === Phase 4: Variable expansion integration tests (§6) ===

    /// device 変数展開: `var dev = mutant_brain` → `device dev` で展開される
    /// Device variable expansion: `var dev = mutant_brain` → `device dev` is expanded
    #[test]
    fn eval_var_expansion_device() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var dev = mutant_brain

device mutant_brain {
  port Mutant Brain
}

instrument bass {
  device dev
  channel 1
}
"#;
        ev.eval_source(source).unwrap();
        let inst = ev.registry().get_instrument("bass").unwrap();
        assert_eq!(inst.device, "mutant_brain");
    }

    /// channel 変数展開: `var ch = 3` → `channel ch` で展開される
    /// Channel variable expansion: `var ch = 3` → `channel ch` is expanded
    #[test]
    fn eval_var_expansion_channel() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var ch = 3

instrument bass {
  device mb
  channel ch
}
"#;
        ev.eval_source(source).unwrap();
        let inst = ev.registry().get_instrument("bass").unwrap();
        assert_eq!(inst.channel, 3);
    }

    /// gate_normal 変数展開
    /// gate_normal variable expansion
    #[test]
    fn eval_var_expansion_gate_normal() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var gn = 100

instrument bass {
  device mb
  channel 1
  gate_normal gn
}
"#;
        ev.eval_source(source).unwrap();
        let inst = ev.registry().get_instrument("bass").unwrap();
        assert_eq!(inst.gate_normal, Some(100));
    }

    /// cc cc_number 変数展開
    /// cc cc_number variable expansion
    #[test]
    fn eval_var_expansion_cc_number() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var cc_num = 74

instrument bass {
  device mb
  channel 1
  cc filter cc_num
}
"#;
        ev.eval_source(source).unwrap();
        let inst = ev.registry().get_instrument("bass").unwrap();
        assert_eq!(inst.cc_mappings[0].cc_number, 74);
    }

    /// ブロックスコープ + シャドーイング: ブロック内 var がグローバルを上書き
    /// Block scope + shadowing: block-local var overrides global
    #[test]
    fn eval_var_expansion_block_scope_shadowing() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var ch = 1

instrument bass {
  var ch = 3
  device mb
  channel ch
}
"#;
        ev.eval_source(source).unwrap();
        let inst = ev.registry().get_instrument("bass").unwrap();
        assert_eq!(inst.channel, 3);
        // ブロック後はグローバルスコープに戻る
        // After block, global scope is restored
        assert_eq!(ev.scope().resolve("ch"), Some("1"));
    }

    /// 未定義変数エラー
    /// Undefined variable error
    #[test]
    fn eval_var_expansion_undefined_variable() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
instrument bass {
  device mb
  channel missing_var
}
"#;
        let result = ev.eval_source(source);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::UndefinedVariable { .. }
        ));
    }

    /// 数値変換失敗エラー
    /// Numeric conversion failure error
    #[test]
    fn eval_var_expansion_invalid_value() {
        let mut ev = Evaluator::new(120.0);
        let source = r#"
var ch = abc

instrument bass {
  device mb
  channel ch
}
"#;
        let result = ev.eval_source(source);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::InvalidVariableValue { .. }
        ));
    }
}
