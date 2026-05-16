//! `lcvgc::generator` の統合テスト。
//!
//! 生成された DSL が lcvgc 本体のパーサを通るところまでを確認する。
//! Integration tests for the generator: confirm the emitted DSL round-trips
//! through the main DSL parser.

use std::str::FromStr;

use lcvgc::generator::{generate, InputFormat};
use lcvgc::parser::parse_source;

/// 最小 SMF を組み立てるヘルパ (ch1 で C4, D4 を 1 拍ずつ)。
fn minimal_smf() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&480u16.to_be_bytes());
    let mut track: Vec<u8> = Vec::new();
    track.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);
    track.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08]);
    for n in [60u8, 62u8] {
        track.extend_from_slice(&[0x00, 0x90, n, 100]);
        track.extend_from_slice(&[0x83, 0x60, 0x80, n, 0]);
    }
    track.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

/// 最小 MDX を組み立てるヘルパ。
fn minimal_mdx() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"MDX_SAMPLE");
    out.extend_from_slice(&[0x0d, 0x0a, 0x1a]);
    out.push(0x00); // empty PDX
    out.extend_from_slice(&4u16.to_be_bytes()); // voice off
    out.extend_from_slice(&4u16.to_be_bytes()); // mml off
                                                // C を 4 つ (48 clocks ずつ)
    for _ in 0..4 {
        out.push(0x80);
        out.push(48);
    }
    out.push(0xF1);
    out.push(0x00);
    out
}

#[test]
fn smf_generated_dsl_parses() {
    let bytes = minimal_smf();
    let dsl = generate(InputFormat::Smf, &bytes, "test.mid").expect("generate ok");
    let result = parse_source(&dsl);
    assert!(
        result.is_ok(),
        "generated DSL failed to parse:\n----\n{}\n----\nerror: {:?}",
        dsl,
        result.err()
    );
}

#[test]
fn mdx_generated_dsl_parses() {
    let bytes = minimal_mdx();
    let dsl = generate(InputFormat::Mdx, &bytes, "test.mdx").expect("generate ok");
    let result = parse_source(&dsl);
    assert!(
        result.is_ok(),
        "generated DSL failed to parse:\n----\n{}\n----\nerror: {:?}",
        dsl,
        result.err()
    );
}

#[test]
fn input_format_from_str_recognizes_aliases() {
    assert_eq!(InputFormat::from_str("smf").unwrap(), InputFormat::Smf);
    assert_eq!(InputFormat::from_str("MID").unwrap(), InputFormat::Smf);
    assert_eq!(InputFormat::from_str("midi").unwrap(), InputFormat::Smf);
    assert_eq!(InputFormat::from_str("mdx").unwrap(), InputFormat::Mdx);
    assert!(InputFormat::from_str("xm").is_err());
}
