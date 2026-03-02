use lcvgc_core::ast::common::NoteName;
use lcvgc_core::ast::instrument::InstrumentDef;
use lcvgc_core::ast::scale::ScaleType;
use crate::diatonic;

#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionKind {
    Keyword,
    NoteName,
    ChordName,
    CcAlias,
    Identifier,
}

pub struct CompletionProvider;

impl CompletionProvider {
    pub fn keyword_completions() -> Vec<CompletionItem> {
        [
            "device",
            "instrument",
            "kit",
            "clip",
            "scene",
            "session",
            "tempo",
            "scale",
            "var",
            "include",
            "play",
            "stop",
        ]
        .iter()
        .map(|kw| CompletionItem {
            label: kw.to_string(),
            detail: None,
            kind: CompletionKind::Keyword,
        })
        .collect()
    }

    pub fn note_completions() -> Vec<CompletionItem> {
        [
            "c", "c#", "db", "d", "d#", "eb", "e", "f", "f#", "gb", "g", "g#", "ab", "a", "a#",
            "bb", "b",
        ]
        .iter()
        .map(|n| CompletionItem {
            label: n.to_string(),
            detail: None,
            kind: CompletionKind::NoteName,
        })
        .collect()
    }

    pub fn standard_cc_completions() -> Vec<CompletionItem> {
        [
            (1, "Modulation"),
            (7, "Volume"),
            (10, "Pan"),
            (11, "Expression"),
            (64, "Sustain"),
            (71, "Resonance"),
            (74, "Cutoff"),
        ]
        .iter()
        .map(|(cc, name)| CompletionItem {
            label: name.to_string(),
            detail: Some(format!("CC {}", cc)),
            kind: CompletionKind::CcAlias,
        })
        .collect()
    }

    pub fn instrument_cc_completions(instrument: &InstrumentDef) -> Vec<CompletionItem> {
        instrument
            .cc_mappings
            .iter()
            .map(|m| CompletionItem {
                label: m.alias.clone(),
                detail: Some(format!("CC {}", m.cc_number)),
                kind: CompletionKind::CcAlias,
            })
            .collect()
    }

    pub fn identifier_completions(names: &[String], kind_label: &str) -> Vec<CompletionItem> {
        names
            .iter()
            .map(|name| CompletionItem {
                label: name.clone(),
                detail: Some(kind_label.to_string()),
                kind: CompletionKind::Identifier,
            })
            .collect()
    }

    pub fn diatonic_completions(root: NoteName, scale_type: ScaleType) -> Vec<CompletionItem> {
        diatonic::diatonic_chords(root, scale_type)
            .into_iter()
            .map(|chord| CompletionItem {
                label: chord.label,
                detail: Some(chord.detail),
                kind: CompletionKind::ChordName,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcvgc_core::ast::instrument::CcMapping;

    #[test]
    fn test_keyword_completions_count() {
        assert_eq!(CompletionProvider::keyword_completions().len(), 12);
    }

    #[test]
    fn test_keyword_completions_contains_device() {
        let items = CompletionProvider::keyword_completions();
        assert!(items.iter().any(|i| i.label == "device"));
    }

    #[test]
    fn test_note_completions_count() {
        assert_eq!(CompletionProvider::note_completions().len(), 17);
    }

    #[test]
    fn test_note_completions_contains_sharp() {
        let items = CompletionProvider::note_completions();
        assert!(items.iter().any(|i| i.label == "c#"));
    }

    #[test]
    fn test_note_completions_contains_flat() {
        let items = CompletionProvider::note_completions();
        assert!(items.iter().any(|i| i.label == "eb"));
    }

    #[test]
    fn test_standard_cc_contains_modulation() {
        let items = CompletionProvider::standard_cc_completions();
        assert!(items.iter().any(|i| i.label == "Modulation"));
    }

    #[test]
    fn test_instrument_cc_with_mappings() {
        let inst = InstrumentDef {
            name: "synth".to_string(),
            device: "dev".to_string(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![CcMapping {
                alias: "cutoff".to_string(),
                cc_number: 74,
            }],
        };
        let items = CompletionProvider::instrument_cc_completions(&inst);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "cutoff");
        assert_eq!(items[0].detail, Some("CC 74".to_string()));
    }

    #[test]
    fn test_instrument_cc_empty() {
        let inst = InstrumentDef {
            name: "synth".to_string(),
            device: "dev".to_string(),
            channel: 1,
            note: None,
            gate_normal: None,
            gate_staccato: None,
            cc_mappings: vec![],
        };
        assert!(CompletionProvider::instrument_cc_completions(&inst).is_empty());
    }

    #[test]
    fn test_diatonic_completions_c_major() {
        let items = CompletionProvider::diatonic_completions(NoteName::C, ScaleType::Major);
        assert_eq!(items.len(), 7);
    }

    #[test]
    fn test_identifier_completions_count() {
        let names = vec!["foo".to_string(), "bar".to_string()];
        let items = CompletionProvider::identifier_completions(&names, "variable");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_identifier_completions_empty() {
        let items = CompletionProvider::identifier_completions(&[], "clip");
        assert!(items.is_empty());
    }
}
