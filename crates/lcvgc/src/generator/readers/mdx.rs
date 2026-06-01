//! MDX (X68000 MXDRV) → Score IR reader (FM 音源部 A-H のみ)。
//!
//! MDX のヘッダ仕様:
//! - Shift-JIS 文字列のタイトル + `0x0d 0x0a 0x1a`
//! - PDX ファイル名 + `0x00`
//! - voice data offset (2 byte BE)
//! - チャンネル数分の MML offset 配列 (2 byte BE 各)
//! - 全 offset は voice data offset の **位置** からの相対値
//!
//! 本 reader は FM (A-H = 最初 8 channel) のみを抽出する。PCM (P) と Mercury
//! (Q-W) は無視する。
//!
//! MML コマンド (主要なもの):
//! - `0x00-0x7F`: 休符 (clock = byte value)
//! - `0x80-0xDF`: ノート (clock = 次の 1 byte、MIDI = byte − 0x80 + 3 と仮定するが、
//!                MXDRV の実装では `0x80` がオクターブ 0 の C# 等、解釈に幅がある。
//!                本実装は `MIDI = (byte − 0x80) + 39` で C2 始まりとしている。
//!                これは多くのプレイヤと一致するが、再生時に音域がずれる場合は
//!                定数 `MDX_NOTE_BASE` を環境に合わせて変える。)
//! - `0xFF n`: テンポ (n は OPM タイマー B 値)
//! - `0xF6 cnt 0x00`: ループ開始 (cnt 回繰り返し)
//! - `0xF5 nn nn`: ループ終端 (2 byte signed offset)
//! - 0xFD voice, 0xFC pan, 0xFB volume, 0xF8 staccato, 0xF7 legato 等は読み飛ばす
//!   (引数のみ消費)
//! - 0xE0-0xEE は引数固定の長さで読み飛ばす (note-on delay 0xF0 含む)
//!
//! Reads an MDX file's FM section (channels A-H only). Other channels (PCM P,
//! Mercury Q-W) are ignored.

use crate::generator::score::{Event, Score, Track, TrackKind};
use crate::generator::{GeneratorError, ScoreReader};
use encoding_rs::SHIFT_JIS;

/// MDX の note value (0x80) を MIDI ノート番号に変換するときの加算値。
///
/// MXDRV 系プレイヤと音域を揃えるため `o4 = MIDI 60 (C4)` 付近に寄せる。
/// このリーダは「ループ展開を `(...)*2` で出力する」用途なので、絶対音高が
/// 多少前後しても DSL の構造は変わらない。
const MDX_NOTE_BASE: u8 = 39;

/// MDX ファイルで採用する PPQ (1 四分音符 = 48 clock = MDX のデフォルト解像度)。
const MDX_PPQ: u32 = 48;

/// MDX reader 実装。
/// MDX reader (FM channels only).
pub struct MdxReader;

impl ScoreReader for MdxReader {
    fn read(&self, bytes: &[u8], source_name: &str) -> Result<Score, GeneratorError> {
        self.read_with_pdx(bytes, None, source_name)
    }
}

/// FM チャンネルの最大数 (A-H = 8ch)。
/// Number of FM channels (A-H).
const FM_CHANNELS: usize = 8;

/// PCM (ADPCM) チャンネルの MML offset テーブル上の添字 (FM 8ch の次)。
/// Index of the PCM channel within the MML offset table (right after FM A-H).
const PCM_CHANNEL_INDEX: usize = 8;

