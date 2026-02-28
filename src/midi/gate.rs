/// ゲート計算結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateResult {
    pub on_duration_ms: u64,
    pub off_duration_ms: u64,
}

/// BPM + 音価 + 付点 -> ノート持続時間(ms)
///
/// 四分音符の長さ = 60000 / bpm (ms)
/// duration=1: 全音符(4拍), 2: 半音符(2拍), 4: 四分(1拍), 8: 八分(0.5拍), 16: 十六分(0.25拍)
/// dotted=true の場合 1.5倍
pub fn note_duration_ms(bpm: f64, duration: u16, dotted: bool) -> u64 {
    let quarter_ms = 60000.0 / bpm;
    let beats = 4.0 / duration as f64;
    let ms = quarter_ms * beats;
    let ms = if dotted { ms * 1.5 } else { ms };
    ms as u64
}

/// ノート持続時間 + ゲート比率 -> Gate On/Off期間
///
/// gate_percent=100 -> レガート（off=0）
/// それ以外 -> on = duration * percent / 100, off = duration - on (最小5ms保証)
pub fn calculate_gate(note_duration_ms: u64, gate_percent: u8) -> GateResult {
    if gate_percent == 100 {
        return GateResult {
            on_duration_ms: note_duration_ms,
            off_duration_ms: 0,
        };
    }

    let on = note_duration_ms * gate_percent as u64 / 100;
    let off = note_duration_ms - on;

    if off < 5 {
        let off = 5;
        let on = if note_duration_ms >= 5 {
            note_duration_ms - 5
        } else {
            0
        };
        GateResult {
            on_duration_ms: on,
            off_duration_ms: off,
        }
    } else {
        GateResult {
            on_duration_ms: on,
            off_duration_ms: off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_duration_quarter_at_120bpm() {
        assert_eq!(note_duration_ms(120.0, 4, false), 500);
    }

    #[test]
    fn note_duration_eighth_at_120bpm() {
        assert_eq!(note_duration_ms(120.0, 8, false), 250);
    }

    #[test]
    fn note_duration_dotted_quarter_at_120bpm() {
        assert_eq!(note_duration_ms(120.0, 4, true), 750);
    }

    #[test]
    fn gate_80_percent() {
        let result = calculate_gate(500, 80);
        assert_eq!(result, GateResult { on_duration_ms: 400, off_duration_ms: 100 });
    }

    #[test]
    fn gate_100_percent_legato() {
        let result = calculate_gate(500, 100);
        assert_eq!(result, GateResult { on_duration_ms: 500, off_duration_ms: 0 });
    }

    #[test]
    fn gate_minimum_off_guarantee() {
        let result = calculate_gate(100, 98);
        assert_eq!(result, GateResult { on_duration_ms: 95, off_duration_ms: 5 });
    }

    #[test]
    fn gate_very_short_duration() {
        let result = calculate_gate(10, 80);
        assert_eq!(result, GateResult { on_duration_ms: 5, off_duration_ms: 5 });
    }
}
