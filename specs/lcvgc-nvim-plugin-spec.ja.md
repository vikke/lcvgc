# lcvgc Neovimプラグイン仕様

## 概要

lcvgcエンジン（Rustデーモン）と通信し、DSLのeval・再生制御・フィードバック表示を行うNeovimプラグイン。Luaで実装する。Neovim専用。

---

## 1. アーキテクチャ

```
+---------------------------+------------------+
|                           |                  |
|                           |   ログ tail      |
|   メイン                   |   (engine出力)   |
|   DSL編集 (.cvg)          |                  |
|                           +------------------+
|                           |                  |
|                           |  eval結果        |
|                           |  (成功/エラー)    |
+---------------------------+------------------+
```

lcvgcエンジンはデーモンとして独立起動する。Neovimプラグインはソケット（TCP）でエンジンに接続する。

- エンジンが独立プロセスなので、Neovimが落ちても演奏が続く
- Neovimを再起動して再接続すれば、続きからコーディングできる
- 将来的に他のエディタからも接続可能

---

## 2. 通信方式

`vim.fn.sockconnect` でTCPソケット接続。非同期通信のためNeovimはブロッキングしない。

```lua
local handle = vim.fn.sockconnect('tcp', 'localhost:9876', {
  on_data = function(_, data, _)
    -- エンジンからの応答を処理
  end,
})
```

### プロトコル (JSON)

Neovim → エンジン:

```json
{
  "type": "eval",
  "source": "clip drums_a [bars 1] {\n  use tr808\n  ...\n}"
}
```

エンジン → Neovim (成功):

```json
{
  "type": "ok",
  "block": "clip",
  "name": "drums_a",
  "warnings": ["truncated: 2 notes over 1 bars"],
  "playing_in": ["verse", "chorus"]
}
```

エンジン → Neovim (エラー):

```json
{
  "type": "error",
  "line": 3,
  "message": "unexpected token 'xyz'"
}
```

エンジン → Neovim (状態通知):

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

## 3. 画面レイアウト

起動時に3ペイン構成を自動で作る。

| ペイン | 位置 | 内容 |
|--------|------|------|
| メイン | 左 | DSLファイル編集 (.cvg) |
| ログ | 右上 | エンジンのログ出力 (tail) |
| eval結果 | 右下 | evalの成功/エラー表示 |

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

## 4. キーマップ

| キー | モード | 動作 |
|------|--------|------|
| `Ctrl-E` | ビジュアル | 選択範囲をevalする |
| `Ctrl-E` | ノーマル | 現在の段落（空行区切り）を自動選択してevalする |

```lua
vim.keymap.set('v', '<C-e>', function() M.eval_selection() end)
vim.keymap.set('n', '<C-e>', function() M.eval_paragraph() end)
```

---

## 5. eval処理

### 5.1 選択範囲のeval

```lua
function M.eval_selection()
  local start_line = vim.fn.line("'<")
  local end_line = vim.fn.line("'>")
  local lines = vim.api.nvim_buf_get_lines(0, start_line - 1, end_line, false)
  M.send(lines)
end
```

### 5.2 段落のeval

空行で区切られたブロック（段落）を自動選択してevalする。

```lua
function M.eval_paragraph()
  vim.cmd('normal! vip')
  M.eval_selection()
  vim.cmd('normal! \027')  -- ESCでビジュアルモード解除
end
```

### 5.3 エンジンへの送信

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

## 6. eval結果の表示

エンジンからのJSON応答をeval結果バッファに表示する。成功なら緑系、エラーなら赤系のハイライトを適用する。

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

## 7. 接続管理

### 7.1 接続

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

### 7.2 切断検知

エンジンとの接続が切れた場合、ユーザーに通知する。

```lua
on_data = function(_, data, _)
  if not data or (#data == 1 and data[1] == '') then
    vim.notify('lcvgc engine disconnected', vim.log.levels.WARN)
    return
  end
  M.on_response(data)
end
```

### 7.3 再接続

Neovim再起動後やエンジン再起動後に再接続するコマンドを提供する。

```lua
vim.api.nvim_create_user_command('LcvgcConnect', function(opts)
  local port = tonumber(opts.args) or 9876
  M.connect(port)
end, { nargs = '?' })
```

---

## 8. filetype設定

