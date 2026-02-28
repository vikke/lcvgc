/// Frequency-to-MIDI and MIDI-to-note-name conversion, plus duration quantization.

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Convert frequency in Hz to the nearest MIDI note number (0-127).
pub fn freq_to_midi(freq: f32) -> u8 {
    let midi_float = 69.0 + 12.0 * (freq / 440.0).log2();
    let midi = midi_float.round() as i32;
    midi.clamp(0, 127) as u8
}

/// Convert MIDI note number to DSL note name (e.g. "C4", "D#5").
#[allow(dead_code)]
pub fn midi_to_note_name(midi: u8) -> String {
    let note_index = (midi % 12) as usize;
    let octave = (midi as i32 / 12) - 1;
    format!("{}{}", NOTE_NAMES[note_index], octave)
}

/// Format a MIDI note as DSL text: "c:4:8" (lowercase name, colon-separated octave and duration).
pub fn format_dsl_note(midi: u8, duration: &str) -> String {
    let note_index = (midi % 12) as usize;
    let octave = (midi as i32 / 12) - 1;
    let name_lower = NOTE_NAMES[note_index].to_lowercase();
    format!("{}:{}:{}", name_lower, octave, duration)
}

/// Quantize a duration in milliseconds to the nearest grid division.
/// Returns DSL duration string: "1" = whole, "2" = half, "4" = quarter, "8" = eighth, "16" = sixteenth.
pub fn quantize_duration(onset_ms: f64, grid: &str, bpm: f64) -> String {
    let beat_ms = 60000.0 / bpm;

    // Parse grid string like "1/4", "1/8", "1/16"
    let grid_division: f64 = parse_grid(grid);
    let grid_ms = beat_ms * 4.0 / grid_division;

    // Snap to nearest grid point
    let grid_units = (onset_ms / grid_ms).round().max(1.0);
    let snapped_ms = grid_units * grid_ms;

    // Convert to note value
    let note_value = (beat_ms * 4.0 / snapped_ms).round() as u32;

    // Clamp to valid values
    match note_value {
        0..=1 => "1".to_string(),
        2 => "2".to_string(),
        3..=4 => "4".to_string(),
        5..=8 => "8".to_string(),
        9..=16 => "16".to_string(),
        _ => "32".to_string(),
    }
}

fn parse_grid(grid: &str) -> f64 {
    if let Some((_num, denom)) = grid.split_once('/') {
        denom.parse::<f64>().unwrap_or(8.0)
    } else {
        grid.parse::<f64>().unwrap_or(8.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freq_to_midi_a4() {
        assert_eq!(freq_to_midi(440.0), 69);
    }

    #[test]
    fn test_freq_to_midi_c4() {
        assert_eq!(freq_to_midi(261.63), 60);
    }

    #[test]
    fn test_freq_to_midi_a5() {
        assert_eq!(freq_to_midi(880.0), 81);
    }

    #[test]
    fn test_midi_to_note_name_c4() {
        assert_eq!(midi_to_note_name(60), "C4");
    }

    #[test]
    fn test_midi_to_note_name_a4() {
        assert_eq!(midi_to_note_name(69), "A4");
    }

    #[test]
    fn test_midi_to_note_name_d_sharp_5() {
        assert_eq!(midi_to_note_name(75), "D#5");
    }

    #[test]
    fn test_midi_to_note_name_c_minus1() {
        assert_eq!(midi_to_note_name(0), "C-1");
    }

    #[test]
    fn test_quantize_quarter_note() {
        // At 120 BPM, a quarter note = 500ms
        let result = quantize_duration(500.0, "1/8", 120.0);
        assert_eq!(result, "4");
    }

    #[test]
    fn test_quantize_eighth_note() {
        // At 120 BPM, an eighth note = 250ms
        let result = quantize_duration(250.0, "1/8", 120.0);
        assert_eq!(result, "8");
    }

    #[test]
    fn test_quantize_half_note() {
        // At 120 BPM, a half note = 1000ms
        let result = quantize_duration(1000.0, "1/4", 120.0);
        assert_eq!(result, "2");
    }

    #[test]
    fn test_format_dsl_note_c4_eighth() {
        assert_eq!(format_dsl_note(60, "8"), "c:4:8");
    }

    #[test]
    fn test_format_dsl_note_a4_quarter() {
        assert_eq!(format_dsl_note(69, "4"), "a:4:4");
    }

    #[test]
    fn test_format_dsl_note_d_sharp_5() {
        assert_eq!(format_dsl_note(75, "16"), "d#:5:16");
    }

    #[test]
    fn test_quantize_whole_note() {
        // At 120 BPM, a whole note = 2000ms
        let result = quantize_duration(2000.0, "1/4", 120.0);
        assert_eq!(result, "1");
    }
}