impl MdxReader {
    /// MDX バイト列と (任意の) PDX バイト列から Score を構築する。
    ///
    /// FM チャンネル (A-H) は音程トラックとして読む。PCM チャンネル (index 8) が
    /// あり、かつ `pdx_bytes` が与えられた場合は、PDX 波形を解析してドラム楽器
    /// (kick/snare/oh/ch/cp) を相対的に割り当て、ドラムトラックとして追加する。
    /// PDX が無い場合 PCM チャンネルは出力しない (種別判定できないため)。
    ///
    /// Builds a Score from MDX bytes plus optional PDX bytes. FM channels become
    /// melodic tracks; the PCM channel becomes a drum track when a PDX is
    /// available (its waveforms are analyzed to assign drum voices).
    ///
    /// # 引数 / Arguments
    /// * `bytes` - MDX ファイルのバイト列 / MDX file bytes
    /// * `pdx_bytes` - 対応する PDX のバイト列 (無ければ `None`) / optional PDX bytes
    /// * `source_name` - エラー表示用の名前 / name for error messages
    ///
    /// # Errors
    /// ヘッダのパースに失敗した場合 `GeneratorError::Parse`。
    pub fn read_with_pdx(
        &self,
        bytes: &[u8],
        pdx_bytes: Option<&[u8]>,
        source_name: &str,
    ) -> Result<Score, GeneratorError> {
        let header = parse_header(bytes).map_err(|m| GeneratorError::Parse {
            format: "mdx",
            message: format!("{} ({})", m, source_name),
        })?;

        let mut score = Score {
            ppq: MDX_PPQ,
            initial_bpm: 120.0,
            title: header.title.clone(),
            ..Score::default()
        };

        // 各 FM チャンネル (最大 8) を順に処理
        let mut tempo_samples: Vec<u8> = Vec::new();
        for (idx, &mml_off) in header
            .mml_offsets
            .iter()
            .enumerate()
            .take(FM_CHANNELS.min(header.mml_offsets.len()))
        {
            let mml_start = header.voice_offset_pos + mml_off as usize;
            if mml_start >= bytes.len() {
                continue;
            }
            let ch_name = format!("fm_{}", (b'a' + idx as u8) as char);
            let (events, tempos) = parse_mml(&bytes[mml_start..], false);
            tempo_samples.extend(tempos);
            if events.is_empty() {
                continue;
            }
            score.tracks.push(Track {
                name: ch_name,
                midi_channel: (idx as u8) + 1,
                kind: TrackKind::Melodic,
                events,
            });
        }

        // PCM (ADPCM) チャンネル + PDX があればドラムトラックを構築する。
        if let Some(pdx) = pdx_bytes {
            if let Some(&pcm_off) = header.mml_offsets.get(PCM_CHANNEL_INDEX) {
                let mml_start = header.voice_offset_pos + pcm_off as usize;
                if mml_start < bytes.len() {
                    let (pcm_events, pcm_tempos) = parse_mml(&bytes[mml_start..], true);
                    tempo_samples.extend(pcm_tempos);
                    if let Some(drum_track) = build_drum_track(&pcm_events, pdx) {
                        score.tracks.push(drum_track);
                    }
                }
            }
        }

        // テンポは最頻値 (mode) を採用
        if let Some(mode_byte) = mode_byte(&tempo_samples) {
            score.initial_bpm = opm_timer_b_to_bpm(mode_byte);
        }

        Ok(score)
    }
}

/// PCM チャンネルのイベント (midi_note にサンプル番号を格納) と PDX バイト列から
/// ドラムトラックを構築する。
///
/// 手順:
/// 1. PCM イベントで実際に使われているサンプル番号を集める。
/// 2. PDX を解析・デコードし、使用サンプルの音響特徴量を求める。
/// 3. 特徴量から kick/snare/oh/ch/cp を相対的に割り当てる。
/// 4. 各 PCM イベントのサンプル番号を割り当て楽器の GM ノート番号に置換した
///    `Track` (Drum) を返す。割当が 1 つも得られなければ `None`。
///
/// Builds a drum track from PCM events (whose `midi_note` holds sample numbers)
/// and PDX bytes, by analyzing waveforms and assigning drum voices relatively.
fn build_drum_track(pcm_events: &[Event], pdx_bytes: &[u8]) -> Option<Track> {
    use crate::generator::drum_classify::{classify, extract_features, SampleFeature};
    use crate::generator::readers::pdx::parse_pdx;

    if pcm_events.is_empty() {
        return None;
    }
    let bank = parse_pdx(pdx_bytes);
    if bank.samples.is_empty() {
        return None;
    }

    // 1. 使用サンプル番号を収集。
    let mut used: Vec<u8> = collect_sample_indices(pcm_events);
    used.sort_unstable();
    used.dedup();
    if used.is_empty() {
        return None;
    }

    // 2. 使用サンプルの特徴量。PDX に無い番号はスキップ。
    let mut feats: Vec<SampleFeature> = Vec::new();
    for &slot in &used {
        if let Some(sample) = bank.get(slot as usize) {
            if let Some(features) = extract_features(&sample.pcm) {
                feats.push(SampleFeature {
                    slot: slot as usize,
                    features,
                });
            }
        }
    }
    if feats.is_empty() {
        return None;
    }

    // 3. 相対分類。sample番号 -> GM ドラムノート番号 のマップを作る。
    let assignment = classify(&feats);
    let mut slot_to_gm: std::collections::HashMap<usize, u8> = std::collections::HashMap::new();
    for (slot, voice) in assignment {
        slot_to_gm.insert(slot, drum_voice_to_gm_note(voice));
    }

    // 4. PCM イベントの midi_note (サンプル番号) を GM ノートに置換。
    //    割当の無いサンプルのノートは除外する。
    let drum_events = remap_events_to_gm(pcm_events, &slot_to_gm);
    if !has_any_note(&drum_events) {
        return None;
    }

    Some(Track {
        name: "drums".to_string(),
        midi_channel: 10,
        kind: TrackKind::Drum,
        events: drum_events,
    })
}