`.cvg` ファイルに対してfiletypeを設定する。LSPの起動やシンタックスハイライトの基盤になる。

```lua
vim.filetype.add({
  extension = {
    live = 'cvg',
  },
})
```

---

## 9. ユーザーコマンド

| コマンド | 動作 |
|----------|------|
| `:LcvgcConnect [port]` | エンジンに接続 (デフォルト: 9876) |
| `:LcvgcDisconnect` | 接続を切断 |
| `:LcvgcStatus` | エンジンの状態を表示（接続状況、再生中のscene等） |
| `:LcvgcStop` | 全停止（`stop` をevalするショートカット） |
| `:LcvgcLayout` | 3ペインレイアウトを構築 |
| `:LcvgcMicStart [options]` | マイク入力開始。検出した音名をカーソル位置に挿入。オプション: `--quantize N`, `--key c`, `--scale minor` 等 |
| `:LcvgcMicStop` | マイク入力停止 |

### マイク入力の実装

`lcvgc-mic` 外部バイナリをジョブとして起動し、stdout出力をバッファに挿入する。カーソル位置のclipに `[scale ...]` が指定されている場合、`--key` と `--scale` を自動で付与する。clipレベルのscaleがない場合はグローバルの `scale` 設定にフォールバックする。

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

## 10. 起動フロー

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

### 自動化の例

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

## 11. LSP連携

別途実装するカスタムLSPサーバーと連携する。Neovimの組み込みLSPクライアント (`vim.lsp`) で接続する。

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

### LSPが提供する機能

| 機能 | 内容 |
|------|------|
| 補完 | 全ての文脈で候補 + 説明を表示 |
| インレイヒント | 次に来る値の説明をグレーで表示 |
| 診断 | パースエラー、未定義の参照（存在しないclip名等）、bars超過 |
| 定義ジャンプ | clip名やinstrument名から定義元へジャンプ |
| ホバー | clipやsceneの内容プレビュー |

### i18n

LSPの補完説明（detail/documentation）とインレイヒントはi18n対応する。言語設定はエンジンの起動オプションまたは設定ファイルで指定する。

```bash
lcvgc daemon --port 9876 --lang ja
lcvgc daemon --port 9876 --lang en
```

Rust側の実装イメージ：

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

### 補完・ヒントの文脈依存テーブル

以下の表で「補完」は選択肢として表示されるもの、「ヒント」はインレイヒントとしてグレーで表示される説明。

#### トップレベル

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| ファイル先頭 / ブロック外 | `device`, `instrument`, `kit`, `clip`, `scene`, `session`, `include`, `tempo`, `scale`, `play`, `stop` | — |

#### device

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `device ` の後 | (テキスト入力) | — |
| `device name {` 内 | `port` | — |
| `port ` の後 | (テキスト入力) | OSが認識するMIDIデバイス名 |

#### instrument

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `instrument ` の後 | (テキスト入力) | — |
| `instrument name {` 内 | `device`, `channel`, `note`, `gate_normal`, `gate_staccato` | — |
| `device ` の後 | (定義済みdevice名) | MIDIデバイス |
| `channel ` の後 | `1`..`16` | MIDIチャンネル (1-16) |
| `note ` の後 | `c0`..`b9` | 固定ノート (ドラム用) |
| `gate_normal ` の後 | (数値) | Gate比率 % (デフォルト80) |
| `gate_staccato ` の後 | (数値) | スタッカート時のGate比率 % (デフォルト40) |

#### kit

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `kit ` の後 | (テキスト入力) | — |
| `kit name {` 内 | `device`, (テキスト入力: 楽器名) | — |
| `device ` の後 | (定義済みdevice名) | キット全体のMIDIデバイス |
| `楽器名 ` の後 | `{` | — |
| `楽器名 {` 内 | `channel`, `note` | — |
| `channel ` の後 | `1`..`16` | MIDIチャンネル (1-16) |
| `note ` の後 | `c0`..`b9` | GMドラムマップ: C2=BD, D2=SD, F#2=HH... |
| `gate_normal ` の後 | (数値) | Gate比率 % (デフォルト80) |
| `gate_staccato ` の後 | (数値) | スタッカート時のGate比率 % (デフォルト40) |

