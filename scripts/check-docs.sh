#!/usr/bin/env bash
# 仕様書の更新漏れチェック
#
# main との差分（commit 済み・未 commit・未追跡ファイルすべて）を見て、
# 構文やプロトコルを変更したのに仕様書 (ja/en) が更新されていない場合に失敗する。
#
# Checks that specs (ja/en) are updated together with syntax/protocol changes.
# Looks at every change since the merge base with main: committed, uncommitted
# and untracked files alike.
#
# ルール / Rules:
#   1. crates/lcvgc/src/parser/ または ast/ を変更したら
#      specs/lcvgc-dsl-spec.ja.md と specs/lcvgc-dsl-spec.md の両方を更新する
#   2. crates/lcvgc/src/server/protocol.rs を変更したら
#      specs/lsp-in-out.ja.md と specs/lsp-in-out.md の両方を更新する
#   3. specs/*.ja.md と対応する英語版 specs/*.md は必ず両方そろって更新する
#
# 環境変数 / Environment:
#   SKIP_DOC_CHECK=1  チェックを素通りさせる（仕様書の変更が不要な修正向け）
#                     skip the check (for changes that need no spec update)
#   DOC_CHECK_BASE    比較の基準にする ref（省略時は main / origin/main との merge-base）
#                     ref to compare against (default: merge base with main)
#
# 終了コード / Exit status: 0 = 問題なし / OK, 1 = 更新漏れあり / missing updates,
#                           2 = 実行エラー / runtime error
set -u

if [ "${SKIP_DOC_CHECK:-}" = "1" ]; then
  echo "doc-check: SKIP_DOC_CHECK=1 のためスキップしました / skipped by SKIP_DOC_CHECK=1"
  exit 0
fi

# 比較の基準となる commit を決める
#
# DOC_CHECK_BASE が指定されていればそれを、無ければ main（無ければ origin/main）と
# HEAD の merge-base を使う。
#
# Resolves the base commit to diff against.
#
# Returns: 標準出力に commit ID。解決できなければ終了コード 1
#          the commit id on stdout; exit status 1 if it cannot be resolved
resolve_base() {
  local ref
  for ref in "${DOC_CHECK_BASE:-}" main origin/main; do
    [ -n "$ref" ] || continue
    if git rev-parse --verify -q "$ref^{commit}" >/dev/null; then
      git merge-base HEAD "$ref"
      return
    fi
  done
  return 1
}

# base からの変更ファイル一覧（未追跡ファイルを含む）を1行1件で出力する
#
# Lists files changed since the base, including untracked files, one per line.
#
# Arguments:
#   $1 - 比較の基準 commit / base commit
changed_files() {
  {
    git diff --name-only "$1"
    git ls-files --others --exclude-standard
  } | sort -u
}

# 変更一覧にファイルが含まれるか判定する
#
# Arguments:
#   $1 - 探すファイル（リポジトリ相対）/ file to look for, repo-relative
# Returns: 含まれれば 0 / 0 if the file is in CHANGED
is_changed() {
  grep -qxF -- "$1" <<<"$CHANGED"
}

# 変更一覧に、指定した接頭辞で始まるファイルがあるか判定する
#
# Arguments:
#   $1 - パスの接頭辞 / path prefix
# Returns: あれば 0 / 0 if any changed file starts with the prefix
has_changed_prefix() {
  grep -q -- "^$1" <<<"$CHANGED"
}

# 更新漏れのメッセージを積む / append a missing-update message
#
# Arguments:
#   $1 - 更新が必要なファイル / file that must be updated
#   $2 - 理由 / reason
require() {
  if ! is_changed "$1"; then
    PROBLEMS+=("  - $1 が未更新です / not updated ($2)")
  fi
}

if ! BASE="$(resolve_base)"; then
  echo "doc-check: 比較の基準 (main) が見つかりません / base ref (main) not found" >&2
  exit 2
fi
# base からの変更ファイル一覧 / files changed since the base
CHANGED="$(changed_files "$BASE")"
# 見つかった更新漏れ / missing updates found
PROBLEMS=()

# ルール 1: 構文の変更 → DSL 仕様書 / syntax changes need the DSL spec
if has_changed_prefix "crates/lcvgc/src/parser/" || has_changed_prefix "crates/lcvgc/src/ast/"; then
  require specs/lcvgc-dsl-spec.ja.md "parser/ または ast/ を変更 / parser or ast changed"
  require specs/lcvgc-dsl-spec.md "parser/ または ast/ を変更 / parser or ast changed"
fi

# ルール 2: プロトコルの変更 → lsp-in-out / protocol changes need lsp-in-out
if is_changed crates/lcvgc/src/server/protocol.rs; then
  require specs/lsp-in-out.ja.md "server/protocol.rs を変更 / protocol changed"
  require specs/lsp-in-out.md "server/protocol.rs を変更 / protocol changed"
fi

# ルール 3: 仕様書の ja/en は対で更新 / ja and en specs change together
while IFS= read -r f; do
  case "$f" in
  specs/*.ja.md) require "${f%.ja.md}.md" "$f と対で更新 / pair of $f" ;;
  specs/*.md) require "${f%.md}.ja.md" "$f と対で更新 / pair of $f" ;;
  esac
done <<<"$CHANGED"

if [ "${#PROBLEMS[@]}" -gt 0 ]; then
  echo "doc-check: 仕様書の更新漏れがあります / missing spec updates (base: ${BASE:0:7})"
  printf '%s\n' "${PROBLEMS[@]}" | sort -u
  echo "仕様書の変更が不要な場合は SKIP_DOC_CHECK=1、または commit message に [skip-doc-check] を付けてください。"
  echo "If no spec change is needed, set SKIP_DOC_CHECK=1 or put [skip-doc-check] in the commit message."
  exit 1
fi
echo "doc-check: OK (base: ${BASE:0:7})"
