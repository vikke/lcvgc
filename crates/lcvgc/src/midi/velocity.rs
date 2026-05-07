//! ベロシティ変換モジュール
//! Velocity conversion module

use crate::ast::clip_drum::HitSymbol;

/// ヒットシンボルからMIDIベロシティに変換する
/// Converts a HitSymbol to MIDI velocity
///
/// # 引数 / Arguments
/// * `hit` - ヒットシンボル / Hit symbol
///
/// # 戻り値 / Returns
/// MIDIベロシティ値 (0-127) / MIDI velocity value (0-127)
pub fn hit_velocity(hit: &HitSymbol) -> u8 {
    match hit {
        HitSymbol::Normal => 100,
        HitSymbol::Accent => 127,
        HitSymbol::Ghost => 40,
        HitSymbol::Rest => 0,
    }
}

/// ベロシティ値を0-127の範囲にクランプする
/// Clamps a velocity value to the 0-127 range
///
/// # 引数 / Arguments
/// * `v` - ベロシティ値 / Velocity value
///
/// # 戻り値 / Returns
/// クランプされたベロシティ値 (0-127) / Clamped velocity value (0-127)
pub fn clamp_velocity(v: u8) -> u8 {
    if v > 127 {
        127
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_velocity() {
        assert_eq!(hit_velocity(&HitSymbol::Normal), 100);
    }

    #[test]
    fn accent_velocity() {
        assert_eq!(hit_velocity(&HitSymbol::Accent), 127);
    }

    #[test]
    fn ghost_velocity() {
        assert_eq!(hit_velocity(&HitSymbol::Ghost), 40);
    }

    #[test]
    fn rest_velocity() {
        assert_eq!(hit_velocity(&HitSymbol::Rest), 0);
    }

    #[test]
    fn clamp_zero() {
        assert_eq!(clamp_velocity(0), 0);
    }

    #[test]
    fn clamp_mid() {
        assert_eq!(clamp_velocity(64), 64);
    }

    #[test]
    fn clamp_max_valid() {
        assert_eq!(clamp_velocity(127), 127);
    }

    #[test]
    fn clamp_over_max() {
        assert_eq!(clamp_velocity(128), 127);
        assert_eq!(clamp_velocity(255), 127);
    }
}
