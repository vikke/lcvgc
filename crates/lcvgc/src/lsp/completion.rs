//! 補完候補プロバイダモジュール
//!
//! LSP補完リクエストに対して、コンテキストに応じた補完候補を生成する。
//! キーワード・ノート名・コード名・CC名・識別子など各種候補を提供する。

use std::path::Path;

use super::diatonic;
use crate::ast::instrument::InstrumentDef;
use crate::ast::scale::ScaleType;
use crate::domain::chord::ChordSuffix;
use crate::domain::pitch::NoteName;

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
    /// LSP `sortText` に使う文字列（省略時はクライアント側で label をフォールバック）。
    /// scale 構成音 ("0_..") のように先頭優先したい候補で使用する。
    pub sort_text: Option<String>,
}

/// 補完候補の種別
///
/// LSP の `CompletionItemKind` にマッピングされる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionKind {
    /// キーワード（device, instrument, tempo 等）
    Keyword,
    /// ノート名（c, cs, d 等）
    NoteName,
    /// コード名（ダイアトニックコード、根音 × 全種類のコード）
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

// `scale_prefers_flat` は `diatonic::scale_prefers_flat` に統合済 (pub).
// scale 構成音補完とダイアトニックコード補完で同じ選好を共有する。

/// `NoteName` を補完 `detail` 用の表示表記 (C, C#, Db, ...) に変換する
fn note_display_label(note: NoteName) -> &'static str {
    match note {
        NoteName::C => "C",
        NoteName::Cs => "C#",
        NoteName::Db => "Db",
        NoteName::D => "D",
        NoteName::Ds => "D#",
        NoteName::Eb => "Eb",
        NoteName::E => "E",
        NoteName::F => "F",
        NoteName::Fs => "F#",
        NoteName::Gb => "Gb",
        NoteName::G => "G",
        NoteName::Gs => "G#",
        NoteName::Ab => "Ab",
        NoteName::A => "A",
        NoteName::As => "A#",
        NoteName::Bb => "Bb",
        NoteName::B => "B",
    }
}

/// スケール種別の日本語表示名を返す（scale 補完の `detail` 用）
fn scale_type_japanese_name(scale_type: ScaleType) -> &'static str {
    match scale_type {
        ScaleType::Major => "メジャー",
        ScaleType::Minor => "ナチュラルマイナー",
        ScaleType::HarmonicMinor => "ハーモニックマイナー",
        ScaleType::MelodicMinor => "メロディックマイナー",
        ScaleType::Dorian => "ドリアン",
        ScaleType::Phrygian => "フリジアン",
        ScaleType::Lydian => "リディアン",
        ScaleType::Mixolydian => "ミクソリディアン",
        ScaleType::Locrian => "ロクリアン",
    }
}

