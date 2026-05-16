//! Standard MIDI File → Score IR reader。
//!
//! midly クレートで `.mid` を読み込み、MIDI チャンネル毎に Track を作る。
//! ch 10 は GM ドラム規約に従い `TrackKind::Drum` とする。
//!
//! Implementation notes:
//! - PPQ は header の `Timing::Metrical` から取得 (SMPTE 系は未対応)。
//! - テンポは最初の Tempo meta event を `initial_bpm` に。途中変更はコメント。
//! - 全 track のイベントを一度集約してから、MIDI channel 別に振り分け。
//!
//! Reads a Standard MIDI File via the `midly` crate and produces a `Score`.

use std::collections::HashMap;

use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use crate::generator::score::{Event, Score, TimeSignature, Track, TrackKind};
use crate::generator::{GeneratorError, ScoreReader};

/// SMF reader 実装。
/// SMF reader.
pub struct SmfReader;

impl ScoreReader for SmfReader {
    fn read(&self, bytes: &[u8], source_name: &str) -> Result<Score, GeneratorError> {
        let smf = Smf::parse(bytes).map_err(|e| GeneratorError::Parse {
            format: "smf",
            message: format!("{} ({})", e, source_name),
        })?;

        let ppq = match smf.header.timing {
            Timing::Metrical(t) => u16::from(t) as u32,
            Timing::Timecode(_, _) => {
                return Err(GeneratorError::Parse {
                    format: "smf",
                    message: "SMPTE timecode-based SMF is not supported".into(),
                });
            }
        };

        let mut score = Score {
            ppq,
            ..Score::default()
        };

        // チャンネル番号 (1-16) → Track へのバッファ
        let mut per_channel: HashMap<u8, Vec<Event>> = HashMap::new();
        let mut open_notes: HashMap<(u8, u8), (u64, u8)> = HashMap::new();
        let mut first_tempo_set = false;
        let mut first_time_sig_set = false;

        for track in smf.tracks.iter() {
            let mut abs_tick: u64 = 0;
            for ev in track {
                abs_tick += u32::from(ev.delta) as u64;
                match ev.kind {
                    TrackEventKind::Meta(MetaMessage::Tempo(us_per_quarter))
                        if !first_tempo_set =>
                    {
                        let us = u32::from(us_per_quarter) as f32;
                        score.initial_bpm = 60_000_000.0 / us;
                        first_tempo_set = true;
                    }
                    TrackEventKind::Meta(MetaMessage::TimeSignature(num, den_exp, _, _))
                        if !first_time_sig_set =>
                    {
                        score.time_signature = TimeSignature {
                            numerator: num,
                            denominator: 1u8 << den_exp,
                        };
                        first_time_sig_set = true;
                    }
                    TrackEventKind::Meta(MetaMessage::TrackName(name_bytes))
                        if score.title.is_none() =>
                    {
                        score.title = Some(String::from_utf8_lossy(name_bytes).into_owned());
                    }
                    TrackEventKind::Midi { channel, message } => {
                        let ch1 = u8::from(channel) + 1; // 0-indexed → 1-indexed
                        match message {
                            MidiMessage::NoteOn { key, vel } => {
                                let velocity = u8::from(vel);
                                let key_u8 = u8::from(key);
                                if velocity == 0 {
                                    // velocity 0 の NoteOn は NoteOff と同義
                                    if let Some((start, v)) = open_notes.remove(&(ch1, key_u8)) {
                                        per_channel.entry(ch1).or_default().push(Event::Note {
                                            start_tick: start,
                                            end_tick: abs_tick,
                                            midi_note: key_u8,
                                            velocity: v,
                                        });
                                    }
                                } else {
                                    open_notes.insert((ch1, key_u8), (abs_tick, velocity));
                                }
                            }
                            MidiMessage::NoteOff { key, .. } => {
                                let key_u8 = u8::from(key);
                                if let Some((start, v)) = open_notes.remove(&(ch1, key_u8)) {
                                    per_channel.entry(ch1).or_default().push(Event::Note {
                                        start_tick: start,
                                        end_tick: abs_tick,
                                        midi_note: key_u8,
                                        velocity: v,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        // 開きっぱなしのノートはトラック末尾で閉じる扱い (誤入力対策)
        for ((ch1, key), (start, v)) in open_notes.drain() {
            per_channel.entry(ch1).or_default().push(Event::Note {
                start_tick: start,
                end_tick: start + ppq as u64, // 1 拍仮定
                midi_note: key,
                velocity: v,
            });
        }

        // チャンネル番号順に Track を並べる
        let mut channels: Vec<u8> = per_channel.keys().copied().collect();
        channels.sort_unstable();
        for ch in channels {
            let events = per_channel.remove(&ch).unwrap_or_default();
            let kind = if ch == 10 {
                TrackKind::Drum
            } else {
                TrackKind::Melodic
            };
            let name = if kind == TrackKind::Drum {
                "drums".to_string()
            } else {
                format!("ch_{}", ch)
            };
            score.tracks.push(Track {
                name,
                midi_channel: ch,
                kind,
                events,
            });
        }

        Ok(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小限の SMF を組み立てるヘルパ。
    ///
    /// PPQ=480、1 トラックに ch1 で C4 を 1 拍 → D4 を 1 拍を入れる。
    fn build_minimal_smf() -> Vec<u8> {
        // MThd
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes()); // chunk length
        out.extend_from_slice(&0u16.to_be_bytes()); // format 0
        out.extend_from_slice(&1u16.to_be_bytes()); // 1 track
        out.extend_from_slice(&480u16.to_be_bytes()); // PPQ 480

        // MTrk
        let mut track: Vec<u8> = Vec::new();
        // tempo meta: 500000 us/quarter = 120 BPM
        track.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
        // Time signature 4/4
        track.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
        // NoteOn  C4 (60) vel 100 at delta 0
        track.extend_from_slice(&[0x00, 0x90, 60, 100]);
        // NoteOff C4 at delta 480 (VLQ: 0x83 0x60)
        track.extend_from_slice(&[0x83, 0x60, 0x80, 60, 0]);
        // NoteOn  D4 (62) vel 100 at delta 0
        track.extend_from_slice(&[0x00, 0x90, 62, 100]);
        // NoteOff D4 at delta 480
        track.extend_from_slice(&[0x83, 0x60, 0x80, 62, 0]);
        // End of track
        track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);
        out
    }

    #[test]
    fn parses_ppq_tempo_and_time_signature() {
        let bytes = build_minimal_smf();
        let score = SmfReader.read(&bytes, "test.mid").unwrap();
        assert_eq!(score.ppq, 480);
        assert!((score.initial_bpm - 120.0).abs() < 0.5);
        assert_eq!(score.time_signature.numerator, 4);
        assert_eq!(score.time_signature.denominator, 4);
    }

    #[test]
    fn extracts_notes_with_correct_ticks() {
        let bytes = build_minimal_smf();
        let score = SmfReader.read(&bytes, "test.mid").unwrap();
        assert_eq!(score.tracks.len(), 1);
        let t = &score.tracks[0];
        assert_eq!(t.midi_channel, 1);
        assert_eq!(t.kind, TrackKind::Melodic);
        assert_eq!(t.events.len(), 2);
        match &t.events[0] {
            Event::Note {
                start_tick,
                end_tick,
                midi_note,
                ..
            } => {
                assert_eq!(*start_tick, 0);
                assert_eq!(*end_tick, 480);
                assert_eq!(*midi_note, 60);
            }
            _ => panic!("expected Note"),
        }
        match &t.events[1] {
            Event::Note {
                start_tick,
                end_tick,
                midi_note,
                ..
            } => {
                assert_eq!(*start_tick, 480);
                assert_eq!(*end_tick, 960);
                assert_eq!(*midi_note, 62);
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn note_on_with_velocity_zero_treated_as_note_off() {
        // PPQ=480、ch1 で NoteOn C4 vel 100 → NoteOn C4 vel 0 (=Off) at 240
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&480u16.to_be_bytes());

        let mut track: Vec<u8> = Vec::new();
        track.extend_from_slice(&[0x00, 0x90, 60, 100]);
        // delta 240 (VLQ: 0x81 0x70) → NoteOn C4 vel 0
        track.extend_from_slice(&[0x81, 0x70, 0x90, 60, 0]);
        track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);

        let score = SmfReader.read(&out, "test.mid").unwrap();
        let t = &score.tracks[0];
        assert_eq!(t.events.len(), 1);
        match &t.events[0] {
            Event::Note {
                start_tick,
                end_tick,
                ..
            } => {
                assert_eq!(*start_tick, 0);
                assert_eq!(*end_tick, 240);
            }
            _ => panic!("expected Note"),
        }
    }

    #[test]
    fn ch10_is_classified_as_drum() {
        // ch10 (MIDI ch number = 9 in zero-indexed wire format) で 1 ヒット
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&480u16.to_be_bytes());

        let mut track: Vec<u8> = Vec::new();
        // 0x99 = NoteOn ch 10 (9 zero-indexed)
        track.extend_from_slice(&[0x00, 0x99, 36, 100]);
        // delta 120 → NoteOff
        track.extend_from_slice(&[0x78, 0x89, 36, 0]);
        track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);

        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);

        let score = SmfReader.read(&out, "test.mid").unwrap();
        assert_eq!(score.tracks.len(), 1);
        assert_eq!(score.tracks[0].kind, TrackKind::Drum);
        assert_eq!(score.tracks[0].midi_channel, 10);
    }

    #[test]
    fn rejects_smpte_timecode() {
        // MThd 内で Timing が SMPTE
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b"MThd");
        out.extend_from_slice(&6u32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        // negative SMPTE (top bit set) → SMPTE 系
        out.extend_from_slice(&[0xE7, 0x28]);

        // 空 track
        let track = vec![0x00, 0xFF, 0x2F, 0x00];
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);

        let err = SmfReader.read(&out, "test.mid").unwrap_err();
        match err {
            GeneratorError::Parse { format, message } => {
                assert_eq!(format, "smf");
                assert!(message.contains("SMPTE"));
            }
            _ => panic!("expected Parse error"),
        }
    }
}
