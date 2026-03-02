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

    /// device ブロック内で有効なキーワード
    pub fn device_body_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "port".to_string(),
            detail: Some("MIDIポート名".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// instrument ブロック内で有効なキーワード
    pub fn instrument_body_completions() -> Vec<CompletionItem> {
        [
            ("device", "MIDIデバイス参照"),
            ("channel", "MIDIチャンネル (1-16)"),
            ("note", "固定ノート (ドラム用)"),
            ("gate_normal", "通常Gate比率 (%)"),
            ("gate_staccato", "スタッカートGate比率 (%)"),
            ("cc", "CCマッピング (エイリアス CC番号)"),
            ("var", "ローカル変数定義"),
        ]
        .iter()
        .map(|(kw, detail)| CompletionItem {
            label: kw.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
        })
        .collect()
    }

    /// kit ブロック内で有効なキーワード
    pub fn kit_body_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "device".to_string(),
            detail: Some("MIDIデバイス参照".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// clip オプションのキーワード
    pub fn clip_option_completions() -> Vec<CompletionItem> {
        [
            ("bars", "小節数"),
            ("time", "拍子 (例: 3/4)"),
            ("scale", "スケール指定"),
        ]
        .iter()
        .map(|(kw, detail)| CompletionItem {
            label: kw.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
        })
        .collect()
    }

    /// drum clip 内で有効なキーワード
    pub fn drum_clip_body_completions() -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "use".to_string(),
                detail: Some("ドラムキット参照".to_string()),
                kind: CompletionKind::Keyword,
            },
            CompletionItem {
                label: "resolution".to_string(),
                detail: Some("ステップ解像度 (例: 16)".to_string()),
                kind: CompletionKind::Keyword,
            },
        ]
    }

    /// scene ブロック内で有効な追加キーワード
    pub fn scene_body_keyword_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "tempo".to_string(),
            detail: Some("テンポ変化 (絶対値 or +N)".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// session エントリのオプション
    pub fn session_entry_option_completions() -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "repeat".to_string(),
                detail: Some("繰り返し回数".to_string()),
                kind: CompletionKind::Keyword,
            },
            CompletionItem {
                label: "loop".to_string(),
                detail: Some("無限ループ".to_string()),
                kind: CompletionKind::Keyword,
            },
        ]
    }

    /// scale タイプの補完
    pub fn scale_type_completions() -> Vec<CompletionItem> {
        [
            ("major", "メジャー"),
            ("minor", "ナチュラルマイナー"),
            ("harmonic_minor", "ハーモニックマイナー"),
            ("melodic_minor", "メロディックマイナー"),
            ("dorian", "ドリアン"),
            ("phrygian", "フリジアン"),
            ("lydian", "リディアン"),
            ("mixolydian", "ミクソリディアン"),
            ("locrian", "ロクリアン"),
        ]
        .iter()
        .map(|(name, detail)| CompletionItem {
            label: name.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
        })
        .collect()
    }

    /// play の後のターゲット補完
    pub fn play_keyword_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "session".to_string(),
            detail: Some("セッション再生".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// アルペジオ方向の補完
    pub fn arpeggio_direction_completions() -> Vec<CompletionItem> {
        [
            ("up", "上昇"),
            ("down", "下降"),
            ("updown", "上昇→下降"),
            ("random", "ランダム"),
        ]
        .iter()
        .map(|(dir, detail)| CompletionItem {
            label: dir.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
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
