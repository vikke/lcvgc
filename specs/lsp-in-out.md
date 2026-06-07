# lcvgc Daemon Protocol Specification

## 1. Overview

The lcvgc daemon provides DSL evaluation and LSP features over a TCP socket connection.
The Neovim plugin (lcvgc.nvim) sends JSON messages to the daemon to use features such as DSL evaluation, status queries, MIDI port listing, completion, hover information, and diagnostics.

This document defines the request and response message formats accepted and returned by the daemon.

---

## 2. Communication Method

| Item          | Details                                          |
|---------------|--------------------------------------------------|
| Protocol      | TCP                                              |
| Port          | `9876`                                           |
| Encoding      | UTF-8                                            |
| Format        | Line-delimited JSON (1 request = 1 line)         |
| Line terminator | `\n` (LF)                                     |

### Communication Flow

```
Client → Daemon : JSON request\n
Daemon → Client : JSON response\n
```

Each request is an independent transaction. No session state is retained between requests.

> **Exception (server-initiated push)**: Only on a connection that has
> subscribed to MIDI input via `subscribe_midi_in` (4.10), the daemon goes
> beyond the one-request-equals-one-response rule above and actively emits
> `midi_in_event` messages (Section 4.12) without a triggering request. This
> subscription is the only retained session state, scoped to that connection
> (released on disconnect or via `unsubscribe_midi_in`). Push messages arrive
> interleaved on the same connection as ordinary responses, as newline-delimited
> JSON, so the client distinguishes them by the top-level `type` field.

---

## 3. Common Response Structure

All responses share the following common structure. The fields used depend on the request type.

```json
{
  "success": true,
  "message": "<success message>",
  "error": "<error message>",
  "ports": [...],
  "lsp": {...}
}
```

| Field     | Type            | Description                                                  |
|-----------|-----------------|--------------------------------------------------------------|
| `success` | boolean         | Processing success flag                                      |
| `message` | string \| null  | Success message (used by eval / preload / status)            |
| `error`   | string \| null  | Error message (present only on failure)                      |
| `ports`   | array \| null   | MIDI port list (used by list_ports)                          |
| `lsp`     | object \| null  | LSP result (used by lsp_* requests)                          |

> **Note**: `message`, `error`, `ports`, and `lsp` are omitted from the response JSON when their value is `null`.

---

## 4. Request / Response Specifications

### 4.1 eval (DSL Source Evaluation)

Evaluates DSL source and executes MIDI message sending, etc. All blocks including `play` / `stop` are evaluated.

#### Request

```json
{"type": "eval", "source": "<DSL source text>"}
```

| Field    | Type   | Description                    |
|----------|--------|--------------------------------|
| `type`   | string | Fixed value `"eval"`           |
| `source` | string | Full DSL source text to evaluate |

#### Response (success)

```json
{"success": true, "message": "<string representation of evaluation results>"}
```

#### Response (error)

```json
{"success": false, "error": "<error message>"}
```

| Field     | Type    | Description                              |
|-----------|---------|------------------------------------------|
| `success` | boolean | Processing success flag                  |
| `message` | string  | Debug string of evaluation results       |
| `error`   | string  | Details of parse errors, etc.            |

---

### 4.2 preload (Preload Evaluation)

Evaluates DSL source excluding `play` / `stop` blocks. Used for registering definitions into the registry when a file is opened.

#### Request

```json
{"type": "preload", "source": "<DSL source text>"}
```

| Field    | Type   | Description                    |
|----------|--------|--------------------------------|
| `type`   | string | Fixed value `"preload"`        |
| `source` | string | Full DSL source text to evaluate |

#### Response (success)

```json
{"success": true, "message": "<string representation of evaluation results>"}
```

#### Response (error)

```json
{"success": false, "error": "<error message>"}
```

| Field     | Type    | Description                              |
|-----------|---------|------------------------------------------|
| `success` | boolean | Processing success flag                  |
| `message` | string  | Debug string of evaluation results       |
| `error`   | string  | Details of parse errors, etc.            |

---

### 4.3 status (Status Query)

Returns the current state of the daemon (BPM, playback state).

#### Request

```json
{"type": "status"}
```

| Field  | Type   | Description              |
|--------|--------|--------------------------|
| `type` | string | Fixed value `"status"`   |

#### Response

```json
{"success": true, "message": "BPM: 120.0, State: Idle"}
```

| Field     | Type    | Description                                          |
|-----------|---------|------------------------------------------------------|
| `success` | boolean | Processing success flag                              |
| `message` | string  | String in format `BPM: <value>, State: <state>`      |

---

### 4.4 list_ports (List MIDI Ports)

Returns a list of available MIDI input and output ports.

#### Request

```json
{"type": "list_ports"}
```

