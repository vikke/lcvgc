# lcvgc Neovim Plugin Specification

## Overview

A Neovim plugin implemented in Lua that communicates with the lcvgc engine (a Rust daemon) to perform DSL evaluation, playback control, and feedback display. Neovim only.

---

## 1. Architecture

```
+---------------------------+------------------+
|                           |                  |
|                           |   Log tail       |
|   Main                    |   (engine output)|
|   DSL editing (.cvg)      |                  |
|                           +------------------+
|                           |                  |
|                           |  Eval result     |
|                           |  (success/error) |
+---------------------------+------------------+
```

The lcvgc engine runs independently as a daemon. The Neovim plugin connects to the engine via a TCP socket.

- Since the engine is a separate process, playback continues even if Neovim crashes
- Reconnecting after restarting Neovim allows you to resume coding from where you left off
- In the future, other editors can also connect to the engine

---

## 2. Communication Protocol

TCP socket connection via `vim.fn.sockconnect`. Asynchronous communication ensures Neovim does not block.

```lua
local handle = vim.fn.sockconnect('tcp', 'localhost:9876', {
  on_data = function(_, data, _)
    -- エンジンからの応答を処理
  end,
})
```

### Protocol (JSON)

Neovim → Engine:

```json
{
  "type": "eval",
  "source": "clip drums_a [bars 1] {\n  use tr808\n  ...\n}"
}
```

Engine → Neovim (success):

```json
{
  "type": "ok",
  "block": "clip",
  "name": "drums_a",
  "warnings": ["truncated: 2 notes over 1 bars"],
  "playing_in": ["verse", "chorus"]
}
```

Engine → Neovim (error):

```json
{
  "type": "error",
  "line": 3,
  "message": "unexpected token 'xyz'"
}
```

Engine → Neovim (status notification):

```json
{
  "type": "status",
  "tempo": 125,
  "scene": "verse",
  "playing_clips": ["drums_a", "bass_a", "lead_a"],
  "position": { "bar": 3, "beat": 2 }
}
```

---

## 3. Screen Layout

A 3-pane layout is automatically created on startup.

| Pane | Position | Content |
|------|----------|---------|
| Main | Left | DSL file editing (.cvg) |
| Log | Upper right | Engine log output (tail) |
| Eval result | Lower right | Eval success/error display |

```lua
function M.setup_layout()
  -- メインバッファでDSLファイルを開く（引数で渡されたファイル）

  -- 右に縦分割 → ログ用ターミナル
  vim.cmd('vsplit')
  vim.cmd('terminal tail -f /tmp/lcvgc.log')

  -- 右ペインを上下分割 → eval結果バッファ
  vim.cmd('split')
  eval_buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_set_current_buf(eval_buf)

  -- メインペインにフォーカスを戻す
  vim.cmd('wincmd h')
end
```

---

## 4. Key Mappings

| Key | Mode | Action |
|-----|------|--------|
| `Ctrl-E` | Visual | Evaluate the selected range |
| `Ctrl-E` | Normal | Automatically select and evaluate the current paragraph (delimited by blank lines) |

```lua
vim.keymap.set('v', '<C-e>', function() M.eval_selection() end)
vim.keymap.set('n', '<C-e>', function() M.eval_paragraph() end)
```

---

## 5. Eval Processing

### 5.1 Evaluating a Selection

```lua
function M.eval_selection()
  local start_line = vim.fn.line("'<")
  local end_line = vim.fn.line("'>")
  local lines = vim.api.nvim_buf_get_lines(0, start_line - 1, end_line, false)
  M.send(lines)
end
```

### 5.2 Evaluating a Paragraph

Automatically selects and evaluates a block (paragraph) delimited by blank lines.

```lua
function M.eval_paragraph()
  vim.cmd('normal! vip')
  M.eval_selection()
  vim.cmd('normal! \027')  -- ESCでビジュアルモード解除
end
```

### 5.3 Sending to the Engine

