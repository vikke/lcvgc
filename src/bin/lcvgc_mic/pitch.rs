/// McLeod pitch detection algorithm using Normalized Square Difference Function (NSDF)
/// with parabolic interpolation for sub-sample accuracy.

pub struct McLeodDetector {
    pub sample_rate: f32,
    #[allow(dead_code)]
    pub buffer_size: usize,
    pub power_threshold: f32,
    pub clarity_threshold: f32,
}

impl McLeodDetector {
    pub fn new(sample_rate: f32, buffer_size: usize) -> Self {
        Self {
            sample_rate,
            buffer_size,
            power_threshold: 0.01,
            clarity_threshold: 0.9,
        }
    }

    /// Detect pitch from audio samples. Returns frequency in Hz if detected.
    pub fn detect_pitch(&self, samples: &[f32]) -> Option<f32> {
        let n = samples.len();
        if n < 2 {
            return None;
        }

        // Check power threshold
        let power: f32 = samples.iter().map(|s| s * s).sum::<f32>() / n as f32;
        if power < self.power_threshold {
            return None;
        }

        // Compute NSDF
        let nsdf = self.normalized_square_difference(samples);

        // Find positive peaks after the first zero crossing
        let peaks = self.pick_peaks(&nsdf);
        if peaks.is_empty() {
            return None;
        }

        // Find the first peak above clarity threshold (standard McLeod approach)
        let best_peak = peaks
            .iter()
            .find(|&&(_, val)| val >= self.clarity_threshold);

        // Fall back to the highest peak if none meets the threshold
        let &(peak_idx, _) = match best_peak {
            Some(p) => p,
            None => peaks
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?,
        };

        // Parabolic interpolation for sub-sample accuracy
        let refined = self.parabolic_interpolation(&nsdf, peak_idx);

        if refined <= 0.0 {
            return None;
        }

        let freq = self.sample_rate / refined;

        // Sanity check: audible range
        if freq < 20.0 || freq > 5000.0 {
            return None;
        }

        Some(freq)
    }

    /// Compute the Normalized Square Difference Function
    fn normalized_square_difference(&self, samples: &[f32]) -> Vec<f32> {
        let n = samples.len();
        let mut nsdf = vec![0.0f32; n];

        for tau in 0..n {
            let mut acf = 0.0f32;
            let mut energy = 0.0f32;

            for j in 0..(n - tau) {
                acf += samples[j] * samples[j + tau];
                energy += samples[j] * samples[j] + samples[j + tau] * samples[j + tau];
            }

            nsdf[tau] = if energy > 0.0 {
                2.0 * acf / energy
            } else {
                0.0
            };
        }

        nsdf
    }

    /// Pick positive peaks from the NSDF after the first zero crossing from positive to negative.
    fn pick_peaks(&self, nsdf: &[f32]) -> Vec<(usize, f32)> {
        let n = nsdf.len();
        let mut peaks = Vec::new();

        // Skip the initial positive region (tau=0 is always 1.0) by finding first negative value
        let start = match (1..n).find(|&i| nsdf[i] < 0.0) {
            Some(i) => i,
            None => return peaks,
        };

        let mut in_positive = false;
        let mut local_max_idx = 0;
        let mut local_max_val = 0.0f32;

        for i in start..n {
            if nsdf[i] > 0.0 {
                if !in_positive {
                    in_positive = true;
                    local_max_idx = i;
                    local_max_val = nsdf[i];
                } else if nsdf[i] > local_max_val {
                    local_max_idx = i;
                    local_max_val = nsdf[i];
                }
            } else if in_positive {
                peaks.push((local_max_idx, local_max_val));
                in_positive = false;
                local_max_val = 0.0;
            }
        }

        if in_positive {
            peaks.push((local_max_idx, local_max_val));
        }

        peaks
    }

    /// Parabolic interpolation around a peak for sub-sample accuracy.
    /// Returns the refined lag in samples.
    fn parabolic_interpolation(&self, nsdf: &[f32], peak_idx: usize) -> f32 {
        if peak_idx == 0 || peak_idx >= nsdf.len() - 1 {
            return peak_idx as f32;
        }

        let alpha = nsdf[peak_idx - 1];
        let beta = nsdf[peak_idx];
        let gamma = nsdf[peak_idx + 1];

        let denominator = 2.0 * (2.0 * beta - alpha - gamma);
        if denominator.abs() < 1e-10 {
            return peak_idx as f32;
        }

        let delta = (alpha - gamma) / denominator;
        peak_idx as f32 + delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn generate_sine(freq: f32, sample_rate: f32, num_samples: usize) -> Vec<f32> {
        (0..num_samples)
            .map(|i| (2.0 * PI * freq * i as f32 / sample_rate).sin())
            .collect()
    }

    #[test]
    fn test_detect_a4_440hz() {
        let detector = McLeodDetector::new(44100.0, 2048);
        let samples = generate_sine(440.0, 44100.0, 2048);
        let freq = detector.detect_pitch(&samples).expect("Should detect A4");
        assert!(
            (freq - 440.0).abs() < 5.0,
            "Expected ~440Hz, got {freq}Hz"
        );
    }

    #[test]
    fn test_detect_c4_261hz() {
        let detector = McLeodDetector::new(44100.0, 2048);
        let samples = generate_sine(261.63, 44100.0, 2048);
        let freq = detector.detect_pitch(&samples).expect("Should detect C4");
        assert!(
            (freq - 261.63).abs() < 5.0,
            "Expected ~261.63Hz, got {freq}Hz"
        );
    }

    #[test]
    fn test_detect_a5_880hz() {
        let detector = McLeodDetector::new(44100.0, 2048);
        let samples = generate_sine(880.0, 44100.0, 2048);
        let freq = detector.detect_pitch(&samples).expect("Should detect A5");
        assert!(
            (freq - 880.0).abs() < 5.0,
            "Expected ~880Hz, got {freq}Hz"
        );
    }

    #[test]
    fn test_silence_returns_none() {
        let detector = McLeodDetector::new(44100.0, 1024);
        let samples = vec![0.0f32; 1024];
        assert!(detector.detect_pitch(&samples).is_none());
    }
}