| Field  | Type   | Description                 |
|--------|--------|-----------------------------|
| `type` | string | Fixed value `"list_ports"`  |

#### Response (success)

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

#### Response (error)

```json
{"success": false, "error": "<error message>"}
```

| Field                | Type    | Description                                |
|----------------------|---------|--------------------------------------------|
| `success`            | boolean | Processing success flag                    |
| `ports`              | array   | Array of MIDI port information             |
| `ports[].name`       | string  | Port name                                  |
| `ports[].direction`  | string  | Port direction (`"in"` or `"out"`)         |

---

### 4.5 lsp_completion (Completion Candidates)

Returns a list of completion candidates at the cursor position.

#### Request

```json
{"type": "lsp_completion", "source": "<DSL source text>", "offset": <byte offset>, "include_sources": [{"path": "bass.cvg", "source": "clip bass {\n  c4\n}"}]}
```

| Field              | Type                      | Description                                      |
|--------------------|---------------------------|--------------------------------------------------|
| `type`             | string                    | Fixed value `"lsp_completion"`                   |
| `source`           | string                    | Full DSL source text                             |
| `offset`           | number                    | Byte offset of the cursor position (0-based)     |
| `include_sources`  | array \| null             | Include file source information (optional)       |
| `include_sources[].path`   | string           | Include file path                                |
| `include_sources[].source` | string           | Include file content                             |

#### Response

```json
{
  "success": true,
  "lsp": {
    "type": "completion",
    "items": [
      {"label": "note_on", "detail": "MIDI note-on keyword", "kind": "Keyword"},
      {"label": "C4",      "detail": "Note name",            "kind": "NoteName"}
    ]
  }
}
```

| Field               | Type    | Description                                       |
|--------------------|---------|---------------------------------------------------|
| `success`           | boolean | Processing success flag                           |
| `lsp.type`          | string  | Fixed value `"completion"`                        |
| `lsp.items`         | array   | Array of completion candidates                    |
| `lsp.items[].label` | string  | Label string of the completion candidate          |
| `lsp.items[].detail`| string  | Description of the completion candidate           |
| `lsp.items[].kind`  | string  | Completion kind (see `CompletionKind`)            |

---

### 4.6 lsp_hover (Hover Information)

Returns hover information (Markdown text) about the symbol at the cursor position.

#### Request

```json
{"type": "lsp_hover", "source": "<DSL source text>", "offset": <byte offset>, "include_sources": [...]}
```

| Field              | Type                      | Description                                      |
|--------------------|---------------------------|--------------------------------------------------|
| `type`             | string                    | Fixed value `"lsp_hover"`                        |
| `source`           | string                    | Full DSL source text                             |
| `offset`           | number                    | Byte offset of the cursor position (0-based)     |
| `include_sources`  | array \| null             | Include file source information (optional)       |

#### Response (with information)

```json
{
  "success": true,
  "lsp": {
    "type": "hover",
    "info": {"content": "**note_on** `channel pitch velocity`\n\nSends a MIDI note-on message."}
  }
}
```

#### Response (no information)

```json
{
  "success": true,
  "lsp": {
    "type": "hover",
    "info": null
  }
}
```

| Field              | Type           | Description                                    |
|-------------------|----------------|------------------------------------------------|
| `success`          | boolean        | Processing success flag                        |
| `lsp.type`         | string         | Fixed value `"hover"`                          |
| `lsp.info`         | object \| null | Hover information. `null` if no target found   |
| `lsp.info.content` | string         | Hover text in Markdown format                  |

---

### 4.7 lsp_diagnostics (Diagnostic Information)

Analyzes the entire source and returns a list of errors and warnings.

> **Note**: `include` statements are only allowed at the top of the file. An `include` appearing after a non-`include` block will be reported as an error.

> **Note**: Include file existence checks (`include_diagnostics`) are not performed on the daemon side; they are handled on the Lua (client) side.

#### Request

```json
{"type": "lsp_diagnostics", "source": "<DSL source text>", "include_sources": [{"path": "bass.cvg", "source": "clip bass {\n  c4\n}"}]}
```

| Field              | Type                      | Description                                                              |
|--------------------|---------------------------|--------------------------------------------------------------------------|
| `type`             | string                    | Fixed value `"lsp_diagnostics"`                                          |
| `source`           | string                    | Full DSL source text                                                     |
| `include_sources`  | array \| null             | Include file source information (optional). When provided, resolves definitions from includes |
| `include_sources[].path`   | string           | Include file path                                                        |
| `include_sources[].source` | string           | Include file content                                                     |