```lua
function M.send(lines)
  local text = table.concat(lines, "\n")
  local payload = vim.fn.json_encode({
    type = "eval",
    source = text,
  })
  vim.fn.chansend(handle, payload .. "\n")
end
```

---

## 6. Eval Result Display

Displays the JSON response from the engine in the eval result buffer. Success responses are highlighted in green tones, and errors in red tones.

```lua
function M.on_response(data)
  local raw = table.concat(data, "")
  local ok_parse, msg = pcall(vim.fn.json_decode, raw)
  if not ok_parse then return end

  local lines = {}
  local is_error = msg.type == "error"

  if is_error then
    table.insert(lines, "ERR line " .. (msg.line or "?") .. ": " .. msg.message)
  else
    table.insert(lines, "OK " .. (msg.block or "") .. ":" .. (msg.name or ""))
    if msg.warnings then
      for _, w in ipairs(msg.warnings) do
        table.insert(lines, "  WARN: " .. w)
      end
    end
    if msg.playing_in and #msg.playing_in > 0 then
      table.insert(lines, "  playing in: " .. table.concat(msg.playing_in, ", "))
    end
  end

  vim.api.nvim_buf_set_lines(eval_buf, 0, -1, false, lines)

  local hl = is_error and 'ErrorMsg' or 'DiffAdd'
  vim.api.nvim_win_set_option(eval_win, 'winhighlight', 'Normal:' .. hl)
end
```

---

## 7. Connection Management

### 7.1 Connecting

```lua
function M.connect(port)
  port = port or 9876
  M.handle = vim.fn.sockconnect('tcp', 'localhost:' .. port, {
    on_data = function(_, data, _)
      M.on_response(data)
    end,
  })
  if M.handle == 0 then
    vim.notify('lcvgc engine not running on port ' .. port, vim.log.levels.ERROR)
  end
end
```

### 7.2 Disconnect Detection

Notifies the user when the connection to the engine is lost.

```lua
on_data = function(_, data, _)
  if not data or (#data == 1 and data[1] == '') then
    vim.notify('lcvgc engine disconnected', vim.log.levels.WARN)
    return
  end
  M.on_response(data)
end
```

### 7.3 Reconnection

Provides a command to reconnect after restarting Neovim or the engine.

```lua
vim.api.nvim_create_user_command('LcvgcConnect', function(opts)
  local port = tonumber(opts.args) or 9876
  M.connect(port)
end, { nargs = '?' })
```

---

## 8. Filetype Configuration

Sets the filetype for `.cvg` files. This serves as the foundation for LSP startup and syntax highlighting.

```lua
vim.filetype.add({
  extension = {
    live = 'cvg',
  },
})
```

---

## 9. User Commands

| Command | Action |
|---------|--------|
| `:LcvgcConnect [port]` | Connect to the engine (default: 9876) |
| `:LcvgcDisconnect` | Disconnect from the engine |
| `:LcvgcStatus` | Display engine status (connection state, currently playing scene, etc.) |
| `:LcvgcStop` | Stop all (shortcut for evaluating `stop`) |
| `:LcvgcLayout` | Build the 3-pane layout |
| `:LcvgcMicStart [options]` | Start microphone input. Inserts detected note names at cursor position. Options: `--quantize N`, `--key c`, `--scale minor`, etc. |
| `:LcvgcMicStop` | Stop microphone input |

### Microphone Input Implementation

Launches the `lcvgc-mic` external binary as a job and inserts its stdout output into the buffer. If the clip at the cursor position has `[scale ...]` specified, `--key` and `--scale` are automatically appended. Falls back to the global `scale` setting if no clip-level scale is found.

