mod pitch;
mod quantize;
mod scale;

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver};
use std::time::Instant;

use pitch::McLeodDetector;
use quantize::{freq_to_midi, midi_to_note_name, quantize_duration};
use scale::ScaleFilter;

#[derive(Parser, Debug)]
#[command(name = "lcvgc-mic", about = "Microphone input to lcvgc DSL note text")]
struct Cli {
    /// Audio input device name
    #[arg(long)]
    device: Option<String>,

    /// Sample rate in Hz
    #[arg(long, default_value_t = 44100)]
    sample_rate: u32,

    /// Audio buffer size in samples
    #[arg(long, default_value_t = 1024)]
    buffer_size: u32,

    /// Quantize grid: "1/4", "1/8", "1/16"
    #[arg(long, default_value = "1/8")]
    grid: String,

    /// Tempo in BPM
    #[arg(long, default_value_t = 120.0)]
    bpm: f64,

    /// Scale filter (e.g. "C major", "A minor")
    #[arg(long)]
    scale: Option<String>,

    /// List available audio input devices
    #[arg(long)]
    list_devices: bool,
}

fn list_input_devices() {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => {
            println!("Available input devices:");
            for (i, device) in devices.enumerate() {
                let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
                println!("  {}: {}", i, name);
            }
        }
        Err(e) => {
            eprintln!("Error listing devices: {}", e);
        }
    }
}

fn find_input_device(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if let Some(name) = name {
        host.input_devices().ok()?.find(|d| {
            d.name()
                .map(|n| n.contains(name))
                .unwrap_or(false)
        })
    } else {
        host.default_input_device()
    }
}

fn start_audio_stream(
    device: &cpal::Device,
    sample_rate: u32,
    buffer_size: u32,
) -> Result<(cpal::Stream, Receiver<Vec<f32>>), Box<dyn std::error::Error>> {
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Fixed(buffer_size),
    };

    let (sender, receiver) = bounded::<Vec<f32>>(16);

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let _ = sender.try_send(data.to_vec());
        },
        |err| {
            eprintln!("Audio stream error: {}", err);
        },
        None,
    )?;

    stream.play()?;
    Ok((stream, receiver))
}

fn main() {
    let cli = Cli::parse();

    if cli.list_devices {
        list_input_devices();
        return;
    }

    let device = match find_input_device(cli.device.as_deref()) {
        Some(d) => d,
        None => {
            eprintln!("No audio input device found. Use --list-devices to see available devices.");
            std::process::exit(1);
        }
    };

    let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
    eprintln!("Using device: {}", device_name);

    let scale_filter = cli.scale.as_ref().and_then(|s| {
        let f = ScaleFilter::from_str(s);
        if f.is_none() {
            eprintln!("Warning: Unknown scale '{}', ignoring", s);
        }
        f
    });

    let (_stream, receiver) =
        match start_audio_stream(&device, cli.sample_rate, cli.buffer_size) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to start audio stream: {}", e);
                std::process::exit(1);
            }
        };

    let detector = McLeodDetector::new(cli.sample_rate as f32, cli.buffer_size as usize);
    let mut last_note: Option<u8> = None;
    let mut note_onset = Instant::now();

    eprintln!("Listening... (Ctrl+C to stop)");

    loop {
        match receiver.recv() {
            Ok(samples) => {
                if let Some(freq) = detector.detect_pitch(&samples) {
                    let mut midi = freq_to_midi(freq);

                    if let Some(ref filter) = scale_filter {
                        midi = filter.snap_to_scale(midi);
                    }

                    match last_note {
                        Some(prev) if prev == midi => {
                            // Same note, continue
                        }
                        _ => {
                            // New note or first note
                            if let Some(prev) = last_note {
                                let duration_ms = note_onset.elapsed().as_millis() as f64;
                                let dur =
                                    quantize_duration(duration_ms, &cli.grid, cli.bpm);
                                let name = midi_to_note_name(prev);
                                print!("{}{} ", name, dur);
                            }
                            last_note = Some(midi);
                            note_onset = Instant::now();
                        }
                    }
                } else if let Some(prev) = last_note.take() {
                    // Silence detected, emit previous note
                    let duration_ms = note_onset.elapsed().as_millis() as f64;
                    let dur = quantize_duration(duration_ms, &cli.grid, cli.bpm);
                    let name = midi_to_note_name(prev);
                    println!("{}{}", name, dur);
                }
            }
            Err(_) => break,
        }
    }
}
