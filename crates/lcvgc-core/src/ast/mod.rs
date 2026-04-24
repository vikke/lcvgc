/// ASTモジュール: DSLの抽象構文木を定義する
/// AST module: defines the abstract syntax tree for the DSL
pub mod clip;
pub mod clip_cc;
pub mod clip_drum;
pub mod clip_note;
pub mod common;
pub mod device;
pub mod include;
pub mod instrument;
pub mod kit;
pub mod playback;
pub mod scale;
pub mod scene;
pub mod session;
pub mod tempo;
pub mod unresolved;
pub mod var;

use clip::ClipDef;
use device::DeviceDef;
use include::IncludeDef;
use instrument::InstrumentDef;
use kit::KitDef;
use playback::{PauseCommand, PlayCommand, ResumeCommand, StopCommand};
use scale::ScaleDef;
use scene::SceneDef;
use session::SessionDef;
use tempo::Tempo;
use var::VarDef;

/// DSLのトップレベルブロック
/// A top-level block in the DSL.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// デバイス定義ブロック
    /// Device definition block
    Device(DeviceDef),
    /// インストゥルメント定義ブロック
    /// Instrument definition block
    Instrument(InstrumentDef),
    /// キット定義ブロック
    /// Kit definition block
    Kit(KitDef),
    /// クリップ定義ブロック
    /// Clip definition block
    Clip(ClipDef),
    /// シーン定義ブロック
    /// Scene definition block
    Scene(SceneDef),
    /// セッション定義ブロック
    /// Session definition block
    Session(SessionDef),
    /// テンポ設定ブロック
    /// Tempo setting block
    Tempo(Tempo),
    /// スケール定義ブロック
    /// Scale definition block
    Scale(ScaleDef),
    /// 変数定義ブロック
    /// Variable definition block
    Var(VarDef),
    /// インクルード定義ブロック
    /// Include definition block
    Include(IncludeDef),
    /// 再生コマンドブロック
    /// Play command block
    Play(PlayCommand),
    /// 停止コマンドブロック
    /// Stop command block
    Stop(StopCommand),
    /// ポーズコマンドブロック（§10.4）
    /// Pause command block (§10.4)
    Pause(PauseCommand),
    /// 再開コマンドブロック（§10.4）
    /// Resume command block (§10.4)
    Resume(ResumeCommand),
}