```lua
local mic_job = nil

-- カーソル位置のclipからscale情報を取得
local function get_clip_scale()
  -- 現在行から上方向にclipヘッダを探す
  local bufnr = vim.api.nvim_get_current_buf()
  local row = vim.api.nvim_win_get_cursor(0)[1]
  for i = row, 1, -1 do
    local line = vim.api.nvim_buf_get_lines(bufnr, i - 1, i, false)[1]
    local root, scale_type = line:match('%[scale%s+(%S+)%s+(%S+)%]')
    if root and scale_type then
      return { root = root, type = scale_type }
    end
    -- clipブロックの外に出たら探索打ち切り
    if line:match('^%s*clip%s') or line:match('^%s*scene%s') or line:match('^%s*session%s') then
      break
    end
  end
  -- clipレベルのscaleが見つからない場合、グローバルscaleを探す
  for i = 1, vim.api.nvim_buf_line_count(bufnr) do
    local line = vim.api.nvim_buf_get_lines(bufnr, i - 1, i, false)[1]
    local root, scale_type = line:match('^%s*scale%s+(%S+)%s+(%S+)')
    if root and scale_type then
      return { root = root, type = scale_type }
    end
  end
  return nil
end

vim.api.nvim_create_user_command('LcvgcMicStart', function(opts)
  local args = { 'lcvgc-mic' }
  -- ユーザーの引数をそのまま渡す
  for _, arg in ipairs(vim.split(opts.args, ' ')) do
    if arg ~= '' then table.insert(args, arg) end
  end

  -- --key が未指定なら、現在のclipのscaleから自動取得
  if not opts.args:match('%-%-key') then
    local scale = get_clip_scale()
    if scale then
      table.insert(args, '--key')
      table.insert(args, scale.root)
      table.insert(args, '--scale')
      table.insert(args, scale.type)
    end
  end

  mic_job = vim.fn.jobstart(args, {
    on_stdout = function(_, data, _)
      local text = table.concat(data, ' '):gsub('%s+$', '')
      if text ~= '' then
        vim.schedule(function()
          vim.api.nvim_put({ text }, 'c', true, true)
        end)
      end
    end,
    on_stderr = function(_, data, _)
      local msg = table.concat(data, '')
      if msg ~= '' then
        vim.notify('lcvgc-mic: ' .. msg, vim.log.levels.WARN)
      end
    end,
  })
end, { nargs = '*' })

vim.api.nvim_create_user_command('LcvgcMicStop', function()
  if mic_job then
    vim.fn.jobstop(mic_job)
    mic_job = nil
  end
end, {})
```

---

## 10. Startup Flow

```bash
# 1. エンジンを起動（バックグラウンド or 別ターミナル）
lcvgc daemon --port 9876 --log /tmp/lcvgc.log

# 2. Neovimを起動
nvim song.cvg

# 3. Neovim内で接続 + レイアウト構築
:LcvgcConnect
:LcvgcLayout

# または init.lua で自動化
# autocmd BufRead *.cvg lua require('lcvgc').connect(); require('lcvgc').setup_layout()
```

### Automation Example

```lua
-- init.lua
vim.api.nvim_create_autocmd('BufRead', {
  pattern = '*.cvg',
  callback = function()
    local lcvgc = require('lcvgc')
    lcvgc.connect()
    lcvgc.setup_layout()
  end,
})
```

---

## 11. LSP Integration

Integrates with a separately implemented custom LSP server. Connects using Neovim's built-in LSP client (`vim.lsp`).

```lua
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'cvg',
  callback = function()
    vim.lsp.start({
      name = 'lcvgc-lsp',
      cmd = { 'lcvgc', 'lsp' },    -- エンジンのサブコマンドとしてLSPモードを起動
      root_dir = vim.fn.getcwd(),
    })
  end,
})
```

### Features Provided by the LSP

| Feature | Description |
|---------|-------------|
| Completion | Display candidates + descriptions in all contexts |
| Inlay hints | Display descriptions of expected next values in gray |
| Diagnostics | Parse errors, undefined references (non-existent clip names, etc.), bars overflow |
| Go to definition | Jump from clip names or instrument names to their definitions |
| Hover | Preview clip and scene contents |

### i18n

LSP completion descriptions (detail/documentation) and inlay hints support i18n. The language setting is specified via engine startup options or a configuration file.

```bash
lcvgc daemon --port 9876 --lang ja
lcvgc daemon --port 9876 --lang en
```

Rust-side implementation concept:

