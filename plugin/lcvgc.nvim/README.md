# lcvgc.nvim

Neovim plugin for lcvgc (live coding MIDI sequencer).
Communicates with the lcvgc daemon over TCP, enabling direct DSL code evaluation and playback control from the editor.

## Installation

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

## Setup

```lua
require("lcvgc").setup({
  host = "127.0.0.1",  -- lcvgc daemon host
  port = 5555,          -- lcvgc daemon port
})
```

## Commands

| Command | Description |
|---------|-------------|
| `:LcvgcConnect` | Connect to lcvgc daemon |
| `:LcvgcDisconnect` | Disconnect from daemon |
| `:LcvgcEval` | Evaluate current line (supports visual selection) |
| `:LcvgcLoad {path}` | Load a file |
| `:LcvgcPlay {scene}` | Play a scene in loop |
| `:LcvgcStop [target]` | Stop playback (optional target) |
| `:LcvgcStatus` | Show playback status |

## Keybinding Examples

```lua
vim.keymap.set("n", "<leader>lc", "<cmd>LcvgcConnect<cr>", { desc = "lcvgc: connect" })
vim.keymap.set("n", "<leader>le", "<cmd>LcvgcEval<cr>", { desc = "lcvgc: eval current line" })
vim.keymap.set("v", "<leader>le", ":LcvgcEval<cr>", { desc = "lcvgc: eval selection" })
vim.keymap.set("n", "<leader>ls", "<cmd>LcvgcStop<cr>", { desc = "lcvgc: stop" })
vim.keymap.set("n", "<leader>li", "<cmd>LcvgcStatus<cr>", { desc = "lcvgc: status" })
```

## Usage

1. Start the lcvgc daemon: `lcvgc`
2. Run `:LcvgcConnect` in Neovim
3. Open a `.cvg` file and write DSL code
4. Select lines and run `:LcvgcEval` to evaluate
5. Run `:LcvgcPlay scene_name` to start playback
6. Run `:LcvgcStop` to stop

## Protocol

Communicates with the lcvgc daemon via JSON-over-TCP (one message per line).

```json
{"type":"eval","source":"tempo 120\n"}
{"type":"load","path":"song.cvg"}
{"type":"status"}
```
