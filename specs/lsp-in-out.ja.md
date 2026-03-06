# LSP over Daemon プロトコル仕様書

## 1. 概要

lcvgc デーモンは TCP ソケット経由で LSP 機能を提供する。
Neovim プラグイン（lcvgc-lsp）はデーモンに JSON メッセージを送信し、補完・ホバー・診断などの
エディタ支援機能を受け取る。

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

## 3. 各リクエスト / レスポンス仕様

### 3.1 lsp_completion（補完候補取得）

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

### 3.2 lsp_hover（ホバー情報取得）

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

### 3.3 lsp_diagnostics（診断情報取得）

ソース全体を解析し、エラーや警告の一覧を返す。

> **注意**: `include` 文はファイル先頭にのみ記述可能です。非 `include` ブロックの後に `include` がある場合はエラーとして報告されます。

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

### 3.4 lsp_goto_definition（定義ジャンプ）

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

### 3.5 lsp_document_symbols（ドキュメントシンボル一覧取得）

ソース内に定義されているシンボル（ブロック）の一覧を返す。

#### リクエスト

```json
{"type": "lsp_document_symbols", "source": "<DSL ソーステキスト>", "include_sources": [...]}
```

| フィールド         | 型                        | 説明                               |
|-------------------|---------------------------|------------------------------------|
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

## 4. CompletionKind の値一覧

補完候補の種別を表す文字列。

| 値             | 説明                                      |
|---------------|------------------------------------------|
| `Keyword`     | DSL キーワード（`note_on`, `cc` 等）      |
| `NoteName`    | 音名（`C4`, `A#3` 等）                   |
| `ChordName`   | コード名（`Cmaj`, `Dm7` 等）              |
| `CcAlias`     | CC エイリアス（`modwheel`, `volume` 等）  |
| `Identifier`  | ユーザー定義識別子（変数名、ブロック名等）  |

---

## 5. DiagnosticSeverity の値一覧

診断項目の重大度を表す文字列。

| 値        | 説明                                     |
|----------|------------------------------------------|
| `Error`   | 解析または実行を阻止する致命的エラー     |
| `Warning` | 動作に影響しないが注意が必要な警告       |

---

## 6. SymbolKind の値一覧

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

## 7. エラーレスポンス

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
