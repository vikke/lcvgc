# コードベース構造

```
lcvgc/
├── Cargo.toml              # Rust パッケージ定義 (edition 2021)
├── Cargo.lock
├── src/
│   └── main.rs             # エントリポイント（現在はHello World）
├── specs/
│   ├── lcvgc-dsl-spec.md   # DSL仕様書（device, instrument, kit, clip, scene, session等）
│   └── lcvgc-nvim-plugin-spec.md  # Neovimプラグイン仕様書
├── README.md               # 英語README
├── README.jp.md            # 日本語README
├── project.yml             # Serena設定 (rust + markdown)
└── LICENSE
```

## 将来のコンポーネント
- lcvgc エンジン（デーモン、TCPサーバー、port 9876）
- lcvgc lsp（LSPサーバーサブコマンド）
- lcvgc-mic（別バイナリ、マイク入力→音名テキスト）
- lcvgc.nvim（Neovimプラグイン、Lua）
- tree-sitter-cvg（Tree-sitter grammar）