```rust
// i18n/ja.toml
[completion]
midi_channel = "MIDIチャンネル (1-16)"
fixed_note = "固定ノート (ドラム用)"
gm_drum_map = "GMドラムマップ: C2=BD, D2=SD, F#2=HH..."
bars_count = "小節数"
time_sig = "拍子"
scale_root = "スケールのルート音"
scale_type = "スケールタイプ"
drum_kit = "ドラムキット"
step_resolution = "1文字の音符解像度"
note_or_rest = "音名 / 休符 / 和音"
octave = "オクターブ (0-9)"
duration = "音長"
duration_values = "1=全 2=半 4=四分 8=八分 16=十六分"
arp_direction = "アルペジオ方向"
arp_interval = "発音間隔の音符解像度"
hit_chars = "x=通常 X=アクセント o=ゴースト .=休符"
bar_jump = "小節ジャンプ (1-bars)"
shuffle = "|=シャッフル 1-9=発音確率10-90%"
shuffle_weight = "シャッフル重み"
tempo_value = "BPM値 / +N,-Nで相対変化"
repeat_count = "繰り返し回数"
play_target = "再生するsceneまたはsession"
stop_target = "省略で全停止、指定でそのclipだけミュート"
include_path = "相対パス (.cvgファイル)"
midi_device = "MIDIデバイス"
midi_device_os = "OSが認識するMIDIデバイス名"
device_for_kit = "キット全体のMIDIデバイス"

// i18n/en.toml
[completion]
midi_channel = "MIDI channel (1-16)"
fixed_note = "Fixed note (for drums)"
gm_drum_map = "GM drum map: C2=BD, D2=SD, F#2=HH..."
bars_count = "Number of bars"
...
```

### Context-Dependent Completion and Hint Table

In the tables below, "Completion" refers to items displayed as selectable candidates, and "Hint" refers to descriptions displayed in gray as inlay hints.

#### Top Level

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| Beginning of file / outside blocks | `device`, `instrument`, `kit`, `clip`, `scene`, `session`, `include`, `tempo`, `scale`, `play`, `stop` | — |

#### device

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `device ` | (text input) | — |
| Inside `device name {` | `port` | — |
| After `port ` | (text input) | OS-recognized MIDI device name |

#### instrument

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `instrument ` | (text input) | — |
| Inside `instrument name {` | `device`, `channel`, `note`, `gate_normal`, `gate_staccato` | — |
| After `device ` | (defined device names) | MIDI device |
| After `channel ` | `1`..`16` | MIDI channel (1-16) |
| After `note ` | `c0`..`b9` | Fixed note (for drums) |
| After `gate_normal ` | (numeric) | Gate ratio % (default 80) |
| After `gate_staccato ` | (numeric) | Staccato gate ratio % (default 40) |

#### kit

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `kit ` | (text input) | — |
| Inside `kit name {` | `device`, (text input: instrument name) | — |
| After `device ` | (defined device names) | MIDI device for the entire kit |
| After `instrument name ` | `{` | — |
| Inside `instrument name {` | `channel`, `note` | — |
| After `channel ` | `1`..`16` | MIDI channel (1-16) |
| After `note ` | `c0`..`b9` | GM drum map: C2=BD, D2=SD, F#2=HH... |
| After `gate_normal ` | (numeric) | Gate ratio % (default 80) |
| After `gate_staccato ` | (numeric) | Staccato gate ratio % (default 40) |

#### clip (Header)

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `clip ` | (text input) | — |
| After `clip name ` | `[`, `{` | — |
| After `[` | `bars`, `time`, `scale` | — |
| After `[bars ` | (numeric) | Number of bars |
| After `[time ` | `3/4`, `4/4`, `5/4`, `6/8`, `7/8` | Time signature |
| After `[scale ` | `c`, `c#`, `db`, ..., `b` | Scale root note |
| After `[scale C ` | `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian` | Scale type |

