//! ADPCM サンプル波形からドラム楽器 (kick/snare/oh/ch/cp) を推定する分類器。
//!
//! 設計方針: 絶対的な音色辞書を持たず、「**その曲で実際に使われている n 種の
//! サンプル集合の中で相対的に**どれが kick らしいか、hat らしいか」を割り当てる。
//! これにより、曲ごとに音色 (PCM 番号と中身) が異なっても汎用的に機能する。
//!
//! 各サンプルから音響特徴量 (長さ・減衰時間・高域比・ゼロ交差率・低域比) を求め、
//! 優先順位つき貪欲法で 5 種の楽器ラベルに割り当てる。
//!
//! Estimates drum instruments from ADPCM waveforms. Rather than an absolute
//! timbre dictionary, it assigns labels *relative to the set of samples used in
//! the song*: among the used samples, which is most kick-like, most hat-like,
//! etc. This generalizes across songs whose PCM banks differ.

use super::readers::pdx::ADPCM_SAMPLE_RATE;

/// 推定対象のドラム楽器ラベル。emitter の行ラベルにそのまま使う。
/// Drum instrument labels emitted as drum-row labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumVoice {
    /// バスドラム / kick
    Kick,
    /// スネア / snare
    Snare,
    /// クローズドハイハット / closed hi-hat
    ClosedHat,
    /// オープンハイハット / open hi-hat
    OpenHat,
    /// クラップ / clap
    Clap,
}

impl DrumVoice {
    /// emitter 用の楽器名文字列を返す。
    /// Returns the instrument label string used by the emitter.
    pub fn label(self) -> &'static str {
        match self {
            DrumVoice::Kick => "kick",
            DrumVoice::Snare => "snare",
            DrumVoice::ClosedHat => "ch",
            DrumVoice::OpenHat => "oh",
            DrumVoice::Clap => "cp",
        }
    }
}

/// 1 サンプルの音響特徴量。
/// Acoustic features of one sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Features {
    /// サンプル長 (ミリ秒)
    /// Length in milliseconds
    pub dur_ms: f32,
    /// 振幅包絡がピークから -20dB (10%) に落ちるまでの時間 (ミリ秒)
    /// Decay time from peak to -20 dB (10%) in milliseconds
    pub decay_ms: f32,
    /// ゼロ交差率 (0..1)。ノイズ性・高域成分が強いほど高い
    /// Zero-crossing rate (0..1); higher for noisy/high-frequency content
    pub zcr: f32,
    /// 高域エネルギー比 (1 次差分パワー / 全パワー)。高域ほど大きい
    /// High-frequency energy ratio (first-difference power / total power)
    pub hf_ratio: f32,
    /// 低域エネルギー比 (1 - hf_ratio に相当する低域寄与)。kick で大きい
    /// Low-frequency energy ratio; large for kicks
    pub lf_ratio: f32,
}

/// PCM サンプル列から特徴量を計算する。サンプルが短すぎる場合は `None`。
///
/// Computes features from a PCM sample. Returns `None` if too short.
///
/// # 引数 / Arguments
/// * `pcm` - デコード済み PCM サンプル列 / decoded PCM samples
///
/// # 戻り値 / Returns
/// 特徴量 / the extracted features (or `None` when too short)
pub fn extract_features(pcm: &[i32]) -> Option<Features> {
    let n = pcm.len();
    if n < 16 {
        return None;
    }
    // ピーク正規化 (絶対値の最大で割る)。
    let peak = pcm
        .iter()
        .map(|&v| v.unsigned_abs())
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let s: Vec<f32> = pcm.iter().map(|&v| v as f32 / peak).collect();

    let dur_ms = n as f32 / ADPCM_SAMPLE_RATE * 1000.0;

    // ゼロ交差率。
    let mut zc = 0usize;
    for i in 1..n {
        if (s[i - 1] < 0.0) != (s[i] < 0.0) {
            zc += 1;
        }
    }
    let zcr = zc as f32 / n as f32;

    // 高域比: 1 次差分パワー / 全パワー。差分が大きい = 高域成分が強い。
    let mut diff_energy = 0.0f32;
    let mut tot_energy = 0.0f32;
    for i in 0..n {
        tot_energy += s[i] * s[i];
        if i > 0 {
            let d = s[i] - s[i - 1];
            diff_energy += d * d;
        }
    }
    let tot_energy = tot_energy.max(1e-9);
    // 差分パワーは元パワーの最大 4 倍程度になるので 1/4 で 0..1 付近に正規化。
    let hf_ratio = (diff_energy / (tot_energy * 4.0)).clamp(0.0, 1.0);
    let lf_ratio = 1.0 - hf_ratio;

    // 減衰時間: 64 サンプルごとの RMS 包絡を作り、ピーク後に 10% を切るまでの長さ。
    let frame = 64usize;
    let mut env: Vec<f32> = Vec::new();
    let mut i = 0;
    while i + frame <= n {
        let e: f32 = s[i..i + frame].iter().map(|&v| v * v).sum();
        env.push((e / frame as f32).sqrt());
        i += frame;
    }
    let decay_ms = if env.is_empty() {
        dur_ms
    } else {
        let (peak_idx, &emax) = env
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        let threshold = emax * 0.1;
        let mut decay_frames = env.len() - peak_idx;
        for (k, &e) in env.iter().enumerate().skip(peak_idx) {
            if e < threshold {
                decay_frames = k - peak_idx;
                break;
            }
        }
        decay_frames as f32 * frame as f32 / ADPCM_SAMPLE_RATE * 1000.0
    };

    Some(Features {
        dur_ms,
        decay_ms,
        zcr,
        hf_ratio,
        lf_ratio,
    })
}

