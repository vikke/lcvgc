# lcvgc Makefile
#
# ビルド環境 = 実行環境（ネイティブビルドのみ）
# 注: $(shell) 等 GNU make 機能に依存。FreeBSD では gmake を使用すること。

CARGO        := cargo
VERSION      := $(shell grep '^version' crates/lcvgc/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
RELEASE_DIR  := target/release
REPO         := https://github.com/vikke/lcvgc

.PHONY: help all build test test-integration test-all bench lint fmt fmt-check \
        clean install binstall lsp run run-watch

## ── ヘルプ ──────────────────────────────────────────
# help を先頭ターゲットにすることで GNU/BSD make 双方で既定ゴールになる。
# 切り出し・整形は単一の POSIX awk に集約し、sed の \t/-E 差(GNU/BSD)を回避。

help: ## このヘルプを表示
	@echo "lcvgc v$(VERSION) - Makefile ターゲット一覧"
	@echo ""
	@awk 'BEGIN{FS=":.*## "} /^[a-zA-Z_-]+:.*## /{printf "  \033[36m%-16s\033[0m %s\n",$$1,$$2}' $(MAKEFILE_LIST)

## ── ビルド ──────────────────────────────────────────

all: build ## デフォルトビルド (= build)

build: ## ネイティブビルド (lcvgc + lcvgc-gen)
	$(CARGO) build --release
	@echo "✓ $(RELEASE_DIR)/lcvgc"
	@echo "✓ $(RELEASE_DIR)/lcvgc-gen"

## ── テスト ──────────────────────────────────────────

test: ## 全テスト実行
	$(CARGO) test --workspace

test-integration: ## 統合テスト実行 (integration test)
	$(CARGO) test --package lcvgc --test integration

test-all: lint test ## lint + テスト

## ── ベンチマーク ────────────────────────────────────

bench: ## ベンチマーク実行 (parser_bench)
	$(CARGO) bench --package lcvgc --bench parser_bench

## ── コード品質 ──────────────────────────────────────

lint: ## 警告・clippy チェック
	$(CARGO) build --release 2>&1 | grep -q "warning" && exit 1 || true
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt: ## コードフォーマット
	$(CARGO) fmt --all

fmt-check: ## フォーマットチェック (CI 用)
	$(CARGO) fmt --all -- --check

## ── LSP ─────────────────────────────────────────────

lsp: ## LSP 内蔵デーモン起動 (run と同一: TCP 5555)
	$(CARGO) run --release --package lcvgc

## ── サーバー ────────────────────────────────────────

run: ## daemon 起動
	$(CARGO) run --release --package lcvgc

run-watch: ## daemon + ホットリロード
	$(CARGO) run --release --package lcvgc -- --watch .

## ── インストール ────────────────────────────────────

install: ## ソースからビルドして ~/.cargo/bin へインストール
	$(CARGO) install --path crates/lcvgc --force

binstall: ## ビルド済みバイナリを Release から取得 (cargo-binstall)
	@command -v cargo-binstall >/dev/null 2>&1 || { echo "cargo-binstall 未導入: cargo install cargo-binstall"; exit 1; }
	$(CARGO) binstall --git $(REPO) lcvgc --force

## ── その他 ──────────────────────────────────────────
deps: ## 依存図の生成
	$(CARGO) modules dependencies --lib -p lcvgc --no-externs --no-fns --no-sysroot  | dot -Tsvg > deps.svg

clean: ## ビルド成果物削除
	$(CARGO) clean
