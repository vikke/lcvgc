# lcvgc - Live CV Gate Coder

## 目的
テキストベースのDSLでMIDIシーケンスを記述し、リアルタイムに評価・再生するライブコーディングエンジン。
モジュラーシンセ（CV/Gate）からMIDIシンセ全般のシーケンスに対応。

## コンポーネント
1. **lcvgc エンジン** (Rust) - DSLパーサー、MIDIシーケンサー、TCPサーバー（デーモン）
2. **lcvgc-lsp** - カスタムLSPサーバー（エンジンのサブコマンド `lcvgc lsp`）
3. **lcvgc-mic** - マイク入力→音名テキスト変換（別バイナリ、Phase 4以降）
4. **lcvgc.nvim** - Neovimプラグイン（Lua）

## DSLファイル
- 拡張子: `.cvg`
- ブロック: device, instrument, kit, clip, scene, session, tempo, play, stop, include, var
- 各ブロックは独立してeval可能、同名ブロックは上書き

## Tech Stack
- 言語: Rust (edition 2021)
- Neovimプラグイン: Lua
- 通信: TCP ソケット (JSON プロトコル)
- Tree-sitter grammar for シンタックスハイライト

## 設計方針
- **音は絶対に止めない**: 全エラーは通知のみ、再生に影響しない
- エンジンはデーモンとして独立起動、Neovimが落ちても演奏継続
- evalで上書き、削除操作なし
