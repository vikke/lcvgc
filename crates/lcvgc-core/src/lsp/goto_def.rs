//! 定義ジャンプ（Go to Definition）プロバイダモジュール
//! Go to definition provider module
//!
//! 識別子名からブロック定義位置を検索し、
//! LSPの定義ジャンプ機能を提供する。
//! Searches for block definition positions by identifier name
//! and provides the LSP go to definition feature.

use super::span_parser::{Span, SpannedBlock};
use crate::ast::Block;

/// 定義ジャンププロバイダ
/// Go to definition provider
///
/// ブロック名から定義位置（`Span`）を検索する静的メソッドを提供する。
/// Provides static methods to search for definition positions (`Span`) by block name.
pub struct GotoDefinitionProvider;

impl GotoDefinitionProvider {
    /// 指定名前に一致するブロック定義の位置を返す
    /// Returns the position of the block definition matching the given name
    ///
    /// 名前付きブロック（device, instrument, kit, clip, scene, session, var）を
    /// 走査し、最初に一致したブロックの `name_span`（存在しない場合は `span`）を返す。
    /// Scans named blocks (device, instrument, kit, clip, scene, session, var)
    /// and returns the `name_span` (or `span` if absent) of the first match.
    ///
    /// # Arguments
    /// * `name` - 検索する識別子名 / Identifier name to search for
    /// * `blocks` - スパン付きブロック一覧 / List of spanned blocks
    ///
    /// # Returns
    /// 定義が見つかった場合は `Some(Span)`、見つからない場合は `None`
    /// `Some(Span)` if a definition is found, `None` otherwise
    pub fn find_definition(name: &str, blocks: &[SpannedBlock]) -> Option<Span> {
        for sb in blocks {
            let block_name = match &sb.block {
                Block::Device(d) => Some(d.name.as_str()),
                Block::Instrument(i) => Some(i.name.as_str()),
                Block::Kit(k) => Some(k.name.as_str()),
                Block::Clip(c) => Some(c.name.as_str()),
                Block::Scene(s) => Some(s.name.as_str()),
                Block::Session(s) => Some(s.name.as_str()),
                Block::Var(v) => Some(v.name.as_str()),
                _ => None,
            };
            if block_name == Some(name) {
                return sb.name_span.or(Some(sb.span));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::span_parser::{Span, SpannedBlock};
    use super::*;
    use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody};
    use crate::ast::device::DeviceDef;
    use crate::ast::instrument::InstrumentDef;
    use crate::ast::scene::SceneDef;
    use crate::ast::tempo::Tempo;
    use crate::ast::var::VarDef;
    use crate::parser::clip_options::ClipOptions;

    fn device_block(name: &str, span: Span, name_span: Option<Span>) -> SpannedBlock {
        SpannedBlock {
            block: Block::Device(DeviceDef {
                name: name.into(),
                port: "port".into(),
            }),
            span,
            name_span,
        }
    }

    fn clip_block(name: &str, span: Span, name_span: Option<Span>) -> SpannedBlock {
        SpannedBlock {
            block: Block::Clip(ClipDef {
                name: name.into(),
                options: ClipOptions::default(),
                body: ClipBody::Pitched(PitchedClipBody {
                    lines: vec![],
                    cc_automations: vec![],
                }),
            }),
            span,
            name_span,
        }
    }

    fn instrument_block(name: &str, span: Span, name_span: Option<Span>) -> SpannedBlock {
        SpannedBlock {
            block: Block::Instrument(InstrumentDef {
                name: name.into(),
                device: "dev".into(),
                channel: 1,
                note: None,
                gate_normal: None,
                gate_staccato: None,
                cc_mappings: vec![],
                local_vars: vec![],
            }),
            span,
            name_span,
        }
    }

    fn scene_block(name: &str, span: Span, name_span: Option<Span>) -> SpannedBlock {
        SpannedBlock {
            block: Block::Scene(SceneDef {
                name: name.into(),
                entries: vec![],
            }),
            span,
            name_span,
        }
    }

    #[test]
    fn find_existing_device_returns_name_span() {
        let blocks = vec![device_block(
            "synth",
            Span { start: 0, end: 50 },
            Some(Span { start: 7, end: 12 }),
        )];
        let result = GotoDefinitionProvider::find_definition("synth", &blocks);
        assert_eq!(result, Some(Span { start: 7, end: 12 }));
    }

    #[test]
    fn find_existing_clip_returns_span() {
        let blocks = vec![clip_block("riff", Span { start: 0, end: 100 }, None)];
        let result = GotoDefinitionProvider::find_definition("riff", &blocks);
        assert_eq!(result, Some(Span { start: 0, end: 100 }));
    }

    #[test]
    fn find_non_existing_returns_none() {
        let blocks = vec![device_block("synth", Span { start: 0, end: 50 }, None)];
        let result = GotoDefinitionProvider::find_definition("unknown", &blocks);
        assert_eq!(result, None);
    }

    #[test]
    fn find_in_empty_blocks_returns_none() {
        let result = GotoDefinitionProvider::find_definition("anything", &[]);
        assert_eq!(result, None);
    }

    #[test]
    fn find_scene_by_name() {
        let blocks = vec![scene_block(
            "intro",
            Span { start: 10, end: 60 },
            Some(Span { start: 16, end: 21 }),
        )];
        let result = GotoDefinitionProvider::find_definition("intro", &blocks);
        assert_eq!(result, Some(Span { start: 16, end: 21 }));
    }

    #[test]
    fn find_instrument_by_name() {
        let blocks = vec![instrument_block(
            "piano",
            Span { start: 0, end: 80 },
            Some(Span { start: 11, end: 16 }),
        )];
        let result = GotoDefinitionProvider::find_definition("piano", &blocks);
        assert_eq!(result, Some(Span { start: 11, end: 16 }));
    }

    #[test]
    fn returns_name_span_when_available() {
        let blocks = vec![device_block(
            "dev",
            Span { start: 0, end: 50 },
            Some(Span { start: 7, end: 10 }),
        )];
        let result = GotoDefinitionProvider::find_definition("dev", &blocks);
        assert_eq!(result, Some(Span { start: 7, end: 10 }));
    }

    #[test]
    fn returns_block_span_when_name_span_is_none() {
        let blocks = vec![device_block("dev", Span { start: 5, end: 55 }, None)];
        let result = GotoDefinitionProvider::find_definition("dev", &blocks);
        assert_eq!(result, Some(Span { start: 5, end: 55 }));
    }

    #[test]
    fn find_var_by_name() {
        let blocks = vec![SpannedBlock {
            block: Block::Var(VarDef {
                name: "bpm".into(),
                value: "120".into(),
            }),
            span: Span { start: 0, end: 20 },
            name_span: Some(Span { start: 4, end: 7 }),
        }];
        let result = GotoDefinitionProvider::find_definition("bpm", &blocks);
        assert_eq!(result, Some(Span { start: 4, end: 7 }));
    }

    #[test]
    fn tempo_block_is_skipped() {
        let blocks = vec![SpannedBlock {
            block: Block::Tempo(Tempo::Absolute(120)),
            span: Span { start: 0, end: 15 },
            name_span: None,
        }];
        // Tempo has no name, so searching any name should return None
        let result = GotoDefinitionProvider::find_definition("tempo", &blocks);
        assert_eq!(result, None);
    }

    #[test]
    fn finds_first_matching_block() {
        let blocks = vec![
            device_block(
                "synth",
                Span { start: 0, end: 50 },
                Some(Span { start: 7, end: 12 }),
            ),
            device_block(
                "synth",
                Span {
                    start: 60,
                    end: 110,
                },
                Some(Span { start: 67, end: 72 }),
            ),
        ];
        let result = GotoDefinitionProvider::find_definition("synth", &blocks);
        assert_eq!(result, Some(Span { start: 7, end: 12 }));
    }
}
