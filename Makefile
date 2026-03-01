# lcvgc Makefile
#
# ターゲット:
#   Linux (ネイティブ): lcvgc daemon + LSP
#   Windows (クロスコンパイル): lcvgc.exe
#
# 前提条件 (Windows クロスコンパイル):
#   cargo install cross --git https://github.com/cross-rs/cross
#   Docker Desktop WSL Integration を有効化

CARGO        := cargo
CROSS        := cross
VERSION      := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
RELEASE_DIR  := target/release
WIN_TARGET   := x86_64-pc-windows-gnu
WIN_DIR      := target/$(WIN_TARGET)/release
DIST_DIR     := dist

.PHONY: all build build-win test test-all bench lint fmt clean \
        install setup-win dist help

# デフォルト: Linux ネイティブビルド
all: build

## ── ビルド ──────────────────────────────────────────

# Linux ネイティブ
build:
	$(CARGO) build --release
	@echo "✓ $(RELEASE_DIR)/lcvgc"

# Windows クロスコンパイル (cross + Docker)
build-win:
	$(CROSS) build --release --target $(WIN_TARGET)
	@echo "✓ $(WIN_DIR)/lcvgc.exe"

# 両プラットフォーム
build-all: build build-win

## ── テスト ──────────────────────────────────────────

# ユニットテスト + 統合テスト
test:
	$(CARGO) test

# 統合テストのみ
test-integration:
	$(CARGO) test --test integration

# 全テスト (警告チェック含む)
test-all: lint test

## ── ベンチマーク ────────────────────────────────────

bench:
	$(CARGO) bench --bench parser_bench

## ── コード品質 ──────────────────────────────────────

# 警告ゼロ確認
lint:
	$(CARGO) build --release 2>&1 | grep -q "warning" && exit 1 || true
	$(CARGO) clippy --all-targets -- -D warnings

# フォーマット
fmt:
	$(CARGO) fmt

# フォーマットチェック
fmt-check:
	$(CARGO) fmt -- --check

## ── 配布 ────────────────────────────────────────────

# dist/ に成果物を集約
dist: build build-win
	@mkdir -p $(DIST_DIR)/linux $(DIST_DIR)/windows
	cp $(RELEASE_DIR)/lcvgc $(DIST_DIR)/linux/
	cp $(WIN_DIR)/lcvgc.exe $(DIST_DIR)/windows/
	@echo "✓ dist/ に配布ファイルを作成しました"

## ── セットアップ ────────────────────────────────────

# Windows クロスコンパイル環境セットアップ
setup-win:
	cargo install cross --git https://github.com/cross-rs/cross
	rustup target add $(WIN_TARGET)
	@echo ""
	@echo "✓ cross インストール完了"
	@echo "  Docker Desktop の WSL Integration が有効であることを確認してください"
	@echo "  Docker Desktop → Settings → Resources → WSL Integration → Ubuntu"

# Tree-sitter 文法生成
tree-sitter-generate:
	cd tree-sitter-lcvgc && npx tree-sitter generate

## ── LSP ─────────────────────────────────────────────

# LSP サーバー起動 (開発用)
lsp:
	$(CARGO) run --release -- lsp

## ── サーバー ────────────────────────────────────────

# daemon 起動 (開発用)
run:
	$(CARGO) run --release

# daemon 起動 + ホットリロード
run-watch:
	$(CARGO) run --release -- --watch .

## ── クリーンアップ ──────────────────────────────────

clean:
	$(CARGO) clean
	rm -rf $(DIST_DIR)

## ── ヘルプ ──────────────────────────────────────────

help:
	@echo "lcvgc v$(VERSION) - Makefile ターゲット一覧"
	@echo ""
	@echo "ビルド:"
	@echo "  make build       Linux ネイティブビルド"
	@echo "  make build-win   Windows クロスコンパイル (cross + Docker)"
	@echo "  make build-all   両プラットフォーム"
	@echo ""
	@echo "テスト:"
	@echo "  make test        全テスト実行"
	@echo "  make test-all    lint + テスト"
	@echo "  make bench       ベンチマーク実行"
	@echo ""
	@echo "品質:"
	@echo "  make lint        警告・clippy チェック"
	@echo "  make fmt         コードフォーマット"
	@echo ""
	@echo "配布:"
	@echo "  make dist        dist/ に成果物集約"
	@echo ""
	@echo "実行:"
	@echo "  make run          daemon 起動"
	@echo "  make run-watch    daemon + ホットリロード"
	@echo "  make lsp          LSP サーバー起動"
	@echo ""
	@echo "セットアップ:"
	@echo "  make setup-win   cross + Docker 環境構築"
	@echo "  make clean       ビルド成果物削除"
