//! ホバー情報プロバイダモジュール
//!
//! カーソル位置のブロックに対して、Markdown形式のホバー情報を生成する。
//! デバイス・インストゥルメント・クリップ等の詳細情報を提供する。

use super::span_parser::SpannedBlock;
use crate::ast::clip::ClipBody;
use crate::ast::Block;

/// ホバー情報プロバイダ
///
/// `SpannedBlock` からMarkdown形式のホバーテキストを生成する。
pub struct HoverProvider;

impl HoverProvider {
    /// スパン付きブロックからホバー用Markdownテキストを生成する
    ///
    /// ブロック種別に応じて、名前・ポート・チャンネル・エントリ数等の
    /// 詳細情報を含むMarkdown文字列を返す。
    /// play, stop, include ブロックにはホバー情報は提供しない。
    ///
    /// # Arguments
    /// * `sb` - スパン付きブロック
    ///
    /// # Returns
    /// Markdown形式のホバーテキスト。対応しないブロック種別の場合は `None`
    pub fn hover_content(sb: &SpannedBlock) -> Option<String> {
        match &sb.block {
            Block::Device(d) => Some(format!("**device** `{}`\n- port: `\"{}\"`", d.name, d.port)),
            Block::Instrument(i) => {
                let mut s = format!(
                    "**instrument** `{}`\n- device: `{}`\n- channel: `{}`",
                    i.name, i.device, i.channel
                );
                if let Some(gn) = i.gate_normal {
                    s += &format!("\n- gate_normal: `{}%`", gn);
                }
                if let Some(gs) = i.gate_staccato {
                    s += &format!("\n- gate_staccato: `{}%`", gs);
                }
                if !i.cc_mappings.is_empty() {
                    let cc_str: Vec<String> = i
                        .cc_mappings
                        .iter()
                        .map(|m| format!("{} → CC {}", m.alias, m.cc_number))
                        .collect();
                    s += &format!("\n- CC: {}", cc_str.join(", "));
                }
                Some(s)
            }
            Block::Kit(k) => Some(format!(
                "**kit** `{}`\n- device: `{}`\n- instruments: {}",
                k.name,
                k.device,
                k.instruments.len()
            )),
            Block::Clip(c) => {
                let body_type = match &c.body {
                    ClipBody::Pitched(_) => "pitched",
                    ClipBody::Drum(_) => "drum",
                };
                let mut s = format!("**clip** `{}`\n- type: {}", c.name, body_type);
                if let Some(bars) = c.options.bars {
                    s += &format!("\n- bars: {}", bars);
                }
                Some(s)
            }
            Block::Scene(scene) => Some(format!(
                "**scene** `{}`\n- entries: {}",
                scene.name,
                scene.entries.len()
            )),
            Block::Session(session) => Some(format!(
                "**session** `{}`\n- entries: {}",
                session.name,
                session.entries.len()
            )),
            Block::Tempo(t) => Some(format!("**tempo** `{:?}`", t)),
            Block::Scale(s) => Some(format!("**scale** `{:?} {:?}`", s.root, s.scale_type)),
            Block::Var(v) => Some(format!("**var** `{}` = `{}`", v.name, v.value)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::span_parser::{Span, SpannedBlock};
    use super::*;
    use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
    use crate::ast::common::NoteName;
    use crate::ast::device::DeviceDef;
    use crate::ast::include::IncludeDef;
    use crate::ast::instrument::{CcMapping, InstrumentDef};
    use crate::ast::kit::{KitDef, KitInstrument, KitInstrumentNote};
    use crate::ast::playback::{PlayCommand, PlayTarget, RepeatSpec, StopCommand};
    use crate::ast::scale::{ScaleDef, ScaleType};
    use crate::ast::scene::SceneDef;
    use crate::ast::session::SessionDef;
    use crate::ast::tempo::Tempo;
    use crate::ast::var::VarDef;
    use crate::parser::clip_options::ClipOptions;

    fn sb(block: Block) -> SpannedBlock {
        SpannedBlock {
            block,
            span: Span { start: 0, end: 10 },
            name_span: None,
        }
    }

    #[test]
    fn device_hover_shows_port() {
        let result = HoverProvider::hover_content(&sb(Block::Device(DeviceDef {
            name: "synth".into(),
            port: "USB MIDI".into(),
        })));
        let text = result.unwrap();
        assert!(text.contains("**device** `synth`"));
        assert!(text.contains("port: `\"USB MIDI\"`"));
    }

    #[test]
    fn instrument_hover_shows_channel_and_device() {
        let result = HoverProvider::hover_content(&sb(Block::Instrument(InstrumentDef {
            name: "piano".into(),
            device: "synth".into(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
        })));
        let text = result.unwrap();
        assert!(text.contains("device: `synth`"));
        assert!(text.contains("channel: `1`"));
    }

    #[test]
    fn instrument_hover_with_cc_mappings() {
        let result = HoverProvider::hover_content(&sb(Block::Instrument(InstrumentDef {
            name: "piano".into(),
            device: "synth".into(),
            channel: 1,
            note: None,
            gate_normal: Some(80),
            gate_staccato: Some(40),
            cc_mappings: vec![CcMapping {
                alias: "mod".into(),
                cc_number: 1,
            }],
        })));
        let text = result.unwrap();
        assert!(text.contains("gate_normal: `80%`"));
        assert!(text.contains("gate_staccato: `40%`"));
        assert!(text.contains("mod → CC 1"));
    }

    #[test]
    fn kit_hover_shows_device_and_count() {
        let result = HoverProvider::hover_content(&sb(Block::Kit(KitDef {
            name: "drums".into(),
            device: "drum_machine".into(),
            instruments: vec![KitInstrument {
                name: "kick".into(),
                channel: 10,
                note: KitInstrumentNote {
                    name: NoteName::C,
                    octave: 2,
                },
                gate_normal: None,
                gate_staccato: None,
            }],
        })));
        let text = result.unwrap();
        assert!(text.contains("**kit** `drums`"));
        assert!(text.contains("instruments: 1"));
    }

    #[test]
    fn clip_pitched_hover_shows_type() {
        let result = HoverProvider::hover_content(&sb(Block::Clip(ClipDef {
            name: "riff".into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: vec![],
                cc_automations: vec![],
            }),
        })));
        let text = result.unwrap();
        assert!(text.contains("type: pitched"));
    }

    #[test]
    fn scene_hover_shows_entry_count() {
        let result = HoverProvider::hover_content(&sb(Block::Scene(SceneDef {
            name: "intro".into(),
            entries: vec![],
        })));
        let text = result.unwrap();
        assert!(text.contains("**scene** `intro`"));
        assert!(text.contains("entries: 0"));
    }

    #[test]
    fn session_hover_shows_entry_count() {
        let result = HoverProvider::hover_content(&sb(Block::Session(SessionDef {
            name: "song".into(),
            entries: vec![],
        })));
        let text = result.unwrap();
        assert!(text.contains("**session** `song`"));
        assert!(text.contains("entries: 0"));
    }

    #[test]
    fn tempo_hover_shows_value() {
        let result = HoverProvider::hover_content(&sb(Block::Tempo(Tempo::Absolute(120))));
        let text = result.unwrap();
        assert!(text.contains("**tempo**"));
        assert!(text.contains("Absolute(120)"));
    }

    #[test]
    fn scale_hover_shows_root_and_type() {
        let result = HoverProvider::hover_content(&sb(Block::Scale(ScaleDef {
            root: NoteName::C,
            scale_type: ScaleType::Major,
        })));
        let text = result.unwrap();
        assert!(text.contains("**scale**"));
        assert!(text.contains("Major"));
    }

    #[test]
    fn var_hover_shows_name_and_value() {
        let result = HoverProvider::hover_content(&sb(Block::Var(VarDef {
            name: "bpm".into(),
            value: "120".into(),
        })));
        let text = result.unwrap();
        assert!(text.contains("**var** `bpm` = `120`"));
    }

    #[test]
    fn play_returns_none() {
        let result = HoverProvider::hover_content(&sb(Block::Play(PlayCommand {
            target: PlayTarget::Scene("intro".into()),
            repeat: RepeatSpec::Once,
        })));
        assert!(result.is_none());
    }

    #[test]
    fn stop_returns_none() {
        let result = HoverProvider::hover_content(&sb(Block::Stop(StopCommand { target: None })));
        assert!(result.is_none());
    }

    #[test]
    fn include_returns_none() {
        let result = HoverProvider::hover_content(&sb(Block::Include(IncludeDef {
            path: "other.lcvgc".into(),
        })));
        assert!(result.is_none());
    }
}
