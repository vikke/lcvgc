use crate::ast::tempo::Tempo;

/// 拍子記号（例: 4/4, 3/4, 6/8）
/// Time signature (e.g. 4/4, 3/4, 6/8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeSignature {
    /// 拍子の分子（1小節あたりの拍数）
    /// Numerator (number of beats per bar)
    pub numerator: u8,
    /// 拍子の分母（1拍の音価）
    /// Denominator (note value of one beat)
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

/// MIDIクロック。テンポ・PPQ・拍子を保持し、ティック計算を提供する
/// MIDI clock. Holds tempo, PPQ, and time signature, and provides tick calculations
#[derive(Debug, Clone)]
pub struct Clock {
    /// 四分音符あたりのティック数 (Pulses Per Quarter note)
    /// Pulses per quarter note (ticks per quarter note)
    ppq: u16,
    /// テンポ（BPM: beats per minute）
    /// Tempo in beats per minute (BPM)
    bpm: f64,
    /// 拍子記号
    /// Time signature
    time_sig: TimeSignature,
}

impl Clock {
    /// デフォルトPPQ(480)で新しいクロックを生成する
    /// Creates a new clock with the default PPQ (480)
    ///
    /// # 引数 / Arguments
    /// * `bpm` - テンポ（BPM） / Tempo in beats per minute
    pub fn new(bpm: f64) -> Self {
        Self {
            ppq: 480,
            bpm,
            time_sig: TimeSignature::default(),
        }
    }

    /// PPQを指定して新しいクロックを生成する
    /// Creates a new clock with a specified PPQ value
    ///
    /// # 引数 / Arguments
    /// * `bpm` - テンポ（BPM） / Tempo in beats per minute
    /// * `ppq` - 四分音符あたりのティック数 / Pulses per quarter note
    pub fn with_ppq(bpm: f64, ppq: u16) -> Self {
        Self {
            ppq,
            bpm,
            time_sig: TimeSignature::default(),
        }
    }

    /// 現在のBPMを返す
    /// Returns the current BPM
    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// 現在のPPQを返す
    /// Returns the current PPQ
    pub fn ppq(&self) -> u16 {
        self.ppq
    }

    /// 現在の拍子記号を返す
    /// Returns the current time signature
    pub fn time_sig(&self) -> &TimeSignature {
        &self.time_sig
    }

    /// BPMを設定する
    /// Sets the BPM
    ///
    /// # 引数 / Arguments
    /// * `bpm` - 新しいテンポ（BPM） / New tempo in beats per minute
    pub fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    /// PPQ（四分音符あたりのティック数）を設定する
    /// Sets the PPQ (ticks per quarter note).
    ///
    /// # 引数 / Arguments
    /// * `ppq` - 新しい PPQ / New pulses per quarter note
    pub fn set_ppq(&mut self, ppq: u16) {
        self.ppq = ppq;
    }

    /// 拍子記号を設定する
    /// Sets the time signature
    ///
    /// # 引数 / Arguments
    /// * `ts` - 新しい拍子記号 / New time signature
    pub fn set_time_sig(&mut self, ts: TimeSignature) {
        self.time_sig = ts;
    }

    /// 1ティックのマイクロ秒 = 60_000_000 / (bpm * ppq)
    /// Microseconds per tick = 60,000,000 / (bpm * ppq)
    pub fn tick_duration_us(&self) -> u64 {
        (60_000_000.0 / (self.bpm * f64::from(self.ppq))) as u64
    }

    /// 1小節のティック数 = ppq * numerator * (4 / denominator)
    /// Ticks per bar = ppq * numerator * (4 / denominator)
    pub fn ticks_per_bar(&self) -> u64 {
        u64::from(self.ppq) * u64::from(self.time_sig.numerator) * 4
            / u64::from(self.time_sig.denominator)
    }

    /// 1拍のティック数 = ppq * (4 / denominator)
    ///
    /// 拍子分母に応じた「1 拍」のティック数を返す。
    /// 4/4 では 4 分音符 = ppq、6/8 では 8 分音符 = ppq/2 となる。
    ///
    /// Ticks per beat = ppq * (4 / denominator). One "beat" depends on the
    /// time signature denominator (quarter for 4/4, eighth for 6/8, etc.).
    pub fn ticks_per_beat(&self) -> u64 {
        u64::from(self.ppq) * 4 / u64::from(self.time_sig.denominator)
    }

    /// 音価と付点からティック数を計算する
    /// Converts a note duration and dotted flag to tick count
    ///
    /// # 引数 / Arguments
    /// * `duration` - 音価（4=四分音符, 8=八分音符 等） / Note duration (4=quarter, 8=eighth, etc.)
    /// * `dotted` - 付点の有無 / Whether the note is dotted
    pub fn duration_to_ticks(&self, duration: u16, dotted: bool) -> u64 {
        let base = u64::from(self.ppq) * 4 / u64::from(duration);
        if dotted {
            base * 3 / 2
        } else {
            base
        }
    }

