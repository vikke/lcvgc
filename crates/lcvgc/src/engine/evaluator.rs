//! evalコマンドディスパッチャ
//!
//! DSLのBlockをレジストリ・クロック・ステートに振り分けて評価する。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::ast::playback::PlayTarget;
use crate::ast::scene::SceneDef;
use crate::ast::Block;
use crate::engine::clock::Clock;
use crate::engine::compiler::{compile_clip, CompiledClip};
use crate::engine::device_event::{DeviceEvent, DeviceEventTx};
use crate::engine::error::EngineError;
use crate::engine::player::ScenePlayer;
use crate::engine::registry::Registry;
use crate::engine::resolver;
use crate::engine::scene_runner::{extract_tempo_change, initial_muted_clips, resolve_scene};
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
    /// ポーズ成功（§10.4）
    /// Pause succeeded (§10.4)
    Paused {
        /// ポーズ対象名（None = 全体） / Pause target (None = whole)
        target: Option<String>,
    },
    /// ポーズが no-op になった（§10.4 名前不一致等）
    /// Pause was a no-op (§10.4 name mismatch, nothing to pause, etc.)
    PausedNoop {
        /// 理由メッセージ / Reason message
        reason: String,
    },
    /// 再開成功（§10.4）
    /// Resume succeeded (§10.4)
    Resumed {
        /// 再開対象名（None = 全体） / Resume target (None = whole)
        target: Option<String>,
    },
    /// 再開が no-op になった（§10.4 Paused でない、名前不一致等）
    /// Resume was a no-op (§10.4 not paused, name mismatch, etc.)
    ResumedNoop {
        /// 理由メッセージ / Reason message
        reason: String,
    },
    /// クリップ・ミュート成功（§10.4）
    /// Clip mute succeeded (§10.4)
    Muted {
        /// ミュート対象の clip 名 / Muted clip name
        target: String,
    },
    /// クリップ・ミュートが no-op になった（§10.4 clip 名不一致等）
    /// Clip mute was a no-op (§10.4 unknown clip name, no active scene, etc.)
    MutedNoop {
        /// 理由メッセージ / Reason message
        reason: String,
    },
    /// クリップ・アンミュート成功（§10.4）
    /// Clip unmute succeeded (§10.4)
    Unmuted {
        /// アンミュート対象の clip 名 / Unmuted clip name
        target: String,
    },
    /// クリップ・アンミュートが no-op になった（§10.4 clip 名不一致等）
    /// Clip unmute was a no-op (§10.4 unknown clip name, no active scene, etc.)
    UnmutedNoop {
        /// 理由メッセージ / Reason message
        reason: String,
    },
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

/// device の MIDI ポート接続失敗を表す情報
///
/// LSP diagnostic で「device <name> の port "..." への接続に失敗した」旨を
/// エディタに表示する用途で保持される。`port` は当該 device に最後に指定
/// された port 文字列、`message` は基底ライブラリ (midir) が返したエラー文を
/// そのまま格納する。
///
/// Connection failure record for a `device` MIDI port. Stored to surface a
/// diagnostic in the editor showing the port string that failed and the
/// underlying error message from the MIDI backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceConnectionError {
    pub port: String,
    pub message: String,
}

/// ロック外 eval (prepare/apply 分離) のためのコンパイル成果物キャッシュモード。
///
/// 再生もたつき対策として、重い parse+compile を再生スレッドと同じ `Evaluator`
/// ロックの外で行い、結果差し替え (swap) だけを短時間ロックで行うために使う。
/// `eval_block` 内の 2 つの compile 箇所 (Clip 差し替え時の `compile_clip`、Play 時の
/// `build_scene_player`) だけがこのモードを参照する。
///
/// - `Off`: 通常評価。compile をその場で行う (既存挙動)。
/// - `Record`: 使い捨て Evaluator 上 (ロック外) で compile を実行し、成果物を名前キーで
///   記録する。`prepare_*` が使用。
/// - `Replay`: live Evaluator 上 (ロック内) で、記録済み成果物があれば compile を省略して
///   再利用する。キャッシュミス時は通常 compile にフォールバックする。`apply_prepared` が使用。
///
/// Compile-artifact cache mode for off-lock eval (the prepare/apply split).
/// Heavy parse+compile runs outside the `Evaluator` lock shared with the playback
/// thread; only the swap of results happens under a short lock. Only the two
/// compile sites inside `eval_block` (the `compile_clip` on clip replacement and
/// `build_scene_player` on Play) consult this mode.
#[derive(Debug, Default)]
enum PrecompileMode {
    /// 通常評価 (その場で compile)。 / Normal evaluation (compile in place).
    #[default]
    Off,
    /// ロック外で compile し成果物を記録する。 / Compile off-lock and record artifacts.
    Record {
        clips: HashMap<String, CompiledClip>,
        players: HashMap<String, ScenePlayer>,
    },
    /// 記録済み成果物を再利用する (ミス時は通常 compile)。 / Reuse recorded artifacts (compile on miss).
    Replay {
        clips: HashMap<String, CompiledClip>,
        players: HashMap<String, ScenePlayer>,
    },
}

/// `prepare_*` に渡す不変スナップショット。
///
/// snapshot は live `Evaluator` ロックを**短時間**だけ握って取得し、以降のロック外
/// prepare (parse + compile) はこのスナップショットのみに依存する。`StateManager` は
/// 非 Clone かつ prepare には不要なので含めない。`active_scene` は Clip 差し替えの
/// in_use 判定を忠実に再現するために複製する。
///
/// Immutable snapshot passed to `prepare_*`. Taken while holding the live
/// `Evaluator` lock only briefly; the subsequent off-lock prepare depends solely
/// on this snapshot. `StateManager` is excluded (non-Clone and unneeded for
/// prepare). `active_scene` is cloned so the throwaway can faithfully reproduce
/// the in-use decision for clip replacement.
pub struct EvalSnapshot {
    /// registry は `Arc<Registry>` (PR #107 で Arc 化済み) なので、snapshot は
    /// deep clone ではなく Arc クローン (参照カウント増加) で済む。throwaway 側で
    /// `register_block` するときに初めて copy-on-write される。
    ///
    /// `registry` is `Arc<Registry>` (Arc-ified in PR #107), so snapshotting is a
    /// cheap refcount bump rather than a deep clone; copy-on-write happens only when
    /// the throwaway calls `register_block`.
    registry: Arc<Registry>,
    scope: ScopeChain,
    clock: Clock,
    active_scene: Option<ScenePlayer>,
    active_scene_name: Option<String>,
}

/// ロック外 prepare の成果物。`apply_prepared` がこれを live state へ機械適用する。
///
/// `blocks` は (include 展開済みの) 評価対象ブロック列。`clips`/`players` は
/// prepare 時に compile 済みの成果物 (名前キー)。apply は `blocks` を live 上で
/// `eval_block` 評価するが、compile は `clips`/`players` から再利用するため
/// ロック保持時間が compile コストに依存しない。
///
/// Result of off-lock prepare; `apply_prepared` applies it to live state. `blocks`
/// is the (include-expanded) block list to evaluate; `clips`/`players` are the
/// artifacts pre-compiled during prepare (keyed by name). apply evaluates `blocks`
/// on live via `eval_block` but reuses compiled artifacts, so lock-hold time does
/// not depend on compile cost.
pub struct PreparedProgram {
    blocks: Vec<Block>,
    clips: HashMap<String, CompiledClip>,
    players: HashMap<String, ScenePlayer>,
}

impl EvalSnapshot {
    /// `eval_source` のロック外 prepare 版。parse と compile をロック外で行う。
    ///
    /// `Evaluator::apply_prepared` と組で使うと `eval_source` と意味的に等価になる。
    ///
    /// Off-lock prepare counterpart of `eval_source`; combined with
    /// `Evaluator::apply_prepared` it is semantically equivalent to `eval_source`.
    pub fn prepare_source(self, source: &str) -> Result<PreparedProgram, EngineError> {
        let (_, blocks) = crate::parser::parse_source(source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;
        self.prepare_blocks(blocks)
    }

    /// `eval_source_preload` のロック外 prepare 版 (play/stop を除外)。
    ///
    /// Off-lock prepare counterpart of `eval_source_preload` (play/stop excluded).
    pub fn prepare_source_preload(self, source: &str) -> Result<PreparedProgram, EngineError> {
        let (_, blocks) = crate::parser::parse_source(source)
            .map_err(|e| EngineError::ParseError(e.to_string()))?;
        let filtered: Vec<Block> = blocks
            .into_iter()
            .filter(|b| !matches!(b, Block::Play(_) | Block::Stop(_)))
            .collect();
        self.prepare_blocks(filtered)
    }

    /// `eval_file` のロック外 prepare 版。include 展開・parse・compile をロック外で行う。
    ///
    /// include 展開は `expand_file_blocks` が `Evaluator::eval_file_recursive` と
    /// 同順序のフラットなブロック列へ畳み込む。展開結果を `prepare_blocks` へ渡す。
    ///
    /// Off-lock prepare counterpart of `eval_file`: include expansion, parse, and
    /// compile all run off-lock. `expand_file_blocks` flattens includes in the same
    /// order as `Evaluator::eval_file_recursive`.
    pub fn prepare_file(self, path: &Path) -> Result<PreparedProgram, EngineError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| EngineError::IncludeNotFound(path.display().to_string()))?;
        let mut include_stack = HashSet::new();
        include_stack.insert(canonical.clone());
        let mut included_files = HashSet::new();
        included_files.insert(canonical.clone());
        let mut blocks = Vec::new();
        expand_file_blocks(
            &canonical,
            &mut include_stack,
            &mut included_files,
            &mut blocks,
        )?;
        self.prepare_blocks(blocks)
    }

    /// ブロック列をロック外 prepare する (内部共通処理)。
    ///
    /// 使い捨て Evaluator を `Record` モードで起動し、`blocks` のクローンを
    /// `eval_block` 評価して compile 成果物を収集する。元の `blocks` は
    /// `PreparedProgram` に保持し、後で `Evaluator::apply_prepared` が live 上で
    /// 再評価する。
    ///
    /// Off-lock prepare for a block list: runs a throwaway in `Record` mode over a
    /// clone of `blocks` to collect compiled artifacts, keeping the originals for
    /// `Evaluator::apply_prepared` to re-evaluate on live.
    fn prepare_blocks(self, blocks: Vec<Block>) -> Result<PreparedProgram, EngineError> {
        let mut throwaway = Evaluator::from_snapshot(
            self,
            PrecompileMode::Record {
                clips: HashMap::new(),
                players: HashMap::new(),
            },
        );
        for block in blocks.iter().cloned() {
            throwaway.eval_block(block)?;
        }
        let (clips, players) = match throwaway.precompile {
            PrecompileMode::Record { clips, players } => (clips, players),
            _ => (HashMap::new(), HashMap::new()),
        };
        Ok(PreparedProgram {
            blocks,
            clips,
            players,
        })
    }
}

