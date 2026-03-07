# lcvgc デーモン プロトコル仕様書

## 1. 概要

lcvgc デーモンは TCP ソケット経由で DSL 評価および LSP 機能を提供する。
Neovim プラグイン（lcvgc.nvim）はデーモンに JSON メッセージを送信し、DSL の評価・ステータス確認・MIDIポート取得・補完・ホバー・診断などの機能を利用する。

本仕様書は、デーモンが受け付けるリクエスト電文と、返却するレスポンス電文のフォーマットを定義する。

---

## 2. 通信方式

| 項目         | 内容                                     |
|--------------|------------------------------------------|
| プロトコル   | TCP                                      |
| ポート番号   | `9876`                                   |
| エンコーディング | UTF-8                                |
| フォーマット | 改行区切り JSON（1 リクエスト = 1 行）   |
| 改行文字     | `\n`（LF）                               |

### 通信フロー

```
クライアント → デーモン : JSON リクエスト\n
デーモン → クライアント : JSON レスポンス\n
```

各リクエストは独立したトランザクションであり、セッション状態は保持されない。

---

## 3. 共通レスポンス構造

すべてのレスポンスは以下の共通構造を持つ。リクエスト種別に応じて、使用されるフィールドが異なる。

```json
{
  "success": true,
  "message": "<成功メッセージ>",
  "error": "<エラーメッセージ>",
  "ports": [...],
  "lsp": {...}
}
```

| フィールド | 型              | 説明                                           |
|-----------|-----------------|------------------------------------------------|
| `success` | boolean         | 処理成功フラグ                                 |
| `message` | string \| null  | 成功時のメッセージ（eval / preload / status で使用） |
| `error`   | string \| null  | エラー時のメッセージ（失敗時のみ存在）         |
| `ports`   | array \| null   | MIDIポート一覧（list_ports で使用）            |
| `lsp`     | object \| null  | LSP 結果（lsp_* リクエストで使用）             |

> **注意**: `message`, `error`, `ports`, `lsp` は値が `null` の場合、レスポンス JSON から省略される。

---

## 4. 各リクエスト / レスポンス仕様

### 4.1 eval（DSL ソース評価）

DSL ソースを評価し、MIDI メッセージの送信等を実行する。`play` / `stop` ブロックも含めて全て評価する。

#### リクエスト

```json
{"type": "eval", "source": "<DSL ソーステキスト>"}
```

| フィールド | 型     | 説明                           |
|-----------|--------|--------------------------------|
| `type`    | string | 固定値 `"eval"`                |
| `source`  | string | 評価する DSL ソーステキスト全文 |

#### レスポンス（成功）

```json
{"success": true, "message": "<評価結果の文字列表現>"}
```

#### レスポンス（エラー）

```json
{"success": false, "error": "<エラーメッセージ>"}
```

| フィールド | 型      | 説明                     |
|-----------|---------|--------------------------|
| `success` | boolean | 処理成功フラグ           |
| `message` | string  | 評価結果のデバッグ文字列 |
| `error`   | string  | パースエラー等の詳細     |

---

### 4.2 preload（プリロード評価）

DSL ソースを `play` / `stop` ブロックを除外して評価する。ファイルオープン時にレジストリへ定義を登録する用途で使用する。

#### リクエスト

```json
{"type": "preload", "source": "<DSL ソーステキスト>"}
```

| フィールド | 型     | 説明                           |
|-----------|--------|--------------------------------|
| `type`    | string | 固定値 `"preload"`             |
| `source`  | string | 評価する DSL ソーステキスト全文 |

#### レスポンス（成功）

```json
{"success": true, "message": "<評価結果の文字列表現>"}
```

#### レスポンス（エラー）

```json
{"success": false, "error": "<エラーメッセージ>"}
```

| フィールド | 型      | 説明                     |
|-----------|---------|--------------------------|
| `success` | boolean | 処理成功フラグ           |
| `message` | string  | 評価結果のデバッグ文字列 |
| `error`   | string  | パースエラー等の詳細     |

---

### 4.3 status（ステータス問い合わせ）

デーモンの現在の状態（BPM、再生状態）を返す。

#### リクエスト

```json
{"type": "status"}
```

| フィールド | 型     | 説明                 |
|-----------|--------|----------------------|
| `type`    | string | 固定値 `"status"`    |

#### レスポンス

```json
{"success": true, "message": "BPM: 120.0, State: Idle"}
```

