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

### インストール

```sh
cargo install --git https://github.com/vikke/lcvgc lcvgc
```

LSP機能はlcvgcデーモンに統合されています。エンジンとLSPを別々にインストールする必要はありません。
WSL2 on Windowsの場合、Windows上でlcvgcを実行し、Neovimプラグイン(lcvgc.nvim)からTCP接続してください。

## 関連プロジェクト

lcvgc は以下のプロジェクトと連携して動作します:

- [lcvgc.nvim](https://github.com/vikke/lcvgc.nvim) — Neovim プラグイン。エディタ上からlcvgcエンジンへの接続・評価・再生を行うフロントエンド
- [lcvgc_mic](https://github.com/vikke/lcvgc_mic) — マイク入力からリアルタイムにピッチを検出し、lcvgc DSL形式のノートテキストを生成するCLIツール
- [tree-sitter-cvg](https://github.com/vikke/tree-sitter-cvg) — lcvgc DSL（.cvgファイル）用の Tree-sitter 文法。lcvgc.nvim でのシンタックスハイライトに使用

## 仕様書

- [DSL 仕様書（日本語）](specs/lcvgc-dsl-spec.ja.md)
- [DSL Specification（English）](specs/lcvgc-dsl-spec.md)