#### clip (Drum Notation)

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| Right after clip `{` | `use`, `resolution`, (defined instrument names) | — |
| After `use ` | (defined kit names) | Drum kit |
| After `resolution ` | `4`, `8`, `16`, `32` | Note resolution per character |
| After instrument name (step input) | `x`, `X`, `o`, `.`, `\|`, `(` | x=normal X=accent o=ghost .=rest |
| After `)*` | `1`..`9` | Repeat count |
| After `>` | `1`..`N` | Bar jump (1-bars) |

#### clip (Pitched Instrument Notation)

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After instrument name | `c`, `c#`, `db`, ..., `b`, `r`, `[`, `(`, `>`, chord name | Note name / rest / chord / repeat / chord name |
| After note name or chord name | `:` | Octave delimiter |
| After `:` (2nd section) | `0`..`9` | Octave (0-9) |
| After octave | `:` | Duration delimiter |
| After `:` (3rd section) | `1`, `2`, `4`, `4.`, `8`, `8.`, `16` | 1=whole 2=half 4=quarter 8=eighth 16=sixteenth |
| After duration | `'`, `g` | '=staccato g=direct gate ratio |
| After `g` | (numeric) | Gate ratio % (1-100) |
| After `)*` | `1`..`9` | Repeat count |
| After `[...]` chord / chord name | `arp` | Arpeggio |
| After `arp(` | `up`, `down`, `updown`, `random` | Arpeggio direction |
| After `arp(up, ` | `4`, `8`, `16` | Note resolution for triggering interval |
| Chord position in a scale-specified clip | Diatonic chord candidates | Degree info (e.g., IVm7 - subdominant) |

#### scene

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `scene ` | (text input) | — |
| Inside `scene name {` | (defined clip names), `tempo` | — |
| After clip name | `\|`, `1`..`9`, (newline) | \|=shuffle 1-9=trigger probability 10-90% |
| After `\|` | (defined clip names) | Shuffle candidates |
| After clip name `*` | `1`..`9` | Shuffle weight |
| After `tempo ` | (numeric), `+`, `-` | BPM value / +N,-N for relative change |

#### session

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `session ` | (text input) | — |
| Inside `session name {` | (defined scene names) | — |
| After scene name | `[` | — |
| After `[` | `repeat`, `loop` | — |
| After `[repeat ` | (numeric) | Repeat count |

#### play / stop

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `play ` | (defined scene names), `session` | Scene or session to play |
| After `play session ` | (defined session names) | — |
| After `play name ` | `[` | — |
| After `play name [` | `repeat`, `loop` | — |
| After `play name [repeat ` | (numeric) | Repeat count |
| After `stop ` | (defined clip names) | Omit to stop all, specify to mute that clip only |

#### scale (Global)

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `scale ` | `c`, `c#`, `db`, ..., `b` | Scale root note |
| After `scale c ` | `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian` | Scale type |

The global `scale` is applied when a clip's `[scale ...]` is not specified. Can be overridden by clip-level specification.

#### include / tempo

| Cursor Position | Completion | Hint |
|-----------------|------------|------|
| After `include ` | `"` | Relative path (.cvg file) |
| After `include "` | (.cvg file path completion) | — |
| After `tempo ` | (numeric) | BPM |

---

## 12. Syntax Highlighting with Tree-sitter

A Tree-sitter grammar is created for the lcvgc DSL to provide accurate syntax-based highlighting.

### 12.1 Grammar Structure

```
tree-sitter-cvg/
├── grammar.js           -- 文法定義
├── queries/
│   └── highlights.scm   -- ハイライトクエリ
├── src/
│   └── parser.c          -- grammar.jsから生成
└── package.json
```

Registering with Neovim:

```lua
vim.api.nvim_create_autocmd('User', {
  pattern = 'TSUpdate',
  callback = function()
    require('nvim-treesitter.parsers').cvg = {
      install_info = {
        url = 'https://github.com/<user>/tree-sitter-cvg',
        files = { 'src/parser.c' },
      },
    }
  end,
})
```

### 12.2 Highlight Queries (highlights.scm)