/// イベント列 (LoopBlock 内も含む) で使われるサンプル番号を集める。
/// Collects sample indices used across events (recursing into loop blocks).
fn collect_sample_indices(events: &[Event]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::Note { midi_note, .. } => out.push(*midi_note),
            Event::LoopBlock { events, .. } => out.extend(collect_sample_indices(events)),
        }
    }
    out
}

/// `DrumVoice` を GM ドラムマップの MIDI ノート番号に対応づける。
/// emitter 側の `drum_label` が同じノートで同じラベルを返すため、これにより
/// 分類結果がそのまま kick/snare/oh/ch/cp 行として出力される。
///
/// Maps a `DrumVoice` to its GM drum note so the emitter's `drum_label` emits
/// the matching kick/snare/oh/ch/cp row.
fn drum_voice_to_gm_note(voice: crate::generator::drum_classify::DrumVoice) -> u8 {
    use crate::generator::drum_classify::DrumVoice;
    match voice {
        DrumVoice::Kick => 36,      // drum_label(36) = "kick"
        DrumVoice::Snare => 38,     // drum_label(38) = "snare"
        DrumVoice::ClosedHat => 42, // drum_label(42) = "ch"
        DrumVoice::OpenHat => 46,   // drum_label(46) = "oh"
        DrumVoice::Clap => 39,      // drum_label(39) = "cp"
    }
}

/// イベント列のサンプル番号を GM ノートに置換する (LoopBlock 内も再帰)。
/// マップに無いサンプルのノートは取り除く。
///
/// Remaps sample indices to GM notes (recursing into loop blocks); drops notes
/// whose sample has no assignment.
fn remap_events_to_gm(
    events: &[Event],
    slot_to_gm: &std::collections::HashMap<usize, u8>,
) -> Vec<Event> {
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::Note {
                start_tick,
                end_tick,
                midi_note,
                velocity,
            } => {
                if let Some(&gm) = slot_to_gm.get(&(*midi_note as usize)) {
                    out.push(Event::Note {
                        start_tick: *start_tick,
                        end_tick: *end_tick,
                        midi_note: gm,
                        velocity: *velocity,
                    });
                }
            }
            Event::LoopBlock {
                start_tick,
                events,
                count,
            } => {
                let inner = remap_events_to_gm(events, slot_to_gm);
                if has_any_note(&inner) {
                    out.push(Event::LoopBlock {
                        start_tick: *start_tick,
                        events: inner,
                        count: *count,
                    });
                }
            }
        }
    }
    out
}

/// イベント列に Note が 1 つでも含まれるか (LoopBlock 内も再帰)。
/// Whether any note exists in the events (recursing into loop blocks).
fn has_any_note(events: &[Event]) -> bool {
    events.iter().any(|e| match e {
        Event::Note { .. } => true,
        Event::LoopBlock { events, .. } => has_any_note(events),
    })
}

/// MDX ヘッダのパース結果。
/// Parsed MDX header.
struct MdxHeader {
    title: Option<String>,
    /// PDX (ADPCM サンプルバンク) ファイル名。空文字なら PDX 不要 (FM のみ)。
    /// PDX sample-bank filename; empty when no PDX is referenced.
    pdx_name: String,
    /// voice data offset (2byte BE) が書かれていたバイト位置。
    /// 全 offset はここからの相対値。
    voice_offset_pos: usize,
    /// 各チャンネルの MML offset (voice_offset_pos からの相対値)
    mml_offsets: Vec<u16>,
}

/// MDX バイト列から、参照される PDX ファイル名を取り出す。
/// ヘッダが壊れている場合や PDX 名が空の場合は `None`。
///
/// Extracts the referenced PDX filename from MDX bytes, or `None` if the header
/// is unparsable or the name is empty.
pub fn pdx_filename(bytes: &[u8]) -> Option<String> {
    let header = parse_header(bytes).ok()?;
    if header.pdx_name.is_empty() {
        None
    } else {
        Some(header.pdx_name)
    }
}

