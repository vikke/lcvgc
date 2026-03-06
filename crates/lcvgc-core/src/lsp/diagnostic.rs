use std::path::Path;

use super::span_parser::{Span, SpanError, SpannedBlock};
/// 診断プロバイダ（パースエラー＋未定義参照）
use crate::ast::clip::ClipBody;
use crate::ast::scene::SceneEntry;
use crate::ast::Block;
use crate::engine::registry::Registry;

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub severity: DiagnosticSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

pub struct DiagnosticProvider;

impl DiagnosticProvider {
    /// パースエラー→Diagnostic変換
    pub fn from_parse_errors(errors: &[SpanError]) -> Vec<Diagnostic> {
        errors
            .iter()
            .map(|e| Diagnostic {
                span: e.span,
                message: e.message.clone(),
                severity: DiagnosticSeverity::Error,
            })
            .collect()
    }

    /// 未定義参照の検出
    pub fn undefined_references(blocks: &[SpannedBlock], registry: &Registry) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for sb in blocks {
            match &sb.block {
                Block::Scene(scene) => {
                    for entry in &scene.entries {
                        if let SceneEntry::Clip {
                            candidates,
                            probability: _,
                        } = entry
                        {
                            for candidate in candidates {
                                if registry.get_clip(&candidate.clip).is_none() {
                                    diagnostics.push(Diagnostic {
                                        span: sb.span,
                                        message: format!("未定義のクリップ: '{}'", candidate.clip),
                                        severity: DiagnosticSeverity::Error,
                                    });
                                }
                            }
                        }
                    }
                }
                Block::Session(session) => {
                    for entry in &session.entries {
                        if registry.get_scene(&entry.scene).is_none() {
                            diagnostics.push(Diagnostic {
                                span: sb.span,
                                message: format!("未定義のシーン: '{}'", entry.scene),
                                severity: DiagnosticSeverity::Error,
                            });
                        }
                    }
                }
                Block::Clip(clip) => {
                    if let ClipBody::Pitched(body) = &clip.body {
                        for line in &body.lines {
                            if registry.get_instrument(&line.instrument).is_none()
                                && registry.get_kit(&line.instrument).is_none()
                            {
                                diagnostics.push(Diagnostic {
                                    span: sb.span,
                                    message: format!(
                                        "未定義のインストゥルメント: '{}'",
                                        line.instrument
                                    ),
                                    severity: DiagnosticSeverity::Warning,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        diagnostics
    }

    /// インクルードファイルの存在チェック
    /// Checks if included files exist
    ///
    /// # Arguments
    /// * `blocks` - スパン付きブロックのスライス / Slice of spanned blocks
    /// * `base_path` - ベースディレクトリのパス / Base directory path
    ///
    /// # Returns
    /// ファイルが存在しない場合のWarning診断リスト / List of Warning diagnostics for missing files
    pub fn include_diagnostics(blocks: &[SpannedBlock], base_path: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for sb in blocks {
            if let Block::Include(inc) = &sb.block {
                let full_path = base_path.join(&inc.path);
                if !full_path.exists() {
                    diagnostics.push(Diagnostic {
                        span: sb.span,
                        message: format!("インクルードファイル未検出: '{}'", inc.path),
                        severity: DiagnosticSeverity::Warning,
                    });
                }
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::clip::{ClipBody, ClipDef, PitchedClipBody, PitchedLine};
    use crate::ast::instrument::InstrumentDef;
    use crate::ast::scene::{SceneDef, SceneEntry, ShuffleCandidate};
    use crate::ast::session::{SessionDef, SessionEntry, SessionRepeat};
    use crate::ast::tempo::Tempo;
    use crate::parser::clip_options::ClipOptions;

    fn make_span(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    fn make_clip_block(name: &str, instruments: &[&str]) -> Block {
        Block::Clip(ClipDef {
            name: name.into(),
            options: ClipOptions::default(),
            body: ClipBody::Pitched(PitchedClipBody {
                lines: instruments
                    .iter()
                    .map(|i| PitchedLine {
                        instrument: (*i).into(),
                        elements: vec![],
                    })
                    .collect(),
                cc_automations: vec![],
            }),
        })
    }

    fn make_scene_block(name: &str, clip_names: &[&str]) -> Block {
        Block::Scene(SceneDef {
            name: name.into(),
            entries: clip_names
                .iter()
                .map(|c| SceneEntry::Clip {
                    candidates: vec![ShuffleCandidate {
                        clip: (*c).into(),
                        weight: 1,
                    }],
                    probability: None,
                })
                .collect(),
        })
    }

    fn make_session_block(name: &str, scene_names: &[&str]) -> Block {
        Block::Session(SessionDef {
            name: name.into(),
            entries: scene_names
                .iter()
                .map(|s| SessionEntry {
                    scene: (*s).into(),
                    repeat: SessionRepeat::Once,
                })
                .collect(),
        })
    }

    fn spanned(block: Block) -> SpannedBlock {
        SpannedBlock {
            block,
            span: make_span(0, 10),
            name_span: None,
        }
    }

    #[test]
    fn from_parse_errors_empty() {
        let result = DiagnosticProvider::from_parse_errors(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn from_parse_errors_converts() {
        let errors = vec![SpanError {
            span: make_span(5, 15),
            message: "unexpected token".into(),
        }];
        let diags = DiagnosticProvider::from_parse_errors(&errors);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn from_parse_errors_preserves_span_and_message() {
        let errors = vec![SpanError {
            span: make_span(10, 20),
            message: "parse error".into(),
        }];
        let diags = DiagnosticProvider::from_parse_errors(&errors);
        assert_eq!(diags[0].span, make_span(10, 20));
        assert_eq!(diags[0].message, "parse error");
    }

    #[test]
    fn undefined_refs_empty() {
        let reg = Registry::new();
        let result = DiagnosticProvider::undefined_references(&[], &reg);
        assert!(result.is_empty());
    }

    #[test]
    fn scene_refs_existing_clip() {
        let mut reg = Registry::new();
        reg.register_block(make_clip_block("intro", &[]));
        let blocks = vec![spanned(make_scene_block("s1", &["intro"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert!(diags.is_empty());
    }

    #[test]
    fn scene_refs_missing_clip() {
        let reg = Registry::new();
        let blocks = vec![spanned(make_scene_block("s1", &["missing"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert!(diags[0].message.contains("missing"));
    }

    #[test]
    fn session_refs_existing_scene() {
        let mut reg = Registry::new();
        reg.register_block(Block::Scene(SceneDef {
            name: "verse".into(),
            entries: vec![],
        }));
        let blocks = vec![spanned(make_session_block("main", &["verse"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert!(diags.is_empty());
    }

    #[test]
    fn session_refs_missing_scene() {
        let reg = Registry::new();
        let blocks = vec![spanned(make_session_block("main", &["ghost"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
        assert!(diags[0].message.contains("ghost"));
    }

    #[test]
    fn clip_refs_existing_instrument() {
        let mut reg = Registry::new();
        reg.register_block(Block::Instrument(InstrumentDef {
            name: "piano".into(),
            device: "synth".into(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
        }));
        let blocks = vec![spanned(make_clip_block("c1", &["piano"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert!(diags.is_empty());
    }

    #[test]
    fn clip_refs_missing_instrument() {
        let reg = Registry::new();
        let blocks = vec![spanned(make_clip_block("c1", &["unknown"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert!(diags[0].message.contains("unknown"));
    }

    #[test]
    fn multiple_undefined_refs() {
        let reg = Registry::new();
        let blocks = vec![spanned(make_scene_block("s1", &["a", "b", "c"]))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn tempo_block_no_diagnostics() {
        let reg = Registry::new();
        let blocks = vec![spanned(Block::Tempo(Tempo::Absolute(120)))];
        let diags = DiagnosticProvider::undefined_references(&blocks, &reg);
        assert!(diags.is_empty());
    }

    #[test]
    fn include_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub.cvg");
        std::fs::write(&file, "tempo 120").unwrap();

        let blocks = vec![spanned(Block::Include(crate::ast::include::IncludeDef {
            path: "sub.cvg".into(),
        }))];
        let diags = DiagnosticProvider::include_diagnostics(&blocks, dir.path());
        assert!(diags.is_empty());
    }

    #[test]
    fn include_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let blocks = vec![spanned(Block::Include(crate::ast::include::IncludeDef {
            path: "missing.cvg".into(),
        }))];
        let diags = DiagnosticProvider::include_diagnostics(&blocks, dir.path());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, DiagnosticSeverity::Warning);
        assert!(diags[0].message.contains("missing.cvg"));
    }
}