```scheme
;; 予約語
["device" "instrument" "kit" "clip" "scene" "session"
 "include" "use" "resolution" "tempo" "time"
 "port" "channel" "note" "bars" "arp"] @keyword

;; play / stop は専用グループ（太字で目立たせる）
["play"] @keyword.play
["stop"] @keyword.stop

;; loop / repeat
["loop" "repeat"] @keyword.repeat

;; ユーザー定義名（定義側）
(device_def (identifier) @type.definition)
(instrument_def (identifier) @type.definition)
(kit_def (identifier) @type.definition)
(clip_def (identifier) @function.definition)
(scene_def (identifier) @function.definition)
(session_def (identifier) @function.definition)

;; ユーザー定義名（参照側）
(scene_slot (identifier) @function)
(play_stmt (identifier) @function)
(session_entry (identifier) @function)
(use_stmt (identifier) @type)

;; 音名
(note_name) @constant

;; コード名
(chord_name) @string.special

;; 数値（オクターブ、音長、テンポ等）
(octave) @number
(duration) @number
(number) @number

;; ステップシーケンサーパターン (x.oX|)
(step_pattern) @string

;; 確率行 (..5...7.)
(probability_row) @number.special

;; scene内の確率
(scene_slot (probability) @number.special)

;; scene内のシャッフル |
(scene_slot "|" @operator)

;; 重み (*3)
(weight) @number.weight

;; 小節ジャンプ (>N)
(bar_jump) @keyword.jump

;; テンポ変化 (+5, -3)
(tempo_delta) @number.special

;; アルペジオ方向
(arp_direction) @constant.builtin

;; 和音の括弧
["[" "]"] @punctuation.bracket

;; 文字列（ポート名、includeパス）
(string) @string
(include_stmt (string) @string.special.path)

;; コメント
(comment) @comment
```

---

## 13. Color Scheme

A color scheme designed for visibility on dark terminal backgrounds and in dark environments (such as clubs). Based on Material Darker, with reduced saturation while maintaining sufficient contrast.

### 13.1 Color Palette

| Element | Highlight Group | Color | Intent |
|---------|----------------|-------|--------|
| Keywords (`device`, `clip`, etc.) | `@keyword.cvg` | `#C792EA` light purple | Subtle enough to show structure |
| Definition names (`drums_a`, `tr808`) | `@type.definition.cvg` / `@function.definition.cvg` | `#FFCB6B` bright yellow | Definitions stand out |
| Reference names (clip names in scenes, etc.) | `@function.cvg` / `@type.cvg` | `#82AAFF` bright blue | Distinguishable from definitions while maintaining visibility |
| Note names (`c`, `eb`, `f#`) | `@constant.cvg` | `#F78C6C` bright orange | The star of the score, most eye-catching |
| Numbers (octave, duration) | `@number.cvg` | `#89DDFF` cyan | Readable next to note names |
| Step patterns (`x.oX`) | `@string.cvg` | `#C3E88D` bright green | Patterns recognizable at a glance |
| Probability rows (`..5...7.`) | `@number.special.cvg` | `#FF5370` red | Clear contrast with step patterns (green) |
| Chord names (`cm7`) | `@string.special.cvg` | `#FF9CAC` pink | Distinguishable from note name orange |
| Arpeggio direction | `@constant.builtin.cvg` | `#80CBC4` light cyan | Subtle but visible |
| Shuffle `\|` | `@operator.cvg` | `#FFFFFF` white bold | Clear as a delimiter |
| Weight (`*3`) | `@number.weight.cvg` | `#FF5370` red | Same family as probability |
| Bar jump (`>3`) | `@keyword.jump.cvg` | `#FFCB6B` bright yellow bold | Stands out as a bar structure marker |
| Tempo change (`+5`) | `@number.special.cvg` | `#FF5370` red | Changing values use red tones |
| play | `@keyword.play.cvg` | `#C3E88D` green bold | Play = GO |
| stop | `@keyword.stop.cvg` | `#FF5370` red bold | Stop = STOP |
| loop / repeat | `@keyword.repeat.cvg` | `#C792EA` light purple italic | Same family as keywords but distinguished by italic |
| Strings (port names) | `@string.path.cvg` | `#A5D6A7` muted green | Does not need to stand out |
| Comments | `@comment.cvg` | `#546E7A` dark gray italic | Non-intrusive during performance |
| Brackets `[ ]` | `@punctuation.bracket.cvg` | `#89DDFF` cyan | Same family as numbers |

