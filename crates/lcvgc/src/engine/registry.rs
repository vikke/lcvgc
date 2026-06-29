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

/// DSL定義ブロックの名前付きレジストリ
/// Named registry for DSL definition blocks
///
/// パース済みのデバイス・インストゥルメント・キット・クリップ・シーン・
/// セッション・変数・テンポ・スケール定義を保持し、名前で検索可能にする。
/// Holds parsed device, instrument, kit, clip, scene, session, variable,
/// tempo, and scale definitions, making them searchable by name.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    /// デバイス定義のマップ（名前 → 定義）
    /// Map of device definitions (name -> definition)
    devices: HashMap<String, DeviceDef>,
    /// `transport = true` を持つ device 名のキャッシュ（ソート済）。
    /// `transport_enabled_device_names` 経由で再生ドライバが毎 tick 参照する
    /// ため、device 登録時に `rebuild_transport_cache` で更新する。
    /// 順序を決定論的にするためソート (HashMap iteration の不安定さを排除)。
    ///
    /// Cached list of devices whose `transport` flag is `true` (sorted).
    /// The playback driver reads this every tick via
    /// `transport_enabled_device_names`, so it is rebuilt whenever a device
    /// is (re)registered. Sorted for deterministic ordering.
    transport_enabled_cache: Vec<String>,
    /// インストゥルメント定義のマップ（名前 → 定義）
    /// Map of instrument definitions (name -> definition)
    instruments: HashMap<String, InstrumentDef>,
    /// キット定義のマップ（名前 → 定義）
    /// Map of kit definitions (name -> definition)
    kits: HashMap<String, KitDef>,
    /// クリップ定義のマップ（名前 → 定義）
    /// Map of clip definitions (name -> definition)
    clips: HashMap<String, ClipDef>,
    /// シーン定義のマップ（名前 → 定義）
    /// Map of scene definitions (name -> definition)
    scenes: HashMap<String, SceneDef>,
    /// セッション定義のマップ（名前 → 定義）
    /// Map of session definitions (name -> definition)
    sessions: HashMap<String, SessionDef>,
    /// 変数のマップ（名前 → 値）
    /// Map of variables (name -> value)
    variables: HashMap<String, String>,
    /// グローバルテンポ設定
    /// Global tempo setting
    tempo: Option<Tempo>,
    /// グローバルスケール設定
    /// Global scale setting
    scale: Option<ScaleDef>,
}

impl Registry {
    /// 空のレジストリを作成する
    /// Creates a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Blockを種別に応じて登録。同名は上書き。
    /// Registers a block by its type. Overwrites if the same name exists.
    ///
    /// Play/Stop/Includeは登録対象外（falseを返す）
    /// Play/Stop/Include are not registered (returns false)
    pub fn register_block(&mut self, block: Block) -> bool {
        match block {
            Block::Device(d) => {
                self.devices.insert(d.name.clone(), d);
                self.rebuild_transport_cache();
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
            Block::Play(_)
            | Block::Stop(_)
            | Block::Pause(_)
            | Block::Resume(_)
            | Block::Mute(_)
            | Block::Unmute(_)
            | Block::Include(_) => false,
        }
    }

    /// 指定名のデバイス定義を取得する
    /// Retrieves the device definition with the given name
    pub fn get_device(&self, name: &str) -> Option<&DeviceDef> {
        self.devices.get(name)
    }

    /// 指定名のインストゥルメント定義を取得する
    /// Retrieves the instrument definition with the given name
    pub fn get_instrument(&self, name: &str) -> Option<&InstrumentDef> {
        self.instruments.get(name)
    }

    /// 指定名のキット定義を取得する
    /// Retrieves the kit definition with the given name
    pub fn get_kit(&self, name: &str) -> Option<&KitDef> {
        self.kits.get(name)
    }

    /// 指定名のクリップ定義を取得する
    /// Retrieves the clip definition with the given name
    pub fn get_clip(&self, name: &str) -> Option<&ClipDef> {
        self.clips.get(name)
    }

    /// 指定名のシーン定義を取得する
    /// Retrieves the scene definition with the given name
    pub fn get_scene(&self, name: &str) -> Option<&SceneDef> {
        self.scenes.get(name)
    }

    /// 指定名のセッション定義を取得する
    /// Retrieves the session definition with the given name
    pub fn get_session(&self, name: &str) -> Option<&SessionDef> {
        self.sessions.get(name)
    }

    /// 指定名の変数の値を取得する
    /// Retrieves the value of the variable with the given name
    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(|s| s.as_str())
    }

    /// グローバルテンポ設定を取得する
    /// Retrieves the global tempo setting
    pub fn tempo(&self) -> Option<&Tempo> {
        self.tempo.as_ref()
    }

    /// グローバルスケール設定を取得する
    /// Retrieves the global scale setting
    pub fn scale(&self) -> Option<&ScaleDef> {
        self.scale.as_ref()
    }

    /// 登録済みデバイス名の一覧を返す
    /// Returns a list of all registered device names
    pub fn device_names(&self) -> Vec<String> {
        self.devices.keys().cloned().collect()
    }

    /// `transport = true` を持つ device 名のキャッシュ済スライスを返す。
    ///
    /// 再生ドライバが毎 tick の Timing Clock 送出時に参照するため、
    /// 線形走査 / 再 allocation を避ける目的で `register_block(Device)`
    /// 経由で予め組み立てておいた結果を返す。
    ///
    /// Returns the cached slice of device names whose `transport` flag is
    /// `true`. The playback driver hits this every tick when emitting Timing
    /// Clock, so we expose a precomputed slice instead of rebuilding from
    /// the HashMap each time.
    pub fn transport_enabled_device_names(&self) -> &[String] {
        &self.transport_enabled_cache
    }