#### Response

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
        "message": "Undefined variable 'foo'",
        "severity": "Error"
      },
      {
        "start_line": 3,
        "start_col": 2,
        "end_line": 3,
        "end_col": 10,
        "message": "Deprecated syntax",
        "severity": "Warning"
      }
    ]
  }
}
```

| Field                    | Type    | Description                                               |
|-------------------------|---------|-----------------------------------------------------------|
| `success`                | boolean | Processing success flag                                   |
| `lsp.type`               | string  | Fixed value `"diagnostics"`                               |
| `lsp.items`              | array   | Array of diagnostic items (empty array if no issues)      |
| `lsp.items[].start_line` | number  | Start line of the issue (0-based)                         |
| `lsp.items[].start_col`  | number  | Start column of the issue (0-based, byte offset)          |
| `lsp.items[].end_line`   | number  | End line of the issue (0-based)                           |
| `lsp.items[].end_col`    | number  | End column of the issue (0-based, byte offset)            |
| `lsp.items[].message`    | string  | Diagnostic message                                        |
| `lsp.items[].severity`   | string  | Severity level (see `DiagnosticSeverity`)                 |

---

### 4.8 lsp_goto_definition (Go to Definition)

Returns the position where the symbol at the cursor position is defined.

#### Request

```json
{"type": "lsp_goto_definition", "source": "<DSL source text>", "offset": <byte offset>, "include_sources": [...]}
```

| Field              | Type                      | Description                                      |
|--------------------|---------------------------|--------------------------------------------------|
| `type`             | string                    | Fixed value `"lsp_goto_definition"`              |
| `source`           | string                    | Full DSL source text                             |
| `offset`           | number                    | Byte offset of the cursor position (0-based)     |
| `include_sources`  | array \| null             | Include file source information (optional)       |

#### Response (definition found)

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

#### Response (definition not found)

```json
{
  "success": true,
  "lsp": {
    "type": "goto_definition",
    "location": null
  }
}
```

| Field                     | Type           | Description                                        |
|--------------------------|----------------|---------------------------------------------------|
| `success`                 | boolean        | Processing success flag                            |
| `lsp.type`                | string         | Fixed value `"goto_definition"`                    |
| `lsp.location`            | object \| null | Definition location. `null` if not found           |
| `lsp.location.start_line` | number         | Start line of the definition (0-based)             |
| `lsp.location.start_col`  | number         | Start column of the definition (0-based, byte offset)|
| `lsp.location.end_line`   | number         | End line of the definition (0-based)               |
| `lsp.location.end_col`    | number         | End column of the definition (0-based, byte offset) |

---

### 4.9 lsp_document_symbols (Document Symbol List)

Returns a list of symbols (blocks) defined in the source.

#### Request

```json
{"type": "lsp_document_symbols", "source": "<DSL source text>", "include_sources": [...]}
```

| Field              | Type                      | Description                            |
|--------------------|---------------------------|----------------------------------------|
| `type`             | string                    | Fixed value `"lsp_document_symbols"`   |
| `source`           | string                    | Full DSL source text                   |
| `include_sources`  | array \| null             | Include file source information (optional) |

#### Response

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

| Field                    | Type    | Description                                               |
|-------------------------|---------|-----------------------------------------------------------|
| `success`                | boolean | Processing success flag                                   |
| `lsp.type`               | string  | Fixed value `"document_symbols"`                          |
| `lsp.items`              | array   | Array of symbol items                                     |
| `lsp.items[].name`       | string  | Symbol name                                               |
| `lsp.items[].kind`       | string  | Symbol kind (see `SymbolKind`)                            |
| `lsp.items[].start_line` | number  | Symbol start line (0-based)                               |
| `lsp.items[].start_col`  | number  | Symbol start column (0-based, byte offset)                |
| `lsp.items[].end_line`   | number  | Symbol end line (0-based)                                 |
| `lsp.items[].end_col`    | number  | Symbol end column (0-based, byte offset)                  |

---

### 4.10 subscribe_midi_in (Start MIDI Input Subscription)

Subscribes to the specified MIDI input port. Thereafter, on this connection,
played notes are received asynchronously as `midi_in_event` messages
(Section 4.12), each rendered as a DSL token. An immediate response reports
whether the subscription succeeded.

There is one subscription per connection. Sending `subscribe_midi_in` again
while subscribed switches the subscription to the new port. The subscription
continues until the connection closes or `unsubscribe_midi_in` (4.11) is sent.

#### Request

```json
{"type": "subscribe_midi_in", "port": "IAC Driver Bus 1"}
```

| Field    | Type   | Description                                                       |
|----------|--------|-------------------------------------------------------------------|
| `type`   | string | Fixed value `"subscribe_midi_in"`                                 |
| `port`   | string | Input port name to subscribe (one of `list_ports`' `direction:"in"` ports) |

#### Response (success)

```json
{"success": true, "message": "subscribed: IAC Driver Bus 1"}
```

#### Response (error)

```json
{"success": false, "error": "<error message>"}
```

Returns an error when the port is not found or the MIDI subsystem cannot be
opened. Even on error the connection is not torn down; subsequent requests are
processed normally.

---

### 4.11 unsubscribe_midi_in (Cancel MIDI Input Subscription)

Cancels this connection's MIDI input subscription. Returns success even when not
subscribed (idempotent).

#### Request

```json
{"type": "unsubscribe_midi_in"}
```

| Field  | Type   | Description                          |
|--------|--------|--------------------------------------|
| `type` | string | Fixed value `"unsubscribe_midi_in"`  |

#### Response

```json
{"success": true, "message": "unsubscribed"}
```

---

### 4.12 midi_in_event (Server-Initiated Push Message)

An event message that the daemon actively sends, without a triggering request,
on a connection subscribed via `subscribe_midi_in` (4.10). Each **note onset
(NoteOn with velocity > 0)** received on the MIDI input port is converted into a
DSL pitch token and reported one at a time.

> **Note**: This message is a server-initiated push, not a response. Unlike an
> ordinary response (which carries a `success` field), it carries a top-level
> `type` field. The client distinguishes the two by the presence of `type` on
> the received line.
>
> **Excluded**: NoteOff, NoteOn with velocity 0, Control Change, Program Change,
> and System Real-Time (Clock/Start/Stop/Continue) produce no DSL text and are
> therefore not emitted. SysEx, timing clock, and active sensing are ignored on
> the daemon side.

#### Message (Daemon → Client)

```json
{"type": "midi_in_event", "dsl": "c:4", "note": 60, "raw": [144, 60, 100]}
```

| Field   | Type          | Description                                                  |
|---------|---------------|-------------------------------------------------------------|
| `type`  | string        | Fixed value `"midi_in_event"`                               |
| `dsl`   | string        | DSL pitch token (`name:octave` form, e.g. `c:4` / `c#:4`); a self-contained notation that can be inserted directly into a clip body |
| `note`  | number        | MIDI note number (0-127)                                     |
| `raw`   | array(number) | Raw received MIDI bytes (for debugging/extension)           |

