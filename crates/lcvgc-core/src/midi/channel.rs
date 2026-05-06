//! MIDIチャンネル型モジュール
//! MIDI channel type module
//!
//! DSL では 1-16 (1-based)、MIDI ワイヤフォーマットでは 0-15 (0-based) と
//! 表現が異なるため、newtype で内部表現を 0-based に固定し、境界での
//! 変換を型で強制する。
//!
//! Because DSL uses 1-16 (1-based) while the MIDI wire format uses 0-15
//! (0-based), this newtype fixes the internal representation to 0-based
//! and enforces conversion at boundaries via the type system.

use thiserror::Error;

/// MIDIチャンネル: 内部は 0-based (0-15) で保持する
/// MIDI channel: internally stored as 0-based (0-15)
///
/// 構築は `from_one_based` (DSL 入力 1-16) または `from_zero_based`
/// (MIDI ワイヤ入力 0-15) 経由でのみ可能。範囲外は `MidiChannelError`
/// として弾く。
///
/// Construction is only allowed via `from_one_based` (DSL input, 1-16) or
/// `from_zero_based` (MIDI wire input, 0-15). Out-of-range values are
/// rejected with `MidiChannelError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MidiChannel(u8);

/// `MidiChannel` 構築時のエラー
/// Error returned when constructing a `MidiChannel`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MidiChannelError {
    /// 1-based の値が範囲 1..=16 外
    /// One-based value is out of range 1..=16
    #[error("MIDIチャンネル(1-based)は1-16の範囲である必要があります: 受け取った値={value}")]
    OneBasedOutOfRange {
        /// 受け取った値 / Received value
        value: u8,
    },
    /// 0-based の値が範囲 0..=15 外
    /// Zero-based value is out of range 0..=15
    #[error("MIDIチャンネル(0-based)は0-15の範囲である必要があります: 受け取った値={value}")]
    ZeroBasedOutOfRange {
        /// 受け取った値 / Received value
        value: u8,
    },
}

impl MidiChannel {
    /// 1-based 値 (DSL 表記、1-16) から `MidiChannel` を構築する
    /// Construct a `MidiChannel` from a 1-based value (DSL notation, 1-16)
    ///
    /// # 引数 / Arguments
    /// * `value` - 1-based のチャンネル番号 (1-16) / 1-based channel number (1-16)
    ///
    /// # 戻り値 / Returns
    /// 範囲内なら `Ok(MidiChannel)`、範囲外なら
    /// `Err(MidiChannelError::OneBasedOutOfRange)`
    /// `Ok(MidiChannel)` if in range, otherwise
    /// `Err(MidiChannelError::OneBasedOutOfRange)`
    pub fn from_one_based(value: u8) -> Result<Self, MidiChannelError> {
        if (1..=16).contains(&value) {
            Ok(Self(value - 1))
        } else {
            Err(MidiChannelError::OneBasedOutOfRange { value })
        }
    }

    /// 0-based 値 (MIDI ワイヤ表記、0-15) から `MidiChannel` を構築する
    /// Construct a `MidiChannel` from a 0-based value (MIDI wire notation, 0-15)
    ///
    /// # 引数 / Arguments
    /// * `value` - 0-based のチャンネル番号 (0-15) / 0-based channel number (0-15)
    ///
    /// # 戻り値 / Returns
    /// 範囲内なら `Ok(MidiChannel)`、範囲外なら
    /// `Err(MidiChannelError::ZeroBasedOutOfRange)`
    /// `Ok(MidiChannel)` if in range, otherwise
    /// `Err(MidiChannelError::ZeroBasedOutOfRange)`
    pub fn from_zero_based(value: u8) -> Result<Self, MidiChannelError> {
        if value <= 15 {
            Ok(Self(value))
        } else {
            Err(MidiChannelError::ZeroBasedOutOfRange { value })
        }
    }

    /// 0-based 表現 (0-15) で値を取り出す。MIDI ワイヤ送信用。
    /// Return the value in 0-based representation (0-15). For MIDI wire transmission.
    pub fn as_zero_based(self) -> u8 {
        self.0
    }

    /// 1-based 表現 (1-16) で値を取り出す。ユーザ向け表示・診断用。
    /// Return the value in 1-based representation (1-16). For user-facing display and diagnostics.
    pub fn as_one_based(self) -> u8 {
        self.0 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_one_based_min() {
        let ch = MidiChannel::from_one_based(1).unwrap();
        assert_eq!(ch.as_zero_based(), 0);
        assert_eq!(ch.as_one_based(), 1);
    }

    #[test]
    fn from_one_based_max() {
        let ch = MidiChannel::from_one_based(16).unwrap();
        assert_eq!(ch.as_zero_based(), 15);
        assert_eq!(ch.as_one_based(), 16);
    }

    #[test]
    fn from_one_based_drum_channel_10() {
        // 伝統的な GM ドラムチャンネル (DAW 表示の 10) は 0-based で 9 になるべき
        // The traditional GM drum channel (channel 10 in DAW display) should be 9 in 0-based
        let ch = MidiChannel::from_one_based(10).unwrap();
        assert_eq!(ch.as_zero_based(), 9);
    }

    #[test]
    fn from_one_based_zero_is_rejected() {
        let err = MidiChannel::from_one_based(0).unwrap_err();
        assert_eq!(err, MidiChannelError::OneBasedOutOfRange { value: 0 });
    }

    #[test]
    fn from_one_based_seventeen_is_rejected() {
        let err = MidiChannel::from_one_based(17).unwrap_err();
        assert_eq!(err, MidiChannelError::OneBasedOutOfRange { value: 17 });
    }

    #[test]
    fn from_zero_based_min() {
        let ch = MidiChannel::from_zero_based(0).unwrap();
        assert_eq!(ch.as_zero_based(), 0);
        assert_eq!(ch.as_one_based(), 1);
    }

    #[test]
    fn from_zero_based_max() {
        let ch = MidiChannel::from_zero_based(15).unwrap();
        assert_eq!(ch.as_zero_based(), 15);
        assert_eq!(ch.as_one_based(), 16);
    }

    #[test]
    fn from_zero_based_sixteen_is_rejected() {
        let err = MidiChannel::from_zero_based(16).unwrap_err();
        assert_eq!(err, MidiChannelError::ZeroBasedOutOfRange { value: 16 });
    }

    #[test]
    fn round_trip_one_based() {
        for n in 1u8..=16 {
            let ch = MidiChannel::from_one_based(n).unwrap();
            assert_eq!(ch.as_one_based(), n);
        }
    }

    #[test]
    fn round_trip_zero_based() {
        for n in 0u8..=15 {
            let ch = MidiChannel::from_zero_based(n).unwrap();
            assert_eq!(ch.as_zero_based(), n);
        }
    }

    #[test]
    fn equality_same_value_from_different_constructors() {
        let a = MidiChannel::from_one_based(1).unwrap();
        let b = MidiChannel::from_zero_based(0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_consistent_with_zero_based_value() {
        let ch1 = MidiChannel::from_one_based(1).unwrap();
        let ch2 = MidiChannel::from_one_based(2).unwrap();
        let ch16 = MidiChannel::from_one_based(16).unwrap();
        assert!(ch1 < ch2);
        assert!(ch2 < ch16);
    }
}
