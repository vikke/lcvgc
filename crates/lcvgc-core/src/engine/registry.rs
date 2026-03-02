use std::collections::HashMap;

use crate::ast::clip::ClipDef;
use crate::ast::device::DeviceDef;
use crate::ast::instrument::InstrumentDef;
use crate::ast::kit::KitDef;
use crate::ast::scale::ScaleDef;
use crate::ast::scene::SceneDef;
use crate::ast::session::SessionDef;
use crate::ast::tempo::Tempo;
use crate::ast::Block;

#[derive(Debug, Default)]
pub struct Registry {
    devices: HashMap<String, DeviceDef>,
    instruments: HashMap<String, InstrumentDef>,
    kits: HashMap<String, KitDef>,
    clips: HashMap<String, ClipDef>,
    scenes: HashMap<String, SceneDef>,
    sessions: HashMap<String, SessionDef>,
    variables: HashMap<String, String>,
    tempo: Option<Tempo>,
    scale: Option<ScaleDef>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Blockを種別に応じて登録。同名は上書き。
    /// Play/Stop/Includeは登録対象外（falseを返す）
    pub fn register_block(&mut self, block: Block) -> bool {
        match block {
            Block::Device(d) => {
                self.devices.insert(d.name.clone(), d);
                true
            }
            Block::Instrument(i) => {
                self.instruments.insert(i.name.clone(), i);
                true
            }
            Block::Kit(k) => {
                self.kits.insert(k.name.clone(), k);
                true
            }
            Block::Clip(c) => {
                self.clips.insert(c.name.clone(), c);
                true
            }
            Block::Scene(s) => {
                self.scenes.insert(s.name.clone(), s);
                true
            }
            Block::Session(s) => {
                self.sessions.insert(s.name.clone(), s);
                true
            }
            Block::Var(v) => {
                self.variables.insert(v.name.clone(), v.value);
                true
            }
            Block::Tempo(t) => {
                self.tempo = Some(t);
                true
            }
            Block::Scale(s) => {
                self.scale = Some(s);
                true
            }
            Block::Play(_) | Block::Stop(_) | Block::Include(_) => false,
        }
    }

    pub fn get_device(&self, name: &str) -> Option<&DeviceDef> {
        self.devices.get(name)
    }

    pub fn get_instrument(&self, name: &str) -> Option<&InstrumentDef> {
        self.instruments.get(name)
    }

    pub fn get_kit(&self, name: &str) -> Option<&KitDef> {
        self.kits.get(name)
    }

    pub fn get_clip(&self, name: &str) -> Option<&ClipDef> {
        self.clips.get(name)
    }

    pub fn get_scene(&self, name: &str) -> Option<&SceneDef> {
        self.scenes.get(name)
    }

    pub fn get_session(&self, name: &str) -> Option<&SessionDef> {
        self.sessions.get(name)
    }

    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(|s| s.as_str())
    }

    pub fn tempo(&self) -> Option<&Tempo> {
        self.tempo.as_ref()
    }

    pub fn scale(&self) -> Option<&ScaleDef> {
        self.scale.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipBody, PitchedClipBody};
    use crate::ast::common::NoteName;
    use crate::ast::include::IncludeDef;
    use crate::ast::playback::{PlayCommand, PlayTarget, RepeatSpec, StopCommand};
    use crate::ast::scale::ScaleType;
    use crate::ast::var::VarDef;
    use crate::parser::clip_options::ClipOptions;

    #[test]
    fn new_is_empty() {
        let reg = Registry::new();
        assert!(reg.get_device("any").is_none());
        assert!(reg.get_instrument("any").is_none());
        assert!(reg.get_kit("any").is_none());
        assert!(reg.get_clip("any").is_none());
        assert!(reg.get_scene("any").is_none());
        assert!(reg.get_session("any").is_none());
        assert!(reg.get_var("any").is_none());
        assert!(reg.tempo().is_none());
        assert!(reg.scale().is_none());
    }

    #[test]
    fn register_device() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "IAC Bus 1".into(),
        }));
        assert!(result);
        let d = reg.get_device("synth").unwrap();
        assert_eq!(d.port, "IAC Bus 1");
    }

    #[test]
    fn device_overwrite() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port1".into(),
        }));
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port2".into(),
        }));
        assert_eq!(reg.get_device("synth").unwrap().port, "port2");
    }

    #[test]
    fn get_device_unknown() {
        let reg = Registry::new();
        assert!(reg.get_device("unknown").is_none());
    }

    #[test]
    fn register_instrument() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Instrument(InstrumentDef {
            name: "piano".into(),
            device: "synth".into(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
        }));
        assert!(result);
        let i = reg.get_instrument("piano").unwrap();
        assert_eq!(i.channel, 1);
    }

    #[test]
    fn register_kit() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Kit(KitDef {
            name: "drums".into(),
            device: "synth".into(),
            instruments: vec![],
        }));
        assert!(result);
        assert!(reg.get_kit("drums").is_some());
    }

    #[test]
    fn register_clip() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Clip(ClipDef {
            name: "intro".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        }));
        assert!(result);
        assert!(reg.get_clip("intro").is_some());
    }

    #[test]
    fn register_scene() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![],
        }));
        assert!(result);
        assert!(reg.get_scene("verse").is_some());
    }

    #[test]
    fn register_session() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Session(SessionDef {
            name: "main".into(),
            entries: vec![],
        }));
        assert!(result);
        assert!(reg.get_session("main").is_some());
    }

    #[test]
    fn register_var() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Var(VarDef {
            name: "bpm".into(),
            value: "120".into(),
        }));
        assert!(result);
        assert_eq!(reg.get_var("bpm"), Some("120"));
    }

    #[test]
    fn register_tempo() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Tempo(Tempo::Absolute(140)));
        assert!(result);
        assert_eq!(reg.tempo(), Some(&Tempo::Absolute(140)));
    }

    #[test]
    fn register_scale() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Scale(ScaleDef {
            root: NoteName::C,
            scale_type: ScaleType::Major,
        }));
        assert!(result);
        let s = reg.scale().unwrap();
        assert_eq!(s.root, NoteName::C);
    }

    #[test]
    fn play_not_registered() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Play(PlayCommand {
            target: PlayTarget::Scene("verse".into()),
            repeat: RepeatSpec::Once,
        }));
        assert!(!result);
    }

    #[test]
    fn stop_not_registered() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Stop(StopCommand { target: None }));
        assert!(!result);
    }

    #[test]
    fn include_not_registered() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Include(IncludeDef {
            path: "other.lcvgc".into(),
        }));
        assert!(!result);
    }
}
