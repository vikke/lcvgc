---
name: rust-error-handling
description: Rustのエラーハンドリング設計を支援するスキル。thiserror/anyhowの使い分け、カスタムエラー型設計、unwrap/expectの撲滅、Result型のイディオマティックな扱い、エラー伝播パターンをカバー。「エラー処理」「エラーハンドリング」「unwrap減らしたい」「thiserror」「anyhow」「Result」「?演算子」「パニック」「エラー型設計」などエラーに関する話題が出たら必ずこのスキルを使うこと。既存コードのリファクタリングでunwrapを見つけたときも参照すること。
---

# Rust エラーハンドリングスキル

Rustらしいエラーハンドリングでパニックを防ぎ、デバッグ可能なエラー情報を提供するためのガイド。

## 大原則

- **ライブラリ → `thiserror`** で構造化されたエラー型を定義
- **アプリケーション → `anyhow`** でエラーを簡潔に伝播
- **`unwrap()` は本番コードでは原則禁止**（テストとプロトタイプのみ）
- **`expect("理由")` は unwrap より良いが、やはり最小限に**

## thiserror によるエラー型設計

```toml
[dependencies]
thiserror = "2"
```

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("不正なノート名: {0}")]
    InvalidNote(String),

    #[error("オクターブ {octave} は範囲外です (0-9)")]
    OctaveOutOfRange { octave: i32 },

    #[error("入力が空です")]
    EmptyInput,

    #[error("MIDI値 {0} は 0-127 の範囲外です")]
    MidiValueOutOfRange(u8),
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("パースエラー: {0}")]
    Parse(#[from] ParseError),

    #[error("MIDI送信失敗: {0}")]
    MidiSend(#[from] MidiError),

    #[error("設定エラー: {0}")]
    Config(String),

    #[error("I/Oエラー: {0}")]
    Io(#[from] std::io::Error),
}
```

### エラー型設計のガイドライン

1. **enum で分岐** — 呼び出し側が `match` でハンドリングできる
2. **`#[from]` で自動変換** — `?` 演算子での伝播が楽になる
3. **エラーメッセージは人間が読める形** — デバッグ時に助かる
4. **内部エラーの情報を保持** — source chain を辿れるように
5. **公開APIのエラーは安定に** — 内部実装の詳細を露出させない

### エラーのネスト構造

大きなプロジェクトではモジュールごとにエラー型を定義し、上位で集約：

```rust
// crate::parser::Error
// crate::midi::Error
// crate::engine::Error (parser::Error, midi::Error を含む)

// トップレベル
pub type Result<T> = std::result::Result<T, EngineError>;
```

## anyhow によるアプリケーションエラー

```toml
[dependencies]
anyhow = "1"
```

```rust
use anyhow::{Context, Result, bail, ensure};

fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .context(format!("{path} の読み込みに失敗"))?;  // context でエラーに情報追加

    let config: Config = toml::from_str(&content)
        .context("設定ファイルのパースに失敗")?;

    ensure!(config.sample_rate > 0, "sample_rate は正の値でなければなりません");

    if config.buffer_size == 0 {
        bail!("buffer_size が 0 です");  // bail! で即座にエラーを返す
    }

    Ok(config)
}

fn main() -> Result<()> {
    let config = load_config("config.toml")?;
    run_engine(config)?;
    Ok(())
}
```

### context と with_context の使い分け

```rust
// context: 文字列が常に生成される（安いメッセージ向け）
file.read_to_string(&mut buf).context("ファイル読み込み失敗")?;

// with_context: エラー時のみ文字列を生成（高コストなフォーマット向け）
file.read_to_string(&mut buf)
    .with_context(|| format!("ファイル {} の読み込みに失敗", path.display()))?;
```

## unwrap / expect の撲滅パターン

### パターン1: Option → `ok_or` / `ok_or_else`

```rust
// Before
let value = map.get("key").unwrap();

// After
let value = map.get("key")
    .ok_or_else(|| anyhow::anyhow!("key が見つかりません"))?;
```

### パターン2: 初期化時の expect → 起動時バリデーション

```rust
// Before
let re = Regex::new(PATTERN).expect("invalid regex");

// After: コンパイル時に検証（可能な場合）
use std::sync::LazyLock;
static RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(PATTERN).expect("PATTERN is a compile-time constant and always valid")
});
// この expect はコンパイル時定数に対するものなので許容される
```

### パターン3: インデックスアクセス → get

```rust
// Before
let item = vec[idx];  // パニックの可能性

// After
let item = vec.get(idx)
    .ok_or(ParseError::IndexOutOfRange(idx))?;
```

### パターン4: 文字列パース → map_err

```rust
// Before
let port: u16 = env::var("PORT").unwrap().parse().unwrap();

// After
let port: u16 = env::var("PORT")
    .context("PORT 環境変数が設定されていません")?
    .parse()
    .context("PORT の値が不正です")?;
```

## Result の合成パターン

### 複数の Result をまとめて処理

```rust
// collect で Vec<Result<T>> → Result<Vec<T>>
let results: Result<Vec<_>, _> = inputs.iter()
    .map(|input| parse_note(input))
    .collect();

// エラーを蓄積して全部返す
let (oks, errs): (Vec<_>, Vec<_>) = inputs.iter()
    .map(|input| parse_note(input))
    .partition(Result::is_ok);

let values: Vec<_> = oks.into_iter().map(Result::unwrap).collect();
let errors: Vec<_> = errs.into_iter().map(Result::unwrap_err).collect();
```

### try ブロック（nightly / 将来安定化予定）

stable では即時実行クロージャで代用：

```rust
let result: Result<(), EngineError> = (|| {
    let config = load_config()?;
    let engine = Engine::new(config)?;
    engine.start()?;
    Ok(())
})();
```

## パニック vs エラーの判断基準

**パニックが適切な場合：**
- プログラムのバグ（配列の範囲外アクセスなど論理エラー）
- 回復不能な状態（メモリ枯渇）
- テストコード内

**Result/Option が適切な場合：**
- ユーザー入力のバリデーション
- ファイルI/O、ネットワーク
- 外部データのパース
- 設定の読み込み

基本方針：**外部からの入力に対してパニックしてはならない**。