#### clip (ヘッダ)

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `clip ` の後 | (テキスト入力) | — |
| `clip name ` の後 | `[`, `{` | — |
| `[` の後 | `bars`, `time`, `scale` | — |
| `[bars ` の後 | (数値) | 小節数 |
| `[time ` の後 | `3/4`, `4/4`, `5/4`, `6/8`, `7/8` | 拍子 |
| `[scale ` の後 | `c`, `c#`, `db`, ..., `b` | スケールのルート音 |
| `[scale C ` の後 | `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian` | スケールタイプ |

#### clip (ドラム記法)

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| clip `{` 直後 | `use`, `resolution`, (定義済みinstrument名) | — |
| `use ` の後 | (定義済みkit名) | ドラムキット |
| `resolution ` の後 | `4`, `8`, `16`, `32` | 1文字の音符解像度 |
| 楽器名の後 (ステップ入力) | `x`, `X`, `o`, `.`, `\|`, `(` | x=通常 X=アクセント o=ゴースト .=休符 |
| `)*` の後 | `1`..`9` | 繰り返し回数 |
| `>` の後 | `1`..`N` | 小節ジャンプ (1-bars) |

#### clip (音程楽器記法)

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| 楽器名の後 | `c`, `c#`, `db`, ..., `b`, `r`, `[`, `(`, `>`, コード名 | 音名 / 休符 / 和音 / 繰り返し / コード名 |
| 音名 or コード名の後 | `:` | オクターブ区切り |
| `:` の後（第2セクション） | `0`..`9` | オクターブ (0-9) |
| オクターブの後 | `:` | 音長区切り |
| `:` の後（第3セクション） | `1`, `2`, `4`, `4.`, `8`, `8.`, `16` | 1=全 2=半 4=四分 8=八分 16=十六分 |
| 音長の後 | `'`, `g` | '=スタッカート g=Gate比率直接指定 |
| `g` の後 | (数値) | Gate比率 % (1-100) |
| `)*` の後 | `1`..`9` | 繰り返し回数 |
| `[...]` 和音 / コード名の後 | `arp` | アルペジオ |
| `arp(` の後 | `up`, `down`, `updown`, `random` | アルペジオ方向 |
| `arp(up, ` の後 | `4`, `8`, `16` | 発音間隔の音符解像度 |
| scale指定済みclip内のコード位置 | ダイアトニックコード候補 | degree情報 (例: IVm7 - subdominant) |

#### scene

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `scene ` の後 | (テキスト入力) | — |
| `scene name {` 内 | (定義済みclip名), `tempo` | — |
| clip名の後 | `\|`, `1`..`9`, (改行) | \|=シャッフル 1-9=発音確率10-90% |
| `\|` の後 | (定義済みclip名) | シャッフル候補 |
| clip名 `*` の後 | `1`..`9` | シャッフル重み |
| `tempo ` の後 | (数値), `+`, `-` | BPM値 / +N,-Nで相対変化 |

#### session

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `session ` の後 | (テキスト入力) | — |
| `session name {` 内 | (定義済みscene名) | — |
| scene名の後 | `[` | — |
| `[` の後 | `repeat`, `loop` | — |
| `[repeat ` の後 | (数値) | 繰り返し回数 |

#### play / stop

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `play ` の後 | (定義済みscene名), `session` | 再生するsceneまたはsession |
| `play session ` の後 | (定義済みsession名) | — |
| `play name ` の後 | `[` | — |
| `play name [` の後 | `repeat`, `loop` | — |
| `play name [repeat ` の後 | (数値) | 繰り返し回数 |
| `stop ` の後 | (定義済みclip名) | 省略で全停止、指定でそのclipだけミュート |

#### scale (グローバル)

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `scale ` の後 | `c`, `c#`, `db`, ..., `b` | スケールのルート音 |
| `scale c ` の後 | `major`, `minor`, `harmonic_minor`, `melodic_minor`, `dorian`, `phrygian`, `lydian`, `mixolydian`, `locrian` | スケールタイプ |

グローバルの `scale` はclipの `[scale ...]` が未指定の場合に適用される。clipレベルの指定で上書き可能。

#### include / tempo

| カーソル位置 | 補完 | ヒント |
|-------------|------|--------|
| `include ` の後 | `"` | 相対パス (.cvgファイル) |
| `include "` の後 | (.cvgファイルパス補完) | — |
| `tempo ` の後 | (数値) | BPM |

