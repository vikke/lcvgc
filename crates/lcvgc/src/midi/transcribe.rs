//! MIDI メッセージ → lcvgc DSL トークンへのリアルタイム変換モジュール。
//!
//! MIDI 入力ポートから届いた演奏を、その場で DSL の音程トークン
//! (`c:4`, `c#:4` など) へ変換する。SMF 全体を量子化してまとめる
//! [`crate::generator`] とは異なり、ここでは 1 メッセージ単位の軽量・純粋な
//! 変換のみを行う（タイミング/音長は付与しない）。`subscribe_midi_in` で
//! 購読中のクライアントへ逐次プッシュする文字列の生成に使う。
//!
//! Real-time conversion from MIDI messages to lcvgc DSL pitch tokens
//! (e.g. `c:4`, `c#:4`). Unlike [`crate::generator`], which quantizes an
//! entire SMF, this performs a lightweight, pure, per-message conversion
//! (no timing/duration). Used to build the strings pushed to clients
//! subscribed via `subscribe_midi_in`.

use crate::midi::message::MidiMessage;

/// 12 音の音名（lcvgc DSL の小文字 + `#` 表記）。
/// The twelve pitch-class names in lcvgc DSL notation (lowercase + `#`).
const NOTE_NAMES: [&str; 12] = [
    "c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b",
];

/// MIDI ノート番号 (0-127) を DSL 音程トークン `name:oct` に変換する。
///
/// オクターブは MIDI 60 = C4 の規約に従う（`crate::generator::emitter` と一致）。
/// 省略記法（直前の値の引き継ぎ）には依存せず、常にオクターブを明示するため、
/// clip 本体のどこに挿入しても意味が変わらない自己完結トークンになる。
///
/// Converts a MIDI note number (0-127) into the DSL pitch token `name:oct`
/// (MIDI 60 = C4). The octave is always spelled out so the token is
/// self-contained regardless of insertion position.
///
/// # 引数 / Arguments
/// * `midi` - MIDIノート番号 (0-127) / MIDI note number (0-127)
///
/// # 戻り値 / Returns
/// `name:oct` 形式の DSL トークン / DSL token in `name:oct` form
pub fn note_token(midi: u8) -> String {
    let name = NOTE_NAMES[(midi % 12) as usize];
    let octave = (midi / 12).saturating_sub(1); // MIDI 60 → 4
    format!("{}:{}", name, octave)
}

/// `MidiMessage` を、クライアントへ送る DSL トークン文字列へ変換する。
///
/// 現状の対応は「発音イベント」のみ:
/// - `NoteOn` かつ `velocity > 0` → 音程トークン（`note_token`）
/// - それ以外（`NoteOff`、`velocity == 0` の `NoteOn`、CC、Program、
///   System Real-Time）→ `None`（DSL テキストとしては出力しない）
///
/// velocity 0 の NoteOn は MIDI 慣習上 NoteOff と等価なので発音とはみなさない。
///
/// Converts a `MidiMessage` into a DSL token string for the client. Only
/// note-on events with non-zero velocity produce output; everything else
/// (note-off, zero-velocity note-on, CC, program change, real-time) yields
/// `None`. A zero-velocity note-on is treated as a note-off per MIDI custom.
///
/// # 引数 / Arguments
/// * `msg` - 変換対象の MIDIメッセージ / the MIDI message to convert
///
/// # 戻り値 / Returns
/// 発音イベントなら `Some(DSLトークン)`、それ以外は `None`
/// `Some(token)` for note-on events, `None` otherwise
pub fn message_to_dsl(msg: &MidiMessage) -> Option<String> {
    match msg {
        MidiMessage::NoteOn { note, velocity, .. } if *velocity > 0 => Some(note_token(*note)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::channel::MidiChannel;

    fn ch0() -> MidiChannel {
        MidiChannel::from_zero_based(0).unwrap()
    }

    #[test]
    fn note_token_middle_c() {
        assert_eq!(note_token(60), "c:4");
    }

    #[test]
    fn note_token_a4() {
        assert_eq!(note_token(69), "a:4");
    }

    #[test]
    fn note_token_sharp() {
        assert_eq!(note_token(61), "c#:4");
    }

    #[test]
    fn note_token_low_and_high() {
        assert_eq!(note_token(12), "c:0"); // MIDI 12 = C0
        assert_eq!(note_token(127), "g:9"); // MIDI 127 = G9
    }

    #[test]
    fn message_to_dsl_note_on() {
        let msg = MidiMessage::NoteOn {
            channel: ch0(),
            note: 60,
            velocity: 100,
        };
        assert_eq!(message_to_dsl(&msg).as_deref(), Some("c:4"));
    }

    /// velocity 0 の NoteOn は NoteOff 相当なので出力しない。
    /// A zero-velocity note-on is treated as note-off (no output).
    #[test]
    fn message_to_dsl_note_on_velocity_zero_is_none() {
        let msg = MidiMessage::NoteOn {
            channel: ch0(),
            note: 60,
            velocity: 0,
        };
        assert!(message_to_dsl(&msg).is_none());
    }

    #[test]
    fn message_to_dsl_note_off_is_none() {
        let msg = MidiMessage::NoteOff {
            channel: ch0(),
            note: 60,
            velocity: 64,
        };
        assert!(message_to_dsl(&msg).is_none());
    }

    #[test]
    fn message_to_dsl_cc_is_none() {
        let msg = MidiMessage::ControlChange {
            channel: ch0(),
            cc: 74,
            value: 64,
        };
        assert!(message_to_dsl(&msg).is_none());
    }

    #[test]
    fn message_to_dsl_realtime_is_none() {
        assert!(message_to_dsl(&MidiMessage::Clock).is_none());
        assert!(message_to_dsl(&MidiMessage::Start).is_none());
    }

    /// バイト列 → メッセージ → DSL の一気通貫変換。
    /// End-to-end bytes → message → DSL conversion.
    #[test]
    fn bytes_to_dsl_end_to_end() {
        let msg = MidiMessage::from_bytes(&[0x90, 64, 100]).unwrap();
        assert_eq!(message_to_dsl(&msg).as_deref(), Some("e:4"));
    }
}