/// MDX ヘッダをパースする。
/// Parses the MDX header.
fn parse_header(bytes: &[u8]) -> Result<MdxHeader, &'static str> {
    // 1) タイトル: 0x0d 0x0a 0x1a で終端、本文は Shift-JIS
    // MDX 仕様上タイトルは Shift-JIS で記録されている。UTF-8 として読むと
    // 日本語タイトルが化けるので encoding_rs でデコードする。
    let mut i;
    let title_end = find_subseq(bytes, &[0x0d, 0x0a, 0x1a]).ok_or("missing title terminator")?;
    let title = if title_end > 0 {
        let (cow, _enc, _had_errors) = SHIFT_JIS.decode(&bytes[..title_end]);
        Some(cow.trim().to_string())
    } else {
        None
    };
    i = title_end + 3;

    // 2) PDX ファイル名: 0x00 終端 (Shift-JIS)
    let pdx_start = i;
    while i < bytes.len() && bytes[i] != 0x00 {
        i += 1;
    }
    if i >= bytes.len() {
        return Err("missing PDX filename terminator");
    }
    let (pdx_cow, _enc, _err) = SHIFT_JIS.decode(&bytes[pdx_start..i]);
    let pdx_name = pdx_cow.trim().to_string();
    i += 1; // skip 0x00

    // 3) voice data offset (2 byte BE)
    if i + 2 > bytes.len() {
        return Err("missing voice data offset");
    }
    let voice_offset_pos = i;
    let voice_off = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
    // 以降 `i` は更新しない (header の最終フィールドのため)

    // 4) MML offset 配列: voice_offset_pos からの相対で voice_off の手前まで
    //    (チャンネル数 = (voice_off - 2) / 2 + 1。先頭ch offset 自体が
    //    「先頭ch offset」を指す関係でこの式を使う)
    let mml_table_end = voice_offset_pos + voice_off as usize;
    if mml_table_end > bytes.len() {
        return Err("voice offset beyond file");
    }
    // 先頭の MML offset の値そのものが「先頭 MML が始まる位置」を指すので、
    // それを基準にチャンネル数を逆算する。
    if voice_offset_pos + 2 > bytes.len() {
        return Err("missing first MML offset");
    }
    let first_mml_off =
        u16::from_be_bytes([bytes[voice_offset_pos + 2], bytes[voice_offset_pos + 3]]);
    let mml_offsets_len = ((first_mml_off as usize).saturating_sub(2)) / 2 + 1;
    let mut mml_offsets = Vec::with_capacity(mml_offsets_len);
    for k in 0..mml_offsets_len {
        let pos = voice_offset_pos + 2 + k * 2;
        if pos + 2 > bytes.len() {
            break;
        }
        let off = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]);
        mml_offsets.push(off);
    }

    Ok(MdxHeader {
        title,
        pdx_name,
        voice_offset_pos,
        mml_offsets,
    })
}

