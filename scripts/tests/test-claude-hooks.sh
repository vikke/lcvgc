#!/usr/bin/env bash
# .claude/hooks/ の PreToolUse hook スクリプトのテスト
#
# Claude Code と同じく、hook の入力 JSON を標準入力に流し込んで実行し、
# 終了コード・標準出力・標準エラーを検証する。
#
# Tests for the PreToolUse hook scripts under .claude/hooks/. The hook input
# JSON is piped on stdin, exactly as Claude Code does.
#
# 使い方 / Usage: bash scripts/tests/test-claude-hooks.sh
set -u

# リポジトリのルート / repository root
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# テスト対象の hook / hooks under test
DOC_HOOK="$ROOT/.claude/hooks/pre-commit-doc-check.sh"
VERSION_HOOK="$ROOT/.claude/hooks/pre-commit-version-check.sh"
# 成功・失敗件数 / pass and fail counters
PASS=0
FAIL=0
# スタブの記録先 / file the stub check command records its calls to
STUB_LOG="$(mktemp)"
trap 'rm -f "$STUB_LOG"' EXIT

# hook 入力 JSON を組み立てる
#
# Arguments:
#   $1 - Bash ツールに渡されたコマンド文字列 / the Bash command string
#   $2 - hook 入力の cwd（省略時はリポジトリのルート）/ cwd field (default: repo root)
# Returns: 標準出力に JSON / the JSON on stdout
hook_input() {
  jq -n --arg cmd "$1" --arg cwd "${2:-$ROOT}" \
    '{session_id: "test", hook_event_name: "PreToolUse", tool_name: "Bash", cwd: $cwd, tool_input: {command: $cmd}}'
}

# hook を実行して結果を検証する
#
# Arguments:
#   $1 - テスト名 / test name
#   $2 - hook スクリプト / hook script
#   $3 - Bash コマンド文字列 / Bash command string
#   $4 - 期待する終了コード / expected exit code
#   $5 - 標準出力+標準エラーに含まれるべき文字列（空なら出力が空であること）
#        expected substring of stdout+stderr (empty = output must be empty)
#   $6 - スタブが呼ばれるべきか（yes / no / -=検証しない）/ stub must be called?
#   残り - hook 実行時の環境変数（KEY=VALUE）/ extra env assignments
#   HOOK_CWD（呼び出し側の変数）- hook 入力の cwd を差し替える / overrides the input cwd
assert_hook() {
  local name="$1" hook="$2" cmd="$3" expected_code="$4" expected_text="$5" expect_call="$6"
  shift 6
  : >"$STUB_LOG"
  local out code
  out="$(hook_input "$cmd" "${HOOK_CWD:-$ROOT}" | env CLAUDE_PROJECT_DIR="$ROOT" "$@" bash "$hook" 2>&1)"
  code=$?
  local problem=""
  if [ "$code" -ne "$expected_code" ]; then
    problem="exit $code, expected $expected_code"
  elif [ -z "$expected_text" ] && [ -n "$out" ]; then
    problem="output should be empty"
  elif [ -n "$expected_text" ] && ! grep -qF -- "$expected_text" <<<"$out"; then
    problem="output lacks: $expected_text"
  elif [ "$expect_call" = "yes" ] && [ ! -s "$STUB_LOG" ]; then
    problem="check command was not called"
  elif [ "$expect_call" = "no" ] && [ -s "$STUB_LOG" ]; then
    problem="check command should not be called"
  fi
  if [ -n "$problem" ]; then
    echo "FAIL: $name ($problem)"
    echo "$out" | sed 's/^/    /'
    FAIL=$((FAIL + 1))
    return
  fi
  echo "ok:   $name"
  PASS=$((PASS + 1))
}

# 呼ばれたことと SKIP_DOC_CHECK の値を記録して成功するスタブ
# stub that records the call (and SKIP_DOC_CHECK) and succeeds
STUB_OK="echo called SKIP=\${SKIP_DOC_CHECK:-} >>$STUB_LOG"
# 呼ばれたことを記録し、メッセージを出して失敗するスタブ
# stub that records the call, prints a message and fails
STUB_NG="echo called >>$STUB_LOG; echo 'doc-check: missing spec updates'; exit 1"

