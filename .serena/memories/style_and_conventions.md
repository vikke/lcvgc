# コードスタイル・規約

## Rust
- Edition: 2021
- フォーマッター: `cargo fmt` (rustfmt)
- リンター: `cargo clippy`
- エラーハンドリング: パニックはcatchし、再生状態を維持する方針
- DSLパーサーは独立してevalできるブロック単位で設計

## プロジェクト構成
- `src/` - Rust ソースコード
- `specs/` - DSL仕様書とプラグイン仕様書（日本語）
- `project.yml` - Serena設定

## ドキュメント
- 仕様書は日本語で記述
- README.md（英語）、README.jp.md（日本語）