---

## 12. Tree-sitterによるシンタックスハイライト

lcvgcのDSL用にTree-sitterのgrammarを作成し、構文に基づいた正確なハイライトを提供する。

### 12.1 grammarの構成

```
tree-sitter-cvg/
├── grammar.js           -- 文法定義
├── queries/
│   └── highlights.scm   -- ハイライトクエリ
├── src/
│   └── parser.c          -- grammar.jsから生成
└── package.json
```

Neovimへの登録：

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

### 12.2 ハイライトクエリ (highlights.scm)

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

## 13. カラースキーム

暗いターミナル背景・暗い環境（クラブ等）で視認性を確保する配色。Material Darker系をベースに、彩度を抑えつつ十分なコントラストを保つ。

### 13.1 配色一覧

| 要素 | ハイライトグループ | 色 | 意図 |
|------|-------------------|------|------|
| 予約語 (`device`, `clip` 等) | `@keyword.cvg` | `#C792EA` 薄紫 | 構造が見える程度に控えめ |
| 定義名 (`drums_a`, `tr808`) | `@type.definition.cvg` / `@function.definition.cvg` | `#FFCB6B` 明るい黄 | 定義は目立つ |
| 参照名 (scene内のclip名等) | `@function.cvg` / `@type.cvg` | `#82AAFF` 明るい青 | 定義と区別しつつ視認性確保 |
| 音名 (`c`, `eb`, `f#`) | `@constant.cvg` | `#F78C6C` 明るいオレンジ | 楽譜の主役、最も目を引く |
| 数値 (オクターブ, 音長) | `@number.cvg` | `#89DDFF` シアン | 音名の隣で読みやすい |
| ステップパターン (`x.oX`) | `@string.cvg` | `#C3E88D` 明るい緑 | パターンが一目でわかる |
| 確率行 (`..5...7.`) | `@number.special.cvg` | `#FF5370` 赤 | ステップパターン（緑）と明確にコントラスト |
| コード名 (`cm7`) | `@string.special.cvg` | `#FF9CAC` ピンク | 音名オレンジと区別 |
| アルペジオ方向 | `@constant.builtin.cvg` | `#80CBC4` 薄いシアン | 控えめだが見える |
| シャッフル `\|` | `@operator.cvg` | `#FFFFFF` 白太字 | 区切りとして明確に |
| 重み (`*3`) | `@number.weight.cvg` | `#FF5370` 赤 | 確率と同系統 |
| 小節ジャンプ (`>3`) | `@keyword.jump.cvg` | `#FFCB6B` 明るい黄太字 | 小節構造の目印として目立つ |
| テンポ変化 (`+5`) | `@number.special.cvg` | `#FF5370` 赤 | 変化する値は赤系 |
| play | `@keyword.play.cvg` | `#C3E88D` 緑太字 | 再生 = GO |
| stop | `@keyword.stop.cvg` | `#FF5370` 赤太字 | 停止 = STOP |
| loop / repeat | `@keyword.repeat.cvg` | `#C792EA` 薄紫イタリック | 予約語と同系統だがイタリックで区別 |
| 文字列 (ポート名) | `@string.path.cvg` | `#A5D6A7` 控えめな緑 | 目立たなくて良い |
| コメント | `@comment.cvg` | `#546E7A` 暗いグレーイタリック | 演奏中に邪魔にならない |
| 括弧 `[ ]` | `@punctuation.bracket.cvg` | `#89DDFF` シアン | 数値と同系統 |

### 13.2 実装

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

### 13.3 設計方針

- **ステップパターン（緑）と確率行（赤）**が上下で並ぶため、暗い中でも一目で区別できる
- **音名（オレンジ）**が最も目を引く色。暗い環境で楽譜として読む中心部分
- **play（緑太字）/ stop（赤太字）**はライブ中に最も重要な操作。信号機の色で直感的
- **小節ジャンプ（黄太字）**は小節構造の目印として目立たせる
- **コメント（暗いグレー）**は演奏中に視界の邪魔にならない
- 全体的に彩度を抑えつつコントラストを確保し、長時間の演奏でも目が疲れにくい

---

## 14. プラグイン構成

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