/// バイト列の中から最初の `needle` を見つけた位置を返す。
fn find_subseq(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 1 チャンネル分の MML をパースしてイベント列を作る。
///
/// 戻り値: `(events, tempo_samples)`。tempo_samples は出てきた `0xFF n` の n を集めたもの。
///
/// Parses one channel's MML and returns events plus collected tempo bytes.
fn parse_mml(data: &[u8], is_pcm: bool) -> (Vec<Event>, Vec<u8>) {
    let mut events: Vec<Event> = Vec::new();
    let mut tempos: Vec<u8> = Vec::new();
    let mut cursor_tick: u64 = 0;
    let mut i = 0usize;

    // ループ開始位置をスタックで覚える: (data 上の位置, count, events.len() 時の値, cursor_tick)
    struct LoopFrame {
        events_start: usize,
        cursor_start: u64,
    }
    let mut loop_frames: Vec<LoopFrame> = Vec::new();

    while i < data.len() {
        let b = data[i];
        match b {
            0x00..=0x7F => {
                // Rest: 1 byte, clock = value
                let clocks = b as u64;
                cursor_tick += clocks;
                i += 1;
            }
            0x80..=0xDF => {
                // Note: 2 byte。
                // FM チャンネルでは `byte - 0x80` が音高オフセット (MIDI に換算)。
                // PCM チャンネルでは `byte - 0x80` が PDX サンプル番号 (index)。
                // 後者はここでは `midi_note` フィールドにサンプル番号をそのまま
                // 格納し、reader 側で分類結果に基づき GM ドラムノートへ置換する。
                if i + 1 >= data.len() {
                    break;
                }
                let duration = data[i + 1] as u64;
                let value = if is_pcm {
                    // PCM: サンプル番号 (0..95) をそのまま保持。
                    b - 0x80
                } else {
                    MDX_NOTE_BASE.saturating_add(b - 0x80)
                };
                events.push(Event::Note {
                    start_tick: cursor_tick,
                    end_tick: cursor_tick + duration,
                    midi_note: value,
                    velocity: 100,
                });
                cursor_tick += duration;
                i += 2;
            }
            // コマンド長は mdxtools/mdx_decompiler.c の実装に従う。
            // The lengths below follow the actual mdxtools decompiler — the
            // docs/MDX.md tables are inconsistent with the player and would
            // mis-parse real songs.
            0xE7 => {
                // Fade out: 0 byte (no output)
                i += 1;
            }
            0xE8 => {
                // PCM8 enable: 0 byte
                i += 1;
            }
            0xE9 => {
                // Modulation delay (LFO key-on delay): 1 byte
                i += 2;
            }
            0xEA => {
                // OPM LFO (MH): switch byte が 0x80/0x81 なら 2 byte (ON/OFF)、
                // それ以外は opcode + 6 params = 7 byte。
                if i + 1 >= data.len() {
                    break;
                }
                let sw = data[i + 1];
                let n = if sw == 0x80 || sw == 0x81 { 2 } else { 7 };
                i += n;
            }
            0xEB | 0xEC => {
                // Modulation amplitude (MA) / Modulation pitch (MP):
                // switch byte が 0x80/0x81 なら 2 byte、それ以外は 4 byte (opcode + 3 params)。
                if i + 1 >= data.len() {
                    break;
                }
                let sw = data[i + 1];
                let n = if sw == 0x80 || sw == 0x81 { 2 } else { 4 };
                i += n;
            }
            0xED => {
                // ADPCM/Waveform set: 1 byte
                i += 2;
            }
            0xEE => {
                // PCM wait (W): 0 byte
                i += 1;
            }
            0xEF => {
                // Sample select (S): 1 byte
                i += 2;
            }
            0xF0 => {
                // Key code (k) / key-on delay: 1 byte
                i += 2;
            }
            0xF1 => {
                // End marker / loop pointer: 0x00 で曲終了、その他は曲全体
                // ループの戻り先マーカー。本 reader はループ展開を扱わない
                // (チャネル末端を意味する) ため、いずれの場合も break する。
                // (mdx_decompiler.c も `0xF1` 以降の MML を出力しない)
                break;
            }
            0xF2 => {
                // Portamento: 2 byte (opcode + signed 16-bit)
                i += 3;
            }
            0xF3 => {
                // Detune: 2 byte (opcode + signed 16-bit)
                i += 3;
            }
            0xF4 => {
                // Sync `/`: 0 byte
                i += 1;
            }
            0xF5 => {
                // Loop end `]nn`: 2 byte (signed 16-bit back-offset)
                // 対応する 0xF6 (loop start) のフレームがあれば、ループブロック
                // としてまとめる。仕様で「任意回数のループは 2 回固定」と
                // 決めているため、count = 2 を常に採用する。
                if let Some(frame) = loop_frames.pop() {
                    let inner: Vec<Event> = events.drain(frame.events_start..).collect();
                    let one_iter_len = cursor_tick - frame.cursor_start;
                    events.push(Event::LoopBlock {
                        start_tick: frame.cursor_start,
                        events: inner,
                        count: 2,
                    });
                    cursor_tick = frame.cursor_start + one_iter_len * 2;
                }
                i += 3;
            }
            0xF6 => {
                // Loop start `[`: 0 byte (marker only)
                loop_frames.push(LoopFrame {
                    events_start: events.len(),
                    cursor_start: cursor_tick,
                });
                i += 1;
            }
            0xF7 => {
                // Key off flag (sets next_key_off): 0 byte
                i += 1;
            }
            0xF8 => {
                // Gate time (q): 1 byte
                i += 2;
            }
            0xF9 => {
                // Macro close `)`: 0 byte
                i += 1;
            }
            0xFA => {
                // Macro open `(`: 0 byte
                i += 1;
            }
            0xFB => {
                // Volume: 1 byte
                i += 2;
            }
            0xFC => {
                // Pan: 1 byte
                i += 2;
            }
            0xFD => {
                // Voice select: 1 byte
                i += 2;
            }
            0xFE => {
                // LFO `y`: 2 byte
                i += 3;
            }
            0xFF => {
                // Tempo: 1 byte
                if i + 1 < data.len() {
                    tempos.push(data[i + 1]);
                }
                i += 2;
            }
            // 0xE0-0xE6 は mdx_decompiler でも未定義。出会ったらこのチャネルの
            // パースを中断する (フォールスルーで音符として誤読しないため)。
            // Anything in 0xE0-0xE6 is undefined per mdxtools — stop parsing
            // this channel rather than risk mis-aligning the byte stream.
            _ => {
                break;
            }
        }
    }

    (events, tempos)
}

/// バイト列の最頻値を返す。
fn mode_byte(bytes: &[u8]) -> Option<u8> {
    let mut counts: [u32; 256] = [0; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let (idx, max) = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, &c)| c)
        .unwrap_or((0, &0));
    if *max == 0 {
        None
    } else {
        Some(idx as u8)
    }
}

