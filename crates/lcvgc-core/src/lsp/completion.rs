//! 補完候補プロバイダモジュール
//!
//! LSP補完リクエストに対して、コンテキストに応じた補完候補を生成する。
//! キーワード・ノート名・コード名・CC名・識別子など各種候補を提供する。

use std::path::Path;

use super::diatonic;
use crate::ast::common::NoteName;
use crate::ast::instrument::InstrumentDef;
use crate::ast::scale::ScaleType;

/// 補完候補アイテム
///
/// LSPの `CompletionItem` に変換される内部表現。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    /// 補完候補のラベル（表示テキスト）
    pub label: String,
    /// 補完候補の詳細説明（省略可能）
    pub detail: Option<String>,
    /// 補完候補の種別
    pub kind: CompletionKind,
}

/// 補完候補の種別
///
/// LSP の `CompletionItemKind` にマッピングされる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionKind {
    /// キーワード（device, instrument, tempo 等）
    Keyword,
    /// ノート名（c, c#, d 等）
    NoteName,
    /// コード名（ダイアトニックコード）
    ChordName,
    /// CCエイリアス名
    CcAlias,
    /// 識別子（ユーザー定義の名前）
    Identifier,
}

/// 補完候補プロバイダ
///
/// 各コンテキストに応じた補完候補リストを生成する静的メソッド群。
pub struct CompletionProvider;

impl CompletionProvider {
    /// トップレベルのブロックキーワード補完候補を返す
    ///
    /// # Returns
    /// DSLのトップレベルキーワード一覧（device, instrument, clip 等）
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

    /// ノート名の補完候補を返す
    ///
    /// # Returns
    /// 半音階のノート名一覧（シャープ・フラット含む17種）
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

    /// 標準MIDIコントロールチェンジの補完候補を返す
    ///
    /// # Returns
    /// 汎用的なCC名と番号のペア一覧（Modulation, Volume 等）
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

    /// インストゥルメント定義のCCマッピングから補完候補を返す
    ///
    /// # Arguments
    /// * `instrument` - CCマッピングを持つインストゥルメント定義
    ///
    /// # Returns
    /// インストゥルメントに定義されたCCエイリアスの一覧
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

    /// 識別子名の補完候補を返す
    ///
    /// レジストリに登録された名前から補完候補を生成する。
    ///
    /// # Arguments
    /// * `names` - 登録済み名前のスライス
    /// * `kind_label` - 種別ラベル（"device", "instrument" 等）
    ///
    /// # Returns
    /// 識別子の補完候補リスト
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

    /// ダイアトニックコードの補完候補を返す
    ///
    /// 指定されたルート音とスケールタイプから7つのダイアトニックコードを生成する。
    ///
    /// # Arguments
    /// * `root` - スケールのルート音
    /// * `scale_type` - スケールタイプ（Major, Minor 等）
    ///
    /// # Returns
    /// ダイアトニックコードの補完候補リスト（7個）
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

    /// device ブロック内で有効なキーワード補完候補を返す
    ///
    /// # Returns
    /// device ブロック内のキーワード（`port`）
    pub fn device_body_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "port".to_string(),
            detail: Some("MIDIポート名".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// instrument ブロック内で有効なキーワード補完候補を返す
    ///
    /// # Returns
    /// instrument ブロック内のキーワード（device, channel, note 等）
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

    /// kit ブロック内で有効なキーワード補完候補を返す
    ///
    /// # Returns
    /// kit ブロック内のキーワード（`device`）
    pub fn kit_body_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "device".to_string(),
            detail: Some("MIDIデバイス参照".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// clip オプション `[...]` 内のキーワード補完候補を返す
    ///
    /// # Returns
    /// clip オプションのキーワード（bars, time, scale）
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

    /// ドラム clip 内で有効なキーワード補完候補を返す
    ///
    /// # Returns
    /// ドラム clip 内のキーワード（use, resolution）
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

    /// scene ブロック内で有効な追加キーワード補完候補を返す
    ///
    /// # Returns
    /// scene ブロック内の追加キーワード（`tempo`）
    pub fn scene_body_keyword_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "tempo".to_string(),
            detail: Some("テンポ変化 (絶対値 or +N)".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// session エントリのオプション補完候補を返す
    ///
    /// # Returns
    /// session エントリのオプション（repeat, loop）
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

    /// スケールタイプの補完候補を返す
    ///
    /// # Returns
    /// 利用可能なスケールタイプ一覧（major, minor, dorian 等）
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

    /// play コマンドの後のターゲット補完候補を返す
    ///
    /// # Returns
    /// play ターゲットのキーワード（`session`）
    pub fn play_keyword_completions() -> Vec<CompletionItem> {
        vec![CompletionItem {
            label: "session".to_string(),
            detail: Some("セッション再生".to_string()),
            kind: CompletionKind::Keyword,
        }]
    }

    /// アルペジオ方向の補完候補を返す
    ///
    /// # Returns
    /// アルペジオ方向の一覧（up, down, updown, random）
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

    /// インクルードパスの補完候補を返す（.cvg/.lcvgc ファイル）
    /// Returns completion candidates for include paths (.cvg/.lcvgc files)
    ///
    /// # Arguments
    /// * `base_path` - ベースディレクトリのパス / Base directory path
    ///
    /// # Returns
    /// .cvg/.lcvgc ファイルのパス補完候補リスト / List of .cvg/.lcvgc file path completion candidates
    pub fn include_path_completions(base_path: &Path) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        if let Ok(entries) = std::fs::read_dir(base_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if ext == "cvg" || ext == "lcvgc" {
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                items.push(CompletionItem {
                                    label: name.to_string(),
                                    detail: Some("include file".to_string()),
                                    kind: CompletionKind::Identifier,
                                });
                            }
                        }
                    }
                } else if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        // ドットで始まるディレクトリはスキップ
                        // Skip directories starting with a dot
                        if !name.starts_with('.') {
                            items.push(CompletionItem {
                                label: format!("{}/", name),
                                detail: Some("directory".to_string()),
                                kind: CompletionKind::Identifier,
                            });
                        }
                    }
                }
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::instrument::CcMapping;

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
                cc_number_ref: None,
            }],
            local_vars: vec![],
            unresolved: Default::default(),
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
            local_vars: vec![],
            unresolved: Default::default(),
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

    #[test]
    fn test_include_path_completions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup.cvg"), "").unwrap();
        std::fs::write(dir.path().join("drums.lcvgc"), "").unwrap();
        std::fs::write(dir.path().join("readme.md"), "").unwrap();
        std::fs::create_dir(dir.path().join("clips")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();

        let items = CompletionProvider::include_path_completions(dir.path());
        // .cvg と .lcvgc ファイルのみ + ディレクトリ（.hidden除外）
        assert!(items.iter().any(|i| i.label == "setup.cvg"));
        assert!(items.iter().any(|i| i.label == "drums.lcvgc"));
        assert!(items.iter().any(|i| i.label == "clips/"));
        assert!(!items.iter().any(|i| i.label == "readme.md"));
        assert!(!items.iter().any(|i| i.label.contains(".hidden")));
    }

    #[test]
    fn test_include_path_completions_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let items = CompletionProvider::include_path_completions(dir.path());
        assert!(items.is_empty());
    }

    #[test]
    fn test_include_path_completions_nonexistent() {
        use std::path::Path;
        let items = CompletionProvider::include_path_completions(Path::new("/nonexistent/path"));
        assert!(items.is_empty());
    }
}