/// コードサフィックスの日本語表示名を返す（コード名補完の `detail` 用）
/// Returns the Japanese display name of a chord suffix (for chord completion `detail`).
///
/// # Arguments
/// * `suffix` - コードサフィックス / Chord suffix
///
/// # Returns
/// 日本語の種別名（例: `マイナーセブンス`） / Japanese quality name (e.g. `マイナーセブンス`)
fn chord_suffix_japanese_name(suffix: ChordSuffix) -> &'static str {
    match suffix {
        ChordSuffix::Maj => "メジャー",
        ChordSuffix::Maj7 => "メジャーセブンス",
        ChordSuffix::Min => "マイナー",
        ChordSuffix::Min7 => "マイナーセブンス",
        ChordSuffix::Dom7 => "ドミナントセブンス",
        ChordSuffix::Dim => "ディミニッシュ",
        ChordSuffix::Dim7 => "ディミニッシュセブンス",
        ChordSuffix::Aug => "オーギュメント",
        ChordSuffix::AugMaj7 => "オーギュメントメジャーセブンス",
        ChordSuffix::Min7b5 => "マイナーセブンフラットファイブ",
        ChordSuffix::MinMaj7 => "マイナーメジャーセブンス",
        ChordSuffix::Sus4 => "サスフォー",
        ChordSuffix::Sus2 => "サスツー",
        ChordSuffix::Sixth => "シックスス",
        ChordSuffix::Min6 => "マイナーシックスス",
        ChordSuffix::Ninth => "ナインス",
        ChordSuffix::Min9 => "マイナーナインス",
        ChordSuffix::Add9 => "アドナインス",
        ChordSuffix::Thirteenth => "サーティーンス",
        ChordSuffix::Min13 => "マイナーサーティーンス",
    }
}

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
            "pause",
            "resume",
            "mute",
            "unmute",
        ]
        .iter()
        .map(|kw| CompletionItem {
            label: kw.to_string(),
            detail: None,
            kind: CompletionKind::Keyword,
            sort_text: None,
        })
        .collect()
    }

    /// ノート名の補完候補を返す
    ///
    /// シャープは `cs` のような `s` 形式で出す（nvim の標準 `iskeyword` が `#` を含まないため）。
    /// sort_text は `2_<index>` で、scale 構成音 (`0_`)・ダイアトニック (`1_`) の下、
    /// 根音 × 全種類のコード名 (`3_`) の上に並ぶ。
    ///
    /// Returns note-name completions. Sharps use the `s` form (e.g. `cs`).
    /// sort_text is `2_<index>`: below scale notes (`0_`) / diatonic (`1_`),
    /// above all chord names (`3_`).
    ///
    /// # Returns
    /// 半音階のノート名一覧（シャープ・フラット含む17種）
    pub fn note_completions() -> Vec<CompletionItem> {
        NoteName::ALL
            .iter()
            .enumerate()
            .map(|(i, n)| CompletionItem {
                label: n.keyword().to_string(),
                detail: None,
                kind: CompletionKind::NoteName,
                sort_text: Some(format!("2_{i:02}")),
            })
            .collect()
    }

    /// 「根音 × 全種類」のコード名補完候補を返す
    ///
    /// 17 音名 × `ChordSuffix` の全表記（20 種 + 別名 4 = 24 表記）から
    /// `<root><suffix>`（例: `csm7`, `cssus4`, `cMaj7`）を総当たりで生成する。
    /// シャープは `s` 形式。`csus4` は C sus4、`cssus4` は C# sus4 とパースされる。
    ///
    /// Returns chord-name completions for every root × every suffix spelling
    /// (17 roots × 24 spellings). Sharps use the `s` form; `csus4` parses as
    /// C sus4 and `cssus4` as C# sus4.
    ///
    /// # Returns
    /// 408 件の補完候補。`detail` は「根音 + 日本語種別名」（例: `C# マイナーセブンス`）、
    /// `sort_text` は `3_<根音>_<種別>_<表記>` で根音順 → 種別順 → 表記順。
    /// 408 candidates. `detail` is "root + Japanese quality name";
    /// `sort_text` is `3_<root>_<suffix>_<spelling>`.
    pub fn chord_completions() -> Vec<CompletionItem> {
        NoteName::ALL
            .iter()
            .enumerate()
            .flat_map(|(ri, root)| {
                ChordSuffix::ALL
                    .iter()
                    .enumerate()
                    .flat_map(move |(si, suffix)| {
                        suffix
                            .spellings()
                            .iter()
                            .enumerate()
                            .map(move |(pi, spelling)| CompletionItem {
                                label: format!("{}{}", root.keyword(), spelling),
                                detail: Some(format!(
                                    "{} {}",
                                    note_display_label(*root),
                                    chord_suffix_japanese_name(*suffix)
                                )),
                                kind: CompletionKind::ChordName,
                                sort_text: Some(format!("3_{ri:02}_{si:02}_{pi}")),
                            })
                    })
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
            sort_text: None,
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
                sort_text: None,
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
                sort_text: None,
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
    /// ダイアトニックコードの補完候補リスト（7個）。sort_text は `1_<index>`
    /// （scale 構成音 `0_` の次に並ぶ）。
    pub fn diatonic_completions(root: NoteName, scale_type: ScaleType) -> Vec<CompletionItem> {
        diatonic::diatonic_chords(root, scale_type)
            .into_iter()
            .enumerate()
            .map(|(i, chord)| CompletionItem {
                label: chord.label,
                detail: Some(chord.detail),
                kind: CompletionKind::ChordName,
                sort_text: Some(format!("1_{i}")),
            })
            .collect()
    }

    /// device ブロック内で有効なキーワード補完候補を返す
    ///
    /// # Returns
    /// device ブロック内のキーワード（`port`, `transport`）
    pub fn device_body_completions() -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "port".to_string(),
                detail: Some("MIDIポート名".to_string()),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
            CompletionItem {
                label: "transport".to_string(),
                detail: Some(
                    "MIDI System Real-Time (Start/Stop) 送出の有効化 (true|false, 既定: true)"
                        .to_string(),
                ),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
        ]
    }

    /// 実 MIDI ポート名のスライスから補完候補を生成する
    ///
    /// `device foo { port |` 位置で `lcvgc` プロセスが認識している MIDI 出力
    /// ポート一覧を補完候補に出すための純粋関数。`ports` には呼び出し側で
    /// `lcvgc::midi::port::list_ports()` の結果を渡す（環境依存の I/O は
    /// 呼び出し側で吸収）。
    ///
    /// Pure helper that turns a slice of MIDI port names into completion items
    /// for the `device foo { port |` position. Callers are expected to pass in
    /// the result of `lcvgc::midi::port::list_ports()` so this function can
    /// stay free of environment-dependent I/O.
    ///
    /// # Arguments
    /// * `ports` - 実 MIDI 出力ポート名のスライス
    ///
    /// # Returns
    /// 各ポート名を `CompletionKind::Identifier` として包んだ補完候補リスト。
    /// 入力が空なら空 vec を返す。
    pub fn midi_port_completions(ports: &[String]) -> Vec<CompletionItem> {
        ports
            .iter()
            .map(|name| CompletionItem {
                label: name.clone(),
                detail: Some("MIDI port".to_string()),
                kind: CompletionKind::Identifier,
                sort_text: None,
            })
            .collect()
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
            ("velocity_normal", "通常ベロシティ (0-127)"),
            ("velocity_accent", "アクセントベロシティ (0-127)"),
            ("velocity_ghost", "ゴーストベロシティ (0-127)"),
            ("cc", "CCマッピング (エイリアス CC番号)"),
            ("var", "ローカル変数定義"),
        ]
        .iter()
        .map(|(kw, detail)| CompletionItem {
            label: kw.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
            sort_text: None,
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
            sort_text: None,
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
            sort_text: None,
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
                sort_text: None,
            },
            CompletionItem {
                label: "resolution".to_string(),
                detail: Some("ステップ解像度 (例: 16)".to_string()),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
        ]
    }

    /// scene ブロック内で有効な追加キーワード補完候補を返す
    ///
    /// # Returns
    /// scene ブロック内の追加キーワード（`tempo`, `mute`）
    /// - `tempo`: テンポ変化（§8.4）
    /// - `mute`: clip 行への前置で初期 mute 状態でロード（§8.6）
    pub fn scene_body_keyword_completions() -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "tempo".to_string(),
                detail: Some("テンポ変化 (絶対値 or +N)".to_string()),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
            CompletionItem {
                label: "mute".to_string(),
                detail: Some("§8.6: 続く clip 行を初期 mute 状態でロード".to_string()),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
        ]
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
                sort_text: None,
            },
            CompletionItem {
                label: "loop".to_string(),
                detail: Some("無限ループ".to_string()),
                kind: CompletionKind::Keyword,
                sort_text: None,
            },
        ]
    }

    /// `scale ` / `[scale ` 直後の補完候補を返す。
    /// 音名 17 種 × スケール種別 9 種の総当たりで `<root>_<type>` を生成する
    /// （例: `c_major`, `cs_minor`）。シャープは `s` 形式で出す。
    ///
    /// Returns completion candidates right after `scale ` / `[scale `.
    /// Generates every `<root>_<type>` from 17 note names × 9 scale types
    /// (e.g. `c_major`, `cs_minor`). Sharps use the `s` form.
    ///
    /// # Returns
    /// 153 件の補完候補。`detail` は日本語の種別名（例: `C ハーモニックマイナー`）、
    /// `sort_text` は音名順 → 種別順。
    /// 153 candidates. `detail` is the Japanese type name (e.g. `C ハーモニックマイナー`);
    /// `sort_text` orders by note name, then by type.
    pub fn scale_completions() -> Vec<CompletionItem> {
        NoteName::ALL
            .iter()
            .enumerate()
            .flat_map(|(ri, root)| {
                ScaleType::ALL
                    .iter()
                    .enumerate()
                    .map(move |(ti, st)| CompletionItem {
                        label: format!("{}_{}", root.keyword(), st.keyword()),
                        detail: Some(format!(
                            "{} {}",
                            note_display_label(*root),
                            scale_type_japanese_name(*st)
                        )),
                        kind: CompletionKind::Keyword,
                        sort_text: Some(format!("{ri:02}_{ti:02}")),
                    })
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
            sort_text: None,
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
            sort_text: None,
        })
        .collect()
    }

    /// アルペジオ第2引数（音価）の補完候補を返す
    ///
    /// 主要な音価（4分音符 / 8分音符 / 16分音符 / 32分音符）の数値だけを並べる。
    /// 列挙していない音価（2, 64 など）も実際にはパース可能だが、補完候補としては
    /// 主要なものに絞ることでユーザーの選択コストを下げる。
    ///
    /// # Returns
    /// 主要音価の一覧（4, 8, 16, 32）
    pub fn arpeggio_resolution_completions() -> Vec<CompletionItem> {
        [
            ("4", "4分音符間隔"),
            ("8", "8分音符間隔"),
            ("16", "16分音符間隔"),
            ("32", "32分音符間隔"),
        ]
        .iter()
        .map(|(res, detail)| CompletionItem {
            label: res.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionKind::Keyword,
            sort_text: None,
        })
        .collect()
    }

    /// スケール構成音7音の補完候補を返す（先頭優先 sortText 付き）
    /// Returns 7 in-scale note completions with top-priority sortText.
    ///
    /// `[scale c_major]` のように clip ローカル / トップレベルのスケールが
    /// 解決されたときに、ノート補完で構成音を最上位に並べるためのヘルパ。
    /// sortText は `"0_<index>"` 形式で、半音階17音 (`"2_..."`) より上に来る。
    ///
    /// Helper that lifts in-scale notes to the top of pitched-clip completions
    /// when a scale (clip-local or top-level) is resolved. Each item carries a
    /// `sort_text` like `"0_<index>"` so they precede the chromatic 17-note
    /// fallback (which uses `"2_..."`). Sharps use the `s` form (e.g. `fs`).
    ///
    /// # Arguments
    /// * `root` - スケールのルート音 / Scale root note
    /// * `scale_type` - スケールタイプ / Scale type (Major, Minor, ...)
    ///
    /// # Returns
    /// スケール構成音 7 件の補完候補（昇順、ルート始まり）。
    pub fn scale_note_completions(root: NoteName, scale_type: ScaleType) -> Vec<CompletionItem> {
        let intervals = diatonic::scale_intervals(scale_type);
        let root_semi = diatonic::note_to_semitone(root);
        let detail = format!(
            "in-scale note ({} {})",
            root.keyword(),
            scale_type.keyword()
        );
        let prefer_flat = diatonic::scale_prefers_flat(scale_type);
        intervals
            .iter()
            .enumerate()
            .map(|(i, semi)| {
                let semitone = (root_semi + *semi) % 12;
                CompletionItem {
                    label: diatonic::semitone_to_dsl_note(semitone, prefer_flat)
                        .keyword()
                        .to_string(),
                    detail: Some(detail.clone()),
                    kind: CompletionKind::NoteName,
                    sort_text: Some(format!("0_{i}")),
                }
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
                                    sort_text: None,
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
                                sort_text: None,
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
    use crate::ast::clip_note::NoteEvent;
    use crate::ast::instrument::CcMapping;
    use crate::domain::channel::MidiChannel;
    use crate::domain::chord::ChordSuffix;
    use crate::parser::clip_note::parse_note_event;

    #[test]
    fn test_keyword_completions_count() {
        assert_eq!(CompletionProvider::keyword_completions().len(), 16);
    }

    #[test]
    fn test_keyword_completions_contains_device() {
        let items = CompletionProvider::keyword_completions();
        assert!(items.iter().any(|i| i.label == "device"));
    }

    /// §10.4: pause / resume がキーワード補完に含まれる
    /// §10.4: pause and resume are included in keyword completions
    #[test]
    fn test_keyword_completions_contains_pause_resume() {
        let items = CompletionProvider::keyword_completions();
        assert!(items.iter().any(|i| i.label == "pause"));
        assert!(items.iter().any(|i| i.label == "resume"));
    }

    /// Issue #50: device ブロック内補完に `port` と `transport` の両方が含まれる
    /// Issue #50: device body completions include both `port` and `transport`
    #[test]
    fn test_device_body_completions_contains_port_and_transport() {
        let items = CompletionProvider::device_body_completions();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "port"));
        assert!(items.iter().any(|i| i.label == "transport"));
    }

    /// §10.4: mute / unmute がキーワード補完に含まれる
    /// §10.4: mute and unmute are included in keyword completions
    #[test]
    fn test_keyword_completions_contains_mute_unmute() {
        let items = CompletionProvider::keyword_completions();
        assert!(items.iter().any(|i| i.label == "mute"));
        assert!(items.iter().any(|i| i.label == "unmute"));
    }

    /// §8.6: scene ブロック内のキーワード補完に `mute` が含まれる
    /// §8.6: `mute` is included in scene-body keyword completions
    #[test]
    fn test_scene_body_keyword_completions_contains_mute() {
        let items = CompletionProvider::scene_body_keyword_completions();
        assert!(
            items.iter().any(|i| i.label == "mute"),
            "scene body completions should include `mute` for §8.6 prefix"
        );
        assert!(
            items.iter().any(|i| i.label == "tempo"),
            "scene body completions should still include `tempo`"
        );
    }

    /// §8.6: scene ブロック内には `unmute` 前置は無いので候補に出さない
    /// §8.6: `unmute` prefix is not supported inside scene blocks, so it must not appear
    #[test]
    fn test_scene_body_keyword_completions_excludes_unmute() {
        let items = CompletionProvider::scene_body_keyword_completions();
        assert!(
            !items.iter().any(|i| i.label == "unmute"),
            "scene body completions must not include `unmute` (default is unmuted)"
        );
    }

    #[test]
    fn test_note_completions_count() {
        assert_eq!(CompletionProvider::note_completions().len(), 17);
    }

    #[test]
    fn test_note_completions_contains_sharp() {
        let items = CompletionProvider::note_completions();
        assert!(items.iter().any(|i| i.label == "cs"));
    }

    /// 単音補完のラベルは `s` 形式（`NoteName::ALL` の keyword と同順）で、`#` を含まない。
    /// Single-note labels use the `s` form in `NoteName::ALL` order and never contain `#`.
    #[test]
    fn note_completions_labels_use_s_form() {
        let labels: Vec<String> = CompletionProvider::note_completions()
            .into_iter()
            .map(|i| i.label)
            .collect();
        let expected: Vec<String> = NoteName::ALL
            .iter()
            .map(|n| n.keyword().to_string())
            .collect();
        assert_eq!(labels, expected);
    }

    /// 単音補完の sort_text は `2_` 始まり（構成音・ダイアトニックより下、全コードより上）。
    /// Single-note sort_text starts with `2_` (below scale notes/diatonic, above all chords).
    #[test]
    fn note_completions_sort_text_prefixed_2() {
        for item in CompletionProvider::note_completions() {
            let st = item.sort_text.as_deref().expect("sort_text must be set");
            assert!(st.starts_with("2_"), "unexpected sort_text {st}");
        }
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
            channel: MidiChannel::from_one_based(1).unwrap(),
            note: None,
            gate_normal: None,
            gate_staccato: None,
            velocity_normal: None,
            velocity_accent: None,
            velocity_ghost: None,
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
        };
        assert!(CompletionProvider::instrument_cc_completions(&inst).is_empty());
    }

    #[test]
    fn test_diatonic_completions_c_major() {
        let items = CompletionProvider::diatonic_completions(NoteName::C, ScaleType::Major);
        assert_eq!(items.len(), 7);
    }

    /// ダイアトニックコード補完の label は DSL に直挿し可能な表記。
    /// 根音は小文字（d minor は flat 選好で `bb`）で、`A#` は決して含まない。
    /// メジャーコードはサフィックス `M` 付き（大文字はサフィックス部分のみ）。
    /// Diatonic completion labels are DSL-insertable strings: the root is lowercase
    /// (d minor prefers flats, `bb`), never `A#`; major chords carry the `M` suffix.
    #[test]
    fn diatonic_completions_d_minor_labels_are_dsl_insertable() {
        let items = CompletionProvider::diatonic_completions(NoteName::D, ScaleType::Minor);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["dm", "edim", "fM", "gm", "am", "bbM", "cM"]);
        for label in &labels {
            let root: String = label
                .chars()
                .take_while(|c| c.is_ascii_lowercase())
                .collect();
            assert!(
                !root.is_empty(),
                "label `{label}` must start with a lowercase root"
            );
            assert!(!label.contains('#'), "label `{label}` must not contain `#`");
            assert_ne!(
                *label, "A#",
                "A# must never appear as a chord completion label"
            );
        }
    }

    /// ダイアトニックコード補完の sort_text は `1_<index>`（構成音の次）。
    /// Diatonic sort_text is `1_<index>` (right after the scale notes).
    #[test]
    fn diatonic_completions_sort_text_prefixed_1() {
        let items = CompletionProvider::diatonic_completions(NoteName::C, ScaleType::Major);
        let sts: Vec<&str> = items
            .iter()
            .map(|i| i.sort_text.as_deref().expect("sort_text must be set"))
            .collect();
        assert_eq!(sts, vec!["1_0", "1_1", "1_2", "1_3", "1_4", "1_5", "1_6"]);
    }

    /// 全ルート × 全スケールのダイアトニックコードのラベルが、単音ではなくコード名としてパースできる。
    /// Every diatonic label (all roots × all scales) parses as a chord name, not a single note.
    #[test]
    fn diatonic_completions_labels_parse_as_chords() {
        for root in NoteName::ALL {
            for st in ScaleType::ALL {
                for item in CompletionProvider::diatonic_completions(root, st) {
                    let parsed = parse_note_event(&item.label);
                    assert!(
                        matches!(parsed, Ok(("", NoteEvent::ChordName { .. }))),
                        "`{}` must parse as a chord name, got {parsed:?}",
                        item.label
                    );
                }
            }
        }
    }

    /// ダイアトニックコード補完の kind は ChordName のまま。
    #[test]
    fn diatonic_completions_kind_remains_chord_name() {
        let items = CompletionProvider::diatonic_completions(NoteName::D, ScaleType::Minor);
        for item in &items {
            assert_eq!(item.kind, CompletionKind::ChordName);
        }
    }

    /// scale 構成音補完: c major は c d e f g a b の7件を返す
    #[test]
    fn scale_note_completions_c_major_returns_7_in_order() {
        let items = CompletionProvider::scale_note_completions(NoteName::C, ScaleType::Major);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["c", "d", "e", "f", "g", "a", "b"]);
    }

    /// scale 構成音補完: c minor は c d eb f g ab bb の7件
    #[test]
    fn scale_note_completions_c_minor_returns_natural_minor() {
        let items = CompletionProvider::scale_note_completions(NoteName::C, ScaleType::Minor);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["c", "d", "eb", "f", "g", "ab", "bb"]);
    }

    /// scale 構成音補完: シャープは `s` 形式で出す（e major → fs, gs, cs, ds）
    /// In-scale note labels use the `s`-form sharps (e major → fs, gs, cs, ds).
    #[test]
    fn scale_note_completions_e_major_uses_s_form() {
        let items = CompletionProvider::scale_note_completions(NoteName::E, ScaleType::Major);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["e", "fs", "gs", "a", "b", "cs", "ds"]);
    }

    /// コード名補完: 17 音 × 24 表記（20 種 + 別名 4）= 408 件
    /// Chord-name completions: 17 roots × 24 spellings (20 suffixes + 4 aliases) = 408.
    #[test]
    fn chord_completions_is_full_cartesian_product() {
        assert_eq!(CompletionProvider::chord_completions().len(), 17 * 24);
    }

    /// コード名補完: すべて ChordName 種別で、`#` 形式の根音（`c#` 等）は出さない。
    /// Chord-name completions are all ChordName and never use `#`-form roots.
    #[test]
    fn chord_completions_kind_and_s_form_roots() {
        for item in CompletionProvider::chord_completions() {
            assert_eq!(item.kind, CompletionKind::ChordName);
            assert!(
                !item.label[..2].contains('#'),
                "root must use the `s` form: {}",
                item.label
            );
        }
    }

    /// コード名補完: 全ラベルがパーサで元の (根音, サフィックス) に戻る。
    /// `csus4` は C sus4、`cssus4` は C# sus4 になることもここで保証される。
    /// Every label round-trips through the parser to its (root, suffix).
    /// This also guarantees `csus4` = C sus4 and `cssus4` = C# sus4.
    #[test]
    fn chord_completions_round_trip_through_parser() {
        let items = CompletionProvider::chord_completions();
        let mut i = 0;
        for root in NoteName::ALL {
            for suffix in ChordSuffix::ALL {
                for _ in suffix.spellings() {
                    let label = &items[i].label;
                    match parse_note_event(label) {
                        Ok((
                            "",
                            NoteEvent::ChordName {
                                root: r, suffix: s, ..
                            },
                        )) => {
                            assert_eq!((r, s), (root, suffix), "label `{label}`");
                        }
                        other => panic!("`{label}` did not parse as a chord: {other:?}"),
                    }
                    i += 1;
                }
            }
        }
    }

    /// コード名補完: Issue #111 の例が候補に含まれる。
    /// Chord-name completions include the examples from Issue #111.
    #[test]
    fn chord_completions_contains_issue_examples() {
        let items = CompletionProvider::chord_completions();
        let labels: std::collections::HashSet<&str> =
            items.iter().map(|i| i.label.as_str()).collect();
        for l in [
            "csus4", "cdim", "cadd9", "csm7", "csM7", "csus2", "cssus2", "cssus4", "cMaj7",
            "cmMaj7", "cM7#5", "cMaj7#5",
        ] {
            assert!(labels.contains(l), "missing `{l}`");
        }
    }

    /// コード名補完: detail は「表示用根音 + 日本語種別名」。
    /// Chord-name detail is "display root + Japanese quality name".
    #[test]
    fn chord_completions_detail_is_japanese_name() {
        let items = CompletionProvider::chord_completions();
        let find = |l: &str| {
            items
                .iter()
                .find(|i| i.label == l)
                .and_then(|i| i.detail.clone())
        };
        assert_eq!(find("csm7").as_deref(), Some("C# マイナーセブンス"));
        assert_eq!(find("csus4").as_deref(), Some("C サスフォー"));
        assert_eq!(find("cMaj").as_deref(), Some("C メジャー"));
    }

    /// コード名補完: sort_text は `3_` 始まりで、根音順 → 種別順 → 表記順に並ぶ。
    /// Chord-name sort_text starts with `3_` and orders by root, suffix, then spelling.
    #[test]
    fn chord_completions_sort_text_orders_root_suffix_spelling() {
        let items = CompletionProvider::chord_completions();
        let sts: Vec<&str> = items
            .iter()
            .map(|i| i.sort_text.as_deref().expect("sort_text must be set"))
            .collect();
        assert!(sts.iter().all(|s| s.starts_with("3_")));
        let mut sorted = sts.clone();
        sorted.sort();
        assert_eq!(sts, sorted, "items must already be in sort_text order");
        assert_eq!(items[0].label, "cM");
        assert_eq!(items[1].label, "cMaj");
    }

    /// scale 構成音補完: sortText が "0_<index>" 形式で先頭ソート用
    #[test]
    fn scale_note_completions_sort_text_prefixed_for_top_priority() {
        let items = CompletionProvider::scale_note_completions(NoteName::C, ScaleType::Major);
        for (i, item) in items.iter().enumerate() {
            let st = item.sort_text.as_deref().expect("sort_text must be set");
            assert!(
                st.starts_with("0_"),
                "expected '0_' prefix to keep scale notes on top, got {st}"
            );
            // 7音以内でも安定ソートできるように index を付与
            assert!(
                st.contains(&format!("{i}")),
                "sort_text {st} should contain index {i}"
            );
        }
    }

    /// scale 構成音補完: kind は NoteName
    #[test]
    fn scale_note_completions_kind_is_note_name() {
        let items = CompletionProvider::scale_note_completions(NoteName::C, ScaleType::Major);
        for item in &items {
            assert_eq!(item.kind, CompletionKind::NoteName);
        }
    }

    /// scale 構成音補完: detail にスケール情報が入る
    #[test]
    fn scale_note_completions_detail_mentions_scale() {
        let items = CompletionProvider::scale_note_completions(NoteName::C, ScaleType::Major);
        let detail = items[0].detail.as_deref().unwrap_or("");
        assert!(
            detail.to_lowercase().contains("scale"),
            "expected 'scale' in detail, got {detail}"
        );
    }

    /// scale 補完: 音名 17 × 種別 9 = 153 件の総当たり
    #[test]
    fn scale_completions_is_full_cartesian_product() {
        let items = CompletionProvider::scale_completions();
        assert_eq!(items.len(), 17 * 9);
        assert!(items.iter().all(|i| i.kind == CompletionKind::Keyword));
    }

    /// scale 補完: `<root>_<type>` の単一トークンで、シャープは `s` 形式
    #[test]
    fn scale_completions_labels_are_single_s_form_tokens() {
        let items = CompletionProvider::scale_completions();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"c_major"));
        assert!(labels.contains(&"cs_minor"));
        assert!(labels.contains(&"eb_dorian"));
        assert!(labels.contains(&"b_harmonic_minor"));
        assert!(labels.iter().all(|l| !l.contains('#') && !l.contains(' ')));
    }

    /// scale 補完: 補完ラベルはすべてパーサで読める
    #[test]
    fn scale_completions_labels_are_parseable() {
        for item in CompletionProvider::scale_completions() {
            let (rest, _) = crate::parser::scale::scale_token(&item.label).unwrap();
            assert_eq!(rest, "", "not fully parsed: {}", item.label);
        }
    }

    /// scale 補完: detail に日本語の種別名が入る
    #[test]
    fn scale_completions_detail_is_japanese_name() {
        let items = CompletionProvider::scale_completions();
        let detail_of = |label: &str| {
            items
                .iter()
                .find(|i| i.label == label)
                .and_then(|i| i.detail.clone())
                .unwrap()
        };
        assert_eq!(detail_of("c_harmonic_minor"), "C ハーモニックマイナー");
        assert_eq!(detail_of("cs_minor"), "C# ナチュラルマイナー");
        assert_eq!(detail_of("bb_lydian"), "Bb リディアン");
    }

    /// scale 補完: sort_text は音名順 → 種別順で安定
    #[test]
    fn scale_completions_sort_text_orders_root_then_type() {
        let items = CompletionProvider::scale_completions();
        let mut sorted = items.clone();
        sorted.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        assert_eq!(sorted, items);
        assert_eq!(items[0].label, "c_major");
        assert_eq!(items[1].label, "c_minor");
        assert_eq!(items[9].label, "cs_major");
        assert_eq!(items[152].label, "b_locrian");
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

    /// PR #55: 入力が空なら midi_port_completions は空を返す
    /// PR #55: midi_port_completions returns empty when input is empty
    #[test]
    fn midi_port_completions_empty_input_returns_empty() {
        let items = CompletionProvider::midi_port_completions(&[]);
        assert!(items.is_empty());
    }

    /// PR #55: 各 port 名を Identifier kind の補完候補にマップする
    /// PR #55: maps each port name to an Identifier-kind completion item
    #[test]
    fn midi_port_completions_maps_each_port_to_identifier_item() {
        let ports = vec![
            "Volca FM".to_string(),
            "Volca Bass".to_string(),
            "IAC Driver Bus 1".to_string(),
        ];
        let items = CompletionProvider::midi_port_completions(&ports);
        assert_eq!(items.len(), 3);

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Volca FM"));
        assert!(labels.contains(&"Volca Bass"));
        assert!(labels.contains(&"IAC Driver Bus 1"));

        // すべて Identifier kind で detail = "MIDI port"
        // All items should be Identifier kind with detail = "MIDI port"
        for item in &items {
            assert_eq!(item.kind, CompletionKind::Identifier);
            assert_eq!(item.detail.as_deref(), Some("MIDI port"));
        }
    }

    /// PR #55: 入力スライスの順序を保ったまま補完候補を返す
    /// PR #55: preserves the order of the input slice in the completion items
    #[test]
    fn midi_port_completions_preserves_input_order() {
        let ports = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let items = CompletionProvider::midi_port_completions(&ports);
        assert_eq!(items[0].label, "a");
        assert_eq!(items[1].label, "b");
        assert_eq!(items[2].label, "c");
    }
}
