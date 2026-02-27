---
name: rust-cargo-project
description: Rustプロジェクトのセットアップ、Cargo.toml管理、ワークスペース構成、依存関係管理、feature flags設定、クロスコンパイル設定を支援するスキル。「Rustプロジェクト作成」「Cargo.toml編集」「workspace構成」「クレート追加」「feature flag」「クロスコンパイル」「ビルドターゲット」「cargo new」「依存関係」など、Rustプロジェクトの構造やビルド設定に関する質問・作業が発生したら必ずこのスキルを使うこと。Rustコードを新規に書き始めるときも必ず参照すること。
---

# Rust Cargo プロジェクト管理スキル

Rustプロジェクトの構造設計からビルド設定まで、cargo周りの作業を高品質に行うためのガイド。

## プロジェクト作成の基本方針

新規プロジェクトでは以下を確認してから作業に入る：

1. **バイナリ vs ライブラリ** — `cargo new` か `cargo new --lib` か
2. **edition** — 特別な理由がなければ `edition = "2021"` 以上を使う
3. **MSRV（Minimum Supported Rust Version）** — ライブラリなら `rust-version` フィールドを設定
4. **ライセンス** — `license` フィールドを忘れずに

```toml
[package]
name = "my-project"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
```

## Cargo.toml ベストプラクティス

### 依存関係の追加

`cargo add` コマンドを優先する。手動編集するときはバージョン指定を明示的に：

```toml
# 良い例: セマンティックバージョニングを意識
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }

# dev-dependencies と build-dependencies を分離
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[build-dependencies]
cc = "1.0"
```

### Feature Flags 設計

ライブラリクレートでは feature flags を活用して依存を最小化する：

```toml
[features]
default = ["std"]
std = []
serde = ["dep:serde"]
async = ["dep:tokio"]

# optional dependencies
[dependencies]
serde = { version = "1.0", optional = true, features = ["derive"] }
tokio = { version = "1", optional = true, features = ["rt-multi-thread"] }
```

原則：
- `default` は最小限にする（ユーザが `default-features = false` で外せるように）
- optional な依存は `dep:` 構文で明示
- feature 名はクレート名と一致させると直感的

### プロファイル設定

リリースビルドとデバッグの最適化：

```toml
[profile.release]
lto = true
codegen-units = 1
strip = true

[profile.dev]
opt-level = 0

[profile.dev.package."*"]
opt-level = 2  # 依存クレートはdevでも最適化
```

## Workspace 構成

複数クレートがある場合はworkspaceにする：

```
my-project/
├── Cargo.toml          # workspace root
├── crates/
│   ├── core/           # コアロジック（lib）
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── cli/            # CLIバイナリ
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── plugin-api/     # プラグインAPI（lib）
│       ├── Cargo.toml
│       └── src/lib.rs
└── examples/
```

workspace root の Cargo.toml：

```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.package]
edition = "2024"
license = "MIT OR Apache-2.0"
version = "0.1.0"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
# workspace内クレートの相互参照
my-core = { path = "crates/core" }
```

子クレートからの参照：

```toml
[dependencies]
serde.workspace = true
my-core.workspace = true
```

## クロスコンパイル

### ターゲット追加

```bash
# ターゲット一覧確認
rustup target list

# よく使うターゲット追加
rustup target add x86_64-unknown-linux-musl     # musl静的リンク
rustup target add x86_64-pc-windows-gnu         # Windows (GNU)
rustup target add aarch64-unknown-linux-gnu     # ARM64 Linux
rustup target add wasm32-unknown-unknown        # WebAssembly
```

### .cargo/config.toml でターゲット別設定

```toml
# WSL2からWindowsバイナリをビルドする場合
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"

# musl静的リンク
[target.x86_64-unknown-linux-musl]
rustflags = ["-C", "target-feature=+crt-static"]
```

### cross クレートの活用

Dockerベースでクロスコンパイル環境を自動構築：

```bash
cargo install cross
cross build --target aarch64-unknown-linux-gnu --release
```

## よくあるミスと回避策

- **`Cargo.lock` をコミットするか** — バイナリならコミットする、ライブラリなら `.gitignore` に入れる
- **依存バージョンの `*`** — 絶対に使わない。再現性が壊れる
- **`path` 依存を publish** — `path` と `version` を併記する（`version = "0.1", path = "../core"`）
- **edition の指定忘れ** — デフォルトが古いeditionになる場合がある
