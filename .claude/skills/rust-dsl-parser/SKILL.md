---
name: rust-dsl-parser
description: RustでDSL（ドメイン固有言語）のパーサーを構築するためのスキル。nom、pest、winnowなどのパーサーコンビネータ、手書き再帰下降パーサー、REPL実装、ASTの設計をカバー。「パーサー」「DSL」「nom」「pest」「構文解析」「字句解析」「レキサー」「トークナイザー」「AST」「BNF」「文法定義」「REPL」「ライブコーディング言語」などパーサーや言語設計に関する話題が出たら必ずこのスキルを使うこと。lcvgcのDSL実装時にも積極的に参照すること。
---

# Rust DSL パーサースキル

Rustで独自DSLのパーサーを構築するためのガイド。音楽ライブコーディング言語のような小規模DSLを想定。

## パーサーライブラリの選択

| ライブラリ | 特徴 | 向いている場面 |
|-----------|------|--------------|
| **nom** | コンビネータベース、ゼロコピー、高速 | バイナリ/テキスト両対応、パフォーマンス重視 |
| **pest** | PEG文法ファイルから生成、可読性高 | 文法をBNF的に定義したいとき |
| **winnow** | nom の後継的位置づけ、エラーが良い | nom の改善版が欲しいとき |
| **手書き** | 完全制御、依存なし | シンプルな文法、エラーメッセージを完全制御したいとき |

音楽DSL のような小規模でインタラクティブな言語には **nom** または **手書き** を推奨。

## nom によるパーサー実装

```toml
[dependencies]
nom = "8"
```

### 基本パターン

```rust
use nom::{
    IResult,
    bytes::complete::{tag, take_while1},
    character::complete::{alpha1, digit1, multispace0, char},
    combinator::{map, map_res, opt, value},
    sequence::{delimited, preceded, tuple},
    branch::alt,
    multi::{many0, separated_list0},
};

// ノート名パース: C, D, E, F, G, A, B + オプショナルな # / b
fn note_name(input: &str) -> IResult<&str, NoteName> {
    let (input, name) = alt((
        value(NoteName::C, tag("C")),
        value(NoteName::D, tag("D")),
        value(NoteName::E, tag("E")),
        value(NoteName::F, tag("F")),
        value(NoteName::G, tag("G")),
        value(NoteName::A, tag("A")),
        value(NoteName::B, tag("B")),
    ))(input)?;

    let (input, accidental) = opt(alt((
        value(Accidental::Sharp, tag("#")),
        value(Accidental::Flat, tag("b")),
    )))(input)?;

    Ok((input, NoteName { name, accidental }))
}

// オクターブ: 数字
fn octave(input: &str) -> IResult<&str, u8> {
    map_res(digit1, |s: &str| s.parse::<u8>())(input)
}

// ノート: C#4, Bb3, etc.
fn note(input: &str) -> IResult<&str, Note> {
    let (input, (name, oct)) = tuple((note_name, octave))(input)?;
    Ok((input, Note { name, octave: oct }))
}

// コマンド列: [C4 D4 E4 F4]
fn note_sequence(input: &str) -> IResult<&str, Vec<Note>> {
    delimited(
        char('['),
        separated_list0(multispace0, note),
        char(']'),
    )(input)
}
```

### より複雑な文法

```rust
// DSL例:
// tempo 120
// ch 1: [C4 D4 E4] * 4
// ch 2: [G3 _ G3 _] * 8  (_ は休符)

#[derive(Debug, Clone)]
enum Expr {
    Note(Note),
    Rest,
    Sequence(Vec<Expr>),
    Repeat(Box<Expr>, u32),
    Channel(u8, Box<Expr>),
    Tempo(u32),
    Parallel(Vec<Expr>),
}

fn rest(input: &str) -> IResult<&str, Expr> {
    value(Expr::Rest, tag("_"))(input)
}

fn atom(input: &str) -> IResult<&str, Expr> {
    alt((
        map(note, Expr::Note),
        rest,
        map(note_sequence_expr, |exprs| Expr::Sequence(exprs)),
    ))(input)
}

// 繰り返し: expr * count
fn repeat_expr(input: &str) -> IResult<&str, Expr> {
    let (input, expr) = atom(input)?;
    let (input, count) = opt(preceded(
        delimited(multispace0, char('*'), multispace0),
        map_res(digit1, |s: &str| s.parse::<u32>()),
    ))(input)?;

    match count {
        Some(n) => Ok((input, Expr::Repeat(Box::new(expr), n))),
        None => Ok((input, expr)),
    }
}

// チャンネル指定: ch N: expr
fn channel_expr(input: &str) -> IResult<&str, Expr> {
    let (input, _) = tag("ch")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, ch) = map_res(digit1, |s: &str| s.parse::<u8>())(input)?;
    let (input, _) = delimited(multispace0, char(':'), multispace0)(input)?;
    let (input, expr) = repeat_expr(input)?;
    Ok((input, Expr::Channel(ch, Box::new(expr))))
}
```

## pest による文法定義

```toml
[dependencies]
pest = "2"
pest_derive = "2"
```

```pest
// grammar.pest
WHITESPACE = _{ " " | "\t" }

program = { SOI ~ statement* ~ EOI }

statement = {
    tempo_stmt
    | channel_stmt
}

tempo_stmt = { "tempo" ~ number }

channel_stmt = { "ch" ~ number ~ ":" ~ expr }

expr = { atom ~ ("*" ~ number)? }

atom = {
    sequence
    | note
    | rest
}

sequence = { "[" ~ (note | rest)+ ~ "]" }

note = { note_name ~ accidental? ~ octave }
note_name = { "C" | "D" | "E" | "F" | "G" | "A" | "B" }
accidental = { "#" | "b" }
octave = { ASCII_DIGIT }
rest = { "_" }

number = @{ ASCII_DIGIT+ }
```

