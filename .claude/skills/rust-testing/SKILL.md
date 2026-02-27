---
name: rust-testing
description: Rustのテスト作成、実行、ベンチマークを支援するスキル。ユニットテスト、統合テスト、プロパティベーステスト（proptest/quickcheck）、criterionベンチマーク、テストフィクスチャ、モック、カバレッジ計測をカバー。「テスト書いて」「テスト追加」「ベンチマーク」「proptest」「criterion」「カバレッジ」「テスト設計」「TDD」など、テストやパフォーマンス計測に関する作業が発生したら必ずこのスキルを使うこと。関数やモジュールを新規作成した際のテスト追加提案にも使うこと。
---

# Rust テスト・ベンチマークスキル

Rustのテストを効果的に書き、品質とパフォーマンスを担保するためのガイド。

## ユニットテストの基本

### モジュール内テスト

```rust
// src/parser.rs
pub fn parse_note(input: &str) -> Option<(u8, u8)> {
    // ... パース処理
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_note() {
        assert_eq!(parse_note("C4"), Some((60, 64)));
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert_eq!(parse_note(""), None);
        assert_eq!(parse_note("Z9"), None);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn parse_overflow_panics() {
        parse_note_unchecked("C999");
    }
}
```

### テスト設計の原則

- **1テスト1アサーション** を基本とする（複数 assert がある場合、最初の失敗で後続が見えない）
- **テスト名は振る舞いを記述** — `test_1` ではなく `parse_valid_note_returns_midi_number`
- **Arrange-Act-Assert パターン** を意識する
- **境界値テスト** — 0、最大値、空文字列、Unicode文字列を忘れずに

### テストヘルパーとフィクスチャ

共通のセットアップは関数に切り出す：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn setup_engine() -> Engine {
        Engine::new(Config {
            sample_rate: 44100,
            buffer_size: 256,
            ..Config::default()
        })
    }

    #[test]
    fn engine_processes_midi() {
        let mut engine = setup_engine();
        engine.send_note_on(60, 127);
        assert!(engine.active_voices() > 0);
    }
}
```

### テスト用のトレイトとモック

外部依存をトレイトで抽象化し、テスト時にモックに差し替える：

```rust
pub trait MidiOutput: Send {
    fn send(&mut self, msg: &[u8]) -> Result<(), MidiError>;
}

// 本番実装
pub struct RealMidiOutput { /* ... */ }
impl MidiOutput for RealMidiOutput { /* ... */ }

// テスト用モック
#[cfg(test)]
pub struct MockMidiOutput {
    pub sent: Vec<Vec<u8>>,
}