> **Octave convention**: `note` 60 = `c:4` (MIDI 60 = C4). Note names use
> lowercase + `#` notation, conforming to the pitch-token grammar
> (`instrument note[:octave][:duration]`). No duration or timing is attached
> (real-time, per-event conversion).

---

## 5. CompletionKind Values

String values representing the kind of a completion candidate.

| Value        | Description                                              |
|-------------|----------------------------------------------------------|
| `Keyword`    | DSL keyword (`note_on`, `cc`, etc.)                      |
| `NoteName`   | Note name (`C4`, `A#3`, etc.)                            |
| `ChordName`  | Chord name (`Cmaj`, `Dm7`, etc.)                         |
| `CcAlias`    | CC alias (`modwheel`, `volume`, etc.)                    |
| `Identifier` | User-defined identifier (variable name, block name, etc.) |

---

## 6. DiagnosticSeverity Values

String values representing the severity of a diagnostic item.

| Value     | Description                                             |
|----------|---------------------------------------------------------|
| `Error`   | Fatal error that prevents parsing or execution          |
| `Warning` | Advisory warning that does not affect playback          |

---

## 7. SymbolKind Values

String values representing the kind of a document symbol. Corresponds to DSL block types.

| Value        | Description                                         |
|-------------|-----------------------------------------------------|
| `Device`     | `device` block (MIDI device definition)             |
| `Instrument` | `instrument` block (sound/patch definition)         |
| `Kit`        | `kit` block (drum kit definition)                   |
| `Clip`       | `clip` block (musical phrase definition)            |
| `Scene`      | `scene` block (collection of clips)                 |
| `Session`    | `session` block (session definition)                |
| `Tempo`      | `tempo` block (tempo setting)                       |
| `Scale`      | `scale` block (scale setting)                       |
| `Variable`   | `var` block (variable definition)                   |
| `Include`    | `include` block (file include)                      |
| `Play`       | `play` block (playback instruction)                 |
| `Stop`       | `stop` block (stop instruction)                     |

---

## 8. Error Response

When the daemon fails to process a request, it returns the following error response.

```json
{"success": false, "error": "<error message>"}
```

| Field     | Type    | Description               |
|----------|---------|---------------------------|
| `success` | boolean | Fixed value `false`       |
| `error`   | string  | Detailed error message    |

### Situations That Can Cause an Error

- The `type` field contains an unknown value
- JSON parsing fails
- The `source` field is missing
- An unexpected exception occurs during internal processing
- MIDI port retrieval fails