| フィールド | 型      | 説明                                     |
|-----------|---------|------------------------------------------|
| `success` | boolean | 処理成功フラグ                           |
| `message` | string  | `BPM: <値>, State: <状態>` 形式の文字列 |

---

### 4.4 list_ports（MIDI ポート一覧取得）

利用可能な MIDI 入出力ポートの一覧を返す。

#### リクエスト

```json
{"type": "list_ports"}
```

| フィールド | 型     | 説明                    |
|-----------|--------|-------------------------|
| `type`    | string | 固定値 `"list_ports"`   |

#### レスポンス（成功）

```json
{
  "success": true,
  "ports": [
    {"name": "IAC Driver Bus 1", "direction": "out"},
    {"name": "USB MIDI Interface", "direction": "out"},
    {"name": "IAC Driver Bus 1", "direction": "in"}
  ]
}
```

#### レスポンス（エラー）

```json
{"success": false, "error": "<エラーメッセージ>"}
```

| フィールド              | 型      | 説明                                    |
|------------------------|---------|----------------------------------------|
| `success`              | boolean | 処理成功フラグ                         |
| `ports`                | array   | MIDIポート情報の配列                   |
| `ports[].name`         | string  | ポート名                               |
| `ports[].direction`    | string  | ポート方向（`"in"` または `"out"`）    |

---

### 4.5 lsp_completion（補完候補取得）

カーソル位置における補完候補の一覧を返す。

#### リクエスト

```json
{"type": "lsp_completion", "source": "<DSL ソーステキスト>", "offset": <バイトオフセット>, "include_sources": [{"path": "bass.cvg", "source": "clip bass {\n  c4\n}"}]}
```

| フィールド         | 型                        | 説明                                   |
|-------------------|---------------------------|----------------------------------------|
| `type`            | string                    | 固定値 `"lsp_completion"`              |
| `source`          | string                    | DSL ソーステキスト全文                 |
| `offset`          | number                    | カーソル位置のバイトオフセット（0 始まり） |
| `include_sources` | array \| null             | インクルードファイルのソース情報（省略可） |
| `include_sources[].path`   | string           | インクルードファイルのパス             |
| `include_sources[].source` | string           | インクルードファイルの内容             |

#### レスポンス

```json
{
  "success": true,
  "lsp": {
    "type": "completion",
    "items": [
      {"label": "note_on", "detail": "MIDIノートオンキーワード", "kind": "Keyword"},
      {"label": "C4",      "detail": "音名",                     "kind": "NoteName"}
    ]
  }
}
```

| フィールド          | 型      | 説明                                    |
|--------------------|---------|----------------------------------------|
| `success`          | boolean | 処理成功フラグ                         |
| `lsp.type`         | string  | 固定値 `"completion"`                  |
| `lsp.items`        | array   | 補完候補の配列                         |
| `lsp.items[].label`| string  | 補完候補のラベル文字列                 |
| `lsp.items[].detail`| string | 補完候補の説明文                       |
| `lsp.items[].kind` | string  | 補完種別（`CompletionKind` 参照）      |

---

### 4.6 lsp_hover（ホバー情報取得）

カーソル位置のシンボルに関するホバー情報（Markdown テキスト）を返す。

#### リクエスト

```json
{"type": "lsp_hover", "source": "<DSL ソーステキスト>", "offset": <バイトオフセット>, "include_sources": [...]}
```

| フィールド         | 型                        | 説明                                       |
|-------------------|---------------------------|------------------------------------------|
| `type`            | string                    | 固定値 `"lsp_hover"`                      |
| `source`          | string                    | DSL ソーステキスト全文                    |
| `offset`          | number                    | カーソル位置のバイトオフセット（0 始まり）  |
| `include_sources` | array \| null             | インクルードファイルのソース情報（省略可） |

#### レスポンス（情報あり）

```json
{
  "success": true,
  "lsp": {
    "type": "hover",
    "info": {"content": "**note_on** `channel pitch velocity`\n\nMIDI ノートオンメッセージを送信する。"}
  }
}
```

#### レスポンス（情報なし）

```json
{
  "success": true,
  "lsp": {
    "type": "hover",
    "info": null
  }
}
```

| フィールド           | 型            | 説明                              |
|---------------------|---------------|----------------------------------|
| `success`           | boolean       | 処理成功フラグ                    |
| `lsp.type`          | string        | 固定値 `"hover"`                  |
| `lsp.info`          | object \| null | ホバー情報。対象外の場合は `null`  |
| `lsp.info.content`  | string        | Markdown 形式のホバーテキスト     |

