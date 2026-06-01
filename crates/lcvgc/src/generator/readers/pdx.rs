//! PDX (X68000 MXDRV ADPCM サンプルバンク) パーサと OKI MSM6258 ADPCM デコーダ。
//!
//! PDX はヘッダに 96 個の `(offset: u32 BE, size: u32 BE)` エントリを持ち、各
//! エントリが ADPCM サンプル 1 つの位置と長さ (バイト数) を示す。`offset`/`size`
//! がともに非 0 のエントリが有効サンプル。ADPCM は OKI MSM6258 (Dialogic
//! 4bit ADPCM) で、1 バイトが上位ニブル → 下位ニブルの順に 2 サンプルを表す。
//!
//! PDX is the sample bank that accompanies an MDX song. The header holds 96
//! `(offset, size)` entries (big-endian u32). Each non-empty entry is one
//! ADPCM sample. Samples are OKI MSM6258 4-bit ADPCM (high nibble first).

/// PDX のサンプルエントリ数 (固定 96)。
/// Number of sample slots in a PDX header (fixed at 96).
const PDX_SLOTS: usize = 96;

/// OKI MSM6258 ADPCM のステップサイズテーブル (49 エントリ)。
/// Step-size table for OKI MSM6258 ADPCM (49 entries).
const STEP_TABLE: [i32; 49] = [
    16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552,
];

/// ニブル下位 3bit からステップインデックスの増減を引く調整テーブル。
/// Step-index adjustment indexed by the low 3 bits of a nibble.
const STEP_ADJUST: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// X68000 ADPCM の標準的な再生サンプルレート (約 15.6kHz)。特徴量計算で
/// 「秒」や「ミリ秒」に換算する際の基準に使う。
/// Nominal ADPCM playback rate (~15.6kHz), used to convert sample counts into
/// time for feature extraction.
pub const ADPCM_SAMPLE_RATE: f32 = 15625.0;

/// デコード済みの 1 サンプル (PCM 波形) と元のスロット番号。
/// A decoded PCM sample plus its originating PDX slot index.
#[derive(Debug, Clone, PartialEq)]
pub struct PdxSample {
    /// PDX 内のスロット番号 (0..96)。MDX 側の音色番号と対応する。
    /// Slot index within the PDX (0..96), matching the MDX voice number.
    pub slot: usize,
    /// デコード済み PCM (符号付き 12bit 相当の値域、-2048..=2047)。
    /// Decoded PCM samples (roughly signed 12-bit, -2048..=2047).
    pub pcm: Vec<i32>,
}

/// PDX バンク全体。スロット番号でサンプルを引ける。
/// An entire PDX bank, indexable by slot number.
#[derive(Debug, Clone, Default)]
pub struct PdxBank {
    /// 有効サンプルの一覧 (スロット昇順)。
    /// Valid samples in ascending slot order.
    pub samples: Vec<PdxSample>,
}

impl PdxBank {
    /// 指定スロットのデコード済み PCM を返す。無ければ `None`。
    ///
    /// Returns the decoded PCM for `slot`, or `None` if absent.
    pub fn get(&self, slot: usize) -> Option<&PdxSample> {
        self.samples.iter().find(|s| s.slot == slot)
    }
}

/// 4bit ADPCM ニブルを 1 つデコードし、`sig` (現在値) と `idx` (ステップ
/// インデックス) を更新して新しい `sig` を返す。
///
/// Decodes a single 4-bit ADPCM nibble, updating predictor `sig` and step
/// index `idx`, returning the new `sig`.
#[inline]
fn decode_nibble(nibble: u8, sig: &mut i32, idx: &mut i32) -> i32 {
    let step = STEP_TABLE[*idx as usize];
    let mut diff = step >> 3;
    if nibble & 1 != 0 {
        diff += step >> 2;
    }
    if nibble & 2 != 0 {
        diff += step >> 1;
    }
    if nibble & 4 != 0 {
        diff += step;
    }
    if nibble & 8 != 0 {
        diff = -diff;
    }
    *sig = (*sig + diff).clamp(-2048, 2047);
    *idx = (*idx + STEP_ADJUST[(nibble & 7) as usize]).clamp(0, 48);
    *sig
}

/// ADPCM バイト列を PCM サンプル列にデコードする。1 バイトにつき上位ニブル →
/// 下位ニブルの順で 2 サンプルを生成する。
///
/// Decodes an ADPCM byte slice into PCM samples (high nibble first, two
/// samples per byte).
///
/// # 引数 / Arguments
/// * `bytes` - ADPCM バイト列 / ADPCM bytes
///
/// # 戻り値 / Returns
/// デコード済み PCM サンプル列 / decoded PCM samples
pub fn decode_adpcm(bytes: &[u8]) -> Vec<i32> {
    let mut sig = 0i32;
    let mut idx = 0i32;
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(decode_nibble((b >> 4) & 0x0F, &mut sig, &mut idx));
        out.push(decode_nibble(b & 0x0F, &mut sig, &mut idx));
    }
    out
}

