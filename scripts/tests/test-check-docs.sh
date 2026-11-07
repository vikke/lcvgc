#!/usr/bin/env bash
# scripts/check-docs.sh のテスト
#
# 一時ディレクトリに git リポジトリを作り、main からブランチを切って
# ファイルを変更した状態で check-docs.sh を実行し、終了コードと出力を検証する。
#
# Tests for scripts/check-docs.sh. Each case builds a throwaway git repository,
# branches off main, changes files, runs the checker and asserts on its exit
# code and output.
#
# 使い方 / Usage: bash scripts/tests/test-check-docs.sh
set -u

# テスト対象スクリプトの絶対パス / absolute path of the script under test
CHECK_DOCS="$(cd "$(dirname "$0")/.." && pwd)/check-docs.sh"
# 成功・失敗件数 / pass and fail counters
PASS=0
FAIL=0
# 作成した一時リポジトリ（終了時に削除）/ temp repositories, removed on exit
TMP_REPOS=()
trap 'for d in "${TMP_REPOS[@]}"; do rm -rf "$d"; done' EXIT

# 最小構成の git リポジトリを一時ディレクトリに作り、そのパスを出力する
#
# main ブランチに仕様書・パーサ・プロトコルのダミーファイルを commit し、
# 作業ブランチ work をチェックアウトした状態にする。
#
# Creates a minimal repository (main + branch `work`) and prints its path.
#
# Returns: 標準出力に作成したリポジトリのパス / the repository path on stdout
make_repo() {
  local dir
  dir="$(mktemp -d)"
  (
    cd "$dir" || exit 1
    git init -q -b main
    git config user.email test@example.com
    git config user.name test
    git config commit.gpgsign false
    mkdir -p specs crates/lcvgc/src/parser crates/lcvgc/src/ast crates/lcvgc/src/server crates/lcvgc/src/engine
    for f in specs/lcvgc-dsl-spec.ja.md specs/lcvgc-dsl-spec.md \
      specs/lsp-in-out.ja.md specs/lsp-in-out.md \
      crates/lcvgc/src/parser/tempo.rs crates/lcvgc/src/ast/tempo.rs \
      crates/lcvgc/src/server/protocol.rs crates/lcvgc/src/engine/player.rs; do
      echo "base" >"$f"
    done
    git add -A
    git commit -q -m base
    git checkout -q -b work
  ) || return 1
  echo "$dir"
}

# 指定ファイルに1行追記する（変更を作る）
#
# Arguments:
#   $1 - リポジトリのパス / repository path
#   $@ - 追記するファイル（リポジトリ相対）/ files to touch, repo-relative
touch_files() {
  local repo="$1"
  shift
  local f
  for f in "$@"; do
    mkdir -p "$repo/$(dirname "$f")"
    echo "changed" >>"$repo/$f"
  done
}

# check-docs.sh をリポジトリ内で実行し、終了コードと出力を検証する
#
# Arguments:
#   $1 - テスト名 / test name
#   $2 - 期待する終了コード / expected exit code
#   $3 - 出力に含まれるべき文字列（空なら検証しない）/ expected substring (empty = skip)
#   $4 - リポジトリのパス / repository path
#   残り - check-docs.sh 実行時に付ける環境変数（KEY=VALUE）/ extra env assignments
assert_check() {
  local name="$1" expected_code="$2" expected_text="$3" repo="$4"
  shift 4
  local out code
  out="$(cd "$repo" && env DOC_CHECK_BASE=main "$@" bash "$CHECK_DOCS" 2>&1)"
  code=$?
  if [ "$code" -ne "$expected_code" ]; then
    echo "FAIL: $name (exit $code, expected $expected_code)"
    echo "$out" | sed 's/^/    /'
    FAIL=$((FAIL + 1))
    return
  fi
  if [ -n "$expected_text" ] && ! grep -qF -- "$expected_text" <<<"$out"; then
    echo "FAIL: $name (output lacks: $expected_text)"
    echo "$out" | sed 's/^/    /'
    FAIL=$((FAIL + 1))
    return
  fi
  echo "ok:   $name"
  PASS=$((PASS + 1))
}

# --- ケース / cases ---------------------------------------------------------

repo="$(make_repo)"
TMP_REPOS+=("$repo")
assert_check "変更なしは成功" 0 "" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/engine/player.rs
assert_check "対象外ファイルのみの変更は成功" 0 "" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/tempo.rs
assert_check "parser 変更で DSL 仕様書未更新は失敗" 1 "specs/lcvgc-dsl-spec.ja.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/ast/tempo.rs specs/lcvgc-dsl-spec.ja.md
assert_check "ast 変更で英語版の DSL 仕様書だけ未更新は失敗" 1 "specs/lcvgc-dsl-spec.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/tempo.rs specs/lcvgc-dsl-spec.ja.md specs/lcvgc-dsl-spec.md
assert_check "parser 変更で DSL 仕様書 ja/en 更新済みは成功" 0 "" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/new_syntax.rs specs/lcvgc-dsl-spec.ja.md specs/lcvgc-dsl-spec.md
assert_check "未追跡の新規 parser ファイルも検出対象（仕様書更新済みで成功）" 0 "" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/new_syntax.rs
assert_check "未追跡の新規 parser ファイルで仕様書未更新は失敗" 1 "specs/lcvgc-dsl-spec.ja.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/server/protocol.rs
assert_check "protocol 変更で lsp-in-out 未更新は失敗" 1 "specs/lsp-in-out.ja.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/server/protocol.rs specs/lsp-in-out.ja.md specs/lsp-in-out.md
assert_check "protocol 変更で lsp-in-out ja/en 更新済みは成功" 0 "" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" specs/lsp-in-out.ja.md
assert_check "日本語版の仕様書だけ更新は失敗" 1 "specs/lsp-in-out.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" specs/lsp-in-out.md
assert_check "英語版の仕様書だけ更新は失敗" 1 "specs/lsp-in-out.ja.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" specs/new-doc.ja.md
assert_check "対応する英語版が無い新規 ja 仕様書も失敗" 1 "specs/new-doc.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/tempo.rs
(cd "$repo" && git add -A && git commit -q -m wip)
assert_check "commit 済みの変更も base との差分として検出" 1 "specs/lcvgc-dsl-spec.ja.md" "$repo"

repo="$(make_repo)"
TMP_REPOS+=("$repo")
touch_files "$repo" crates/lcvgc/src/parser/tempo.rs
assert_check "SKIP_DOC_CHECK=1 で素通り" 0 "SKIP_DOC_CHECK" "$repo" SKIP_DOC_CHECK=1

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
