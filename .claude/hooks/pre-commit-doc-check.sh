#!/usr/bin/env bash
# PreToolUse hook: git commit の前に仕様書の更新漏れチェック (make doc-check) を実行する
#
# Claude Code が Bash ツールで git commit を実行しようとしたとき、このリポジトリへの
# commit であれば make doc-check を走らせ、失敗したら終了コード 2 で commit を止める。
# 止めた理由は標準エラーに書き、Claude に返す。
#
# Runs `make doc-check` before Claude commits to this repository and blocks the
# commit (exit 2, reason on stderr) when it fails.
#
# 入力 / Input: 標準入力の hook JSON（tool_input.command と cwd を使う）
#               hook JSON on stdin (uses tool_input.command and cwd)
# 環境変数 / Environment:
#   CLAUDE_PROJECT_DIR  このリポジトリのルート / root of this repository
#   DOC_CHECK_CMD       実行するチェック（テスト用に差し替え可能）
#                       check command to run (overridable for tests)
# 終了コード / Exit status: 0 = 許可 / allow, 2 = commit を止める / block
set -u

# hook 入力 JSON / hook input JSON
INPUT="$(cat)"
# Bash ツールに渡されたコマンド / the Bash command
CMD="$(jq -r '.tool_input.command // empty' <<<"$INPUT")"
# コマンドが実行されるディレクトリ / directory the command runs in
CWD="$(jq -r '.cwd // empty' <<<"$INPUT")"
# このリポジトリのルート / root of this repository
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"

# git commit を実行するコマンドか判定する
#
# 行頭または ; & | ( の直後に `git [-C dir] [-c k=v] commit` が現れるものだけを対象にし、
# `grep commit` のように単語を含むだけのコマンドは対象外にする。
#
# Returns: commit を含めば 0 / 0 if the command runs `git commit`
is_git_commit() {
  grep -qE '(^|[;&|(])[[:space:]]*git([[:space:]]+-[Cc][[:space:]]+[^[:space:]]+)*[[:space:]]+commit([[:space:]]|$)' <<<"$CMD"
}

# commit が実行されるディレクトリを推定する
#
# `git -C <dir> commit` があればその dir、無ければ最後の `cd <dir>`、
# どちらも無ければ hook 入力の cwd を返す。
#
# Guesses the directory the commit runs in: `git -C <dir>`, else the last
# `cd <dir>`, else the input cwd.
#
# Returns: 標準出力にディレクトリ（相対パスのこともある）/ the directory on stdout
commit_dir() {
  local dir
  dir="$(grep -oE 'git[[:space:]]+-C[[:space:]]+[^[:space:]]+[[:space:]]+commit' <<<"$CMD" | head -1 |
    sed -E 's/^git[[:space:]]+-C[[:space:]]+([^[:space:]]+).*/\1/')"
  if [ -z "$dir" ]; then
    dir="$(grep -oE '(^|[;&|(])[[:space:]]*cd[[:space:]]+[^[:space:];&|)]+' <<<"$CMD" | tail -1 |
      sed -E 's/^.*cd[[:space:]]+//')"
  fi
  dir="${dir%\"}"
  dir="${dir#\"}"
  dir="${dir%\'}"
  dir="${dir#\'}"
  echo "${dir:-.}"
}

# commit 先がこのリポジトリか判定する
#
# Returns: このリポジトリなら 0、別リポジトリや解決不能なら 1
#          0 if the commit targets this repository, 1 otherwise
targets_this_repo() {
  local dir top mine
  dir="$(commit_dir)"
  top="$(cd "${CWD:-$PROJECT_DIR}" 2>/dev/null && cd "$dir" 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null)" || return 1
  mine="$(git -C "$PROJECT_DIR" rev-parse --show-toplevel 2>/dev/null)" || return 1
  [ "$top" = "$mine" ]
}

is_git_commit || exit 0
targets_this_repo || exit 0

# commit message に [skip-doc-check] があればチェックを素通りさせる
# a [skip-doc-check] tag in the command skips the check
if grep -qF '[skip-doc-check]' <<<"$CMD"; then
  export SKIP_DOC_CHECK=1
fi

# 実行するチェック / check command to run
CHECK_CMD="${DOC_CHECK_CMD:-make -s doc-check}"
if ! OUT="$(cd "$PROJECT_DIR" && bash -c "$CHECK_CMD" 2>&1)"; then
  {
    echo "commit を止めました: 仕様書の更新漏れチェック (make doc-check) が失敗しました。"
    echo "Commit blocked: make doc-check failed."
    echo "$OUT"
  } >&2
  exit 2
fi
exit 0
