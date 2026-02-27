# 開発コマンド

## ビルド・実行
```bash
cargo build                    # ビルド
cargo run                      # 実行
cargo build --release          # リリースビルド
```

## テスト
```bash
cargo test                     # 全テスト実行
cargo test <test_name>         # 特定テスト実行
```

## コード品質
```bash
cargo fmt                      # フォーマット
cargo clippy                   # リント
cargo check                    # 型チェック（ビルドより速い）
```

## エンジン起動（将来）
```bash
lcvgc daemon --port 9876 --log /tmp/lcvgc.log
lcvgc lsp                     # LSPサーバーモード
```

## システムユーティリティ
```bash
git status / git diff / git log
ls / find / grep
```
