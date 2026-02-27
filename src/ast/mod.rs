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
pub mod var;

pub use common::*;

use clip::ClipDef;
use device::DeviceDef;
use include::IncludeDef;
use instrument::InstrumentDef;
use kit::KitDef;
use playback::{PlayCommand, StopCommand};
use scale::ScaleDef;
use scene::SceneDef;
use session::SessionDef;
use tempo::Tempo;
use var::VarDef;

/// A top-level block in the DSL.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Device(DeviceDef),
    Instrument(InstrumentDef),
    Kit(KitDef),
    Clip(ClipDef),
    Scene(SceneDef),
    Session(SessionDef),
    Tempo(Tempo),
    Scale(ScaleDef),
    Var(VarDef),
    Include(IncludeDef),
    Play(PlayCommand),
    Stop(StopCommand),
}