---

### 4.7 lsp_diagnostics（診断情報取得）

ソース全体を解析し、エラーや警告の一覧を返す。

> **注意**: `include` 文はファイル先頭にのみ記述可能です。非 `include` ブロックの後に `include` がある場合はエラーとして報告されます。

> **注意**: インクルードファイルの存在チェック（`include_diagnostics`）はデーモン側では実施せず、Lua（クライアント）側で行います。

#### リクエスト

```json
{"type": "lsp_diagnostics", "source": "<DSL ソーステキスト>", "include_sources": [{"path": "bass.cvg", "source": "clip bass {\n  c4\n}"}]}
```

| フィールド         | 型                        | 説明                                                         |
|-------------------|---------------------------|--------------------------------------------------------------|
| `type`            | string                    | 固定値 `"lsp_diagnostics"`                                   |
| `source`          | string                    | DSL ソーステキスト全文                                       |
| `include_sources` | array \| null             | インクルードファイルのソース情報（省略可）。指定時はinclude先の定義も解決する |
| `include_sources[].path`   | string           | インクルードファイルのパス                                   |
| `include_sources[].source` | string           | インクルードファイルの内容                                   |

#### レスポンス

```json
{
  "success": true,
  "lsp": {
    "type": "diagnostics",
    "items": [
      {
        "start_line": 0,
        "start_col": 0,
        "end_line": 0,
        "end_col": 5,
        "message": "未定義の変数 'foo'",
        "severity": "Error"
      },
      {
        "start_line": 3,
        "start_col": 2,
        "end_line": 3,
        "end_col": 10,
        "message": "非推奨の構文",
        "severity": "Warning"
      }
    ]
  }
}
```

| フィールド                  | 型      | 説明                                       |
|----------------------------|---------|-------------------------------------------|
| `success`                  | boolean | 処理成功フラグ                             |
| `lsp.type`                 | string  | 固定値 `"diagnostics"`                    |
| `lsp.items`                | array   | 診断項目の配列（問題なしの場合は空配列）    |
| `lsp.items[].start_line`   | number  | 問題箇所の開始行番号（0 始まり）           |
| `lsp.items[].start_col`    | number  | 問題箇所の開始列番号（0 始まり、バイト単位）|
| `lsp.items[].end_line`     | number  | 問題箇所の終了行番号（0 始まり）           |
| `lsp.items[].end_col`      | number  | 問題箇所の終了列番号（0 始まり、バイト単位）|
| `lsp.items[].message`      | string  | 診断メッセージ                             |
| `lsp.items[].severity`     | string  | 重大度（`DiagnosticSeverity` 参照）        |

---

### 4.8 lsp_goto_definition（定義ジャンプ）

カーソル位置のシンボルが定義されている位置を返す。

#### リクエスト

```json
{"type": "lsp_goto_definition", "source": "<DSL ソーステキスト>", "offset": <バイトオフセット>, "include_sources": [...]}
```

| フィールド         | 型                        | 説明                                       |
|-------------------|---------------------------|------------------------------------------|
| `type`            | string                    | 固定値 `"lsp_goto_definition"`            |
| `source`          | string                    | DSL ソーステキスト全文                    |
| `offset`          | number                    | カーソル位置のバイトオフセット（0 始まり）  |
| `include_sources` | array \| null             | インクルードファイルのソース情報（省略可） |

#### レスポンス（定義が見つかった場合）

```json
{
  "success": true,
  "lsp": {
    "type": "goto_definition",
    "location": {
      "start_line": 0,
      "start_col": 0,
      "end_line": 0,
      "end_col": 5
    }
  }
}
```

#### レスポンス（定義が見つからない場合）

```json
{
  "success": true,
  "lsp": {
    "type": "goto_definition",
    "location": null
  }
}
```

| フィールド                | 型            | 説明                                       |
|--------------------------|---------------|-------------------------------------------|
| `success`                | boolean       | 処理成功フラグ                             |
| `lsp.type`               | string        | 固定値 `"goto_definition"`                |
| `lsp.location`           | object \| null | 定義位置。見つからない場合は `null`         |
| `lsp.location.start_line`| number        | 定義開始行番号（0 始まり）                 |
| `lsp.location.start_col` | number        | 定義開始列番号（0 始まり、バイト単位）     |
| `lsp.location.end_line`  | number        | 定義終了行番号（0 始まり）                 |
| `lsp.location.end_col`   | number        | 定義終了列番号（0 始まり、バイト単位）     |

