# アーキテクチャ概観（探査・新規ファイル配置の指針）

`lcvgc` は単一クレート内をモジュールで層化している。依存は **下位層へ一方向**
が原則。`deps.svg`（`make deps` で再生成）が依存図の唯一の正。

## 層構造（下＝基盤、上＝出口。矢印は「依存する」方向）

```
domain   ← 依存ゼロ。全層の共有語彙。これより下は無い。
  ↑
ast      ← domain のみ。DSL の構文木（データモデル）。
  ↑
parser   ← ast, domain。テキスト → ast。
midi     ← domain のみ。MIDI プリミティブ（message/note/cc/gate…）。
  ↑
engine   ← ast, parser, midi, domain。評価・再生のオーケストレータ。
  ↑
lsp      ← ast, engine, parser, midi, domain, (server)。補完/hover/診断/定義/symbol。
  ↑
server   ← lsp, engine, midi。デーモン本体。LSP リクエストの入口。

generator … 上記の層から独立。bin/lcvgc-gen が利用（外部フォーマット → DSL）。
            本番コードは他層へ非依存（engine/midi 参照は test のみ）。
```

## 鉄則（新規コード・参照を足すとき）

1. **新しい値型・列挙が複数層から共有されるなら `domain` に置く。** `domain`
   は他のどのモジュールも import してはならない（依存ゼロを維持）。
2. **依存は必ず上→下。** 下位層（例: `ast`）から上位層（`parser`/`engine`）を
   `use` したくなったら設計を疑う。共有したい型は `domain` か `ast` へ降ろす。
3. **`engine` がハブ。** 再生・評価ロジックは原則ここ。`engine` は ast/parser/midi
   を束ねてよいが、`lsp`/`server` を参照してはならない。
4. **LSP 機能の追加は `lsp/` 配下**、デーモンの I/O・protocol は `server/`。
5. 外部フォーマット変換（MIDI ファイル取り込み等）は `generator/` に閉じる
   （reader → Score(IR) → emitter の3層）。他層から呼ばない。

## 既知の例外・注意（図と実態の差）

- **`lsp ↔ server` は相互参照（循環）が残っている。** `server` は `lsp::*` を
  呼び、`lsp::analyzer` は `server::protocol::IncludeSource` を参照する。新規で
  この2層をまたぐ型を作るなら、共有型を別の場所へ切り出すことを検討する。
- **`ast` → `parser` の隠れエッジ。** `ast/clip_cc.rs` が
  `parser::cell_normalize::CellToken` をジェネリック引数で参照している。
  `cargo modules` はこの入れ子ジェネリックを追わないため **deps.svg には現れない**
  が、実際には ast が parser に薄く依存している。ast を純粋な leaf と見なさない。
- 直近のリファクタ（PR #109）で `ast↔parser` / `midi↔domain` 等の循環を
  `domain` 新設で解消済み。この方向性を壊さないこと。

## 探すときの早見表

| 探したいもの | 見る場所 |
|---|---|
| 音名/オクターブ/コード種別/MIDIチャンネル | `domain/` |
| DSL 構文木の定義 | `ast/`（`clip_*.rs` が中心） |
| テキスト構文の解釈 | `parser/`（ast と 1:1 のファイル構成） |
| MIDI メッセージ/ノート/CC/ゲート | `midi/` |
| 再生・評価・スケジューリング・状態 | `engine/`（evaluator/player/clock/scene_runner…） |
| 補完・hover・診断・定義ジャンプ | `lsp/` |
| デーモン I/O・LSP protocol ハンドラ | `server/` |
| 外部フォーマット取り込み | `generator/`（`lcvgc-gen` バイナリ） |

## 依存図の再生成

```
make deps
# cargo modules dependencies --lib -p lcvgc --no-externs --no-fns --no-sysroot | dot -Tsvg > deps.svg
```

## 構文・プロトコルを変更するとき（更新漏れチェック）

構文やデーモンのプロトコルを変えたら、仕様書（日本語版と英語版）も同じ PR で更新する。
更新漏れは `make doc-check`（`scripts/check-docs.sh`）が main との差分から検出する。

| 変更したもの | 一緒に更新するもの |
|---|---|
| `parser/` または `ast/` | `specs/lcvgc-dsl-spec.ja.md` と `specs/lcvgc-dsl-spec.md` |
| `server/protocol.rs` | `specs/lsp-in-out.ja.md` と `specs/lsp-in-out.md` |
| `specs/*.ja.md` | 対応する英語版 `specs/*.md`（逆も同じ） |

- commit 前の総合チェックは `make check`（fmt・lint・test・スクリプトのテスト・doc-check）。
- Claude Code で作業する場合は、`.claude/settings.json` の hook が `git commit` の前に
  `make doc-check` を実行し、失敗すると commit を止める。
- 仕様書の変更が不要な修正（内部のリファクタリングなど）は、`SKIP_DOC_CHECK=1` を付けるか、
  commit message に `[skip-doc-check]` を入れて素通りさせる。
- 機械的に判定できないもの（説明文が挙動と合っているか、tree-sitter-cvg・lcvgc.nvim への
  影響）は、レビューで確認する。