```rust
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct DslParser;

fn parse_program(input: &str) -> anyhow::Result<Vec<Expr>> {
    let pairs = DslParser::parse(Rule::program, input)?;
    let mut exprs = Vec::new();

    for pair in pairs {
        match pair.as_rule() {
            Rule::statement => {
                let inner = pair.into_inner().next().unwrap();
                exprs.push(parse_statement(inner)?);
            }
            Rule::EOI => {}
            _ => {}
        }
    }
    Ok(exprs)
}
```

## AST 設計のベストプラクティス

### Span 情報を保持する

エラーメッセージのために、各ノードがソース位置を持つようにする：

```rust
#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

type SpannedExpr = Spanned<Expr>;

// エラーメッセージで活用
fn report_error(source: &str, span: &Span, message: &str) {
    let line_start = source[..span.start].rfind('\n').map_or(0, |p| p + 1);
    let line_num = source[..span.start].matches('\n').count() + 1;
    let col = span.start - line_start;
    let line = source[line_start..].lines().next().unwrap_or("");

    eprintln!("Error at line {line_num}, column {col}: {message}");
    eprintln!("  {line}");
    eprintln!("  {}^", " ".repeat(col));
}
```

### Visitor パターン

ASTの走査を柔軟にする：

```rust
pub trait Visitor {
    fn visit_note(&mut self, note: &Note);
    fn visit_rest(&mut self);
    fn visit_sequence(&mut self, exprs: &[Expr]);
    fn visit_repeat(&mut self, expr: &Expr, count: u32);
    fn visit_channel(&mut self, ch: u8, expr: &Expr);
    fn visit_tempo(&mut self, bpm: u32);
}

impl Expr {
    pub fn accept(&self, visitor: &mut dyn Visitor) {
        match self {
            Expr::Note(n) => visitor.visit_note(n),
            Expr::Rest => visitor.visit_rest(),
            Expr::Sequence(exprs) => visitor.visit_sequence(exprs),
            Expr::Repeat(expr, count) => visitor.visit_repeat(expr, *count),
            Expr::Channel(ch, expr) => visitor.visit_channel(*ch, expr),
            Expr::Tempo(bpm) => visitor.visit_tempo(*bpm),
            _ => {}
        }
    }
}

// MIDI 変換ビジター
struct MidiCompiler {
    events: Vec<MidiEvent>,
    current_tick: u64,
    ticks_per_beat: u64,
}

impl Visitor for MidiCompiler {
    fn visit_note(&mut self, note: &Note) {
        let midi_num = note.to_midi_number();
        self.events.push(MidiEvent::NoteOn {
            tick: self.current_tick,
            note: midi_num,
            velocity: 100,
        });
        self.current_tick += self.ticks_per_beat;
        self.events.push(MidiEvent::NoteOff {
            tick: self.current_tick,
            note: midi_num,
        });
    }
    // ...
}
```

## REPL 実装

ライブコーディング向けの対話的評価ループ：

```rust
use std::io::{self, Write, BufRead};

pub struct Repl {
    engine: Engine,
    history: Vec<String>,
}

impl Repl {
    pub fn run(&mut self) -> anyhow::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        loop {
            print!("> ");
            stdout.flush()?;

            let mut line = String::new();
            if stdin.lock().read_line(&mut line)? == 0 {
                break;  // EOF
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match line {
                "quit" | "exit" => break,
                "history" => {
                    for (i, h) in self.history.iter().enumerate() {
                        println!("{i}: {h}");
                    }
                }
                _ => {
                    self.history.push(line.to_string());
                    match self.eval(line) {
                        Ok(result) => println!("{result}"),
                        Err(e) => eprintln!("Error: {e}"),
                    }
                }
            }
        }
        Ok(())
    }

    fn eval(&mut self, input: &str) -> anyhow::Result<String> {
        let ast = parse_program(input)?;
        let result = self.engine.execute(&ast)?;
        Ok(format!("{result:?}"))
    }
}
```

## パーサーのテスト戦略

パーサーはテストが特に重要。以下のカテゴリを網羅する：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // 1. 正常系: 各構文要素
    #[test]
    fn parse_simple_note() {
        let (rest, n) = note("C4").unwrap();
        assert_eq!(rest, "");
        assert_eq!(n.to_midi_number(), 60);
    }

    // 2. エッジケース
    #[test]
    fn parse_note_with_sharp() {
        let (_, n) = note("C#4").unwrap();
        assert_eq!(n.to_midi_number(), 61);
    }

    // 3. エラーケース
    #[test]
    fn parse_invalid_note_fails() {
        assert!(note("Z4").is_err());
    }

    // 4. 空入力
    #[test]
    fn parse_empty_fails() {
        assert!(note("").is_err());
    }

    // 5. 複合的な式
    #[test]
    fn parse_full_program() {
        let input = "tempo 120\nch 1: [C4 D4 E4] * 4";
        let result = parse_program(input);
        assert!(result.is_ok());
    }

    // 6. proptest でファジング
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_never_panics(s in "\\PC{0,200}") {
            let _ = parse_program(&s);  // パニックしなければOK
        }
    }
}
```
