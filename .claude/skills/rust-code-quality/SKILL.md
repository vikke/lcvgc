---
name: rust-code-quality
description: Rustコードの品質管理を支援するスキル。clippy警告の解釈と修正、rustfmtの設定、cargo audit/cargo denyによるセキュリティ監査、unsafe監査、miriによる未定義動作検出をカバー。「clippy」「lint」「rustfmt」「フォーマット」「コード品質」「unsafe」「監査」「audit」「deny」「miri」「コードレビュー」「Rustのベストプラクティス」「イディオマティック」など品質改善に関する要求があれば必ずこのスキルを使うこと。Rustコードのレビュー依頼時にも参照すること。
---

# Rust コード品質スキル

Rustコードの品質を多層的に担保するためのガイド。lint → format → audit → unsafe監査 の順で適用する。

## Clippy（静的解析）

### 基本実行

```bash
# 標準チェック
cargo clippy

# すべてのターゲットに対して（テスト・ベンチ含む）
cargo clippy --all-targets --all-features

# 警告をエラーとして扱う（CI向け）
cargo clippy -- -D warnings
```

### よく出る警告と対処パターン

**`needless_return`** — 末尾 return を式に変換：
```rust
// Before
fn add(a: i32, b: i32) -> i32 {
    return a + b;
}
// After
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

**`clone_on_copy`** — Copy型に `.clone()` は不要：
```rust
let x: i32 = 42;
let y = x;  // .clone() ではなくコピーで十分
```

**`map_unwrap_or`** — `map().unwrap_or()` を `map_or()` に：
```rust
// Before
opt.map(|x| x * 2).unwrap_or(0)
// After
opt.map_or(0, |x| x * 2)
```

**`single_match`** — 1パターンの match は `if let` に：
```rust
// Before
match opt {
    Some(v) => println!("{v}"),
    _ => {}
}
// After
if let Some(v) = opt {
    println!("{v}");
}
```

### プロジェクト全体の Clippy 設定

`clippy.toml` または `Cargo.toml` で設定：

```toml
# Cargo.toml
[lints.clippy]
pedantic = { level = "warn", priority = -1 }
# pedantic の中で許容するもの
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
```

`#![allow()]` はファイル単位ではなく `Cargo.toml` の `[lints]` で管理する方が一元的で見通しが良い。

## rustfmt（フォーマッタ）

### 基本実行

```bash
cargo fmt
cargo fmt -- --check  # CI用、差分があればエラー
```

### rustfmt.toml カスタマイズ

```toml
# rustfmt.toml
edition = "2024"
max_width = 100
tab_spaces = 4
use_field_init_shorthand = true
use_try_shorthand = true
imports_granularity = "Crate"   # use文をクレート単位でまとめる
group_imports = "StdExternalCrate"  # std, external, crate の順
```

`imports_granularity` と `group_imports` は nightly 限定だが、フォーマットの一貫性に大きく貢献する。stable で使いたい場合は手動で同等のルールを守る。

## セキュリティ監査

### cargo audit

既知の脆弱性を検出：

```bash
cargo install cargo-audit
cargo audit

# 自動修正を試みる（fixable な脆弱性）
cargo audit fix
```

### cargo deny

ライセンス互換性 + 脆弱性 + 重複依存を包括的にチェック：

```bash
cargo install cargo-deny
cargo deny init  # deny.toml 生成
cargo deny check
```

`deny.toml` の重要設定：

```toml
[licenses]
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib"]
confidence-threshold = 0.8

[bans]
multiple-versions = "warn"  # 同一クレートの複数バージョン検出

[advisories]
vulnerability = "deny"
unmaintained = "warn"
```

## unsafe 監査

### unsafe ブロックの洗い出し

```bash
# プロジェクト内の unsafe を検索
grep -rn "unsafe" src/

# cargo-geiger で依存含め unsafe 使用量を可視化
cargo install cargo-geiger
cargo geiger
```

### unsafe を使うときのルール

1. **Safety コメント必須** — なぜ安全かを `// SAFETY:` コメントで説明する
2. **最小スコープ** — unsafe ブロックは必要最小限の範囲に
3. **安全な抽象でラップ** — unsafe な操作は安全な関数/型で包む

```rust
/// 指定オフセットからバイトを読む。
///
/// # Safety
/// - `ptr` は有効なメモリを指していること
/// - `ptr + offset` がアロケーション範囲内であること
unsafe fn read_byte(ptr: *const u8, offset: usize) -> u8 {
    // SAFETY: 呼び出し元が上記の条件を保証する
    *ptr.add(offset)
}

// 安全な抽象
pub fn safe_read(slice: &[u8], offset: usize) -> Option<u8> {
    slice.get(offset).copied()
}
```

### miri（未定義動作検出）

```bash
# miri をインストール（nightly必須）
rustup +nightly component add miri

# テストを miri で実行
cargo +nightly miri test

# 特定のテストだけ
cargo +nightly miri test -- test_name
```

miri が検出するもの：
- メモリリーク
- use-after-free
- データ競合
- 不正なアラインメント
- 未初期化メモリの読み取り

miri は遅いので CI では nightly ジョブとして分離するのが一般的。

## CI パイプライン推奨構成

```yaml
# GitHub Actions の例
jobs:
  check:
    steps:
      - run: cargo fmt -- --check
      - run: cargo clippy --all-targets --all-features -- -D warnings
      - run: cargo test --all-features
      - run: cargo audit
      - run: cargo deny check

  miri:
    # nightly で週次実行
    steps:
      - run: rustup default nightly
      - run: rustup component add miri
      - run: cargo miri test
```

## コードレビュー時のチェックリスト

Rustコードをレビューするときは以下を確認：

- [ ] `unwrap()` / `expect()` が本当に必要な箇所だけで使われているか
- [ ] エラー型が適切に設計されているか（後述の error-handling スキル参照）
- [ ] 所有権の移動が必要以上に clone で回避されていないか
- [ ] ライフタイムが不必要に複雑になっていないか
- [ ] pub な API に doc comment があるか
- [ ] `unsafe` に SAFETY コメントがあるか
- [ ] テストが境界値やエラーケースをカバーしているか
