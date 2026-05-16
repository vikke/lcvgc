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
        let header = parse_header(bytes).map_err(|m| GeneratorError::Parse {
            format: "mdx",
            message: format!("{} ({})", m, source_name),
        })?;

        let mut score = Score {
            ppq: MDX_PPQ,
            initial_bpm: 120.0,
            title: header.title,
            ..Score::default()
        };

        // 各 FM チャンネル (最大 8) を順に処理
        let mut tempo_samples: Vec<u8> = Vec::new();
        for (idx, &mml_off) in header
            .mml_offsets
            .iter()
            .enumerate()
            .take(8.min(header.mml_offsets.len()))
        {
            let mml_start = header.voice_offset_pos + mml_off as usize;
            if mml_start >= bytes.len() {
                continue;
            }
            let ch_name = format!("fm_{}", (b'a' + idx as u8) as char);
            let (events, tempos) = parse_mml(&bytes[mml_start..]);
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

        // テンポは最頻値 (mode) を採用
        if let Some(mode_byte) = mode_byte(&tempo_samples) {
            score.initial_bpm = opm_timer_b_to_bpm(mode_byte);
        }

        Ok(score)
    }
}

/// MDX ヘッダのパース結果。
/// Parsed MDX header.
struct MdxHeader {
    title: Option<String>,
    /// voice data offset (2byte BE) が書かれていたバイト位置。
    /// 全 offset はここからの相対値。
    voice_offset_pos: usize,
    /// 各チャンネルの MML offset (voice_offset_pos からの相対値)
    mml_offsets: Vec<u16>,
}

/// MDX ヘッダをパースする。
/// Parses the MDX header.
fn parse_header(bytes: &[u8]) -> Result<MdxHeader, &'static str> {
    // 1) タイトル: 0x0d 0x0a 0x1a で終端
    let mut i;
    let title_end = find_subseq(bytes, &[0x0d, 0x0a, 0x1a]).ok_or("missing title terminator")?;
    let title = if title_end > 0 {
        Some(
            String::from_utf8_lossy(&bytes[..title_end])
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    i = title_end + 3;

    // 2) PDX ファイル名: 0x00 終端
    while i < bytes.len() && bytes[i] != 0x00 {
        i += 1;
    }
    if i >= bytes.len() {
        return Err("missing PDX filename terminator");
    }
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
fn parse_mml(data: &[u8]) -> (Vec<Event>, Vec<u8>) {
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
                // Note: 2 byte
                if i + 1 >= data.len() {
                    break;
                }
                let duration = data[i + 1] as u64;
                let midi = MDX_NOTE_BASE.saturating_add(b - 0x80);
                events.push(Event::Note {
                    start_tick: cursor_tick,
                    end_tick: cursor_tick + duration,
                    midi_note: midi,
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

/// MDX (MXDRV / OPM Timer B) のテンポバイトを BPM 近似値に変換する。
///
/// OPM Timer B クロック式: 1 clock の周期 = (256 - n) * 1024 / (clock_freq / 16)
/// クロック 4MHz、1 四分音符 = 48 clock 前提で BPM 換算する。
///
/// Approximation: `BPM ≈ 78125 / (256 - n)` (1/4 note = 48 clocks, 4 MHz OPM).
fn opm_timer_b_to_bpm(n: u8) -> f32 {
    let denom = 256u32.saturating_sub(n as u32).max(1) as f32;
    // 78125 / (256 - n) は 100 BPM 付近で標準的な MXDRV 速度に近似
    (78_125.0 / denom).clamp(20.0, 480.0)
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
}