/// MDX (MXDRV / OPM Timer B) のテンポバイト (`@t` 値) を BPM に変換する。
///
/// MXDRV のテンポは OPM Timer B 値 `n` で表され、1 四分音符 = 48 OPM clock、
/// OPM クロック = 4MHz、Timer B 1 tick = 1024/(4MHz/16) 秒 を前提とする。
/// mdxtools の換算 `opm_tempo = 256 - 60*opm_clock/(bpm*48*1024)`（opm_clock=4_000_000）
/// を BPM について解くと次式になる。
///
/// `BPM = 60 * 4_000_000 / ((256 - n) * 48 * 1024)`
///
/// 旧実装の `78125 / (256 - n)` は係数が約 16 倍ずれており、
/// 典型値域で clamp 上限 (480) に張り付くバグがあった。
///
/// # 引数 / Arguments
/// * `n` - OPM Timer B 値 (`@t` の引数, 0-255) / OPM Timer B value
///
/// # 戻り値 / Returns
/// BPM (beats per minute)。極端な `n` は音楽的な範囲 (20-400) にクランプする。
/// BPM, clamped to a musical range (20-400) for extreme `n`.
fn opm_timer_b_to_bpm(n: u8) -> f32 {
    // 60 * 4_000_000 / (48 * 1024) = 240_000_000 / 49_152 ≈ 4882.8125
    const NUMERATOR: f32 = 240_000_000.0 / 49_152.0;
    let denom = 256u32.saturating_sub(n as u32).max(1) as f32;
    (NUMERATOR / denom).clamp(20.0, 400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小限の MDX を組み立てるヘルパ。
    ///
    /// 1 ch (FM A) に 4 分音符 C を 4 つ並べる。
    fn build_minimal_mdx() -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        // タイトル
        out.extend_from_slice(b"TEST");
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        // PDX なし (0x00 終端)
        out.push(0x00);

        // この時点で voice offset position が確定する。
        // voice data offset (2 byte BE)。先頭 MML offset は 2 (チャンネル数 1) を指すので、
        // voice off は 4 (= 2 + 2*1) になる。
        let voice_off: u16 = 4;
        out.extend_from_slice(&voice_off.to_be_bytes());
        // MML offset 配列 (1 channel)
        let mml_off: u16 = 4;
        out.extend_from_slice(&mml_off.to_be_bytes());
        // ここから voice data (空でも良い)
        // MML 開始は voice_offset_pos + mml_off。voice_offset_pos は
        // タイトル + 終端 3 + PDX 1 = 4 + 3 + 1 = 8 バイト目。
        // voice_offset_pos + mml_off = 8 + 4 = 12。
        // 現在 out.len() = 8 + 2 + 2 = 12。 ちょうど MML 開始位置。

        // 4 分音符 (48 clock) を C で 4 つ。 0x80 が MIDI C(=39+0)、4 つ並べる
        for _ in 0..4 {
            out.push(0x80); // note
            out.push(48); // duration
        }
        out.push(0xF1); // performance end
        out.push(0x00);
        out
    }

    #[test]
    fn parses_title_and_one_channel() {
        let bytes = build_minimal_mdx();
        let score = MdxReader.read(&bytes, "test.mdx").unwrap();
        assert_eq!(score.title.as_deref(), Some("TEST"));
        assert_eq!(score.tracks.len(), 1);
        let t = &score.tracks[0];
        assert_eq!(t.name, "fm_a");
        assert_eq!(t.kind, TrackKind::Melodic);
        assert_eq!(t.events.len(), 4);
    }

    /// MDX のタイトルは Shift-JIS で記録されているので、UTF-8 ではなく
    /// Shift-JIS としてデコードした結果がタイトルになることを確認する。
    /// Title bytes "テスト" (= 0x83 0x65 0x83 0x58 0x83 0x67 in Shift-JIS)
    /// must be decoded to the UTF-8 string "テスト".
    #[test]
    fn parses_shift_jis_japanese_title() {
        let mut out: Vec<u8> = Vec::new();
        // "テスト" を Shift-JIS で
        out.extend_from_slice(&[0x83, 0x65, 0x83, 0x58, 0x83, 0x67]);
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        out.push(0x00); // PDX 終端
        out.extend_from_slice(&4u16.to_be_bytes()); // voice off
        out.extend_from_slice(&4u16.to_be_bytes()); // mml off
        out.push(0x80);
        out.push(48);
        out.push(0xF1);
        out.push(0x00);

        let score = MdxReader.read(&out, "jp.mdx").unwrap();
        assert_eq!(score.title.as_deref(), Some("テスト"));
    }

    /// 9 チャンネル (FM 8 + PCM 1) の MDX を組み立てるヘルパ。
    /// FM A (index0) に音符を、PCM (index8) に `pcm_mml` を配置する。
    /// FM B-H (index1-7) は即終了 (0xF1 0x00) にする。
    ///
    /// `pcm_mml` は PCM チャンネルの生バイト列 (note=0x80+sample, dur, ... 0xF1 0x00 込み)。
    fn build_mdx_with_pcm(pcm_mml: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"PCMT");
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        out.push(0x00); // PDX 名なし

        let n_ch = 9usize;
        // voice_offset_pos はここ。voice off = 2 + 2*n_ch。
        let voice_off: u16 = (2 + 2 * n_ch) as u16;
        out.extend_from_slice(&voice_off.to_be_bytes());

        // 各チャンネルの MML 本体を組み立て、offset を計算する。
        // FM A: 音符 1 つ、FM B-H: 即終了、PCM: pcm_mml。
        let fm_a: Vec<u8> = vec![0x80, 48, 0xF1, 0x00];
        let fm_other: Vec<u8> = vec![0xF1, 0x00];
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        bodies.push(fm_a);
        for _ in 1..8 {
            bodies.push(fm_other.clone());
        }
        bodies.push(pcm_mml.to_vec());

        // MML offset テーブル: voice_offset_pos からの相対。
        // テーブル自体が voice_off バイト分。本体はその後ろに連結する。
        let mut offsets: Vec<u16> = Vec::new();
        let mut acc = voice_off as usize; // 本体開始は voice_off の直後 (テーブル末尾)
        for b in &bodies {
            offsets.push(acc as u16);
            acc += b.len();
        }
        for off in &offsets {
            out.extend_from_slice(&off.to_be_bytes());
        }
        for b in &bodies {
            out.extend_from_slice(b);
        }
        out
    }

    /// 指定スロットに与えた ADPCM バイト列を載せた PDX を組み立てる。
    fn build_pdx(samples: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let header_len = 96 * 8;
        let mut buf = vec![0u8; header_len];
        let mut data: Vec<u8> = Vec::new();
        for (slot, bytes) in samples {
            let off = header_len + data.len();
            let size = bytes.len();
            let base = slot * 8;
            buf[base..base + 4].copy_from_slice(&(off as u32).to_be_bytes());
            buf[base + 4..base + 8].copy_from_slice(&(size as u32).to_be_bytes());
            data.extend_from_slice(bytes);
        }
        buf.extend_from_slice(&data);
        buf
    }

    #[test]
    fn pcm_channel_builds_drum_track_with_pdx() {
        // PCM チャンネル: sample 0 を 2 回、sample 1 を 1 回鳴らす。
        // note byte = 0x80 + sample, 次バイト = duration。
        let pcm_mml = vec![
            0x80, 24, // sample0
            0x81, 24, // sample1
            0x80, 24, // sample0
            0xF1, 0x00,
        ];
        let mdx = build_mdx_with_pcm(&pcm_mml);

        // PDX: slot0 は低域っぽい波形 (なだらか)、slot1 は高域ノイズ。
        let low: Vec<u8> = (0..200)
            .map(|i| if i % 2 == 0 { 0x11 } else { 0x10 })
            .collect();
        let high: Vec<u8> = (0..200)
            .map(|i| if i % 2 == 0 { 0x8F } else { 0x0F })
            .collect();
        let pdx = build_pdx(&[(0, low), (1, high)]);

        let score = MdxReader
            .read_with_pdx(&mdx, Some(&pdx), "test.mdx")
            .unwrap();
        // ドラムトラックが 1 つ存在する。
        let drum = score
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Drum)
            .expect("drum track should exist");
        assert_eq!(drum.name, "drums");
        assert_eq!(drum.midi_channel, 10);
        // 3 つの発音 → 3 ノート。midi_note は GM ドラムノート (36/38/42/46/39 のいずれか)。
        let notes: Vec<u8> = drum
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Note { midi_note, .. } => Some(*midi_note),
                _ => None,
            })
            .collect();
        assert_eq!(notes.len(), 3);
        assert!(notes.iter().all(|&n| [36, 38, 42, 46, 39].contains(&n)));
    }

    #[test]
    fn pcm_channel_without_pdx_yields_no_drum_track() {
        let pcm_mml = vec![0x80, 24, 0xF1, 0x00];
        let mdx = build_mdx_with_pcm(&pcm_mml);
        // PDX なし → ドラムトラックは作られない。
        let score = MdxReader.read_with_pdx(&mdx, None, "test.mdx").unwrap();
        assert!(score.tracks.iter().all(|t| t.kind != TrackKind::Drum));
    }

    #[test]
    fn note_duration_uses_second_byte() {
        let bytes = build_minimal_mdx();
        let score = MdxReader.read(&bytes, "test.mdx").unwrap();
        let t = &score.tracks[0];
        match &t.events[0] {
            Event::Note {
                start_tick,
                end_tick,
                midi_note,
                ..
            } => {
                assert_eq!(*start_tick, 0);
                assert_eq!(*end_tick, 48); // 1 四分音符
                assert_eq!(*midi_note, MDX_NOTE_BASE);
            }
            _ => panic!("expected Note"),
        }
        // 2 つ目のノートは tick 48 から
        match &t.events[1] {
            Event::Note { start_tick, .. } => assert_eq!(*start_tick, 48),
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn rest_byte_advances_cursor() {
        // タイトル + PDX 終端 + voice offset + 1 ch MML offset
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"R");
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        out.push(0x00);
        out.extend_from_slice(&4u16.to_be_bytes()); // voice off
        out.extend_from_slice(&4u16.to_be_bytes()); // mml off
                                                    // MML: 48 tick の休符 → C を 48 tick
        out.push(0x30); // 0x30 = 48 → rest 48 clock
        out.push(0x80);
        out.push(48);
        out.push(0xF1);
        out.push(0x00);

        let score = MdxReader.read(&out, "rest.mdx").unwrap();
        let t = &score.tracks[0];
        assert_eq!(t.events.len(), 1);
        match &t.events[0] {
            Event::Note {
                start_tick,
                end_tick,
                ..
            } => {
                assert_eq!(*start_tick, 48);
                assert_eq!(*end_tick, 96);
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn loop_block_with_count_2_is_emitted_for_any_loop() {
        // 0xF6 (`[`、1 byte) ... 0xF5 nn nn (`]`、3 byte) を「2 回固定」で
        // LoopBlock に包むことを確認する。本実装は mdxtools の実装に従い
        // 0xF6 を 1 byte command として扱う (docs/MDX.md と実装が食い違うので
        // 注意)。
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"L");
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        out.push(0x00);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        // ループ開始: F6 (1 byte)
        out.push(0xF6);
        // 中身: C (note 0x80, dur 48)
        out.push(0x80);
        out.push(48);
        // ループ終了: F5 -3 (offset, 3 byte)
        out.push(0xF5);
        out.push(0xFF);
        out.push(0xFD);
        // performance end
        out.push(0xF1);
        out.push(0x00);

        let score = MdxReader.read(&out, "loop.mdx").unwrap();
        let t = &score.tracks[0];
        // events は LoopBlock { count: 2, ... } 1 個になる
        assert_eq!(t.events.len(), 1);
        match &t.events[0] {
            Event::LoopBlock { count, events, .. } => {
                assert_eq!(*count, 2);
                assert_eq!(events.len(), 1);
            }
            _ => panic!("expected LoopBlock"),
        }
    }

    #[test]
    fn tempo_mode_byte_drives_bpm() {
        // 0xFF n を 1 つ入れて、BPM が opm_timer_b_to_bpm(n) と一致することを確認
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"T");
        out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
        out.push(0x00);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&4u16.to_be_bytes());
        out.push(0xFF);
        out.push(200);
        out.push(0x80);
        out.push(48);
        out.push(0xF1);
        out.push(0x00);

        let score = MdxReader.read(&out, "tempo.mdx").unwrap();
        let expected = opm_timer_b_to_bpm(200);
        assert!((score.initial_bpm - expected).abs() < 0.5);
    }

    /// OPM Timer B 値 → BPM の正しい換算式を検証する。
    /// 正しい式: BPM = 60 * 4_000_000 / ((256 - n) * 48 * 1024)
    /// （mdxtools の `opm_tempo = 256 - 60*opm_clock/(bpm*48*1024)` の逆算）
    #[test]
    fn opm_timer_b_to_bpm_uses_correct_formula() {
        // @t200 (MXDRV 既定) ≈ 87.2 BPM
        assert!((opm_timer_b_to_bpm(200) - 87.2).abs() < 0.5);
        // n=219 (実曲 lfoma.mdx) ≈ 132 BPM。旧式では 480 に張り付いていた回帰防止
        assert!((opm_timer_b_to_bpm(219) - 132.0).abs() < 0.5);
        // n=224 ≈ 152.6 BPM
        assert!((opm_timer_b_to_bpm(224) - 152.6).abs() < 0.5);
        // 旧バグ: 480 への張り付きが起きないこと（典型値域で 480 を返さない）
        assert!(opm_timer_b_to_bpm(219) < 200.0);
    }
}