    /// MIDI Timing Clock (0xF8) を 1 個送出するごとに進む内部 tick 数
    ///
    /// MIDI 1.0 規格では Timing Clock は 24 PPQN (Pulses Per Quarter Note) 固定。
    /// 内部 PPQ を 24 で割った値が「Clock 1 個分の tick 数」になる。
    /// 内部 PPQ が 24 の倍数でない場合は割り切れないため `0` を返し、
    /// その状態では Timing Clock の送出は無効化される (`is_clock_tick` も
    /// 常に false を返す)。実運用 (PPQ=480) では 20 を返す。
    ///
    /// Returns the number of internal ticks between two consecutive MIDI
    /// Timing Clock pulses (24 PPQN). For non-multiple-of-24 PPQ values
    /// the function returns `0`, which disables Clock emission entirely
    /// via `is_clock_tick`.
    pub fn clock_period_ticks(&self) -> u64 {
        if self.ppq.is_multiple_of(24) {
            u64::from(self.ppq) / 24
        } else {
            0
        }
    }

    /// 指定 tick が MIDI Timing Clock (0xF8) を送出すべき境界か
    ///
    /// `clock_period_ticks()` が 0 の場合（PPQ が 24 の倍数でない場合）は
    /// 常に false を返し、Clock 送出を無効化する。
    ///
    /// # 引数 / Arguments
    /// * `tick` - 評価対象の tick 位置 / Tick position to test
    ///
    /// Tells whether the given `tick` is a Timing Clock boundary. Returns
    /// false unconditionally when the PPQ is not a multiple of 24.
    pub fn is_clock_tick(&self, tick: u64) -> bool {
        let period = self.clock_period_ticks();
        period != 0 && tick.is_multiple_of(period)
    }

    /// テンポASTノードを適用してBPMを更新する
    /// Applies a Tempo AST node to update the BPM
    ///
    /// # 引数 / Arguments
    /// * `tempo` - テンポAST（絶対値または相対値） / Tempo AST node (absolute or relative)
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
    fn ticks_per_beat_4_4() {
        let c = Clock::new(120.0);
        // 4/4: 1 拍 = 4 分音符 = ppq = 480
        assert_eq!(c.ticks_per_beat(), 480);
    }

    #[test]
    fn ticks_per_beat_6_8() {
        let mut c = Clock::new(120.0);
        c.set_time_sig(TimeSignature {
            numerator: 6,
            denominator: 8,
        });
        // 6/8: 1 拍 = 8 分音符 = ppq/2 = 240
        assert_eq!(c.ticks_per_beat(), 240);
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

    /// PPQ=480 → MIDI Timing Clock は 24 PPQN なので 1 clock = 20 ticks
    /// PPQ=480 → MIDI Timing Clock at 24 PPQN means 1 clock per 20 ticks
    #[test]
    fn clock_period_ticks_ppq_480() {
        let c = Clock::new(120.0);
        assert_eq!(c.clock_period_ticks(), 20);
    }

    /// PPQ=24 → 1 clock = 1 tick (MIDI 規格の最小解像度)
    /// PPQ=24 → 1 clock per 1 tick (minimum MIDI resolution)
    #[test]
    fn clock_period_ticks_ppq_24() {
        let c = Clock::with_ppq(120.0, 24);
        assert_eq!(c.clock_period_ticks(), 1);
    }

    /// PPQ=96 → 1 clock = 4 ticks
    #[test]
    fn clock_period_ticks_ppq_96() {
        let c = Clock::with_ppq(120.0, 96);
        assert_eq!(c.clock_period_ticks(), 4);
    }

    /// PPQ が 24 の倍数でない場合は 0 を返す (Clock 送出不可)
    /// Non-multiple-of-24 PPQ returns 0 (Clock emission disabled)
    #[test]
    fn clock_period_ticks_ppq_100_is_zero() {
        let c = Clock::with_ppq(120.0, 100);
        assert_eq!(c.clock_period_ticks(), 0);
    }

    /// `is_clock_tick(tick)` は PPQ=480 で `tick % 20 == 0` のとき真
    /// `is_clock_tick(tick)` is true iff `tick % 20 == 0` when PPQ=480
    #[test]
    fn is_clock_tick_ppq_480() {
        let c = Clock::new(120.0);
        assert!(c.is_clock_tick(0));
        assert!(c.is_clock_tick(20));
        assert!(c.is_clock_tick(40));
        assert!(c.is_clock_tick(480)); // 1 拍境界 = 24 個目の clock
        assert!(!c.is_clock_tick(1));
        assert!(!c.is_clock_tick(19));
        assert!(!c.is_clock_tick(21));
    }

    /// PPQ が 24 の倍数でない場合は `is_clock_tick` は常に false
    /// Non-multiple-of-24 PPQ disables `is_clock_tick` entirely
    #[test]
    fn is_clock_tick_disabled_when_ppq_not_multiple_of_24() {
        let c = Clock::with_ppq(120.0, 100);
        assert!(!c.is_clock_tick(0));
        assert!(!c.is_clock_tick(1));
        assert!(!c.is_clock_tick(100));
    }
}