#[cfg(test)]
impl MidiOutput for MockMidiOutput {
    fn send(&mut self, msg: &[u8]) -> Result<(), MidiError> {
        self.sent.push(msg.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequencer_sends_note_on() {
        let mut output = MockMidiOutput { sent: vec![] };
        let mut seq = Sequencer::new(&mut output);
        seq.trigger(60, 127);
        assert_eq!(output.sent.len(), 1);
        assert_eq!(output.sent[0], vec![0x90, 60, 127]);
    }
}
```

大規模なモックが必要な場合は `mockall` クレートを検討：

```toml
[dev-dependencies]
mockall = "0.13"
```

## 統合テスト

`tests/` ディレクトリに配置。各ファイルが独立したクレートとしてコンパイルされる：

```
my-project/
├── src/
│   └── lib.rs
├── tests/
│   ├── integration_basic.rs
│   ├── integration_midi.rs
│   └── common/
│       └── mod.rs          # テスト間で共有するヘルパー
```

```rust
// tests/common/mod.rs
pub fn create_test_config() -> my_project::Config {
    my_project::Config::default()
}

// tests/integration_basic.rs
mod common;

#[test]
fn full_pipeline_works() {
    let config = common::create_test_config();
    let result = my_project::run(config);
    assert!(result.is_ok());
}
```

## プロパティベーステスト

特定の入力ではなく「性質」を検証する。想定外の入力でバグを発見しやすい。

### proptest

```toml
[dev-dependencies]
proptest = "1.5"
```

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn parse_never_panics(s in "\\PC{0,100}") {
        // どんな文字列でもパニックしない
        let _ = parse_note(&s);
    }

    #[test]
    fn roundtrip_encode_decode(note in 0u8..=127, velocity in 0u8..=127) {
        let encoded = encode_note_on(note, velocity);
        let (decoded_note, decoded_vel) = decode_note_on(&encoded).unwrap();
        prop_assert_eq!(note, decoded_note);
        prop_assert_eq!(velocity, decoded_vel);
    }

    #[test]
    fn velocity_clamped(v in 0u32..1000) {
        let clamped = clamp_velocity(v);
        prop_assert!(clamped <= 127);
    }
}
```

proptestの戦略（Strategy）カスタマイズ：

```rust
use proptest::prelude::*;

fn valid_note_name() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["C", "D", "E", "F", "G", "A", "B"])
        .prop_flat_map(|name| {
            (Just(name.to_string()), 0u8..=9)
                .prop_map(|(n, oct)| format!("{n}{oct}"))
        })
}

proptest! {
    #[test]
    fn parse_valid_notes_always_succeeds(note in valid_note_name()) {
        prop_assert!(parse_note(&note).is_some());
    }
}
```

## ベンチマーク（criterion）

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "parser_bench"
harness = false
```

```rust
// benches/parser_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use my_project::parse_note;

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse C4", |b| {
        b.iter(|| parse_note(black_box("C4")))
    });
}

fn bench_parse_various(c: &mut Criterion) {
    let inputs = vec!["C4", "F#3", "Bb7", "invalid"];
    let mut group = c.benchmark_group("parse_notes");

    for input in &inputs {
        group.bench_with_input(
            BenchmarkId::from_parameter(input),
            input,
            |b, &input| b.iter(|| parse_note(black_box(input))),
        );
    }
    group.finish();
}

fn bench_throughput(c: &mut Criterion) {
    let data: Vec<String> = (0..1000).map(|i| format!("C{}", i % 10)).collect();
    let mut group = c.benchmark_group("throughput");
    group.throughput(criterion::Throughput::Elements(data.len() as u64));
    group.bench_function("batch_parse", |b| {
        b.iter(|| {
            for note in &data {
                black_box(parse_note(note));
            }
        })
    });
    group.finish();
}

criterion_group!(benches, bench_parse, bench_parse_various, bench_throughput);
criterion_main!(benches);
```

実行：

```bash
cargo bench
# HTMLレポートが target/criterion/ に生成される

# 特定のベンチマークだけ
cargo bench -- parse_notes

# ベースラインと比較
cargo bench -- --save-baseline before_optimization
# ... 最適化 ...
cargo bench -- --baseline before_optimization
```

## カバレッジ計測

### cargo-llvm-cov（推奨）

```bash
cargo install cargo-llvm-cov

# カバレッジ付きテスト実行
cargo llvm-cov

# HTML レポート生成
cargo llvm-cov --html
# target/llvm-cov/html/index.html で確認

# lcov形式（CI連携用）
cargo llvm-cov --lcov --output-path lcov.info
```

### カバレッジの見方

- 行カバレッジ80%以上を目標にする（100%は費用対効果が悪い）
- エラーパスとエッジケースのカバレッジを重視
- `#[cfg(not(tarpaulin_include))]` でカバレッジから除外可能（テスト不要なコード用）

## テスト実行のTips

```bash
# 全テスト実行
cargo test

# 特定のテストだけ
cargo test parse_valid

# 特定のモジュールだけ
cargo test --lib parser::tests

# 統合テストだけ
cargo test --test integration_basic

# テスト出力を表示（println! を見たいとき）
cargo test -- --nocapture

# 並列数を制限（リソース競合するテスト向け）
cargo test -- --test-threads=1

# doc test だけ
cargo test --doc
```
