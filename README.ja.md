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

midi portの付いているpc.
```sh
cargo install --git https://github.com/vikke/lcvgc lcvgc
```

lspを走らせるpc.
```sh
cargo install --git https://github.com/vikke/lcvgc lcvgc-lsp
```

これは、例えば、WSL2 on Windows の場合、 WSL2 から midi port を見るのは手続きが必要で面倒な場合がある。
こういった場合、`lcvgc(エンジン)` 自体は Windows 上で、nvim で使う`lcvgc-lsp(lsp)` は WSL2 の Linux から実行する。
macのように両方を1台の上で行なえるなら、両方のコマンドを実行してインストールすれば良い。

## 関連プロジェクト

lcvgc は以下のプロジェクトと連携して動作します:

- [lcvgc.nvim](https://github.com/vikke/lcvgc.nvim) — Neovim プラグイン。エディタ上からlcvgcエンジンへの接続・評価・再生を行うフロントエンド
- [lcvgc_mic](https://github.com/vikke/lcvgc_mic) — マイク入力からリアルタイムにピッチを検出し、lcvgc DSL形式のノートテキストを生成するCLIツール
- [tree-sitter-cvg](https://github.com/vikke/tree-sitter-cvg) — lcvgc DSL（.cvgファイル）用の Tree-sitter 文法。lcvgc.nvim でのシンタックスハイライトに使用

## 仕様書

- [DSL 仕様書（日本語）](specs/lcvgc-dsl-spec.ja.md)
- [DSL Specification（English）](specs/lcvgc-dsl-spec.md)