/// スロット番号と特徴量の組。分類器の入力単位。
/// A (slot, features) pair; the unit of input to the classifier.
#[derive(Debug, Clone, Copy)]
pub struct SampleFeature {
    /// PDX スロット番号 (= MDX 音色番号)
    /// PDX slot number (= MDX voice number)
    pub slot: usize,
    /// 抽出した特徴量
    /// Extracted features
    pub features: Features,
}

/// 使用サンプル集合を 5 種のドラム楽器に**相対的に**割り当てる。
///
/// アルゴリズム (優先順位つき貪欲法):
/// 1. kick: 低域比が最も高い (= スペクトルが最も低い) サンプル。
/// 2. closed hat: 残りのうち高域比が高くかつ減衰が最短のサンプル。
/// 3. open hat: 残りのうち高域比が高くかつ減衰が最長のサンプル。
/// 4. snare: 残りのうちゼロ交差率が中庸〜高め (ノイズ性) のサンプル。
/// 5. clap: 残りのうち snare に次いでノイズ性が高いサンプル。
///
/// 余ったサンプルは割り当てない (None)。サンプル数が 5 未満なら可能な範囲で割当。
///
/// Assigns the used samples to the five drum voices *relative to each other*
/// using a priority greedy method. Leftover samples are unassigned.
///
/// # 引数 / Arguments
/// * `samples` - 使用サンプルの (slot, features) 一覧 / used samples
///
/// # 戻り値 / Returns
/// `slot -> DrumVoice` の割り当て (Vec)。slot 昇順。
/// Assignment of slot to voice (ascending slot order).
pub fn classify(samples: &[SampleFeature]) -> Vec<(usize, DrumVoice)> {
    let mut remaining: Vec<SampleFeature> = samples.to_vec();
    let mut result: Vec<(usize, DrumVoice)> = Vec::new();

    // 1. kick: 低域比が最大。同点はサンプル長が長い方 (kick は伸びる)。
    if let Some(pos) = argmax(&remaining, |f| f.lf_ratio + (f.dur_ms / 10000.0)) {
        result.push((remaining[pos].slot, DrumVoice::Kick));
        remaining.remove(pos);
    }

    // 2. closed hat: 高域比が高く減衰が最短。スコア = hf_ratio - 減衰ペナルティ。
    if let Some(pos) = argmax(&remaining, |f| f.hf_ratio - f.decay_ms / 1000.0) {
        result.push((remaining[pos].slot, DrumVoice::ClosedHat));
        remaining.remove(pos);
    }

    // 3. open hat: 高域比が高く減衰が最長。スコア = hf_ratio + 減衰ボーナス。
    if let Some(pos) = argmax(&remaining, |f| f.hf_ratio + f.decay_ms / 1000.0) {
        result.push((remaining[pos].slot, DrumVoice::OpenHat));
        remaining.remove(pos);
    }

    // 4. snare: ノイズ性 (zcr) が高め。
    if let Some(pos) = argmax(&remaining, |f| f.zcr) {
        result.push((remaining[pos].slot, DrumVoice::Snare));
        remaining.remove(pos);
    }

    // 5. clap: 残りのうち次にノイズ性が高いもの。
    if let Some(pos) = argmax(&remaining, |f| f.zcr) {
        result.push((remaining[pos].slot, DrumVoice::Clap));
        remaining.remove(pos);
    }

    result.sort_by_key(|&(slot, _)| slot);
    result
}