    /// `transport_enabled_cache` を `devices` から再構築する内部ヘルパ。
    /// device 登録 (`register_block`) 経由で呼ばれる。
    /// 出力はソート済で決定論的。
    ///
    /// Rebuilds `transport_enabled_cache` from `devices`. Called from
    /// `register_block` when a `Device` block is (re)registered. Output is
    /// sorted for deterministic ordering.
    fn rebuild_transport_cache(&mut self) {
        let mut names: Vec<String> = self
            .devices
            .iter()
            .filter(|(_, d)| d.transport)
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        self.transport_enabled_cache = names;
    }

    /// 登録済みインストゥルメント名の一覧を返す
    /// Returns a list of all registered instrument names
    pub fn instrument_names(&self) -> Vec<String> {
        self.instruments.keys().cloned().collect()
    }

    /// 登録済みキット名の一覧を返す
    /// Returns a list of all registered kit names
    pub fn kit_names(&self) -> Vec<String> {
        self.kits.keys().cloned().collect()
    }

    /// 登録済みクリップ名の一覧を返す
    /// Returns a list of all registered clip names
    pub fn clip_names(&self) -> Vec<String> {
        self.clips.keys().cloned().collect()
    }

    /// 登録済みシーン名の一覧を返す
    /// Returns a list of all registered scene names
    pub fn scene_names(&self) -> Vec<String> {
        self.scenes.keys().cloned().collect()
    }

    /// 登録済みセッション名の一覧を返す
    /// Returns a list of all registered session names
    pub fn session_names(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    /// 登録済み変数名の一覧を返す
    /// Returns a list of all registered variable names
    pub fn var_names(&self) -> Vec<String> {
        self.variables.keys().cloned().collect()
    }

    /// インストゥルメント定義マップ全体への参照を返す
    /// Returns a reference to the entire instrument definitions map
    pub fn instruments(&self) -> &HashMap<String, InstrumentDef> {
        &self.instruments
    }

    /// キット定義マップ全体への参照を返す
    /// Returns a reference to the entire kit definitions map
    pub fn kits(&self) -> &HashMap<String, KitDef> {
        &self.kits
    }

    /// 全フィールドが空（未登録）かどうかを返す
    /// Returns true if all fields are empty (no definitions registered)
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
            && self.instruments.is_empty()
            && self.kits.is_empty()
            && self.clips.is_empty()
            && self.scenes.is_empty()
            && self.sessions.is_empty()
            && self.variables.is_empty()
            && self.tempo.is_none()
            && self.scale.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipBody, PitchedClipBody};
    use crate::ast::include::IncludeDef;
    use crate::ast::playback::{PlayCommand, PlayTarget, RepeatSpec, StopCommand};
    use crate::ast::scale::ScaleType;
    use crate::ast::var::VarDef;
    use crate::domain::channel::MidiChannel;
    use crate::domain::pitch::NoteName;
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
    fn is_empty_on_new() {
        let reg = Registry::new();
        assert!(reg.is_empty());
    }

    #[test]
    fn is_empty_after_register() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "IAC Bus 1".into(),
            transport: true,
        }));
        assert!(!reg.is_empty());
    }

    #[test]
    fn register_device() {
        let mut reg = Registry::new();
        let result = reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "IAC Bus 1".into(),
            transport: true,
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
            transport: true,
        }));
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port2".into(),
            transport: true,
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
        }));
        assert!(result);
        let i = reg.get_instrument("piano").unwrap();
        assert_eq!(i.channel, MidiChannel::from_one_based(1).unwrap());
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

    /// transport=true の device 登録で transport_enabled_device_names に乗ること
    #[test]
    fn transport_cache_includes_transport_enabled_device() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port1".into(),
            transport: true,
        }));
        assert_eq!(reg.transport_enabled_device_names(), &["synth".to_string()]);
    }

    /// transport=false の device は cache に含まれないこと
    #[test]
    fn transport_cache_excludes_transport_disabled_device() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "drum".into(),
            port: "port1".into(),
            transport: false,
        }));
        assert!(reg.transport_enabled_device_names().is_empty());
    }

    /// 複数 device 登録で cache がソート済かつ全 transport=true device を含むこと
    #[test]
    fn transport_cache_is_sorted_and_complete() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "zeta".into(),
            port: "p1".into(),
            transport: true,
        }));
        reg.register_block(Block::Device(DeviceDef {
            name: "alpha".into(),
            port: "p2".into(),
            transport: true,
        }));
        reg.register_block(Block::Device(DeviceDef {
            name: "drum".into(),
            port: "p3".into(),
            transport: false,
        }));
        assert_eq!(
            reg.transport_enabled_device_names(),
            &["alpha".to_string(), "zeta".to_string()]
        );
    }

    /// 同名 device の上書き登録で cache が再計算されること
    /// (transport=true → 同名で transport=false に上書き → cache から消える)
    #[test]
    fn transport_cache_rebuilds_on_device_overwrite() {
        let mut reg = Registry::new();
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port1".into(),
            transport: true,
        }));
        assert_eq!(reg.transport_enabled_device_names(), &["synth".to_string()]);

        // 同名で transport=false に上書き
        reg.register_block(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "port2".into(),
            transport: false,
        }));
        assert!(reg.transport_enabled_device_names().is_empty());
    }
}
