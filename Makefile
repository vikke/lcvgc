# lcvgc Makefile
#
# ビルド環境 = 実行環境（ネイティブビルドのみ）

CARGO        := cargo
VERSION      := $(shell grep '^version' crates/lcvgc/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
RELEASE_DIR  := target/release

.PHONY: all build test test-integration test-all bench lint fmt fmt-check \
        clean install lsp run run-watch help

# デフォルト: ネイティブビルド
all: build

## ── ビルド ──────────────────────────────────────────

build:
	$(CARGO) build --release
	@echo "✓ $(RELEASE_DIR)/lcvgc"
	@echo "✓ $(RELEASE_DIR)/lcvgc-lsp"

## ── テスト ──────────────────────────────────────────

test:
	$(CARGO) test --workspace

test-integration:
	$(CARGO) test --package lcvgc-core --test integration

# lint + テスト
test-all: lint test

## ── ベンチマーク ────────────────────────────────────

bench:
	$(CARGO) bench --package lcvgc-core --bench parser_bench

## ── コード品質 ──────────────────────────────────────

lint:
	$(CARGO) build --release 2>&1 | grep -q "warning" && exit 1 || true
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

## ── LSP ─────────────────────────────────────────────

lsp:
	$(CARGO) run --release --package lcvgc-lsp

## ── サーバー ────────────────────────────────────────

run:
	$(CARGO) run --release --package lcvgc

run-watch:
	$(CARGO) run --release --package lcvgc -- --watch .

## ── その他 ──────────────────────────────────────────

clean:
	$(CARGO) clean

## ── ヘルプ ──────────────────────────────────────────

help:
	@echo "lcvgc v$(VERSION) - Makefile ターゲット一覧"
	@echo ""
	@echo "ビルド:"
	@echo "  make build       ネイティブビルド (lcvgc + lcvgc-lsp)"
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
	@echo "実行:"
	@echo "  make run          daemon 起動"
	@echo "  make run-watch    daemon + ホットリロード"
	@echo "  make lsp          LSP サーバー起動"
	@echo ""
	@echo "その他:"
	@echo "  make clean       ビルド成果物削除"
