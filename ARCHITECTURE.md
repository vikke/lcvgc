# Architecture Overview (Guide for Exploration and New File Placement)

`lcvgc` layers its modules within a single crate. The rule is that dependencies
flow **one-way, toward the lower layers**. `deps.svg` (regenerated with
`make deps`) is the single source of truth for the dependency graph.

## Layer Structure (bottom = foundation, top = exit; arrows point in the "depends on" direction)

```
domain   ← Zero dependencies. Shared vocabulary for every layer. Nothing sits below it.
  ↑
ast      ← domain only. The DSL syntax tree (data model).
  ↑
parser   ← ast, domain. Text → ast.
midi     ← domain only. MIDI primitives (message/note/cc/gate…).
  ↑
engine   ← ast, parser, midi, domain. Orchestrator for evaluation and playback.
  ↑
lsp      ← ast, engine, parser, midi, domain, (server). completion/hover/diagnostics/definition/symbol.
  ↑
server   ← lsp, engine, midi. The daemon itself. Entry point for LSP requests.

generator … Independent from the layers above. Used by bin/lcvgc-gen (external format → DSL).
            Production code does not depend on other layers (engine/midi references are test-only).
```

## Iron Rules (when adding new code or references)

1. **If a new value type or enum is shared by multiple layers, put it in `domain`.**
   `domain` must not import any other module (it stays dependency-free).
2. **Dependencies must always flow top → bottom.** If you find yourself wanting to
   `use` an upper layer (`parser`/`engine`) from a lower layer (e.g. `ast`), question
   the design. Push the type you want to share down into `domain` or `ast`.
3. **`engine` is the hub.** Playback and evaluation logic belongs here as a rule.
   `engine` may pull together ast/parser/midi, but must not reference `lsp`/`server`.
4. **Add LSP features under `lsp/`**, and put daemon I/O and protocol under `server/`.
5. Keep external-format conversion (importing MIDI files, etc.) contained within
   `generator/` (the three-stage reader → Score(IR) → emitter). Do not call it from other layers.

## Known Exceptions and Caveats (where the diagram and reality diverge)

- **A mutual reference (cycle) remains between `lsp ↔ server`.** `server` calls
  `lsp::*`, and `lsp::analyzer` references `server::protocol::IncludeSource`. If you
  create a new type that spans these two layers, consider carving the shared type out
  into a separate location.
- **A hidden `ast` → `parser` edge.** `ast/clip_cc.rs` references
  `parser::cell_normalize::CellToken` as a generic argument. `cargo modules` does not
  follow this nested generic, so it **does not appear in deps.svg**, but in reality
  ast has a thin dependency on parser. Do not treat ast as a pure leaf.
- The most recent refactor (PR #109) resolved cycles such as `ast↔parser` and
  `midi↔domain` by introducing `domain`. Do not break this direction.

## Quick Reference for Searching

| What you're looking for | Where to look |
|---|---|
| Note name/octave/chord type/MIDI channel | `domain/` |
| DSL syntax tree definitions | `ast/` (centered on `clip_*.rs`) |
| Interpreting text syntax | `parser/` (file layout maps 1:1 to ast) |
| MIDI message/note/CC/gate | `midi/` |
| Playback/evaluation/scheduling/state | `engine/` (evaluator/player/clock/scene_runner…) |
| Completion/hover/diagnostics/go-to-definition | `lsp/` |
| Daemon I/O and LSP protocol handlers | `server/` |
| External-format import | `generator/` (the `lcvgc-gen` binary) |

## Regenerating the Dependency Graph

```
make deps
# cargo modules dependencies --lib -p lcvgc --no-externs --no-fns --no-sysroot | dot -Tsvg > deps.svg
```