# --- pre-commit-doc-check.sh -----------------------------------------------

assert_hook "commit 以外のコマンドは素通り" "$DOC_HOOK" "git status" 0 "" no DOC_CHECK_CMD="$STUB_NG"
assert_hook "git commit でチェックが通れば許可" "$DOC_HOOK" "git commit -m 'feat: x'" 0 "" yes DOC_CHECK_CMD="$STUB_OK"
assert_hook "git commit でチェック失敗なら exit 2 で止める" "$DOC_HOOK" "git commit -m 'feat: x'" 2 "missing spec updates" yes DOC_CHECK_CMD="$STUB_NG"
assert_hook "git add と連結した commit も検出" "$DOC_HOOK" "git add -A && git commit -q -F -" 2 "missing spec updates" yes DOC_CHECK_CMD="$STUB_NG"
assert_hook "[skip-doc-check] タグで SKIP_DOC_CHECK=1 が渡る" "$DOC_HOOK" "git commit -m 'refactor: x [skip-doc-check]'" 0 "" yes DOC_CHECK_CMD="$STUB_OK"
if grep -q 'SKIP=1' "$STUB_LOG"; then
  echo "ok:   (SKIP_DOC_CHECK=1 がスタブに渡っている)"
  PASS=$((PASS + 1))
else
  echo "FAIL: SKIP_DOC_CHECK=1 がスタブに渡っていない / SKIP_DOC_CHECK not passed"
  FAIL=$((FAIL + 1))
fi
assert_hook "cd で別リポジトリに移って commit する場合はチェックしない" "$DOC_HOOK" "cd ../lcvgc.nvim && git commit -m x" 0 "" no DOC_CHECK_CMD="$STUB_NG"
assert_hook "git -C で別リポジトリを指す commit はチェックしない" "$DOC_HOOK" "git -C ../lcvgc.nvim commit -m x" 0 "" no DOC_CHECK_CMD="$STUB_NG"
assert_hook "cd で同じリポジトリ内に移る commit はチェックする" "$DOC_HOOK" "cd $ROOT/crates && git commit -m x" 2 "missing spec updates" yes DOC_CHECK_CMD="$STUB_NG"
HOOK_CWD="$ROOT/../lcvgc.nvim" assert_hook "cwd が別リポジトリの commit はチェックしない" "$DOC_HOOK" "git commit -m x" 0 "" no DOC_CHECK_CMD="$STUB_NG"
assert_hook "commit という単語を含むだけのコマンドは素通り" "$DOC_HOOK" "grep -n commit README.md" 0 "" no DOC_CHECK_CMD="$STUB_NG"

# --- pre-commit-version-check.sh -------------------------------------------

assert_hook "version: commit 以外は何も出さない" "$VERSION_HOOK" "git status" 0 "" -
assert_hook "version: commit 時はユーザー確認 (ask) を要求する" "$VERSION_HOOK" "git commit -m x" 0 '"permissionDecision": "ask"' -
assert_hook "version: 理由に対象コンポーネントが含まれる" "$VERSION_HOOK" "git commit -m x" 0 "crates/lcvgc/Cargo.toml" -
version_out="$(hook_input "git commit -m x" | bash "$VERSION_HOOK" 2>/dev/null)"
if jq -e '.hookSpecificOutput.hookEventName == "PreToolUse"' <<<"$version_out" >/dev/null 2>&1; then
  echo "ok:   version: 出力が PreToolUse の JSON として妥当"
  PASS=$((PASS + 1))
else
  echo "FAIL: version: 出力が JSON として不正 / invalid JSON output"
  echo "$version_out" | sed 's/^/    /'
  FAIL=$((FAIL + 1))
fi
if grep -q 'lcvgc-core' <<<"$version_out"; then
  echo "FAIL: version: 存在しない lcvgc-core が残っている / stale lcvgc-core reference"
  FAIL=$((FAIL + 1))
else
  echo "ok:   version: lcvgc-core を含まない"
  PASS=$((PASS + 1))
fi

echo
echo "passed: $PASS, failed: $FAIL"
[ "$FAIL" -eq 0 ]