/// ファイルを読み・parse し、include を再帰展開してフラットなブロック列を作る。
///
/// `Evaluator::eval_file_recursive` の評価を伴わない版。include の先頭限定・
/// 循環検出・重複スキップのルールを同一に保ち、評価対象 (非 include) ブロックを
/// 評価されるのと同じ順序で `out` へ push する。
///
/// File-read + parse + recursive include expansion into a flat block list. A
/// non-evaluating twin of `Evaluator::eval_file_recursive` that preserves the same
/// include-at-top / cycle / dedup rules and pushes evaluable (non-include) blocks to
/// `out` in evaluation order.
fn expand_file_blocks(
    path: &Path,
    include_stack: &mut HashSet<PathBuf>,
    included_files: &mut HashSet<PathBuf>,
    out: &mut Vec<Block>,
) -> Result<(), EngineError> {
    let source = std::fs::read_to_string(path).map_err(|e| EngineError::IncludeReadError {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let (_, blocks) =
        crate::parser::parse_source(&source).map_err(|e| EngineError::ParseError(e.to_string()))?;

    let mut include_phase_ended = false;
    for block in blocks {
        match block {
            Block::Include(ref inc) => {
                if include_phase_ended {
                    return Err(EngineError::IncludeNotAtTop(inc.path.clone()));
                }
                let base_dir = path.parent().unwrap_or(Path::new("."));
                let include_path = base_dir.join(&inc.path);
                let canonical = include_path
                    .canonicalize()
                    .map_err(|_| EngineError::IncludeNotFound(inc.path.clone()))?;
                // 重複インクルードは展開しない (eval_file_recursive と同じ)。
                // Skip duplicate includes (matches eval_file_recursive).
                if !included_files.insert(canonical.clone()) {
                    continue;
                }
                // 循環検出。 / Cycle detection.
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
                expand_file_blocks(&canonical, include_stack, included_files, out)?;
                include_stack.remove(&canonical);
            }
            _ => {
                include_phase_ended = true;
                out.push(block);
            }
        }
    }
    Ok(())
}

/// evalコマンドディスパッチャ
#[derive(Debug)]
pub struct Evaluator {
    /// DSL 定義レジストリ。`Arc` で保持することで、LSP（補完 / 診断 / hover）が
    /// Evaluator ロック内で `Registry` を deep clone する代わりに `Arc::clone`
    /// （参照カウント増のみ）で snapshot を取れる。これにより演奏中にエディタが
    /// 打鍵するたびに再生スレッドを長くブロックする競合（P1）を解消する。
    /// 定義の登録は `Arc::make_mut` による copy-on-write で行う。
    ///
    /// Registry held behind `Arc` so the LSP can snapshot it via `Arc::clone`
    /// (refcount bump) instead of deep-cloning the whole `Registry` while holding
    /// the Evaluator lock. Writes use `Arc::make_mut` (copy-on-write).
    registry: Arc<Registry>,
    state: StateManager,
    /// テンポ・PPQ を保持する Clock。`PlaybackDriver` と共有するために
    /// `Arc<RwLock<Clock>>` で保持する。`tempo` 評価は `write()` で更新し、
    /// driver 側は毎 tick `read()` で `tick_duration_us` を読む。
    ///
    /// Clock holding tempo and PPQ. Stored as `Arc<RwLock<Clock>>` so that the
    /// `PlaybackDriver` can observe tempo updates from `tempo` blocks. Writes
    /// happen on `tempo` evaluation; the driver reads `tick_duration_us` on
    /// every tick.
    clock: Arc<RwLock<Clock>>,
    /// 変数スコープチェーン（§6.1 ブロックスコープ対応）
    /// Variable scope chain (§6.1 block scope support)
    scope: ScopeChain,
    /// 現在 play 中の ScenePlayer（Phase 3: PlayScene でコンパイル・構築）
    /// Currently active ScenePlayer (Phase 3: built when PlayScene is evaluated)
    active_scene: Option<ScenePlayer>,
    /// 現在 active な scene の名前。`active_scene` の代入とペアで更新する。
    /// scene 内 tempo 行をループ完了境界で apply する際に、どの SceneDef を
    /// 引けば良いか辿るために使う。session 中も `entries` に応じて切り替わる。
    ///
    /// Name of the currently active scene, updated alongside `active_scene`.
    /// Used to look up the right SceneDef when applying scene-level tempo
    /// entries at loop boundaries. Also tracks scene changes inside a session.
    active_scene_name: Option<String>,
    /// Stop/mute 評価時に呼び出し側が送出すべき AllNotesOff の対象
    /// `(device, channel)` 一覧（Phase 5 + Issue #49）
    ///
    /// Issue #49: 複数 device ルーティング対応のため channel のみから
    /// (device 論理名, channel) のペアに拡張。
    ///
    /// Queue of `(device, channel)` pairs that the caller should emit
    /// AllNotesOff (CC#123 value=0) on after Stop/mute. Extended from a bare
    /// channel list to support multi-device routing (Issue #49).
    pending_all_notes_off: Vec<(String, crate::domain::channel::MidiChannel)>,
    /// Play/Stop 評価時に `transport = true` の device へ送出する MIDI System
    /// Real-Time メッセージ (`Start` / `Stop`) のキュー。呼び出し側（tick
    /// driver / daemon）は device 名をキーに対応する `MidiSink` を選び、
    /// 各メッセージをそのまま送出する (Issue #50)。
    ///
    /// Queue of System Real-Time messages (`Start` / `Stop`) to emit to
    /// devices whose `transport` flag is true when evaluating Play/Stop.
    /// The caller dispatches each message to the matching `MidiSink` by
    /// device name (Issue #50).
    pending_transport: Vec<(String, crate::midi::message::MidiMessage)>,
    /// core からバイナリ層への一方向通知 channel。`Block::Device` 評価時に
    /// `DeviceEvent::Upsert` を emit し、受信側 (lcvgc バイナリ) が
    /// `MidirSink` を構築・差し替える (PR #54)。未設定でも eval は通常通り
    /// 動作する（後方互換のため `Option`）。
    ///
    /// One-way notification channel from core to the binary layer. On
    /// `Block::Device` evaluation, an `DeviceEvent::Upsert` is emitted so
    /// that the receiving side (the `lcvgc` binary) can build/swap the
    /// matching `MidirSink` (PR #54). Eval still works normally when this
    /// is `None` (kept `Option` for backward compatibility).
    device_event_tx: Option<DeviceEventTx>,
    /// device 名 → 直近の接続失敗情報のマップ
    ///
    /// main.rs (binary 側) が `MidirSink` 構築失敗時に `record_device_connection_error`
    /// を呼び、成功時に `clear_device_connection_error` を呼ぶ。LSP diagnostic 計算は
    /// このマップから errors を読み出す。
    ///
    /// Map of device name -> latest connection failure. The binary calls
    /// `record_device_connection_error` on `MidirSink` build failure and
    /// `clear_device_connection_error` on success. LSP diagnostic generation
    /// reads this map.
    device_connection_errors: HashMap<String, DeviceConnectionError>,
    /// 更新起因の session scene 強制遷移を、次の4小節グリッド境界で行うフラグ。
    ///
    /// session 再生中に、その session が参照する clip / scene / session 定義が
    /// 上書き（再 eval）されたとき true にする。`PlaybackDriver` が4小節グリッド
    /// 境界（PR #99 の clip swap と同じ境界）で `try_force_advance_session_on_grid`
    /// を呼び、true なら現エントリの LCM 境界を待たず次エントリ（別 scene）へ遷移し、
    /// フラグを false に戻す。clip 単位の差し替え（pending_clip）とは別経路で、
    /// 「scene そのものを別 scene へ切り替える」用途を担う。
    ///
    /// Flag requesting an update-triggered forced session-scene transition at the
    /// next 4-bar grid boundary. Set true when a clip / scene / session definition
    /// referenced by the currently playing session is overwritten (re-evaluated).
    /// The `PlaybackDriver` calls `try_force_advance_session_on_grid` on the 4-bar
    /// grid (the same boundary as PR #99's clip swap); when true, the session
    /// jumps to the next entry (a different scene) without waiting for the current
    /// scene's LCM boundary, then the flag is cleared. This is a separate path
    /// from per-clip swaps (pending_clip) and handles switching the whole scene.
    force_scene_advance_on_grid: bool,
    /// ロック外 eval (prepare/apply 分離) のための compile キャッシュモード。
    /// 通常は `Off`。`prepare_*` が使い捨て Evaluator 上で `Record` にして compile
    /// 成果物を記録し、`apply_prepared` が live 上で `Replay` にして再利用する。
    /// `eval_block` 内の 2 つの compile 箇所だけがこのフィールドを参照する。
    ///
    /// Compile-cache mode for off-lock eval (the prepare/apply split). Normally
    /// `Off`. `prepare_*` sets `Record` on a throwaway Evaluator to capture compiled
    /// artifacts; `apply_prepared` sets `Replay` on live to reuse them. Only the two
    /// compile sites inside `eval_block` consult this field.
    precompile: PrecompileMode,
}

impl Evaluator {
    /// 指定BPMで初期化
    pub fn new(bpm: f64) -> Self {
        Self {
            registry: Arc::new(Registry::new()),
            state: StateManager::new(),
            clock: Arc::new(RwLock::new(Clock::new(bpm))),
            scope: ScopeChain::new(),
            active_scene: None,
            active_scene_name: None,
            pending_all_notes_off: Vec::new(),
            pending_transport: Vec::new(),
            device_event_tx: None,
            device_connection_errors: HashMap::new(),
            force_scene_advance_on_grid: false,
            precompile: PrecompileMode::Off,
        }
    }

    /// `DeviceEvent` 通知用 tx を登録する (PR #54)。
    ///
    /// 設定後、`Block::Device` を eval すると `DeviceEvent::Upsert` が
    /// receiver へ送出される。テストや tx を必要としない使い方では
    /// 呼ばないことで後方互換を保つ。
    ///
    /// Registers the sender used for `DeviceEvent` notifications (PR #54).
    /// Once set, evaluating a `Block::Device` emits `DeviceEvent::Upsert`
    /// to the receiver. Skip this call to preserve the legacy behavior.
    pub fn set_device_event_tx(&mut self, tx: DeviceEventTx) {
        self.device_event_tx = Some(tx);
    }

    /// device の接続失敗を記録する
    ///
    /// `name` の既存エントリは上書きされる（最新の失敗情報のみ保持）。
    ///
    /// Records the latest connection failure for `name`, overwriting any prior
    /// entry for the same device.
    pub fn record_device_connection_error(&mut self, name: String, port: String, message: String) {
        self.device_connection_errors
            .insert(name, DeviceConnectionError { port, message });
    }

    /// device の接続失敗エントリを削除する（成功時に呼ぶ）
    ///
    /// 該当 device が存在しない場合は何もしない。
    ///
    /// Removes the connection-error entry for `name` (called on successful
    /// connect). No-op if the device has no entry.
    pub fn clear_device_connection_error(&mut self, name: &str) {
        self.device_connection_errors.remove(name);
    }

    /// device 接続失敗マップへの不変参照
    ///
    /// LSP diagnostic 計算で device ブロックと突き合わせるために使う。
    ///
    /// Returns an immutable reference to the connection-error map. Used by LSP
    /// diagnostic generation to cross-reference device blocks.
    pub fn device_connection_errors(&self) -> &HashMap<String, DeviceConnectionError> {
        &self.device_connection_errors
    }

    /// Stop/mute 評価で溜まった AllNotesOff 対象の `(device, channel)`
    /// 一覧を取り出してクリアする
    ///
    /// 呼び出し側（tick driver / daemon）は device 名をキーに対応する
    /// `MidiSink` を選び、`MidiMessage::ControlChange { cc: 123, value: 0 }`
    /// を各 channel に送出する。
    ///
    /// Takes the queued AllNotesOff `(device, channel)` pairs and clears the
    /// internal buffer. The caller selects the matching `MidiSink` by device
    /// name and emits CC#123 value=0 on each channel (Issue #49).
    pub fn take_pending_all_notes_off(
        &mut self,
    ) -> Vec<(String, crate::domain::channel::MidiChannel)> {
        std::mem::take(&mut self.pending_all_notes_off)
    }

    /// Play / Stop 評価で溜まった MIDI System Real-Time メッセージの
    /// `(device, message)` 一覧を取り出してクリアする (Issue #50)。
    ///
    /// 呼び出し側（tick driver / daemon）は device 名をキーに対応する
    /// `MidiSink` を選び、`MidiMessage::Start` / `MidiMessage::Stop` を
    /// そのまま送出する。
    ///
    /// Takes the queued System Real-Time `(device, message)` pairs and clears
    /// the internal buffer (Issue #50). The caller selects the matching
    /// `MidiSink` by device name and forwards each message as-is.
    pub fn take_pending_transport(&mut self) -> Vec<(String, crate::midi::message::MidiMessage)> {
        std::mem::take(&mut self.pending_transport)
    }

    /// `transport = true` の device 名一覧を registry から取り出すヘルパー
    /// (Issue #50)。
    ///
    /// PlaybackDriver は再生中の Timing Clock (0xF8) 送出時にこの関数を
    /// 呼んで送信先 device を決定する。
    ///
    /// Returns device names whose `transport` flag is `true` (Issue #50).
    /// Also called from the playback driver when emitting Timing Clock
    /// (0xF8) to determine recipient devices.
    pub fn transport_enabled_devices(&self) -> Vec<String> {
        // Registry 側で precompute されたキャッシュを clone するだけ。
        // 毎 tick この経路を呼ぶ playback driver のために、HashMap 走査と
        // get_device() lookup の繰り返しを避けている。
        // Just clone Registry's precomputed cache; avoids the per-tick HashMap
        // walk + repeated `get_device` lookups that the playback driver
        // would otherwise pay for every tick.
        self.registry.transport_enabled_device_names().to_vec()
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

    /// 現在 active な scene 名を返す（テスト・検証用）
    ///
    /// session の scene 遷移検証で「いまどの scene が再生中か」を直接確認するために
    /// 使う。内部状態の読み取り専用アクセサ。
    ///
    /// Returns the currently active scene name (for tests/verification). Used to
    /// directly assert which scene is playing during session transition tests.
    ///
    /// # Returns
    /// active な scene 名の `&str`、再生していなければ `None`
    pub fn active_scene_name_for_test(&self) -> Option<&str> {
        self.active_scene_name.as_deref()
    }

    /// ScenePlayer を取り出す（Evaluator 側は None に戻る）
    /// Takes the ScenePlayer out, leaving None in the Evaluator
    pub fn take_active_scene(&mut self) -> Option<ScenePlayer> {
        self.active_scene.take()
    }

    /// session 再生中なら、4小節グリッドでの強制 scene 遷移フラグを立てる
    ///
    /// session 以外（停止中・PlayScene 等）では何もしない。clip / scene 定義の
    /// 上書き検知点から呼び、session が参照する構成更新を次の grid 境界で反映させる。
    ///
    /// Sets the 4-bar-grid forced-scene-advance flag when a session is playing.
    /// No-op otherwise (stopped, plain PlayScene, etc.). Called from clip/scene
    /// overwrite detection so that composition updates referenced by a session
    /// take effect on the next grid boundary.
    fn request_force_scene_advance_if_session(&mut self) {
        if matches!(
            self.state.state(),
            crate::engine::state::PlaybackState::PlayingSession { .. }
        ) {
            self.force_scene_advance_on_grid = true;
        }
    }

    /// 指定名の session が現在再生中かどうかを返す
    ///
    /// # Arguments
    /// * `name` - 判定対象の session 名
    ///
    /// # Returns
    /// 同名 session を `PlayingSession` として再生中なら true
    ///
    /// Returns whether a session with the given name is currently playing.
    fn is_playing_session_named(&self, name: &str) -> bool {
        matches!(
            self.state.state(),
            crate::engine::state::PlaybackState::PlayingSession { name: n, .. } if n == name
        )
    }

    /// 4小節グリッド境界で、更新起因の session scene 強制遷移を試みる
    ///
    /// `PlaybackDriver` が4小節グリッド境界（`commit_pending_clips` と同じ境界）で
    /// 呼ぶ。`force_scene_advance_on_grid` が立っていなければ何もせず `Continue` を
    /// 返す。立っていればフラグを下ろし、`StateManager::force_advance_session_scene`
    /// で現エントリの残りを捨てて次エントリへ進め、その結果に応じて `active_scene` を
    /// 差し替える。tempo apply は行わない（途中打ち切りのため）。
    ///
    /// Attempts the update-triggered forced session-scene transition on the 4-bar
    /// grid. Called by the `PlaybackDriver` at the grid boundary (same boundary as
    /// `commit_pending_clips`). Returns `Continue` without side effects if the
    /// flag is not set. When set, clears the flag, advances to the next entry via
    /// `StateManager::force_advance_session_scene` (discarding the current entry's
    /// remaining repeats), and swaps `active_scene` accordingly. Does not apply
    /// tempo (this is a mid-loop cut).
    ///
    /// # Errors
    /// - `EngineError::UnknownScene` - 次エントリの scene が registry に未登録
    /// - `EngineError::UnknownClip` - 次 scene 内の clip が未登録
    pub fn try_force_advance_session_on_grid(
        &mut self,
    ) -> Result<SceneTransitionOutcome, EngineError> {
        if !self.force_scene_advance_on_grid {
            return Ok(SceneTransitionOutcome::Continue);
        }
        self.force_scene_advance_on_grid = false;

        // 進む先エントリが無い（単一エントリ / 末尾エントリ非ループ）session では、
        // 強制遷移すると force_next_entry が末尾超過 Done → SessionComplete を返し、
        // active_scene=None で MIDI clock 含む送出が停止してしまう。
        // この場合は次の曲へ進むのではなく「現 scene を新定義で作り直して鳴らし
        // 続ける」のがユーザーの編集→反映ループの意図に合う（案C）。よって強制遷移
        // せず、現 active_scene を最新の scene 定義でリビルドして Continue を返す。
        // clip 単位の上書きは既に replace_clip で pending 反映済みなので、ここでの
        // リビルドは主に scene 構成上書きを取り込むためのもの。
        //
        // When the session has no next entry to advance to (single-entry, or a
        // non-looping tail entry), a forced advance would return SessionComplete
        // and clear active_scene, stopping all MIDI output (clock included).
        // Instead of advancing, rebuild the current scene from its latest
        // definition and keep playing (case C: this matches the user's
        // edit→reflect loop intent). Per-clip overwrites are already staged via
        // replace_clip; this rebuild mainly absorbs scene-composition overwrites.
        if !self.state.has_forced_next_session_entry() {
            if let Some(name) = self.active_scene_name.clone() {
                if let Some(scene_def) = self.registry.get_scene(&name).cloned() {
                    let player = self.build_scene_player(&scene_def)?;
                    self.active_scene = Some(player);
                }
            }
            return Ok(SceneTransitionOutcome::Continue);
        }

        let action = self.state.force_advance_session_scene();
        match action {
            NextAction::ContinueScene => Ok(SceneTransitionOutcome::Continue),
            NextAction::SceneComplete => {
                self.active_scene = None;
                self.active_scene_name = None;
                Ok(SceneTransitionOutcome::SceneComplete)
            }
            NextAction::SessionComplete => {
                self.active_scene = None;
                self.active_scene_name = None;
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
                self.active_scene_name = Some(scene_name.clone());
                Ok(SceneTransitionOutcome::NextScene { scene_name })
            }
        }
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
        // §8.4: 今 1 ループを終えた scene の tempo 行を apply する。
        // scene activate 直後は apply せず、最初のループ完了境界で初めて apply
        // するセマンティクスのため、ここで state を進める前に処理する。
        //
        // §8.4: apply the tempo entry of the scene that just completed one loop.
        // We deliberately run this before advancing the state so that scene
        // activation itself does not trigger a tempo apply — the very first
        // loop boundary is what actually rolls the tempo forward.
        if let Some(name) = self.active_scene_name.clone() {
            if let Some(scene_def) = self.registry.get_scene(&name).cloned() {
                if let Some(tempo) = extract_tempo_change(&scene_def) {
                    self.clock.write().unwrap().apply_tempo(&tempo);
                }
            }
        }

        let action = self.state.scene_loop_complete();
        match action {
            NextAction::ContinueScene => Ok(SceneTransitionOutcome::Continue),
            NextAction::SceneComplete => {
                self.active_scene = None;
                self.active_scene_name = None;
                Ok(SceneTransitionOutcome::SceneComplete)
            }
            NextAction::SessionComplete => {
                self.active_scene = None;
                self.active_scene_name = None;
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
                self.active_scene_name = Some(scene_name.clone());
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
        // compile_clip は `&Clock` を取るため、共有 Clock のスナップショットを
        // ローカルに保持して借用する。compile 中の tempo 変更は反映しない
        // (clip コンパイル単位での一貫性を優先)。
        //
        // Take a Clock snapshot for `compile_clip`, which expects `&Clock`.
        // Tempo changes during compilation are deliberately ignored to keep
        // each clip compile atomic.
        let clock_snap = self.clock_snapshot();
        for clip_name in &instance.clips {
            let clip_def = self
                .registry
                .get_clip(clip_name)
                .ok_or_else(|| EngineError::UnknownClip(clip_name.clone()))?;
            let compiled = compile_clip(clip_def, &clock_snap, &self.registry)?;
            // Phase 3 では scene 内の全 clip を looping=true として扱う
            // Phase 3 treats all clips in a scene as looping=true
            player.add_clip(clip_name.clone(), compiled, true);
        }

        // §8.6: scene 定義側で `mute` 前置されたエントリの clip を初期 mute 状態でロードする。
        // 該当 clip がこのループの resolve_scene で選ばれていない場合（確率落ち等）は no-op。
        //
        // §8.6: apply initial mute for clips whose scene entry was prefixed with `mute`.
        // If a muted-marked clip wasn't picked by `resolve_scene` this loop (e.g.
        // dropped by probability), the call is a no-op.
        for muted_clip in initial_muted_clips(scene_def) {
            player.mute_clip(&muted_clip);
        }

        Ok(player)
    }

    /// `PrecompileMode` を尊重して clip を compile する。
    ///
    /// - `Replay`: 記録済み成果物があれば compile を省略して clone を返す。
    ///   ミス時は通常 compile にフォールバックする。
    /// - `Record`: 通常 compile した結果を名前キーで記録してから返す。
    /// - `Off`: 通常 compile (既存挙動)。
    ///
    /// 呼び出し元 (`Block::Clip` 差し替えパス) が直前に同名 clip を registry へ
    /// 登録しているため、registry からの取得は `expect` で十分。
    ///
    /// Compiles a clip honoring `PrecompileMode`. `Replay` reuses a recorded
    /// artifact (falling back to a real compile on miss); `Record` records the
    /// freshly compiled artifact keyed by name; `Off` compiles normally.
    fn compile_clip_cached(&mut self, name: &str) -> Result<CompiledClip, EngineError> {
        if let PrecompileMode::Replay { clips, .. } = &self.precompile {
            if let Some(compiled) = clips.get(name) {
                return Ok(compiled.clone());
            }
        }
        let compiled = {
            let clock_snap = self.clock_snapshot();
            let clip_def = self
                .registry
                .get_clip(name)
                .expect("clip was just registered");
            compile_clip(clip_def, &clock_snap, &self.registry)?
        };
        if let PrecompileMode::Record { clips, .. } = &mut self.precompile {
            clips.insert(name.to_string(), compiled.clone());
        }
        Ok(compiled)
    }

    /// `PrecompileMode` を尊重して ScenePlayer を構築する。
    ///
    /// `compile_clip_cached` の ScenePlayer 版。`Replay` は scene 名キーで記録済み
    /// プレイヤーを clone 再利用し、`Record` は構築結果を記録する。`Off` は
    /// `build_scene_player` をそのまま呼ぶ。
    ///
    /// ScenePlayer counterpart of `compile_clip_cached`: `Replay` reuses a recorded
    /// player by scene name, `Record` records the built one, `Off` builds directly.
    fn build_scene_player_cached(
        &mut self,
        scene_name: &str,
        scene_def: &SceneDef,
    ) -> Result<ScenePlayer, EngineError> {
        if let PrecompileMode::Replay { players, .. } = &self.precompile {
            if let Some(player) = players.get(scene_name) {
                return Ok(player.clone());
            }
        }
        let player = self.build_scene_player(scene_def)?;
        if let PrecompileMode::Record { players, .. } = &mut self.precompile {
            players.insert(scene_name.to_string(), player.clone());
        }
        Ok(player)
    }

    /// live `Evaluator` から prepare 用の不変スナップショットを取得する。
    ///
    /// 呼び出し側は live ロックを**短時間**だけ握ってこれを取得し、ロックを解放
    /// してから `prepare_*` (重い parse+compile) を実行する。`StateManager` は
    /// 非 Clone かつ prepare に不要なので含めない。
    ///
    /// Captures an immutable snapshot for the off-lock prepare. Callers hold the
    /// live lock only briefly to take this, release it, then run `prepare_*`.
    pub fn snapshot_for_prepare(&self) -> EvalSnapshot {
        EvalSnapshot {
            registry: self.registry.clone(),
            scope: self.scope.clone(),
            clock: self.clock_snapshot(),
            active_scene: self.active_scene.clone(),
            active_scene_name: self.active_scene_name.clone(),
        }
    }

    /// スナップショットから使い捨て (throwaway) Evaluator を構築する。
    ///
    /// prepare 専用。live と state を共有せず (`StateManager::new`)、`device_event_tx`
    /// も持たない (prepare 中に DeviceEvent を発火させない)。`clock` はスナップショット
    /// 値を新しい `Arc<RwLock<_>>` で包み直す。
    ///
    /// Builds a throwaway Evaluator from a snapshot for prepare only: no shared
    /// state, no `device_event_tx` (prepare must not emit DeviceEvents), and a fresh
    /// `Arc<RwLock<Clock>>` wrapping the snapshot value.
    fn from_snapshot(snapshot: EvalSnapshot, precompile: PrecompileMode) -> Self {
        Self {
            registry: snapshot.registry,
            state: StateManager::new(),
            clock: Arc::new(RwLock::new(snapshot.clock)),
            scope: snapshot.scope,
            active_scene: snapshot.active_scene,
            active_scene_name: snapshot.active_scene_name,
            pending_all_notes_off: Vec::new(),
            pending_transport: Vec::new(),
            device_event_tx: None,
            device_connection_errors: HashMap::new(),
            force_scene_advance_on_grid: false,
            precompile,
        }
    }

    /// `PreparedProgram` を live state へ適用する (ロック内・短時間)。
    ///
    /// `Replay` モードで `blocks` を `eval_block` 評価する。compile は prepare 時の
    /// 成果物 (`clips`/`players`) から再利用されるため、ロック保持時間が compile
    /// コストに依存しない。評価終了後は必ず `Off` に戻す。
    ///
    /// Applies a `PreparedProgram` to live state under a short lock. Evaluates
    /// `blocks` in `Replay` mode so compile is reused from prepare-time artifacts;
    /// lock-hold time is independent of compile cost. The mode is always reset to
    /// `Off` afterward.
    pub fn apply_prepared(
        &mut self,
        prepared: PreparedProgram,
    ) -> Result<Vec<EvalResult>, EngineError> {
        let PreparedProgram {
            blocks,
            clips,
            players,
        } = prepared;
        self.precompile = PrecompileMode::Replay { clips, players };
        let mut results = Vec::with_capacity(blocks.len());
        for block in blocks {
            match self.eval_block(block) {
                Ok(r) => results.push(r),
                Err(e) => {
                    self.precompile = PrecompileMode::Off;
                    return Err(e);
                }
            }
        }
        self.precompile = PrecompileMode::Off;
        Ok(results)
    }

    /// 単一ブロックを評価
    pub fn eval_block(&mut self, block: Block) -> Result<EvalResult, EngineError> {
        match block {
            Block::Device(ref d) => {
                let name = d.name.clone();
                let port = d.port.clone();
                Arc::make_mut(&mut self.registry).register_block(block);
                // tx が設定されていれば DeviceEvent::Upsert を通知する。
                // 受信側が drop されている場合の SendError は意図的に握り潰す
                // （LSP テストのノイズ抑制および後方互換のため）。
                //
                // Notify `DeviceEvent::Upsert` if a tx is registered. Any
                // `SendError` from a dropped receiver is intentionally
                // ignored to keep eval quiet and backward compatible.
                if let Some(tx) = &self.device_event_tx {
                    let _ = tx.send(DeviceEvent::Upsert {
                        name: name.clone(),
                        port,
                    });
                }
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
                Arc::make_mut(&mut self.registry).register_block(Block::Instrument(inst));
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
                Arc::make_mut(&mut self.registry).register_block(Block::Kit(kit));
                Ok(EvalResult::Registered {
                    kind: "Kit".into(),
                    name,
                })
            }
            Block::Clip(ref c) => {
                let name = c.name.clone();
                Arc::make_mut(&mut self.registry).register_block(block);

                // §7/§12: 再生中の scene が同名 clip を使用している場合、新定義を
                // コンパイルして差し替え待機 (pending) に積む。実際の swap は
                // PlaybackDriver が 4 小節グリッド境界で commit_pending_clips を
                // 呼んだ時点で一斉に適用される。コンパイル失敗時は stage せず
                // Err を返すため、再生中の旧 clip はそのまま鳴り続ける。
                //
                // §7/§12: if the playing scene uses a clip with this name,
                // compile the new definition and stage it as a pending swap.
                // The actual swap is applied when the PlaybackDriver commits on
                // the 4-bar grid. On compile error nothing is staged, so the
                // currently playing clip keeps sounding unchanged.
                let in_use = self
                    .active_scene
                    .as_ref()
                    .is_some_and(|scene| scene.has_clip(&name));
                if in_use {
                    let compiled = self.compile_clip_cached(&name)?;
                    if let Some(scene) = self.active_scene.as_mut() {
                        scene.replace_clip(&name, compiled);
                    }
                    // §12: session 再生中なら、この clip を使う scene を LCM 境界まで
                    // 待たず4小節グリッドで次エントリ（別 scene）へ進めるよう要求する。
                    // §12: if a session is playing, request a forced jump to the next
                    // entry (a different scene) on the 4-bar grid instead of waiting
                    // for this scene's LCM boundary.
                    self.request_force_scene_advance_if_session();
                }

                Ok(EvalResult::Registered {
                    kind: "Clip".into(),
                    name,
                })
            }
            Block::Scene(ref s) => {
                let name = s.name.clone();
                // §12: session 再生中に、いま再生中の scene の構成が上書きされた場合、
                // その scene を LCM 境界まで待たず4小節グリッドで次エントリ（別 scene）へ
                // 進めるよう要求する。scene の構成変更は再生中 scene へ即時反映する配線が
                // 無いため、session 文脈では「次 scene へ早送り」で新構成へ移行させる。
                //
                // §12: if the scene whose composition was just overwritten is the one
                // currently playing under a session, request a forced jump to the next
                // entry on the 4-bar grid. There is no wiring to hot-apply a scene
                // composition change to the playing scene, so under a session we move
                // on to the next scene to reach the new arrangement.
                let is_active_scene = self.active_scene_name.as_deref() == Some(name.as_str());
                Arc::make_mut(&mut self.registry).register_block(block);
                if is_active_scene {
                    self.request_force_scene_advance_if_session();
                }
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
                // §12: いま再生中の session 定義が上書きされた場合、次エントリへの
                // 切替（pending_session の差し替え commit を含む）を LCM 境界まで
                // 待たず4小節グリッドで前倒しする。pending_session の runner 差し替え
                // タイミング自体はエントリ境界 commit のまま（force 遷移がその境界を
                // 早く作る）。
                //
                // §12: if the currently playing session definition was overwritten,
                // bring forward the transition to the next entry (including the
                // pending_session swap commit) onto the 4-bar grid instead of the LCM
                // boundary. The pending_session runner-swap timing itself stays at the
                // entry boundary; the forced transition just reaches that boundary
                // sooner.
                if self.is_playing_session_named(&name) {
                    self.force_scene_advance_on_grid = true;
                }
                Arc::make_mut(&mut self.registry).register_block(block);
                Ok(EvalResult::Registered {
                    kind: "Session".into(),
                    name,
                })
            }
            Block::Tempo(ref t) => {
                // 共有 Clock を更新する。RwLock の poisoning は
                // 構造的に発生しない (apply_tempo / bpm のいずれも
                // panic しない) ため `expect` で十分。
                //
                // Updates the shared clock. Poisoning cannot happen in
                // practice because neither `apply_tempo` nor `bpm` can
                // panic, so `expect` suffices.
                let new_bpm = {
                    let mut clock = self.clock.write().expect("clock RwLock poisoned");
                    clock.apply_tempo(t);
                    clock.bpm()
                };
                Arc::make_mut(&mut self.registry).register_block(block);
                Ok(EvalResult::TempoChanged(new_bpm))
            }
            Block::Scale(_) => {
                Arc::make_mut(&mut self.registry).register_block(block);
                Ok(EvalResult::ScaleChanged)
            }
            Block::Var(ref v) => {
                let name = v.name.clone();
                // グローバルスコープに変数を定義（§6 変数）
                // Define variable in global scope (§6 variables)
                self.scope.define_global(v.name.clone(), v.value.clone());
                Arc::make_mut(&mut self.registry).register_block(block);
                Ok(EvalResult::VarDefined { name })
            }
            Block::Play(cmd) => {
                // Issue #50: play 実行時に `transport = true` の device に
                // MIDI System Real-Time Start (0xFA) を送出キューへ積む。
                // scene / session 成功パスで共通なので先にキュー投入する。
                // Issue #50: queue MIDI Start (0xFA) for every device whose
                // `transport` flag is true. Both scene and session targets share
                // this path on success, so enqueue before dispatching.
                let transport_targets = self.transport_enabled_devices();
                match cmd.target {
                    PlayTarget::Scene(name) => {
                        // Phase 3: scene 定義を取り出して ScenePlayer を構築する
                        // Phase 3: resolve the scene definition and build a ScenePlayer
                        let scene_def = self
                            .registry
                            .get_scene(&name)
                            .ok_or_else(|| EngineError::UnknownScene(name.clone()))?
                            .clone();
                        let player = self.build_scene_player_cached(&name, &scene_def)?;
                        self.active_scene = Some(player);
                        self.active_scene_name = Some(name.clone());
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
                                    let player =
                                        self.build_scene_player_cached(&first.scene, &scene_def)?;
                                    self.active_scene = Some(player);
                                    self.active_scene_name = Some(first.scene.clone());
                                } else {
                                    self.active_scene = None;
                                    self.active_scene_name = None;
                                }
                                self.state.apply_play_session(&def, cmd.repeat);
                            }
                            None => return Err(EngineError::UnknownSession(name)),
                        }
                    }
                }
                // エラーで早期 return していない = Play 成功確定。transport
                // 対象 device に Start (0xFA) を enqueue する。
                // Play succeeded (no early return). Enqueue Start (0xFA) for
                // transport-enabled devices.
                for device in transport_targets {
                    self.pending_transport
                        .push((device, crate::midi::message::MidiMessage::Start));
                }
                Ok(EvalResult::PlayStarted)
            }
            Block::Stop(cmd) => {
                // `stop` / `stop <scene>` / `stop <session>` の 3 形式のみ扱う。
                // target が clip 名の場合は §10.4 の `mute <clip>` に移行済みのため、
                // ここでは scene/session 名に一致しない名前は state 委譲で no-op となる。
                //
                // Handles `stop`, `stop <scene>`, and `stop <session>`. Clip targets
                // were moved to `mute <clip>` (§10.4); unknown names therefore become
                // no-ops via the state manager.
                //
                // Issue #50: どの形式でも `transport = true` の device には MIDI
                // System Real-Time Stop (0xFC) を送出キューへ積む。
                // Issue #50: queue MIDI Stop (0xFC) for transport-enabled devices
                // on every stop variant.
                let transport_targets = self.transport_enabled_devices();
                for device in transport_targets {
                    self.pending_transport
                        .push((device, crate::midi::message::MidiMessage::Stop));
                }
                match &cmd.target {
                    None => {
                        if let Some(scene) = &self.active_scene {
                            self.pending_all_notes_off.extend(scene.channels_in_use());
                        }
                        self.state
                            .apply_command(PlaybackCommand::Stop { target: None });
                        self.active_scene = None;
                        self.active_scene_name = None;
                    }
                    Some(name) => {
                        let is_current = self
                            .state
                            .current_scene_name()
                            .map(|n| n == name)
                            .unwrap_or(false);
                        if is_current {
                            if let Some(scene) = &self.active_scene {
                                self.pending_all_notes_off.extend(scene.channels_in_use());
                            }
                            self.state.apply_command(PlaybackCommand::Stop {
                                target: Some(name.clone()),
                            });
                            self.active_scene = None;
                            self.active_scene_name = None;
                        } else {
                            // scene/session 名に一致しない target は no-op。
                            // Target does not match the current scene/session → no-op.
                            self.state.apply_command(PlaybackCommand::Stop {
                                target: Some(name.clone()),
                            });
                        }
                    }
                }
                Ok(EvalResult::Stopped)
            }
            Block::Include(ref inc) => Ok(EvalResult::IncludeProcessed {
                path: inc.path.clone(),
                results_count: 0,
            }),
            // Phase 3 で実装予定のスタブ（§10.4 pause/resume）
            // Stub for pause/resume — full implementation comes in Phase 3 (§10.4)
            Block::Pause(cmd) => self.eval_pause(cmd),
            Block::Resume(cmd) => self.eval_resume(cmd),
            Block::Mute(cmd) => self.eval_mute(cmd),
            Block::Unmute(cmd) => self.eval_unmute(cmd),
        }
    }

    /// `pause` コマンドを評価する（§10.4）
    ///
    /// target の種類によって処理を分岐する：
    /// * `None`: 全体 pause。再生中なら `PlaybackState::Paused` に遷移し、
    ///   active_scene の全 clip を pause する。使用中チャンネル分の AllNotesOff を蓄積。
    /// * `Some(name)`: name の種類を以下の優先順位で判定：
    ///   1. 現在再生中の scene/session 名と一致 → 全体 pause と同等
    ///   2. active_scene に該当 clip がある → clip 単位 pause（該当 ch の AllNotesOff）
    ///   3. いずれでもない → no-op（EvalResult::PausedNoop）
    ///
    /// 名前不一致時は `EvalResult::PausedNoop { reason }` を返し、再生は継続する
    /// （§11 音は絶対に止めない）。
    ///
    /// # 引数 / Arguments
    /// * `cmd` - PauseCommand（target = None で全体、Some で名前指定）
    ///
    /// # 戻り値 / Returns
    /// 成功時 `EvalResult::Paused`、no-op 時 `EvalResult::PausedNoop`。
    fn eval_pause(
        &mut self,
        cmd: crate::ast::playback::PauseCommand,
    ) -> Result<EvalResult, EngineError> {
        match &cmd.target {
            None => {
                // 全体 pause。Stopped なら no-op。
                // Full pause. No-op when stopped.
                let is_playing = matches!(
                    self.state.state(),
                    crate::engine::state::PlaybackState::PlayingScene { .. }
                        | crate::engine::state::PlaybackState::PlayingSession { .. }
                );
                if !is_playing {
                    return Ok(EvalResult::PausedNoop {
                        reason: "nothing is playing".to_string(),
                    });
                }
                // AllNotesOff 蓄積 + active_scene 全 clip を pause
                // Queue AllNotesOff and pause every clip in active_scene
                if let Some(scene) = self.active_scene.as_mut() {
                    self.pending_all_notes_off.extend(scene.channels_in_use());
                    scene.pause_all_clips();
                }
                self.state
                    .apply_command(PlaybackCommand::Pause { target: None });
                Ok(EvalResult::Paused { target: None })
            }
            Some(name) => {
                // 現在再生中の scene/session 名と一致 → 全体 pause と同等
                // Name matches the currently playing scene/session → full pause
                let is_current = self
                    .state
                    .current_scene_name()
                    .map(|n| n == name)
                    .unwrap_or(false);
                let is_playing = matches!(
                    self.state.state(),
                    crate::engine::state::PlaybackState::PlayingScene { .. }
                        | crate::engine::state::PlaybackState::PlayingSession { .. }
                );
                if is_current && is_playing {
                    if let Some(scene) = self.active_scene.as_mut() {
                        self.pending_all_notes_off.extend(scene.channels_in_use());
                        scene.pause_all_clips();
                    }
                    self.state.apply_command(PlaybackCommand::Pause {
                        target: Some(name.clone()),
                    });
                    return Ok(EvalResult::Paused {
                        target: Some(name.clone()),
                    });
                }
                // active_scene に該当 clip があれば clip 単位 pause
                // If the active_scene has the named clip, pause it
                if let Some(scene) = self.active_scene.as_mut() {
                    if scene.has_clip(name) {
                        let channels = scene.channels_of_clip(name);
                        scene.pause_clip(name);
                        self.pending_all_notes_off.extend(channels);
                        return Ok(EvalResult::Paused {
                            target: Some(name.clone()),
                        });
                    }
                }
                // どれにも該当しない → no-op
                // No target matched → no-op
                Ok(EvalResult::PausedNoop {
                    reason: format!(
                        "'{}' is not the current scene/session nor a clip in active scene",
                        name
                    ),
                })
            }
        }
    }

    /// `resume` コマンドを評価する（§10.4）
    ///
    /// target の種類によって処理を分岐する：
    /// * `None`: 全体 resume。Paused なら prev に復元し、active_scene の全 clip の
    ///   pause を解除する。ただし `pause <clip>` された clip は個別に解除する必要があるため、
    ///   ここでは全 clip の pause を解除するが D5 の通り全体 pause の対称操作として扱う
    ///   （= 個別 pause された clip も同時に resume される実装）。
    /// * `Some(name)`: name の種類を以下の優先順位で判定：
    ///   1. Paused の prev scene/session 名と一致 → 全体 resume
    ///   2. active_scene に該当 clip がある → clip 単位 resume
    ///   3. いずれでもない → no-op
    ///
    /// 名前不一致時は `EvalResult::ResumedNoop { reason }` を返す。
    ///
    /// # 引数 / Arguments
    /// * `cmd` - ResumeCommand（target = None で全体、Some で名前指定）
    ///
    /// # 戻り値 / Returns
    /// 成功時 `EvalResult::Resumed`、no-op 時 `EvalResult::ResumedNoop`。
    fn eval_resume(
        &mut self,
        cmd: crate::ast::playback::ResumeCommand,
    ) -> Result<EvalResult, EngineError> {
        match &cmd.target {
            None => {
                // 全体 resume。Paused でなければ no-op。
                // Full resume. No-op when not paused.
                if !self.state.is_paused() {
                    return Ok(EvalResult::ResumedNoop {
                        reason: "not paused".to_string(),
                    });
                }
                // active_scene の全 clip を resume し、state を復元
                // Resume every clip in active_scene and restore the state
                if let Some(scene) = self.active_scene.as_mut() {
                    scene.resume_all_clips();
                }
                self.state
                    .apply_command(PlaybackCommand::Resume { target: None });
                Ok(EvalResult::Resumed { target: None })
            }
            Some(name) => {
                // Paused かつ prev の名前と一致 → 全体 resume
                // If paused and prev name matches → full resume
                if self.state.is_paused() {
                    let is_prev = self
                        .state
                        .current_scene_name()
                        .map(|n| n == name)
                        .unwrap_or(false);
                    if is_prev {
                        if let Some(scene) = self.active_scene.as_mut() {
                            scene.resume_all_clips();
                        }
                        self.state.apply_command(PlaybackCommand::Resume {
                            target: Some(name.clone()),
                        });
                        return Ok(EvalResult::Resumed {
                            target: Some(name.clone()),
                        });
                    }
                }
                // active_scene に該当 clip があれば clip 単位 resume
                // If the active_scene has the named clip, resume it
                if let Some(scene) = self.active_scene.as_mut() {
                    if scene.has_clip(name) {
                        scene.resume_clip(name);
                        return Ok(EvalResult::Resumed {
                            target: Some(name.clone()),
                        });
                    }
                }
                // どれにも該当しない → no-op
                // No target matched → no-op
                Ok(EvalResult::ResumedNoop {
                    reason: format!(
                        "'{}' is not a paused scene/session nor a clip in active scene",
                        name
                    ),
                })
            }
        }
    }

    /// `mute <clip>` コマンドを評価する（§10.4）
    ///
    /// `active_scene` に `cmd.target` と一致する clip が存在すれば、その clip を
    /// mute する（tick は継続・位相維持、発音停止、該当チャンネルの AllNotesOff を蓄積）。
    /// `active_scene` が無い、または clip 名が見つからない場合は `MutedNoop` を返す。
    ///
    /// Evaluates `mute <clip>`. When `active_scene` holds a clip matching
    /// `cmd.target`, it is muted (tick continues, phase preserved, note output
    /// stops, and AllNotesOff is queued for affected channels). If there is no
    /// active scene or the clip is not found, `MutedNoop` is returned.
    ///
    /// # 引数 / Arguments
    /// * `cmd` - MuteCommand（target = 対象 clip 名）
    ///
    /// # 戻り値 / Returns
    /// 成功時 `EvalResult::Muted`、no-op 時 `EvalResult::MutedNoop`。
    fn eval_mute(
        &mut self,
        cmd: crate::ast::playback::MuteCommand,
    ) -> Result<EvalResult, EngineError> {
        let name = cmd.target;
        if let Some(scene) = self.active_scene.as_mut() {
            if scene.has_clip(&name) {
                let channels = scene.channels_of_clip(&name);
                scene.mute_clip(&name);
                self.pending_all_notes_off.extend(channels);
                return Ok(EvalResult::Muted { target: name });
            }
        }
        Ok(EvalResult::MutedNoop {
            reason: format!("'{}' is not a clip in active scene", name),
        })
    }

    /// `unmute <clip>` コマンドを評価する（§10.4）
    ///
    /// `active_scene` に `cmd.target` と一致する clip が存在すれば、ミュートを解除する。
    /// `active_scene` が無い、または clip 名が見つからない場合は `UnmutedNoop` を返す。
    /// 既にミュートされていない clip に対する `unmute` は成功扱い（べき等）。
    ///
    /// Evaluates `unmute <clip>`. When `active_scene` holds a matching clip,
    /// its mute flag is released. If there is no active scene or the clip is
    /// absent, `UnmutedNoop` is returned. Unmuting a non-muted clip succeeds
    /// (idempotent behavior).
    ///
    /// # 引数 / Arguments
    /// * `cmd` - UnmuteCommand（target = 対象 clip 名）
    ///
    /// # 戻り値 / Returns
    /// 成功時 `EvalResult::Unmuted`、no-op 時 `EvalResult::UnmutedNoop`。
    fn eval_unmute(
        &mut self,
        cmd: crate::ast::playback::UnmuteCommand,
    ) -> Result<EvalResult, EngineError> {
        let name = cmd.target;
        if let Some(scene) = self.active_scene.as_mut() {
            if scene.has_clip(&name) {
                scene.unmute_clip(&name);
                return Ok(EvalResult::Unmuted { target: name });
            }
        }
        Ok(EvalResult::UnmutedNoop {
            reason: format!("'{}' is not a clip in active scene", name),
        })
    }

    /// Registry参照
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Registry の共有スナップショットを返す（`Arc::clone` = 参照カウント増のみ）
    ///
    /// P1: LSP（補完 / 診断 / hover）が Evaluator ロック内で `Registry` を deep clone
    /// する代わりに、本メソッドで `Arc` を clone して即ロックを解放できる。deep clone
    /// （8 個の HashMap の複製）がロック外に出るため、演奏中の打鍵が再生スレッドを
    /// ブロックする時間が激減する。
    ///
    /// Returns a shared snapshot of the registry via `Arc::clone` (refcount bump
    /// only). Lets the LSP release the Evaluator lock immediately instead of
    /// deep-cloning the registry inside the lock (P1 lock-contention fix).
    pub fn registry_snapshot(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    /// 共有 Clock のハンドル (Arc clone)
    ///
    /// `PlaybackDriver` など、Evaluator のロックを介さずに最新のテンポ・PPQ を
    /// 参照したい呼び出し側に渡すためのハンドル。受け取った側は `read()` /
    /// `write()` で内部 `Clock` にアクセスする。
    ///
    /// Returns a cloned `Arc<RwLock<Clock>>` handle so that callers such as
    /// `PlaybackDriver` can observe the latest tempo/PPQ without holding the
    /// `Evaluator` lock.
    pub fn clock_handle(&self) -> Arc<RwLock<Clock>> {
        Arc::clone(&self.clock)
    }

    /// 現在の Clock のスナップショット (値コピー)
    ///
    /// `compile_clip` のようにロックを跨いで `Clock` を渡したい場面向け。
    /// 戻り値は呼び出し時点の独立コピーで、以降の `tempo` 更新は反映されない。
    ///
    /// Returns an independent snapshot of the current `Clock`. Useful for code
    /// paths like `compile_clip` that take `&Clock` by reference and should
    /// not see subsequent tempo updates.
    pub fn clock_snapshot(&self) -> Clock {
        self.clock.read().expect("clock RwLock poisoned").clone()
    }

    /// State参照
    pub fn state(&self) -> &StateManager {
        &self.state
    }

    /// 現在のBPM
    pub fn bpm(&self) -> f64 {
        self.clock.read().expect("clock RwLock poisoned").bpm()
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

    /// `eval_source_preload` を `DeviceEvent` 非送出モードで実行する。
    ///
    /// LSP の preload 経路（diagnostics / completion 等のソース解析）は
    /// 副作用のない静的解析を意図しており、`Block::Device` 評価時に
    /// `DeviceEvent::Upsert` を receiver 側へ発火させたくない。本メソッドは
    /// 評価の間だけ `device_event_tx` を一時退避し、終了後に元に戻す事で
    /// silent な preload 評価を提供する（PR #83）。
    ///
    /// Runs `eval_source_preload` with `DeviceEvent` emission suppressed.
    /// The LSP preload path performs side-effect-free static analysis,
    /// so evaluating `Block::Device` should not push `DeviceEvent::Upsert`
    /// through to the receiver. This helper temporarily takes the
    /// `device_event_tx`, runs the preload, and restores it afterwards.
    ///
    /// # Arguments
    /// * `source` - 評価する DSL ソース / DSL source to evaluate
    ///
    /// # Returns
    /// preload 評価結果（play/stop はスキップ済み）
    ///
    /// # Errors
    /// - `EngineError::ParseError` - パースエラー / Parse error
    fn eval_source_preload_silent_devices(
        &mut self,
        source: &str,
    ) -> Result<Vec<EvalResult>, EngineError> {
        // device_event_tx を一時退避して silent な評価を行う。
        // panic 発生時には Mutex が poisoned になる前提のため、手動 restore
        // で十分（drop guard で `&mut self` を保持できないため）。
        //
        // Temporarily take the tx so evaluation stays silent. A panic
        // poisons the wrapping mutex anyway, so manual restore (no drop
        // guard, which can't co-exist with `&mut self`) is sufficient.
        let saved_tx = self.device_event_tx.take();
        let result = self.eval_source_preload(source);
        self.device_event_tx = saved_tx;
        result
    }

    /// registryが空の場合にソースからregistryを自動構築する
    /// Auto-populates registry from source when registry is empty
    ///
    /// LSP 解析経路から呼ばれるため、内部では `eval_source_preload_silent_devices`
    /// を用いて `DeviceEvent::Upsert` の発火を抑制する。明示的に device 接続
    /// を起動させたい場合は呼び出し側で `eval_source_preload` を直接使うこと
    /// (`Request::Preload` ハンドラ参照)。
    ///
    /// Called from LSP analysis paths, so it uses
    /// `eval_source_preload_silent_devices` internally to suppress
    /// `DeviceEvent::Upsert`. Callers that want device connections must use
    /// `eval_source_preload` directly (see the `Request::Preload` handler).
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
        // メインソースをプリロード評価（DeviceEvent は抑制）
        // Preload-evaluate main source (suppressing DeviceEvent)
        if self.eval_source_preload_silent_devices(source).is_err() {
            return false;
        }
        // 追加ソース（include分）をプリロード評価（DeviceEvent は抑制）
        // Preload-evaluate additional sources from includes (suppressing DeviceEvent)
        for additional in additional_sources {
            if self.eval_source_preload_silent_devices(additional).is_err() {
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
    use crate::domain::channel::MidiChannel;
    use crate::domain::pitch::NoteName;
    use crate::engine::state::PlaybackState;
    use crate::parser::clip_options::ClipOptions;

    #[test]
    fn eval_device_registered() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Device(DeviceDef {
                name: "synth".into(),
                port: "IAC Bus 1".into(),
                transport: true,
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
                channel: MidiChannel::from_one_based(1).unwrap(),
                note: None,
                gate_normal: None,
                gate_staccato: None,
                velocity_normal: None,
                velocity_accent: None,
                velocity_ghost: None,
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
        assert_eq!(inst.channel, MidiChannel::from_one_based(1).unwrap());
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

    /// PR #57: `clock_handle()` で取得した Arc が tempo 評価後の最新値を返す
    /// ことを検証する。driver 側はこの Arc を保持して毎 tick 読むため、
    /// `apply_tempo` 後の `tick_duration_us` が反映されることが必須要件。
    ///
    /// PR #57: ensures the `Arc<RwLock<Clock>>` returned by `clock_handle()`
    /// reflects the latest BPM after a `tempo` block is evaluated. The
    /// playback driver relies on this for tempo propagation.
    #[test]
    fn clock_handle_reflects_tempo_change() {
        let mut ev = Evaluator::new(120.0);
        let handle = ev.clock_handle();
        // 評価前
        assert!((handle.read().unwrap().bpm() - 120.0).abs() < f64::EPSILON);

        ev.eval_block(Block::Tempo(Tempo::Absolute(180))).unwrap();

        // 評価後、同じ Arc から見ても新 BPM
        assert!((handle.read().unwrap().bpm() - 180.0).abs() < f64::EPSILON);
        // tick_duration_us も BPM が大きいほど短くなる
        let new_dur = handle.read().unwrap().tick_duration_us();
        let baseline = Clock::new(120.0).tick_duration_us();
        assert!(
            new_dur < baseline,
            "tempo 上昇後の tick_duration_us が短くなっていない: new={}, baseline={}",
            new_dur,
            baseline
        );
    }

    /// PR #57: `clock_snapshot()` は呼び出し時点の独立コピーを返し、
    /// その後の tempo 変更には追従しない。`compile_clip` 系のように
    /// `&Clock` を期待する API に渡すための値型 API。
    ///
    /// PR #57: `clock_snapshot()` returns an independent copy frozen at the
    /// call site so that consumers expecting `&Clock` (e.g. `compile_clip`)
    /// keep a stable view even if tempo changes mid-compile.
    #[test]
    fn clock_snapshot_is_independent_copy() {
        let mut ev = Evaluator::new(120.0);
        let snap = ev.clock_snapshot();
        assert!((snap.bpm() - 120.0).abs() < f64::EPSILON);

        ev.eval_block(Block::Tempo(Tempo::Absolute(200))).unwrap();

        // snap は古い値のまま
        assert!((snap.bpm() - 120.0).abs() < f64::EPSILON);
        // 新しく取り直すと最新値
        assert!((ev.clock_snapshot().bpm() - 200.0).abs() < f64::EPSILON);
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
            channel: MidiChannel::from_one_based(3).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
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
                muted: false,
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
                muted: false,
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
                muted: false,
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
                    muted: false,
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
                muted: false,
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

    // --- scene 内 tempo 行のループ境界 apply (issue: tempo +n が効かない) ---

    /// scene 内 `tempo +5` をループ境界で apply → 累積する
    #[test]
    fn on_scene_loop_complete_applies_relative_tempo_cumulatively() {
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
            name: "buildup".into(),
            entries: vec![
                crate::ast::scene::SceneEntry::Clip {
                    candidates: vec![crate::ast::scene::ShuffleCandidate {
                        clip: "a".into(),
                        weight: 1,
                    }],
                    probability: None,
                    muted: false,
                },
                crate::ast::scene::SceneEntry::Tempo(Tempo::Relative(5)),
            ],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("buildup".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        // activate 直後は 120 のまま (最初のループ完了境界で初めて apply)
        assert!((ev.bpm() - 120.0).abs() < f64::EPSILON);

        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 125.0).abs() < f64::EPSILON);

        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 130.0).abs() < f64::EPSILON);

        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 135.0).abs() < f64::EPSILON);
    }

    /// scene 内 `tempo 120` (絶対値) → ループ境界で毎回 120 にセット
    #[test]
    fn on_scene_loop_complete_applies_absolute_tempo_each_loop() {
        let mut ev = Evaluator::new(140.0);
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
            name: "drop".into(),
            entries: vec![
                crate::ast::scene::SceneEntry::Clip {
                    candidates: vec![crate::ast::scene::ShuffleCandidate {
                        clip: "a".into(),
                        weight: 1,
                    }],
                    probability: None,
                    muted: false,
                },
                crate::ast::scene::SceneEntry::Tempo(Tempo::Absolute(120)),
            ],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("drop".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        // activate 時は 140 のまま、最初のループ完了境界で 120 にセット
        assert!((ev.bpm() - 140.0).abs() < f64::EPSILON);
        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 120.0).abs() < f64::EPSILON);
        // 2 回目も冪等に 120
        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 120.0).abs() < f64::EPSILON);
    }

    /// トップレベル `tempo 140` 動的 eval が起点を変える → 次ループは 140 + relative
    #[test]
    fn on_scene_loop_complete_relative_uses_dynamic_tempo_as_base() {
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
            name: "buildup".into(),
            entries: vec![
                crate::ast::scene::SceneEntry::Clip {
                    candidates: vec![crate::ast::scene::ShuffleCandidate {
                        clip: "a".into(),
                        weight: 1,
                    }],
                    probability: None,
                    muted: false,
                },
                crate::ast::scene::SceneEntry::Tempo(Tempo::Relative(5)),
            ],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("buildup".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        // 1 ループ完了 → 125
        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 125.0).abs() < f64::EPSILON);

        // トップレベル tempo 140 で起点を切り替え
        ev.eval_block(Block::Tempo(Tempo::Absolute(140))).unwrap();
        assert!((ev.bpm() - 140.0).abs() < f64::EPSILON);

        // 次のループ完了 → 140 + 5 = 145
        ev.on_scene_loop_complete().unwrap();
        assert!((ev.bpm() - 145.0).abs() < f64::EPSILON);
    }

    /// tempo 行を含まない scene → bpm 不変
    #[test]
    fn on_scene_loop_complete_no_tempo_entry_keeps_bpm() {
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
                muted: false,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        for _ in 0..5 {
            ev.on_scene_loop_complete().unwrap();
            assert!((ev.bpm() - 120.0).abs() < f64::EPSILON);
        }
    }

    /// session 内 scene 遷移時の tempo 行の作用範囲を検証する。
    /// ループ完了境界は「今 1 ループ終えた scene」の tempo 行を apply する。
    /// 次 scene へ遷移する境界もこれは同様。s2 自身は tempo 行を持たないので、
    /// s2 が active になった後の呼び出しでは bpm は変わらない。
    #[test]
    fn on_scene_loop_complete_session_transition_applies_prev_scene_tempo_only() {
        let mut ev = Evaluator::new(120.0);
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
        // s1 は tempo +5 を持つ、s2 は tempo 行なし
        ev.eval_block(Block::Scene(SceneDef {
            name: "s1".into(),
            entries: vec![
                crate::ast::scene::SceneEntry::Clip {
                    candidates: vec![crate::ast::scene::ShuffleCandidate {
                        clip: "a".into(),
                        weight: 1,
                    }],
                    probability: None,
                    muted: false,
                },
                crate::ast::scene::SceneEntry::Tempo(Tempo::Relative(5)),
            ],
        }))
        .unwrap();
        ev.eval_block(Block::Scene(SceneDef {
            name: "s2".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "b".into(),
                    weight: 1,
                }],
                probability: None,
                muted: false,
            }],
        }))
        .unwrap();
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

        // 1 回目: state は最初 entries[0]=s1 を返す (Phase 4 挙動)。
        // この境界で「直前 active=s1 のループ 1 周完了」として s1 の tempo +5 を apply (= 125)。
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(
            outcome,
            SceneTransitionOutcome::NextScene {
                scene_name: "s1".into()
            }
        );
        assert!((ev.bpm() - 125.0).abs() < f64::EPSILON);

        // 2 回目: s1 (Once) が終わって NextScene{s2} に遷移。
        // 直前 active=s1 のループ 2 周目完了 → さらに +5 apply (= 130)、
        // その後 active を s2 に切り替え。
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(
            outcome,
            SceneTransitionOutcome::NextScene {
                scene_name: "s2".into()
            }
        );
        assert!((ev.bpm() - 130.0).abs() < f64::EPSILON);

        // 3 回目: 直前 active=s2 (tempo 行なし) → bpm 不変、その後 SessionComplete。
        let outcome = ev.on_scene_loop_complete().unwrap();
        assert_eq!(outcome, SceneTransitionOutcome::SessionComplete);
        assert!((ev.bpm() - 130.0).abs() < f64::EPSILON);
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
                muted: false,
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

    // --- Phase 5: Stop の clip ミュート + AllNotesOff テスト ---

    /// テストソースを Evaluator に評価させるヘルパ
    /// Parses and evaluates a DSL source snippet on the given Evaluator.
    fn eval_src(ev: &mut Evaluator, src: &str) {
        let (rest, blocks) = crate::parser::parse_source(src).expect("parse");
        assert!(
            rest.trim().is_empty(),
            "parser left trailing input: {rest:?}"
        );
        for b in blocks {
            ev.eval_block(b).expect("eval");
        }
    }

    /// channel 指定の instrument + 単音 clip + 1-entry scene を構築する共通ソース
    /// Common DSL source producing one instrument on `channel`, one clip named
    /// `clip_name`, and one scene named `scene_name` referencing that clip.
    fn scene_setup_source(
        scene_name: &str,
        clip_name: &str,
        inst_name: &str,
        channel: u8,
    ) -> String {
        format!(
            "device dev {{ port test }}\n\
             instrument {inst} {{\n  device dev\n  channel {ch}\n}}\n\
             clip {clip} [bars 1] {{\n  {inst} c\n}}\n\
             scene {scene} {{ {clip} }}\n",
            inst = inst_name,
            ch = channel,
            clip = clip_name,
            scene = scene_name,
        )
    }

    /// clip/scene 1 件を事前登録した Evaluator を返す
    /// Build an Evaluator with one clip + scene + instrument pre-registered.
    fn setup_with_single_clip(clip_name: &str, scene_name: &str, channel: u8) -> Evaluator {
        let mut ev = Evaluator::new(120.0);
        let src = scene_setup_source(scene_name, clip_name, "inst", channel);
        eval_src(&mut ev, &src);
        ev
    }

    /// events 列から最初の NoteOn のノート番号を取り出す小ヘルパ
    /// Extract the note number of the first NoteOn in an event list.
    fn first_note_on(events: Vec<&crate::engine::compiler::MidiEvent>) -> Option<u8> {
        events.iter().find_map(|e| match e.message {
            crate::midi::message::MidiMessage::NoteOn { note, .. } => Some(note),
            _ => None,
        })
    }

    /// 再生中に同名 clip を eval すると pending swap が積まれ、commit_pending_clips
    /// （= 4 小節グリッド commit のエミュレート）で初めて新内容へ切り替わる。
    /// commit 前は旧 clip のまま鳴り続ける。
    ///
    /// Re-evaluating a clip while a scene plays stages a pending swap that only
    /// takes effect on commit_pending_clips (emulating the 4-bar grid commit).
    /// Before commit the old clip keeps sounding.
    #[test]
    fn reeval_clip_while_playing_stages_pending_and_commit_swaps() {
        let mut ev = setup_with_single_clip("c1", "s1", 1);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("s1".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        // 初期 clip (inst c) の NoteOn を取得
        let before = first_note_on(ev.active_scene().unwrap().events_at(0))
            .expect("initial NoteOn at tick 0");

        // 再生中に c1 を inst e で上書き → active_scene に pending として積まれる
        eval_src(&mut ev, "clip c1 [bars 1] {\n  inst e\n}\n");

        // commit 前は旧 clip (inst c) のまま
        let still = first_note_on(ev.active_scene().unwrap().events_at(0))
            .expect("NoteOn still present before commit");
        assert_eq!(still, before, "commit 前に切り替わってはいけない");

        // 4 小節グリッド commit を模す
        ev.active_scene_mut().unwrap().commit_pending_clips();

        // 新 clip (inst e = inst c の +4 半音) の小節頭が鳴る
        let after =
            first_note_on(ev.active_scene().unwrap().events_at(0)).expect("NoteOn after commit");
        assert_eq!(after, before + 4, "inst e は inst c の +4 半音であるべき");
    }

    /// 2 clip + 2 scene + 2-entry session（s1=a, s2=b）を構築して Play(Session) する
    /// Builds 2 clips + 2 scenes + a 2-entry session (s1=a, s2=b) and plays it.
    fn setup_playing_session_two_entries() -> Evaluator {
        let mut ev = Evaluator::new(120.0);
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
                    muted: false,
                }],
            }))
            .unwrap();
        }
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![
                crate::ast::session::SessionEntry {
                    scene: "s1".into(),
                    // Loop: 通常は s1 に留まり続ける → 強制遷移の効果が際立つ
                    repeat: crate::ast::session::SessionRepeat::Loop,
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
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev
    }

    /// §12: session 再生中に使用中 clip を上書きすると force フラグが立ち、
    /// try_force_advance_session_on_grid で次エントリ（別 scene）へ遷移する。
    /// Overwriting an in-use clip during session playback sets the force flag and
    /// try_force_advance_session_on_grid jumps to the next entry.
    #[test]
    fn reeval_clip_during_session_forces_scene_advance_on_grid() {
        let mut ev = setup_playing_session_two_entries();
        // Loop エントリ s1 を再生中。通常の grid commit では遷移しない（フラグ未設定）
        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::Continue
        );
        assert_eq!(ev.active_scene_name.as_deref(), Some("s1"));

        // s1 が使う clip a を上書き → force フラグが立つ
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();

        // grid 境界で次エントリ s2 へ強制遷移
        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::NextScene {
                scene_name: "s2".into()
            }
        );
        assert_eq!(ev.active_scene_name.as_deref(), Some("s2"));
    }

    /// §12: session 再生中に再生中 scene の構成を上書きすると force フラグが立ち、
    /// grid 境界で次エントリへ遷移する。
    /// Overwriting the playing scene's composition during a session sets the flag
    /// and advances on the grid.
    #[test]
    fn reeval_active_scene_during_session_forces_advance() {
        let mut ev = setup_playing_session_two_entries();
        // 再生中 scene s1 の構成を上書き（clip を b へ差し替え）
        ev.eval_block(Block::Scene(SceneDef {
            name: "s1".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "b".into(),
                    weight: 1,
                }],
                probability: None,
                muted: false,
            }],
        }))
        .unwrap();

        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::NextScene {
                scene_name: "s2".into()
            }
        );
    }

    /// §12: 再生中でない scene の構成を上書きしても force フラグは立たない。
    /// Overwriting a non-active scene during a session does not set the flag.
    #[test]
    fn reeval_inactive_scene_during_session_does_not_force() {
        let mut ev = setup_playing_session_two_entries();
        // 再生中でない s2 を上書き
        ev.eval_block(Block::Scene(SceneDef {
            name: "s2".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
                muted: false,
            }],
        }))
        .unwrap();
        // フラグが立っていないので grid commit は no-op
        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::Continue
        );
        assert_eq!(ev.active_scene_name.as_deref(), Some("s1"));
    }

    /// §12: session 定義の上書きで force フラグが立ち、grid 境界で次エントリへ進む。
    /// Overwriting the session definition sets the flag and advances on the grid.
    #[test]
    fn reeval_session_def_forces_advance_on_grid() {
        let mut ev = setup_playing_session_two_entries();
        // song を上書き（同名 session 更新）
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![
                crate::ast::session::SessionEntry {
                    scene: "s1".into(),
                    repeat: crate::ast::session::SessionRepeat::Loop,
                },
                crate::ast::session::SessionEntry {
                    scene: "s2".into(),
                    repeat: crate::ast::session::SessionRepeat::Once,
                },
            ],
        }))
        .unwrap();
        // force フラグが立ち、grid 境界で次エントリへ
        let outcome = ev.try_force_advance_session_on_grid().unwrap();
        assert!(matches!(outcome, SceneTransitionOutcome::NextScene { .. }));
    }

    /// 1 clip + 1 scene + 単一エントリ session（s1=a, Loop）を構築して Play(Session) する
    ///
    /// 進む先エントリが無い session。再生中の再 eval で §12 強制遷移が発火しても
    /// SessionComplete で止まってはならず、現 scene を維持し続ける事を検証するための土台。
    ///
    /// Builds 1 clip + 1 scene + a single-entry (s1=a, Loop) session and plays it.
    /// There is no next entry to advance to, so a §12 forced transition must not
    /// stop playback (SessionComplete) but keep the current scene active.
    fn setup_playing_session_single_entry() -> Evaluator {
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
            name: "s1".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
                muted: false,
            }],
        }))
        .unwrap();
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![crate::ast::session::SessionEntry {
                scene: "s1".into(),
                repeat: crate::ast::session::SessionRepeat::Loop,
            }],
        }))
        .unwrap();
        // `play session NAME`（repeat 指定なし）は RepeatSpec::Once になる
        // （parser::playback の default）。これが本不具合の現実の再現条件で、
        // session 全体は非ループ（session_looping=false）となり、§12 強制遷移時に
        // 末尾超過 Done → SessionComplete で停止していた。
        // A bare `play session NAME` parses to RepeatSpec::Once (parser default),
        // so the session is non-looping — the exact condition that triggered the
        // stop bug.
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Session("song".into()),
            repeat: RepeatSpec::Once,
        }))
        .unwrap();
        ev
    }

    /// §12 回帰: 単一エントリ session 再生中に使用中 clip を上書きしても、
    /// 進む先エントリが無いため SessionComplete にならず現 scene を維持する。
    /// （旧挙動では force_next_entry が末尾超過 Done → active_scene=None で停止していた）
    ///
    /// Regression: overwriting an in-use clip during a single-entry session must
    /// keep the current scene (no SessionComplete, no stop).
    #[test]
    fn reeval_clip_in_single_entry_session_keeps_scene() {
        let mut ev = setup_playing_session_single_entry();
        assert_eq!(ev.active_scene_name.as_deref(), Some("s1"));

        // s1 が使う clip a を上書き → force フラグが立つ
        ev.eval_block(Block::Clip(ClipDef {
            name: "a".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }))
        .unwrap();

        // grid 境界: 進む先が無いので Continue（現 scene 維持）であるべき。停止してはならない。
        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::Continue
        );
        assert_eq!(
            ev.active_scene_name.as_deref(),
            Some("s1"),
            "単一エントリ session では現 scene を維持し続ける（停止しない）"
        );
        assert!(
            ev.active_scene().is_some(),
            "active_scene が None になってはならない（MIDI clock 停止の原因）"
        );
    }

    /// §12 回帰: 単一エントリ session 再生中に再生中 scene の構成を上書きしても停止しない。
    /// Regression: overwriting the active scene in a single-entry session must not stop.
    #[test]
    fn reeval_active_scene_in_single_entry_session_keeps_scene() {
        let mut ev = setup_playing_session_single_entry();
        // 再生中 scene s1 の構成を上書き
        ev.eval_block(Block::Scene(SceneDef {
            name: "s1".into(),
            entries: vec![crate::ast::scene::SceneEntry::Clip {
                candidates: vec![crate::ast::scene::ShuffleCandidate {
                    clip: "a".into(),
                    weight: 1,
                }],
                probability: None,
                muted: false,
            }],
        }))
        .unwrap();

        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::Continue
        );
        assert_eq!(ev.active_scene_name.as_deref(), Some("s1"));
        assert!(ev.active_scene().is_some());
    }

    /// §12 回帰: 単一エントリ session 定義そのものを上書きしても停止しない。
    ///
    /// このケースは `pending_session` 差し替え経路を通り、新 runner の先頭エントリ
    /// （= 同じ s1）へ着地するため結果は `NextScene { s1 }` になる。clip/scene 上書きと
    /// 違い `Continue` ではないが、いずれにせよ **active_scene を維持し停止しない**点が
    /// 本不具合の防止要件。よってここでは「s1 が active のままで、None にならない」事を検証する。
    ///
    /// Regression: overwriting the single-entry session definition must not stop.
    /// This goes through the `pending_session` swap path and lands on the new
    /// runner's first (same) entry s1, so the outcome is `NextScene { s1 }` rather
    /// than `Continue`. Either way the requirement is: active_scene stays set.
    #[test]
    fn reeval_single_entry_session_def_keeps_scene() {
        let mut ev = setup_playing_session_single_entry();
        // song を上書き（同名・単一エントリのまま）
        ev.eval_block(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![crate::ast::session::SessionEntry {
                scene: "s1".into(),
                repeat: crate::ast::session::SessionRepeat::Loop,
            }],
        }))
        .unwrap();

        let outcome = ev.try_force_advance_session_on_grid().unwrap();
        // 停止系（SceneComplete / SessionComplete）でない事が要件
        assert!(
            !matches!(
                outcome,
                SceneTransitionOutcome::SceneComplete | SceneTransitionOutcome::SessionComplete
            ),
            "停止してはならない（active_scene=None で MIDI clock が止まる）: {outcome:?}"
        );
        assert_eq!(ev.active_scene_name.as_deref(), Some("s1"));
        assert!(
            ev.active_scene().is_some(),
            "active_scene が None になってはならない"
        );
    }

    /// 通常の PlayScene（非 session）では clip 上書きで force フラグは立たない
    /// （clip swap は PR #99 経由）。grid commit は no-op。
    /// In a plain PlayScene, overwriting a clip does not set the force flag.
    #[test]
    fn reeval_clip_in_plain_scene_does_not_force_advance() {
        let mut ev = setup_with_single_clip("c1", "s1", 1);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("s1".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        eval_src(&mut ev, "clip c1 [bars 1] {\n  inst e\n}\n");
        // session ではないので force フラグは立たず no-op
        assert_eq!(
            ev.try_force_advance_session_on_grid().unwrap(),
            SceneTransitionOutcome::Continue
        );
    }

    /// 再生していない（active_scene なし）状態での clip 上書きは pending を積まない
    /// （副作用なく registry 更新のみ）。
    /// Overwriting a clip while nothing plays stages no pending (registry only).
    #[test]
    fn reeval_clip_without_active_scene_is_registry_only() {
        let mut ev = setup_with_single_clip("c1", "s1", 1);
        // play していないので active_scene は None
        assert!(ev.active_scene().is_none());
        // 上書きしてもエラーや panic にならず、registry 更新のみ
        eval_src(&mut ev, "clip c1 [bars 1] {\n  inst e\n}\n");
        assert!(ev.active_scene().is_none());
    }

    /// Stop(None) で active_scene の使用中チャンネル分の AllNotesOff が pending に積まれる
    #[test]
    fn stop_none_queues_all_notes_off_for_active_scene_channels() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Stop(StopCommand { target: None }))
            .unwrap();

        let channels = ev.take_pending_all_notes_off();
        assert_eq!(
            channels,
            vec![("dev".to_string(), MidiChannel::from_one_based(5).unwrap())]
        );
        assert!(ev.active_scene().is_none());
        assert_eq!(*ev.state().state(), PlaybackState::Stopped);
    }

    /// Stop(scene 名) は全停止扱い + AllNotesOff
    #[test]
    fn stop_with_current_scene_name_fully_stops() {
        let mut ev = setup_with_single_clip("a", "verse", 7);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Stop(StopCommand {
            target: Some("verse".into()),
        }))
        .unwrap();
        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![("dev".to_string(), MidiChannel::from_one_based(7).unwrap())]
        );
        assert!(ev.active_scene().is_none());
    }

    /// §10.4: `mute <clip>` は該当 clip をミュートし、そのチャンネル分のみ AllNotesOff を蓄積する
    /// §10.4: `mute <clip>` mutes the named clip and queues AllNotesOff only for its channel
    #[test]
    fn mute_with_clip_name_mutes_and_queues_its_channel() {
        use crate::ast::playback::MuteCommand;
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   instrument inst_b {\n  device dev\n  channel 9\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse { a b }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Mute(MuteCommand { target: "a".into() }))
            .unwrap();

        assert!(matches!(result, EvalResult::Muted { ref target } if target == "a"));
        // active_scene は存続、"a" だけミュートされる
        // active_scene persists; only "a" is muted
        let scene = ev.active_scene().unwrap();
        assert!(scene.is_muted("a"));
        assert!(!scene.is_muted("b"));
        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![("dev".to_string(), MidiChannel::from_one_based(2).unwrap())]
        );
        assert!(matches!(
            ev.state().state(),
            PlaybackState::PlayingScene { .. }
        ));
    }

    /// §10.4: `mute <unknown>` は MutedNoop を返し、active_scene を変更しない
    /// §10.4: `mute <unknown>` returns MutedNoop without altering active_scene
    #[test]
    fn mute_with_unknown_clip_is_noop() {
        use crate::ast::playback::MuteCommand;
        let mut ev = setup_with_single_clip("a", "verse", 3);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Mute(MuteCommand {
                target: "ghost".into(),
            }))
            .unwrap();

        assert!(matches!(result, EvalResult::MutedNoop { .. }));
        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_muted("a"));
        assert!(ev.take_pending_all_notes_off().is_empty());
    }

    /// Issue #49: 複数 device を含む scene で `mute <clip>` すると、
    /// その clip の device と channel の組だけが `pending_all_notes_off` に
    /// 積まれ、他 device は影響を受けない。
    ///
    /// Issue #49: When a scene has clips bound to different devices,
    /// `mute <clip>` queues AllNotesOff only for the (device, channel)
    /// pair of the targeted clip; other devices are not touched.
    #[test]
    fn mute_with_multi_device_queues_only_target_device_channel() {
        use crate::ast::playback::MuteCommand;
        let mut ev = Evaluator::new(120.0);
        let src = "device synth_a { port port_a }\n\
                   device synth_b { port port_b }\n\
                   instrument lead {\n  device synth_a\n  channel 1\n}\n\
                   instrument pad {\n  device synth_b\n  channel 2\n}\n\
                   clip a [bars 1] {\n  lead c\n}\n\
                   clip b [bars 1] {\n  pad c\n}\n\
                   scene verse { a b }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        // clip "a" (device=synth_a, channel=1) だけを mute
        ev.eval_block(Block::Mute(MuteCommand { target: "a".into() }))
            .unwrap();

        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![(
                "synth_a".to_string(),
                MidiChannel::from_one_based(1).unwrap()
            )],
            "mute は該当 device/channel のみ AllNotesOff する"
        );
    }

    /// Issue #49: 複数 device を含む scene で `stop`（None）すると、
    /// active_scene の全 (device, channel) が pending に積まれる。
    ///
    /// Issue #49: `stop` (None) on a multi-device scene queues AllNotesOff
    /// for every (device, channel) pair in the active scene.
    #[test]
    fn stop_with_multi_device_queues_all_device_channel_pairs() {
        let mut ev = Evaluator::new(120.0);
        let src = "device synth_a { port port_a }\n\
                   device synth_b { port port_b }\n\
                   instrument lead {\n  device synth_a\n  channel 1\n}\n\
                   instrument pad {\n  device synth_b\n  channel 2\n}\n\
                   clip a [bars 1] {\n  lead c\n}\n\
                   clip b [bars 1] {\n  pad c\n}\n\
                   scene verse { a b }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Stop(StopCommand { target: None }))
            .unwrap();

        let mut pairs = ev.take_pending_all_notes_off();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                (
                    "synth_a".to_string(),
                    MidiChannel::from_one_based(1).unwrap()
                ),
                (
                    "synth_b".to_string(),
                    MidiChannel::from_one_based(2).unwrap()
                )
            ],
        );
    }

    /// §10.4: active_scene が無いときの `mute` は MutedNoop
    /// §10.4: `mute` without an active scene is a MutedNoop
    #[test]
    fn mute_without_active_scene_is_noop() {
        use crate::ast::playback::MuteCommand;
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Mute(MuteCommand { target: "a".into() }))
            .unwrap();
        assert!(matches!(result, EvalResult::MutedNoop { .. }));
    }

    /// §10.4: `unmute <clip>` はミュート解除され、Unmuted を返す
    /// §10.4: `unmute <clip>` releases mute flag and returns Unmuted
    #[test]
    fn unmute_with_clip_name_releases_mute() {
        use crate::ast::playback::{MuteCommand, UnmuteCommand};
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   scene verse { a }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Mute(MuteCommand { target: "a".into() }))
            .unwrap();
        // pending を一度吸い上げる
        // Drain the pending queue once
        let _ = ev.take_pending_all_notes_off();

        let result = ev
            .eval_block(Block::Unmute(UnmuteCommand { target: "a".into() }))
            .unwrap();

        assert!(matches!(result, EvalResult::Unmuted { ref target } if target == "a"));
        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_muted("a"));
        // unmute は AllNotesOff を蓄積しない（再生再開のため）
        // unmute does not queue AllNotesOff (to allow sound resumption)
        assert!(ev.take_pending_all_notes_off().is_empty());
    }

    /// §10.4: `unmute <unknown>` は UnmutedNoop
    /// §10.4: `unmute <unknown>` yields UnmutedNoop
    #[test]
    fn unmute_with_unknown_clip_is_noop() {
        use crate::ast::playback::UnmuteCommand;
        let mut ev = setup_with_single_clip("a", "verse", 3);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let result = ev
            .eval_block(Block::Unmute(UnmuteCommand {
                target: "ghost".into(),
            }))
            .unwrap();
        assert!(matches!(result, EvalResult::UnmutedNoop { .. }));
    }

    /// §10.4: `stop <clip>` は clip 名に一致しても scene/session ではないため no-op
    /// §10.4: `stop <clip>` is now a no-op because clip targets moved to `mute`
    #[test]
    fn stop_with_clip_name_is_noop() {
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   scene verse { a }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        ev.eval_block(Block::Stop(StopCommand {
            target: Some("a".into()),
        }))
        .unwrap();

        // 再生は継続し、clip も mute されない
        // Playback keeps running and the clip is not muted
        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_muted("a"));
        assert!(ev.take_pending_all_notes_off().is_empty());
        assert!(matches!(
            ev.state().state(),
            PlaybackState::PlayingScene { .. }
        ));
    }

    /// Stop(未知名) は active_scene を変更せず、pending も空
    #[test]
    fn stop_with_unknown_target_is_noop() {
        let mut ev = setup_with_single_clip("a", "verse", 3);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Stop(StopCommand {
            target: Some("ghost".into()),
        }))
        .unwrap();

        assert!(ev.active_scene().is_some());
        assert!(ev.take_pending_all_notes_off().is_empty());
        assert!(matches!(
            ev.state().state(),
            PlaybackState::PlayingScene { .. }
        ));
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
                muted: false,
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
        assert_eq!(inst.channel, MidiChannel::from_one_based(3).unwrap());
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
        assert_eq!(inst.channel, MidiChannel::from_one_based(3).unwrap());
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

    // --- §10.4 pause / resume evaluator tests ---

    use crate::ast::playback::{PauseCommand, ResumeCommand};

    /// Pause(None) で再生中なら Paused に遷移、AllNotesOff 蓄積、全 clip が paused
    /// Pause(None) while playing: transitions to Paused, queues AllNotesOff, all clips paused
    #[test]
    fn pause_none_queues_all_notes_off_and_pauses_clips() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Pause(PauseCommand { target: None }))
            .unwrap();
        assert_eq!(result, EvalResult::Paused { target: None });

        assert!(ev.state().is_paused());
        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![("dev".to_string(), MidiChannel::from_one_based(5).unwrap())]
        );
        let scene = ev.active_scene().unwrap();
        assert!(scene.is_clip_paused("a"));
    }

    /// Pause(scene 名) で一致時は全体 pause 相当（AllNotesOff 全 ch、全 clip paused）
    /// Pause(scene name) when matching: equivalent to full pause
    #[test]
    fn pause_with_current_scene_name_fully_pauses() {
        let mut ev = setup_with_single_clip("a", "verse", 7);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Pause(PauseCommand {
                target: Some("verse".into()),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Paused {
                target: Some("verse".into())
            }
        );

        assert!(ev.state().is_paused());
        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![("dev".to_string(), MidiChannel::from_one_based(7).unwrap())]
        );
    }

    /// Pause(clip 名) は該当 clip のみ pause、そのチャンネルだけ AllNotesOff
    /// Pause(clip name) pauses only that clip and queues only its channel
    #[test]
    fn pause_with_clip_name_pauses_single_clip() {
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   instrument inst_b {\n  device dev\n  channel 9\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse { a b }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Pause(PauseCommand {
                target: Some("a".into()),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Paused {
                target: Some("a".into())
            }
        );

        // 全体 state は PlayingScene のまま、clip "a" だけ paused
        // Global state stays PlayingScene; only clip "a" is paused
        assert!(!ev.state().is_paused());
        let scene = ev.active_scene().unwrap();
        assert!(scene.is_clip_paused("a"));
        assert!(!scene.is_clip_paused("b"));
        assert_eq!(
            ev.take_pending_all_notes_off(),
            vec![("dev".to_string(), MidiChannel::from_one_based(2).unwrap())]
        );
    }

    /// Pause(未知名) は no-op で active_scene も state も不変、pending 空（§10.4 D3）
    /// Pause(unknown) is no-op; active_scene and state unchanged, pending empty (§10.4 D3)
    #[test]
    fn pause_with_unknown_target_is_noop() {
        let mut ev = setup_with_single_clip("a", "verse", 3);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let result = ev
            .eval_block(Block::Pause(PauseCommand {
                target: Some("ghost".into()),
            }))
            .unwrap();
        assert!(matches!(result, EvalResult::PausedNoop { .. }));

        assert!(ev.active_scene().is_some());
        assert!(!ev.state().is_paused());
        assert!(ev.take_pending_all_notes_off().is_empty());
    }

    /// Pause(None) で Stopped 時は no-op
    /// Pause(None) when stopped is no-op
    #[test]
    fn pause_when_stopped_is_noop() {
        let mut ev = Evaluator::new(120.0);
        let result = ev
            .eval_block(Block::Pause(PauseCommand { target: None }))
            .unwrap();
        assert!(matches!(result, EvalResult::PausedNoop { .. }));
        assert!(!ev.state().is_paused());
    }

    /// Resume(None) は Paused を解除して元の state に戻し、全 clip も resume
    /// Resume(None) restores state from Paused and resumes every clip
    #[test]
    fn resume_none_restores_state_and_resumes_clips() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Pause(PauseCommand { target: None }))
            .unwrap();
        let _ = ev.take_pending_all_notes_off();

        let result = ev
            .eval_block(Block::Resume(ResumeCommand { target: None }))
            .unwrap();
        assert_eq!(result, EvalResult::Resumed { target: None });
        assert!(!ev.state().is_paused());
        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_clip_paused("a"));
    }

    /// Resume(scene 名) は prev と一致時のみ復元
    /// Resume(scene name) restores only when prev name matches
    #[test]
    fn resume_with_matching_name_restores_state() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Pause(PauseCommand { target: None }))
            .unwrap();

        let result = ev
            .eval_block(Block::Resume(ResumeCommand {
                target: Some("verse".into()),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Resumed {
                target: Some("verse".into())
            }
        );
        assert!(!ev.state().is_paused());
    }

    /// Resume(不一致名) は no-op で Paused のまま
    /// Resume(mismatched name) is no-op; stays paused
    #[test]
    fn resume_with_mismatched_name_is_noop() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Pause(PauseCommand { target: None }))
            .unwrap();

        let result = ev
            .eval_block(Block::Resume(ResumeCommand {
                target: Some("chorus".into()),
            }))
            .unwrap();
        assert!(matches!(result, EvalResult::ResumedNoop { .. }));
        assert!(ev.state().is_paused());
    }

    /// Resume(clip 名) は該当 clip のみ resume（全体 state は変化しない）
    /// Resume(clip name) resumes only that clip; global state unchanged
    #[test]
    fn resume_with_clip_name_resumes_single_clip() {
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   instrument inst_b {\n  device dev\n  channel 9\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse { a b }\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        ev.eval_block(Block::Pause(PauseCommand {
            target: Some("a".into()),
        }))
        .unwrap();
        let _ = ev.take_pending_all_notes_off();

        let result = ev
            .eval_block(Block::Resume(ResumeCommand {
                target: Some("a".into()),
            }))
            .unwrap();
        assert_eq!(
            result,
            EvalResult::Resumed {
                target: Some("a".into())
            }
        );
        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_clip_paused("a"));
    }

    /// Resume(None) Paused でない場合は no-op（§10.4 D4）
    /// Resume(None) when not paused is no-op (§10.4 D4)
    #[test]
    fn resume_when_not_paused_is_noop() {
        let mut ev = setup_with_single_clip("a", "verse", 5);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let result = ev
            .eval_block(Block::Resume(ResumeCommand { target: None }))
            .unwrap();
        assert!(matches!(result, EvalResult::ResumedNoop { .. }));
    }

    /// Resume(未知名) は no-op
    /// Resume(unknown name) is no-op
    #[test]
    fn resume_with_unknown_target_is_noop() {
        let mut ev = setup_with_single_clip("a", "verse", 3);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let result = ev
            .eval_block(Block::Resume(ResumeCommand {
                target: Some("ghost".into()),
            }))
            .unwrap();
        assert!(matches!(result, EvalResult::ResumedNoop { .. }));
    }

    // =========================================================================
    // Issue #50: MIDI System Real-Time (Start / Stop) 送出
    // Issue #50: MIDI System Real-Time (Start / Stop) transport emission
    // =========================================================================

    /// Issue #50: play 実行時に transport=true の device に Start が積まれる
    #[test]
    fn play_queues_midi_start_for_transport_devices() {
        use crate::midi::message::MidiMessage;
        let mut ev = setup_with_single_clip("a", "verse", 1);
        // setup_with_single_clip は `device dev { port test }` を登録済み（transport 省略 = true）
        assert!(ev.take_pending_transport().is_empty());
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let queue = ev.take_pending_transport();
        assert_eq!(queue, vec![("dev".to_string(), MidiMessage::Start)]);
    }

    /// Issue #50: stop 実行時に transport=true の device に Stop が積まれる
    #[test]
    fn stop_queues_midi_stop_for_transport_devices() {
        use crate::midi::message::MidiMessage;
        let mut ev = setup_with_single_clip("a", "verse", 1);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        // play で積まれた Start を drain してから stop を発行する
        let _ = ev.take_pending_transport();
        ev.eval_block(Block::Stop(StopCommand { target: None }))
            .unwrap();
        let queue = ev.take_pending_transport();
        assert_eq!(queue, vec![("dev".to_string(), MidiMessage::Stop)]);
    }

    /// Issue #50: transport=false の device には Start/Stop が積まれない
    #[test]
    fn transport_false_devices_do_not_receive_transport_messages() {
        let mut ev = Evaluator::new(120.0);
        let src = "\
            device a { port port_a\n  transport true\n}\n\
            device b { port port_b\n  transport false\n}\n\
            instrument inst_a { device a\n  channel 1\n}\n\
            clip c [bars 1] { inst_a c }\n\
            scene s { c }\n";
        eval_src(&mut ev, src);

        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("s".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let queue = ev.take_pending_transport();
        // a (true) のみが含まれ、b (false) は含まれない
        let names: Vec<&str> = queue.iter().map(|(d, _)| d.as_str()).collect();
        assert!(
            names.contains(&"a"),
            "device a (transport=true) must receive Start"
        );
        assert!(
            !names.contains(&"b"),
            "device b (transport=false) must NOT receive Start"
        );

        ev.eval_block(Block::Stop(StopCommand { target: None }))
            .unwrap();
        let queue = ev.take_pending_transport();
        let names: Vec<&str> = queue.iter().map(|(d, _)| d.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(!names.contains(&"b"));
    }

    /// Issue #50: 複数 transport=true device すべてに Start/Stop が積まれる
    #[test]
    fn multiple_transport_true_devices_all_receive_messages() {
        use crate::midi::message::MidiMessage;
        let mut ev = Evaluator::new(120.0);
        let src = "\
            device a { port pa\n  transport true\n}\n\
            device b { port pb\n}\n\
            instrument inst_a { device a\n  channel 1\n}\n\
            clip c [bars 1] { inst_a c }\n\
            scene s { c }\n";
        eval_src(&mut ev, src);

        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("s".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        let mut queue = ev.take_pending_transport();
        queue.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            queue,
            vec![
                ("a".to_string(), MidiMessage::Start),
                ("b".to_string(), MidiMessage::Start),
            ]
        );
    }

    /// Issue #50: play が失敗（未知 scene）した場合、transport キューに何も積まれない
    #[test]
    fn failed_play_does_not_queue_transport() {
        let mut ev = setup_with_single_clip("a", "verse", 1);
        let _ = ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("missing".into()),
            repeat: RepeatSpec::Loop,
        }));
        assert!(ev.take_pending_transport().is_empty());
    }

    // ---------------------------------------------------------------------
    // PR #54: DeviceEvent emission tests
    //
    // Evaluator は `Block::Device` を eval した際に、`set_device_event_tx`
    // で渡された tx 経由で `DeviceEvent::Upsert` を emit する。tx 未設定でも
    // eval は通常通り成功する（後方互換）。
    //
    // Tests for the `DeviceEvent` channel: evaluating a `Block::Device`
    // emits `Upsert` via the registered tx; eval still works when no tx is
    // set (backward compatibility).
    // ---------------------------------------------------------------------

    /// tx 未設定の Evaluator で device を eval しても panic せず後方互換が保たれる
    /// Eval still works (no panic) when no `DeviceEvent` tx is registered.
    #[test]
    fn device_event_tx_unset_does_not_panic_on_device_eval() {
        let mut ev = Evaluator::new(120.0);
        // tx を登録しないまま device を eval。registered で成功し、
        // 後続の eval にも影響を与えないことを確認する。
        eval_src(&mut ev, "device foo { port px }\n");
        assert!(ev.registry().get_device("foo").is_some());
    }

    /// tx を登録した状態で device を eval すると `Upsert` が 1 件届く
    /// Registering a tx and evaluating a new device emits one `Upsert`.
    #[test]
    fn device_event_emits_upsert_on_new_device() {
        use crate::engine::device_event::DeviceEvent;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);
        eval_src(&mut ev, "device foo { port px }\n");
        // 受信側で 1 件取り出せること
        let received = rx.try_recv().expect("expected one DeviceEvent");
        assert_eq!(
            received,
            DeviceEvent::Upsert {
                name: "foo".into(),
                port: "px".into(),
            }
        );
        // それ以上のイベントは無いこと
        assert!(rx.try_recv().is_err());
    }

    /// 同名 device の port を変えて 2 回 eval すると `Upsert` が 2 件届く
    /// （Evaluator は同一性判定をせず、識別は受信側責務）
    /// Re-evaluating the same device name with a different port emits two
    /// `Upsert`s; deduplication is the receiver's responsibility.
    #[test]
    fn device_event_emits_upsert_on_redefinition_with_same_name() {
        use crate::engine::device_event::DeviceEvent;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);
        eval_src(&mut ev, "device foo { port px }\n");
        eval_src(&mut ev, "device foo { port py }\n");
        let first = rx.try_recv().expect("expected first DeviceEvent");
        let second = rx.try_recv().expect("expected second DeviceEvent");
        assert_eq!(
            first,
            DeviceEvent::Upsert {
                name: "foo".into(),
                port: "px".into(),
            }
        );
        assert_eq!(
            second,
            DeviceEvent::Upsert {
                name: "foo".into(),
                port: "py".into(),
            }
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn device_connection_error_record_and_get() {
        let mut ev = Evaluator::new(120.0);
        assert!(ev.device_connection_errors().is_empty());

        ev.record_device_connection_error("synth".into(), "port_a".into(), "not found".into());

        let errs = ev.device_connection_errors();
        assert_eq!(errs.len(), 1);
        let entry = errs.get("synth").expect("synth recorded");
        assert_eq!(entry.port, "port_a");
        assert_eq!(entry.message, "not found");
    }

    #[test]
    fn device_connection_error_overwrites_on_re_record() {
        let mut ev = Evaluator::new(120.0);
        ev.record_device_connection_error("synth".into(), "port_a".into(), "first".into());
        ev.record_device_connection_error("synth".into(), "port_b".into(), "second".into());

        let entry = ev.device_connection_errors().get("synth").unwrap();
        assert_eq!(entry.port, "port_b");
        assert_eq!(entry.message, "second");
    }

    #[test]
    fn device_connection_error_clear_removes_entry() {
        let mut ev = Evaluator::new(120.0);
        ev.record_device_connection_error("synth".into(), "port_a".into(), "err".into());
        ev.clear_device_connection_error("synth");
        assert!(ev.device_connection_errors().get("synth").is_none());
    }

    #[test]
    fn device_connection_error_clear_noop_for_missing_device() {
        let mut ev = Evaluator::new(120.0);
        // 存在しない device を clear しても panic せず空のまま
        ev.clear_device_connection_error("ghost");
        assert!(ev.device_connection_errors().is_empty());
    }

    // ---------------------------------------------------------------------
    // PR #83: preload silent devices tests
    //
    // LSP の preload 経路（diagnostics 等）は副作用のないソース解析を意図
    // するため、device ブロック評価時にも `DeviceEvent::Upsert` を発火させ
    // ない。`preload_from_source` 経由では silent、`eval_source_preload`
    // を直接呼ぶ Request::Preload 経路は従来通り発火、という分離をテスト
    // で固定する。
    //
    // The LSP preload path (used by diagnostics etc.) is meant to perform
    // side-effect-free source analysis, so `Block::Device` evaluation must
    // not emit `DeviceEvent::Upsert`. We pin the separation here:
    // `preload_from_source` is silent; calling `eval_source_preload`
    // directly (Request::Preload) keeps emitting events as before.
    // ---------------------------------------------------------------------

    /// preload_from_source 経由で device を評価しても DeviceEvent が emit されない
    /// `preload_from_source` does not emit `DeviceEvent` even with device blocks.
    #[test]
    fn preload_from_source_does_not_emit_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);

        let source = "device mb {\n  port Mutant Brain\n}\n";
        let result = ev.preload_from_source(source, &[]);
        assert!(result, "preload should succeed on empty registry");
        // registry には登録される
        assert!(ev.registry().get_device("mb").is_some());
        // しかし DeviceEvent は emit されない
        assert!(
            rx.try_recv().is_err(),
            "expected no DeviceEvent from preload_from_source"
        );
    }

    /// eval_source_preload を直接呼ぶ場合は従来通り DeviceEvent が emit される
    /// Calling `eval_source_preload` directly still emits `DeviceEvent` (regression guard).
    #[test]
    fn eval_source_preload_still_emits_device_event() {
        use crate::engine::device_event::DeviceEvent;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);

        let _ = ev
            .eval_source_preload("device foo {\n  port px\n}\n")
            .expect("eval_source_preload should succeed");

        let received = rx
            .try_recv()
            .expect("expected DeviceEvent from eval_source_preload");
        assert_eq!(
            received,
            DeviceEvent::Upsert {
                name: "foo".into(),
                port: "px".into(),
            }
        );
        assert!(rx.try_recv().is_err(), "no further events expected");
    }

    /// preload_from_source は registry を埋めつつ DeviceEvent は emit しない
    /// `preload_from_source` populates the registry while staying silent on events.
    #[test]
    fn preload_from_source_populates_registry_without_device_event() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);

        assert!(ev.registry().is_empty(), "registry must start empty");

        let source = "device synth {\n  port IAC\n}\ndevice drum {\n  port LPK25\n}\n";
        let result = ev.preload_from_source(source, &[]);
        assert!(result);

        assert!(ev.registry().get_device("synth").is_some());
        assert!(ev.registry().get_device("drum").is_some());

        assert!(
            rx.try_recv().is_err(),
            "expected no DeviceEvent even with multiple devices"
        );
    }

    /// preload_from_source の処理後に device_event_tx が復元されている（次回 eval で発火する）
    /// After `preload_from_source`, the device_event_tx is restored so subsequent direct
    /// evaluations emit events again.
    #[test]
    fn preload_from_source_restores_device_event_tx() {
        use crate::engine::device_event::DeviceEvent;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ev = Evaluator::new(120.0);
        ev.set_device_event_tx(tx);

        // 1回目: preload_from_source（silent）
        let result = ev.preload_from_source("device a {\n  port pa\n}\n", &[]);
        assert!(result);
        assert!(rx.try_recv().is_err(), "preload_from_source must be silent");

        // 2回目: eval_block 経由（registry 非空でも eval は走る）。tx 復元の検証
        eval_src(&mut ev, "device b {\n  port pb\n}\n");
        let received = rx
            .try_recv()
            .expect("tx should be restored after preload_from_source");
        assert_eq!(
            received,
            DeviceEvent::Upsert {
                name: "b".into(),
                port: "pb".into(),
            }
        );
    }

    // §8.6: scene 内 `mute` 前置による初期 mute の E2E テスト
    // §8.6: end-to-end tests for the scene-internal `mute` prefix (initial mute)

    /// scene 内で `mute bass` と前置すると、scene activate 時に該当 clip が mute された状態でロードされる。
    /// Prefixing a clip line with `mute` inside the scene block loads it muted on activation.
    #[test]
    fn scene_internal_mute_prefix_initially_mutes_clip_on_activate() {
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   instrument inst_b {\n  device dev\n  channel 3\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse {\n  a\n  mute b\n}\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();

        let scene = ev.active_scene().unwrap();
        assert!(!scene.is_muted("a"), "a は非 mute でロードされる");
        assert!(
            scene.is_muted("b"),
            "mute 前置された b は初期 mute でロードされる"
        );
        // 初期 mute は AllNotesOff を伴わない（まだ鳴っていないので）
        // Initial mute does not require AllNotesOff (clip has not started yet).
        assert!(
            ev.take_pending_all_notes_off().is_empty(),
            "scene activate 時の初期 mute は AllNotesOff を積まない"
        );
    }

    /// scene 内 `mute` 前置はトップレベル `unmute <clip>` で動的に解除できる。
    /// The initial mute set by scene-internal `mute` prefix can be released by top-level `unmute <clip>`.
    #[test]
    fn scene_internal_mute_prefix_can_be_unmuted_dynamically() {
        use crate::ast::playback::UnmuteCommand;
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_b {\n  device dev\n  channel 3\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse {\n  mute b\n}\n";
        eval_src(&mut ev, src);
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        assert!(ev.active_scene().unwrap().is_muted("b"));

        ev.eval_block(Block::Unmute(UnmuteCommand { target: "b".into() }))
            .unwrap();
        assert!(
            !ev.active_scene().unwrap().is_muted("b"),
            "unmute で初期 mute を解除できる"
        );
    }

    /// scene を切り替えると、新 scene の宣言で mute 状態が再初期化される
    /// （前 scene 上で動的に変更された mute 状態は引き継がない）。
    /// Switching scenes re-initializes the mute state per the new scene's declaration;
    /// dynamic mute changes on the previous scene are not carried over.
    #[test]
    fn scene_switch_reinitializes_mute_state_from_declaration() {
        use crate::ast::playback::MuteCommand;
        let mut ev = Evaluator::new(120.0);
        let src = "device dev { port test }\n\
                   instrument inst_a {\n  device dev\n  channel 2\n}\n\
                   instrument inst_b {\n  device dev\n  channel 3\n}\n\
                   clip a [bars 1] {\n  inst_a c\n}\n\
                   clip b [bars 1] {\n  inst_b c\n}\n\
                   scene verse {\n  mute a\n  b\n}\n\
                   scene chorus {\n  a\n  mute b\n}\n";
        eval_src(&mut ev, src);

        // verse: a が初期 mute / b は非 mute
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        assert!(ev.active_scene().unwrap().is_muted("a"));
        assert!(!ev.active_scene().unwrap().is_muted("b"));

        // verse 上で b を動的に mute（この変更は scene 切替で破棄されるべき）
        ev.eval_block(Block::Mute(MuteCommand { target: "b".into() }))
            .unwrap();
        assert!(ev.active_scene().unwrap().is_muted("b"));
        // 動的 mute で AllNotesOff が積まれている分は捨てる
        ev.take_pending_all_notes_off();

        // chorus へ切替: 宣言通り a は非 mute / b は初期 mute
        ev.eval_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("chorus".into()),
            repeat: RepeatSpec::Loop,
        }))
        .unwrap();
        assert!(
            !ev.active_scene().unwrap().is_muted("a"),
            "chorus の宣言で a は非 mute に再初期化される"
        );
        assert!(
            ev.active_scene().unwrap().is_muted("b"),
            "chorus の宣言で b は初期 mute に再初期化される（前 scene の動的 mute は引き継がない）"
        );
    }

    // ===== ロック外 eval (prepare/apply 分離) の意味的等価性テスト =====
    // Semantic-equivalence tests for off-lock eval (the prepare/apply split).

    /// 代表的なマルチブロック source で `snapshot→prepare→apply` の評価結果列が
    /// `eval_source` と完全一致すること (Play の active_scene 構築まで含む)。
    ///
    /// prepare/apply round-trip yields the exact same `EvalResult` sequence as
    /// `eval_source`, including building the active scene on Play.
    #[test]
    fn prepare_apply_equivalent_to_eval_source() {
        let src = "device d { port test }\n\
                   instrument lead { device d\n channel 1 }\n\
                   clip riff [bars 1] { lead c e g c }\n\
                   scene s { riff }\n\
                   play s [loop]\n";

        let mut direct = Evaluator::new(120.0);
        let direct_results = direct.eval_source(src).expect("eval_source");

        let mut split = Evaluator::new(120.0);
        let prepared = split
            .snapshot_for_prepare()
            .prepare_source(src)
            .expect("prepare_source");
        let split_results = split.apply_prepared(prepared).expect("apply_prepared");

        assert_eq!(
            direct_results, split_results,
            "prepare+apply の結果列が eval_source と一致しない"
        );
        // どちらも同じ scene を active にしている。
        assert_eq!(
            direct.active_scene_name_for_test(),
            split.active_scene_name_for_test()
        );
        assert!(split.active_scene().expect("active scene").has_clip("riff"));
        // apply 後はモードが Off に戻り、通常 eval が継続できる。
        assert!(matches!(split.precompile, PrecompileMode::Off));
    }

    /// 同一 source 内で先に定義した instrument を、後続 clip の compile が
    /// 解決できること (prepare 時の throwaway 上で register→compile の順序が保たれる)。
    ///
    /// A clip compiled during prepare resolves an instrument defined earlier in the
    /// same source (register-before-compile order is preserved on the throwaway).
    #[test]
    fn prepare_apply_resolves_instrument_defined_in_same_source() {
        let src = "device d { port test }\n\
                   instrument lead { device d\n channel 3 }\n\
                   clip riff [bars 1] { lead c e g }\n\
                   scene s { riff }\n\
                   play s [loop]\n";

        let mut ev = Evaluator::new(120.0);
        let prepared = ev
            .snapshot_for_prepare()
            .prepare_source(src)
            .expect("prepare_source");
        ev.apply_prepared(prepared)
            .expect("apply must resolve same-source instrument");

        // clip が registry に登録され、active_scene に積まれている = compile 成功。
        assert!(ev.registry().get_clip("riff").is_some());
        assert!(ev.active_scene().expect("active scene").has_clip("riff"));
    }

    /// preload 版 prepare/apply が `eval_source_preload` と等価 (play/stop 除外) で、
    /// play がスキップされ active_scene が構築されないこと。
    ///
    /// The preload prepare/apply variant matches `eval_source_preload` (play/stop
    /// excluded): play is skipped and no active scene is built.
    #[test]
    fn prepare_apply_preload_skips_play() {
        let src = "device d { port test }\n\
                   instrument lead { device d\n channel 1 }\n\
                   clip riff [bars 1] { lead c e g }\n\
                   scene s { riff }\n\
                   play s [loop]\n";

        let mut direct = Evaluator::new(120.0);
        let direct_results = direct
            .eval_source_preload(src)
            .expect("eval_source_preload");

        let mut split = Evaluator::new(120.0);
        let prepared = split
            .snapshot_for_prepare()
            .prepare_source_preload(src)
            .expect("prepare_source_preload");
        let split_results = split.apply_prepared(prepared).expect("apply_prepared");

        assert_eq!(direct_results, split_results);
        assert!(
            split.active_scene().is_none(),
            "preload では play がスキップされ active_scene は構築されない"
        );
    }

    /// 再生中(active_scene あり)に in-use clip を prepare/apply で再定義した結果が
    /// `eval_source` 経路と一致すること。差し替え後も active_scene が clip を保持し、
    /// 再生カーソル保持経路 (replace_clip) を壊さない。
    ///
    /// Redefining an in-use clip via prepare/apply matches the `eval_source` path:
    /// the active scene keeps the clip after the swap, leaving the cursor-preserving
    /// `replace_clip` path intact.
    #[test]
    fn prepare_apply_redefines_in_use_clip() {
        let setup = "device d { port test }\n\
                     instrument lead { device d\n channel 1 }\n\
                     clip riff [bars 1] { lead c e g }\n\
                     scene s { riff }\n\
                     play s [loop]\n";
        let redef = "clip riff [bars 1] { lead g e c }\n";

        let mut direct = Evaluator::new(120.0);
        direct.eval_source(setup).expect("setup");
        let direct_results = direct.eval_source(redef).expect("redef eval_source");

        let mut split = Evaluator::new(120.0);
        split.eval_source(setup).expect("setup");
        let prepared = split
            .snapshot_for_prepare()
            .prepare_source(redef)
            .expect("prepare redef");
        let split_results = split.apply_prepared(prepared).expect("apply redef");

        assert_eq!(direct_results, split_results);
        assert!(
            split.active_scene().expect("active scene").has_clip("riff"),
            "差し替え後も active_scene が riff を保持する"
        );
    }
}