/// PDX バイナリをパースし、全有効サンプルをデコードして `PdxBank` を返す。
///
/// ヘッダが壊れている (短すぎる) 場合や有効サンプルが 1 つも無い場合は空の
/// `PdxBank` を返す (エラーにはしない)。範囲外を指すエントリは無視する。
///
/// Parses a PDX binary, decoding every valid sample. Returns an empty bank when
/// the header is too short or no valid samples exist. Out-of-range entries are
/// skipped.
///
/// # 引数 / Arguments
/// * `bytes` - PDX ファイル全体のバイト列 / the full PDX file bytes
///
/// # 戻り値 / Returns
/// デコード済みサンプルを保持する `PdxBank` / a populated `PdxBank`
pub fn parse_pdx(bytes: &[u8]) -> PdxBank {
    let header_len = PDX_SLOTS * 8;
    if bytes.len() < header_len {
        return PdxBank::default();
    }
    let mut samples = Vec::new();
    for slot in 0..PDX_SLOTS {
        let base = slot * 8;
        let off = u32::from_be_bytes([
            bytes[base],
            bytes[base + 1],
            bytes[base + 2],
            bytes[base + 3],
        ]) as usize;
        let size = u32::from_be_bytes([
            bytes[base + 4],
            bytes[base + 5],
            bytes[base + 6],
            bytes[base + 7],
        ]) as usize;
        if off == 0 || size == 0 {
            continue;
        }
        let end = match off.checked_add(size) {
            Some(e) => e,
            None => continue,
        };
        if end > bytes.len() {
            continue;
        }
        let pcm = decode_adpcm(&bytes[off..end]);
        if pcm.is_empty() {
            continue;
        }
        samples.push(PdxSample { slot, pcm });
    }
    PdxBank { samples }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_short_input_yields_empty_bank() {
        assert!(parse_pdx(&[]).samples.is_empty());
        assert!(parse_pdx(&[0u8; 10]).samples.is_empty());
    }

    #[test]
    fn decodes_single_sample_entry() {
        // ヘッダ 96*8 バイト + 4 バイトの ADPCM データ 1 サンプル。
        let header_len = PDX_SLOTS * 8;
        let mut buf = vec![0u8; header_len];
        // slot 1 に off=header_len, size=4 を書く。
        let off = header_len as u32;
        let size = 4u32;
        let base = 8; // slot 1 のエントリ先頭 (1 * 8)
        buf[base..base + 4].copy_from_slice(&off.to_be_bytes());
        buf[base + 4..base + 8].copy_from_slice(&size.to_be_bytes());
        buf.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // ADPCM 4 バイト = 8 サンプル

        let bank = parse_pdx(&buf);
        assert_eq!(bank.samples.len(), 1);
        assert_eq!(bank.samples[0].slot, 1);
        assert_eq!(bank.samples[0].pcm.len(), 8); // 4 バイト × 2 ニブル
    }

    #[test]
    fn skips_out_of_range_entry() {
        let header_len = PDX_SLOTS * 8;
        let mut buf = vec![0u8; header_len];
        // 範囲外を指すエントリ (off=巨大値) は無視される。
        let base = 2 * 8;
        buf[base..base + 4].copy_from_slice(&(header_len as u32 + 1000).to_be_bytes());
        buf[base + 4..base + 8].copy_from_slice(&100u32.to_be_bytes());
        let bank = parse_pdx(&buf);
        assert!(bank.samples.is_empty());
    }

    #[test]
    fn adpcm_decode_is_deterministic_and_bounded() {
        let pcm = decode_adpcm(&[0xFF, 0x00, 0x88, 0x44, 0xCC]);
        assert_eq!(pcm.len(), 10);
        // 値域は -2048..=2047 に収まる。
        assert!(pcm.iter().all(|&v| (-2048..=2047).contains(&v)));
    }

    #[test]
    fn bank_get_by_slot() {
        let header_len = PDX_SLOTS * 8;
        let mut buf = vec![0u8; header_len];
        let base = 5 * 8;
        buf[base..base + 4].copy_from_slice(&(header_len as u32).to_be_bytes());
        buf[base + 4..base + 8].copy_from_slice(&2u32.to_be_bytes());
        buf.extend_from_slice(&[0x12, 0x34]);
        let bank = parse_pdx(&buf);
        assert!(bank.get(5).is_some());
        assert!(bank.get(0).is_none());
    }
}
