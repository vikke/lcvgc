use lcvgc_core::ast::Block;
use crate::span_parser::{Span, SpannedBlock};

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub name_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymbolKind {
    Device,
    Instrument,
    Kit,
    Clip,
    Scene,
    Session,
    Tempo,
    Scale,
    Variable,
    Include,
    Play,
    Stop,
}

pub struct DocumentSymbolProvider;

impl DocumentSymbolProvider {
    /// SpannedBlockリストからDocumentSymbolリストを生成
    pub fn symbols(blocks: &[SpannedBlock]) -> Vec<DocumentSymbol> {
        blocks
            .iter()
            .map(|sb| {
                let (name, kind) = match &sb.block {
                    Block::Device(d) => (d.name.clone(), SymbolKind::Device),
                    Block::Instrument(i) => (i.name.clone(), SymbolKind::Instrument),
                    Block::Kit(k) => (k.name.clone(), SymbolKind::Kit),
                    Block::Clip(c) => (c.name.clone(), SymbolKind::Clip),
                    Block::Scene(s) => (s.name.clone(), SymbolKind::Scene),
                    Block::Session(s) => (s.name.clone(), SymbolKind::Session),
                    Block::Tempo(t) => (format!("{:?}", t), SymbolKind::Tempo),
                    Block::Scale(s) => {
                        (format!("{:?} {:?}", s.root, s.scale_type), SymbolKind::Scale)
                    }
                    Block::Var(v) => (v.name.clone(), SymbolKind::Variable),
                    Block::Include(i) => (i.path.clone(), SymbolKind::Include),
                    Block::Play(p) => (format!("{:?}", p.target), SymbolKind::Play),
                    Block::Stop(_) => ("stop".into(), SymbolKind::Stop),
                };
                DocumentSymbol {
                    name,
                    kind,
                    span: sb.span,
                    name_span: sb.name_span,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcvgc_core::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
    use lcvgc_core::ast::common::NoteName;
    use lcvgc_core::ast::device::DeviceDef;
    use lcvgc_core::ast::include::IncludeDef;
    use lcvgc_core::ast::playback::{PlayCommand, PlayTarget, RepeatSpec, StopCommand};
    use lcvgc_core::ast::scale::{ScaleDef, ScaleType};
    use lcvgc_core::ast::scene::SceneDef;
    use lcvgc_core::ast::session::SessionDef;
    use lcvgc_core::ast::tempo::Tempo;
    use lcvgc_core::ast::var::VarDef;
    use lcvgc_core::parser::clip_options::ClipOptions;

    fn make_spanned(block: Block, start: usize, end: usize) -> SpannedBlock {
        SpannedBlock {
            block,
            span: Span { start, end },
            name_span: None,
        }
    }

    fn make_spanned_with_name(
        block: Block,
        start: usize,
        end: usize,
        name_start: usize,
        name_end: usize,
    ) -> SpannedBlock {
        SpannedBlock {
            block,
            span: Span { start, end },
            name_span: Some(Span {
                start: name_start,
                end: name_end,
            }),
        }
    }

    #[test]
    fn empty_blocks_returns_empty_symbols() {
        let result = DocumentSymbolProvider::symbols(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_device_symbol() {
        let blocks = vec![make_spanned(
            Block::Device(DeviceDef {
                name: "synth".into(),
                port: "USB MIDI".into(),
            }),
            0,
            50,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "synth");
        assert_eq!(symbols[0].kind, SymbolKind::Device);
    }

    #[test]
    fn single_clip_symbol() {
        let blocks = vec![make_spanned(
            Block::Clip(ClipDef {
                name: "melody".into(),
                options: ClipOptions::default(),
                body: ClipBody::Pitched(PitchedClipBody { lines: vec![], cc_automations: vec![] }),
            }),
            10,
            100,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Clip);
        assert_eq!(symbols[0].name, "melody");
    }

    #[test]
    fn single_scene_symbol() {
        let blocks = vec![make_spanned(
            Block::Scene(SceneDef {
                name: "intro".into(),
                entries: vec![],
            }),
            0,
            30,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Scene);
        assert_eq!(symbols[0].name, "intro");
    }

    #[test]
    fn multiple_blocks_correct_count() {
        let blocks = vec![
            make_spanned(
                Block::Device(DeviceDef {
                    name: "d1".into(),
                    port: "p1".into(),
                }),
                0,
                10,
            ),
            make_spanned(
                Block::Device(DeviceDef {
                    name: "d2".into(),
                    port: "p2".into(),
                }),
                11,
                20,
            ),
            make_spanned(
                Block::Var(VarDef {
                    name: "x".into(),
                    value: "1".into(),
                }),
                21,
                30,
            ),
        ];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols.len(), 3);
    }

    #[test]
    fn tempo_symbol_kind() {
        let blocks = vec![make_spanned(Block::Tempo(Tempo::Absolute(120)), 0, 10)];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Tempo);
    }

    #[test]
    fn scale_symbol_kind() {
        let blocks = vec![make_spanned(
            Block::Scale(ScaleDef {
                root: NoteName::C,
                scale_type: ScaleType::Major,
            }),
            0,
            20,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Scale);
    }

    #[test]
    fn var_symbol_kind() {
        let blocks = vec![make_spanned(
            Block::Var(VarDef {
                name: "bpm".into(),
                value: "120".into(),
            }),
            0,
            15,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Variable);
        assert_eq!(symbols[0].name, "bpm");
    }

    #[test]
    fn symbols_preserve_span() {
        let blocks = vec![make_spanned(
            Block::Device(DeviceDef {
                name: "d".into(),
                port: "p".into(),
            }),
            42,
            99,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].span, Span { start: 42, end: 99 });
    }

    #[test]
    fn symbols_preserve_name_span() {
        let blocks = vec![make_spanned_with_name(
            Block::Device(DeviceDef {
                name: "d".into(),
                port: "p".into(),
            }),
            0,
            50,
            7,
            8,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].name_span, Some(Span { start: 7, end: 8 }));
    }

    #[test]
    fn include_symbol_uses_path_as_name() {
        let blocks = vec![make_spanned(
            Block::Include(IncludeDef {
                path: "lib/drums.lcvgc".into(),
            }),
            0,
            30,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Include);
        assert_eq!(symbols[0].name, "lib/drums.lcvgc");
    }

    #[test]
    fn play_and_stop_kinds() {
        let blocks = vec![
            make_spanned(
                Block::Play(PlayCommand {
                    target: PlayTarget::Scene("intro".into()),
                    repeat: RepeatSpec::Once,
                }),
                0,
                20,
            ),
            make_spanned(
                Block::Stop(StopCommand { target: None }),
                21,
                30,
            ),
        ];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Play);
        assert_eq!(symbols[1].kind, SymbolKind::Stop);
        assert_eq!(symbols[1].name, "stop");
    }

    #[test]
    fn session_symbol() {
        let blocks = vec![make_spanned(
            Block::Session(SessionDef {
                name: "live_set".into(),
                entries: vec![],
            }),
            0,
            40,
        )];
        let symbols = DocumentSymbolProvider::symbols(&blocks);
        assert_eq!(symbols[0].kind, SymbolKind::Session);
        assert_eq!(symbols[0].name, "live_set");
    }
}
