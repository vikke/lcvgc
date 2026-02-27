# lcvgc DSLパーサー実装 TODO

## Phase 1: 基盤（逐次）
- [ ] Cargo.toml に nom, thiserror, pretty_assertions 追加
- [ ] src/error.rs — ParseError定義
- [ ] src/ast/mod.rs — Block enum + モジュール宣言
- [ ] src/ast/common.rs — NoteName, Octave, Duration, GateSpec
- [ ] src/parser/mod.rs — モジュール宣言
- [ ] src/parser/common.rs — ws, comment, ident, note_name, octave, duration パーサー
- [ ] src/lib.rs — pub mod宣言
- [ ] cargo test 通過確認

## Phase 2: 単純ブロック（6並列 agent）
- [ ] Agent A: device (ast/device.rs + parser/device.rs)
- [ ] Agent B: instrument (ast/instrument.rs + parser/instrument.rs)
- [ ] Agent C: kit (ast/kit.rs + parser/kit.rs)
- [ ] Agent D: tempo + scale (ast/tempo.rs + ast/scale.rs + parser/tempo.rs + parser/scale.rs)
- [ ] Agent E: var + include (ast/var.rs + ast/include.rs + parser/var.rs + parser/include.rs)
- [ ] Agent F: play + stop (ast/playback.rs + parser/playback.rs)
- [ ] Phase 2 統合: 全worktreeマージ + cargo test

## Phase 3: Clip内部パーサー（8並列 agent）
- [ ] Agent G: clip_options (parser/clip_options.rs)
- [ ] Agent H: clip_note — 単音 + コード名 (ast/clip_note.rs + parser/clip_note.rs)
- [ ] Agent I: clip_arpeggio (parser/clip_arpeggio.rs)
- [ ] Agent J: clip_articulation (parser/clip_articulation.rs)
- [ ] Agent K: clip_shorthand — CarryOverState (parser/clip_shorthand.rs)
- [ ] Agent L: clip_repetition + clip_bar_jump (parser/clip_repetition.rs + parser/clip_bar_jump.rs)
- [ ] Agent M: clip_drum — ステップシーケンサー (ast/clip_drum.rs + parser/clip_drum.rs)
- [ ] Agent N: clip_cc — CCオートメーション (ast/clip_cc.rs + parser/clip_cc.rs)
- [ ] Phase 3 統合: 全worktreeマージ + cargo test

## Phase 4: Clip統合（逐次）
- [ ] ast/clip.rs — ClipDef, ClipOptions, ClipBody
- [ ] parser/clip.rs — clip orchestrator (pitched/drum判定、サブパーサー統合)
- [ ] cargo test 通過確認

## Phase 5: Scene + Session（2並列 agent）
- [ ] Agent O: scene (ast/scene.rs + parser/scene.rs)
- [ ] Agent P: session (ast/session.rs + parser/session.rs)
- [ ] Phase 5 統合: マージ + cargo test

## Phase 6: トップレベル + 統合テスト（逐次）
- [ ] parser/mod.rs — parse_block dispatcher
- [ ] tests/ — 仕様書の例を使った統合テスト
- [ ] cargo test + cargo clippy 警告ゼロ確認