### 13.2 Implementation

```lua
-- lcvgc/colors.lua

local M = {}

function M.setup()
  local hl = vim.api.nvim_set_hl

  -- 予約語
  hl(0, '@keyword.cvg',              { fg = '#C792EA' })
  hl(0, '@keyword.play.cvg',         { fg = '#C3E88D', bold = true })
  hl(0, '@keyword.stop.cvg',         { fg = '#FF5370', bold = true })
  hl(0, '@keyword.repeat.cvg',       { fg = '#C792EA', italic = true })
  hl(0, '@keyword.jump.cvg',         { fg = '#FFCB6B', bold = true })

  -- 定義名
  hl(0, '@type.definition.cvg',      { fg = '#FFCB6B' })
  hl(0, '@function.definition.cvg',  { fg = '#FFCB6B' })

  -- 参照名
  hl(0, '@function.cvg',             { fg = '#82AAFF' })
  hl(0, '@type.cvg',                 { fg = '#82AAFF' })

  -- 音名
  hl(0, '@constant.cvg',             { fg = '#F78C6C' })

  -- 数値
  hl(0, '@number.cvg',               { fg = '#89DDFF' })

  -- ステップパターン
  hl(0, '@string.cvg',               { fg = '#C3E88D' })

  -- 確率・重み・テンポ変化
  hl(0, '@number.special.cvg',       { fg = '#FF5370' })
  hl(0, '@number.weight.cvg',        { fg = '#FF5370' })

  -- コード名
  hl(0, '@string.special.cvg',       { fg = '#FF9CAC' })

  -- アルペジオ方向
  hl(0, '@constant.builtin.cvg',     { fg = '#80CBC4' })

  -- シャッフル
  hl(0, '@operator.cvg',             { fg = '#FFFFFF', bold = true })

  -- 文字列
  hl(0, '@string.path.cvg',          { fg = '#A5D6A7' })

  -- コメント
  hl(0, '@comment.cvg',              { fg = '#546E7A', italic = true })

  -- 括弧
  hl(0, '@punctuation.bracket.cvg',  { fg = '#89DDFF' })
end

return M
```

### 13.3 Design Philosophy

- **Step patterns (green) and probability rows (red)** are vertically adjacent, making them instantly distinguishable even in dark environments
- **Note names (orange)** are the most eye-catching color, serving as the central element when reading the score in dark environments
- **play (green bold) / stop (red bold)** are the most critical operations during live performance, using traffic light colors for intuitive recognition
- **Bar jumps (yellow bold)** stand out as bar structure markers
- **Comments (dark gray)** do not intrude on the visual field during performance
- Overall saturation is reduced while maintaining contrast, ensuring comfortable viewing during extended performances

---

## 14. Plugin Structure

```
lcvgc.nvim/
├── lua/lcvgc/
│   ├── init.lua          -- エントリポイント、setup()
│   ├── connection.lua    -- ソケット通信 (JSON)
│   ├── eval.lua          -- eval処理
│   ├── layout.lua        -- 3ペインレイアウト
│   ├── display.lua       -- eval結果表示
│   ├── colors.lua        -- カラースキーム定義
│   └── commands.lua      -- ユーザーコマンド定義
├── queries/live/
│   └── highlights.scm    -- Tree-sitterハイライトクエリ
├── ftdetect/
│   └── cvg.lua          -- filetype検出
└── tree-sitter-cvg/     -- Tree-sitter grammar (別リポジトリでも可)
    ├── grammar.js
    ├── queries/
    │   └── highlights.scm
    ├── src/
    │   └── parser.c
    └── package.json
```