/// `items` の中で `score` が最大の要素の添字を返す。空なら `None`。
/// Returns the index of the element maximizing `score`, or `None` if empty.
fn argmax<F: Fn(&Features) -> f32>(items: &[SampleFeature], score: F) -> Option<usize> {
    items
        .iter()
        .enumerate()
        .max_by(|a, b| {
            score(&a.1.features)
                .partial_cmp(&score(&b.1.features))
                .unwrap()
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成波形ヘルパ: 指定長・周波数・減衰のサイン波 + ノイズ。
    fn synth(len: usize, freq: f32, decay: f32, noise: f32) -> Vec<i32> {
        // 疑似乱数 (LCG) — テスト内決定性のため。
        let mut rng: u32 = 12345;
        let mut next = || {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            ((rng >> 16) & 0x7fff) as f32 / 32768.0 * 2.0 - 1.0
        };
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let t = i as f32 / ADPCM_SAMPLE_RATE;
            let env = (-decay * t).exp();
            let tone = (2.0 * std::f32::consts::PI * freq * t).sin();
            let v = (tone * (1.0 - noise) + next() * noise) * env;
            out.push((v * 2000.0) as i32);
        }
        out
    }

    #[test]
    fn kick_has_high_low_freq_ratio() {
        // 低周波 (60Hz) サイン波 = kick らしい。高域比は低い。
        let kick = extract_features(&synth(4000, 60.0, 8.0, 0.05)).unwrap();
        // 高周波ノイズ = hat らしい。高域比は高い。
        let hat = extract_features(&synth(2000, 0.0, 30.0, 0.9)).unwrap();
        assert!(
            kick.lf_ratio > hat.lf_ratio,
            "kick lf={} hat lf={}",
            kick.lf_ratio,
            hat.lf_ratio
        );
    }

    #[test]
    fn closed_hat_decays_faster_than_open_hat() {
        let ch = extract_features(&synth(800, 0.0, 80.0, 0.9)).unwrap(); // 短い減衰
        let oh = extract_features(&synth(6000, 0.0, 5.0, 0.9)).unwrap(); // 長い減衰
        assert!(
            ch.decay_ms < oh.decay_ms,
            "ch decay={} oh decay={}",
            ch.decay_ms,
            oh.decay_ms
        );
    }

    #[test]
    fn classify_assigns_kick_to_low_freq_sample() {
        let kick_f = extract_features(&synth(4000, 50.0, 8.0, 0.05)).unwrap();
        let hat_f = extract_features(&synth(800, 0.0, 80.0, 0.95)).unwrap();
        let samples = vec![
            SampleFeature {
                slot: 0,
                features: kick_f,
            },
            SampleFeature {
                slot: 1,
                features: hat_f,
            },
        ];
        let assigned = classify(&samples);
        // slot 0 が kick になるはず。
        let kick_slot = assigned
            .iter()
            .find(|&&(_, v)| v == DrumVoice::Kick)
            .map(|&(s, _)| s);
        assert_eq!(kick_slot, Some(0));
    }

    #[test]
    fn classify_does_not_exceed_five_voices() {
        let f = extract_features(&synth(2000, 100.0, 20.0, 0.5)).unwrap();
        let samples: Vec<SampleFeature> = (0..8)
            .map(|slot| SampleFeature { slot, features: f })
            .collect();
        let assigned = classify(&samples);
        assert!(assigned.len() <= 5, "assigned {} voices", assigned.len());
    }

    #[test]
    fn classify_handles_fewer_than_five_samples() {
        let f = extract_features(&synth(2000, 100.0, 20.0, 0.5)).unwrap();
        let samples = vec![SampleFeature {
            slot: 3,
            features: f,
        }];
        let assigned = classify(&samples);
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].0, 3);
    }

    #[test]
    fn label_strings_match_emitter_expectations() {
        assert_eq!(DrumVoice::Kick.label(), "kick");
        assert_eq!(DrumVoice::Snare.label(), "snare");
        assert_eq!(DrumVoice::ClosedHat.label(), "ch");
        assert_eq!(DrumVoice::OpenHat.label(), "oh");
        assert_eq!(DrumVoice::Clap.label(), "cp");
    }
}
