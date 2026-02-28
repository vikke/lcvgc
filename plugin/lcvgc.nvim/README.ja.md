# lcvgc.nvim

lcvgc（ライブコーディングMIDIシーケンサー）のNeovimプラグイン。
TCPプロトコルを通じてlcvgcデーモンと通信し、エディタから直接DSLコードを評価・再生制御できます。

## インストール

### lazy.nvim

```lua
{
  dir = "path/to/lcvgc/plugin/lcvgc.nvim",
  config = function()
    require("lcvgc").setup({
      host = "127.0.0.1",
      port = 5555,
    })
  end,
}
```

## セットアップ

```lua
require("lcvgc").setup({
  host = "127.0.0.1",  -- lcvgcデーモンのホスト
  port = 5555,          -- lcvgcデーモンのポート
})
```

## コマンド

| コマンド | 説明 |
|----------|------|
| `:LcvgcConnect` | lcvgcデーモンに接続 |
| `:LcvgcDisconnect` | 接続を切断 |
| `:LcvgcEval` | 現在行を評価（ビジュアル選択対応） |
| `:LcvgcLoad {path}` | ファイルを読み込み |
| `:LcvgcPlay {scene}` | シーンをループ再生 |
| `:LcvgcStop [target]` | 再生停止（対象指定可） |
| `:LcvgcStatus` | 再生状態を表示 |

## キーバインド例

```lua
vim.keymap.set("n", "<leader>lc", "<cmd>LcvgcConnect<cr>", { desc = "lcvgc: 接続" })
vim.keymap.set("n", "<leader>le", "<cmd>LcvgcEval<cr>", { desc = "lcvgc: 現在行を評価" })
vim.keymap.set("v", "<leader>le", ":LcvgcEval<cr>", { desc = "lcvgc: 選択範囲を評価" })
vim.keymap.set("n", "<leader>ls", "<cmd>LcvgcStop<cr>", { desc = "lcvgc: 停止" })
vim.keymap.set("n", "<leader>li", "<cmd>LcvgcStatus<cr>", { desc = "lcvgc: 状態表示" })
```

## 使い方

1. lcvgcデーモンを起動: `lcvgc`
2. Neovimで `:LcvgcConnect` を実行
3. `.cvg` ファイルを開き、DSLコードを書く
4. 行を選択して `:LcvgcEval` で評価
5. `:LcvgcPlay scene_name` で再生開始
6. `:LcvgcStop` で停止

## プロトコル

lcvgcデーモンとはJSON-over-TCP（1行1メッセージ）で通信します。

```json
{"type":"eval","source":"tempo 120\n"}
{"type":"load","path":"song.cvg"}
{"type":"status"}
```
