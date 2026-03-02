# Live CV Gate Coder

## これは何?

テキストベースの DSL で MIDI シーケンスを記述し、リアルタイムに評価・再生するライブコーディングエンジン。
モジュラーシンセ（MIDI to CV）から MIDI シンセ全般のシーケンスに対応したライブコーディングツールキット。

## インストール

### 必要環境

- Rust (edition 2021)
- Linux の場合: ALSA 開発ライブラリ
  - Debian/Ubuntu: `sudo apt install libasound2-dev`
  - Fedora/RHEL: `sudo dnf install alsa-lib-devel`

### ビルド

```sh
git clone https://github.com/vikke/lcvgc.git
cd lcvgc
cargo build --release
```

## 関連プロジェクト

- [lcvgc.nvim](https://github.com/vikke/lcvgc.nvim) — Neovim プラグイン

## 仕様書

- [DSL 仕様書（日本語）](specs/lcvgc-dsl-spec.ja.md)
- [DSL Specification（English）](specs/lcvgc-dsl-spec.md)
