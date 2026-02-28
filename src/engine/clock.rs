use crate::ast::tempo::Tempo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clock {
    ppq: u16,
    bpm: f64,
    time_sig: TimeSignature,
}

impl Clock {
    pub fn new(bpm: f64) -> Self {
        Self {
            ppq: 480,
            bpm,
            time_sig: TimeSignature::default(),
        }
    }

    pub fn with_ppq(bpm: f64, ppq: u16) -> Self {
        Self {
            ppq,
            bpm,
            time_sig: TimeSignature::default(),
        }
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    pub fn ppq(&self) -> u16 {
        self.ppq
    }

    pub fn time_sig(&self) -> &TimeSignature {
        &self.time_sig
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    pub fn set_time_sig(&mut self, ts: TimeSignature) {
        self.time_sig = ts;
    }

    /// 1tickのマイクロ秒 = 60_000_000 / (bpm * ppq)
    pub fn tick_duration_us(&self) -> u64 {
        (60_000_000.0 / (self.bpm * f64::from(self.ppq))) as u64
    }

    /// 1小節のtick数 = ppq * numerator * (4 / denominator)
    pub fn ticks_per_bar(&self) -> u64 {
        u64::from(self.ppq) * u64::from(self.time_sig.numerator) * 4
            / u64::from(self.time_sig.denominator)
    }

    /// 音価+付点 -> tick数
    pub fn duration_to_ticks(&self, duration: u16, dotted: bool) -> u64 {
        let base = u64::from(self.ppq) * 4 / u64::from(duration);
        if dotted {
            base * 3 / 2
        } else {
            base
        }
    }

    /// Tempo AST適用
    pub fn apply_tempo(&mut self, tempo: &Tempo) {
        match tempo {
            Tempo::Absolute(bpm) => self.bpm = f64::from(*bpm),
            Tempo::Relative(delta) => self.bpm += f64::from(*delta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults() {
        let c = Clock::new(120.0);
        assert!((c.bpm() - 120.0).abs() < f64::EPSILON);
        assert_eq!(c.ppq(), 480);
        assert_eq!(*c.time_sig(), TimeSignature::default());
    }

    #[test]
    fn tick_duration_us_bpm120_ppq480() {
        let c = Clock::new(120.0);
        assert_eq!(c.tick_duration_us(), 1041);
    }

    #[test]
    fn ticks_per_bar_4_4() {
        let c = Clock::new(120.0);
        assert_eq!(c.ticks_per_bar(), 1920);
    }

    #[test]
    fn ticks_per_bar_3_4() {
        let mut c = Clock::new(120.0);
        c.set_time_sig(TimeSignature {
            numerator: 3,
            denominator: 4,
        });
        assert_eq!(c.ticks_per_bar(), 1440);
    }

    #[test]
    fn ticks_per_bar_6_8() {
        let mut c = Clock::new(120.0);
        c.set_time_sig(TimeSignature {
            numerator: 6,
            denominator: 8,
        });
        assert_eq!(c.ticks_per_bar(), 1440);
    }

    #[test]
    fn duration_to_ticks_quarter() {
        let c = Clock::new(120.0);
        assert_eq!(c.duration_to_ticks(4, false), 480);
    }

    #[test]
    fn duration_to_ticks_eighth() {
        let c = Clock::new(120.0);
        assert_eq!(c.duration_to_ticks(8, false), 240);
    }

    #[test]
    fn duration_to_ticks_whole() {
        let c = Clock::new(120.0);
        assert_eq!(c.duration_to_ticks(1, false), 1920);
    }

    #[test]
    fn duration_to_ticks_dotted_quarter() {
        let c = Clock::new(120.0);
        assert_eq!(c.duration_to_ticks(4, true), 720);
    }

    #[test]
    fn apply_tempo_absolute() {
        let mut c = Clock::new(120.0);
        c.apply_tempo(&Tempo::Absolute(140));
        assert!((c.bpm() - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apply_tempo_relative() {
        let mut c = Clock::new(120.0);
        c.apply_tempo(&Tempo::Relative(10));
        assert!((c.bpm() - 130.0).abs() < f64::EPSILON);
    }
}