---

### 4.9 lsp_document_symbols（ドキュメントシンボル一覧取得）

ソース内に定義されているシンボル（ブロック）の一覧を返す。

#### リクエスト

```json
{"type": "lsp_document_symbols", "source": "<DSL ソーステキスト>", "include_sources": [...]}
```

| フィールド         | 型                        | 説明                               |
|-------------------|---------------------------|-------------------------------------|
| `type`            | string                    | 固定値 `"lsp_document_symbols"`    |
| `source`          | string                    | DSL ソーステキスト全文             |
| `include_sources` | array \| null             | インクルードファイルのソース情報（省略可） |

#### レスポンス

```json
{
  "success": true,
  "lsp": {
    "type": "document_symbols",
    "items": [
      {
        "name": "my_clip",
        "kind": "Clip",
        "start_line": 0,
        "start_col": 0,
        "end_line": 5,
        "end_col": 1
      },
      {
        "name": "main_scene",
        "kind": "Scene",
        "start_line": 7,
        "start_col": 0,
        "end_line": 12,
        "end_col": 1
      }
    ]
  }
}
```

| フィールド                 | 型      | 説明                                       |
|---------------------------|---------|-------------------------------------------|
| `success`                 | boolean | 処理成功フラグ                             |
| `lsp.type`                | string  | 固定値 `"document_symbols"`               |
| `lsp.items`               | array   | シンボル項目の配列                         |
| `lsp.items[].name`        | string  | シンボル名                                 |
| `lsp.items[].kind`        | string  | シンボル種別（`SymbolKind` 参照）          |
| `lsp.items[].start_line`  | number  | シンボル開始行番号（0 始まり）             |
| `lsp.items[].start_col`   | number  | シンボル開始列番号（0 始まり、バイト単位）  |
| `lsp.items[].end_line`    | number  | シンボル終了行番号（0 始まり）             |
| `lsp.items[].end_col`     | number  | シンボル終了列番号（0 始まり、バイト単位）  |

---

## 5. CompletionKind の値一覧

補完候補の種別を表す文字列。

| 値             | 説明                                      |
|---------------|------------------------------------------|
| `Keyword`     | DSL キーワード（`note_on`, `cc` 等）      |
| `NoteName`    | 音名（`C4`, `A#3` 等）                   |
| `ChordName`   | コード名（`Cmaj`, `Dm7` 等）              |
| `CcAlias`     | CC エイリアス（`modwheel`, `volume` 等）  |
| `Identifier`  | ユーザー定義識別子（変数名、ブロック名等）  |

---

## 6. DiagnosticSeverity の値一覧

診断項目の重大度を表す文字列。

| 値        | 説明                                     |
|----------|------------------------------------------|
| `Error`   | 解析または実行を阻止する致命的エラー     |
| `Warning` | 動作に影響しないが注意が必要な警告       |

---

## 7. SymbolKind の値一覧

ドキュメントシンボルの種別を表す文字列。DSL ブロック種別に対応する。

| 値           | 説明                                |
|-------------|-------------------------------------|
| `Device`    | `device` ブロック（MIDIデバイス定義）  |
| `Instrument`| `instrument` ブロック（音色定義）      |
| `Kit`       | `kit` ブロック（ドラムキット定義）     |
| `Clip`      | `clip` ブロック（音楽フレーズ定義）    |
| `Scene`     | `scene` ブロック（クリップ集合定義）   |
| `Session`   | `session` ブロック（セッション定義）   |
| `Tempo`     | `tempo` ブロック（テンポ設定）        |
| `Scale`     | `scale` ブロック（スケール設定）       |
| `Variable`  | `var` ブロック（変数定義）            |
| `Include`   | `include` ブロック（ファイルインクルード）|
| `Play`      | `play` ブロック（再生指示）           |
| `Stop`      | `stop` ブロック（停止指示）           |

---

## 8. エラーレスポンス

デーモンが処理に失敗した場合、以下のエラーレスポンスを返す。

```json
{"success": false, "error": "<エラーメッセージ>"}
```

| フィールド | 型      | 説明                   |
|-----------|---------|------------------------|
| `success` | boolean | 固定値 `false`         |
| `error`   | string  | エラーの詳細メッセージ  |

### エラーが発生しうる状況

- `type` フィールドが不明な値の場合
- JSON のパースに失敗した場合
- `source` フィールドが存在しない場合
- 内部処理で予期しない例外が発生した場合
- MIDI ポートの取得に失敗した場合
