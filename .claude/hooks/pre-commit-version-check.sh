#!/usr/bin/env bash
# PreToolUse hook: git commit の前に、バージョンインクリメントの確認をユーザーに求める
#
# Claude Code が Bash ツールで git commit を実行しようとしたとき、
# permissionDecision "ask" を返して、commit の実行前にユーザーの確認を挟む。
# 確認画面には、バージョンを確認すべきコンポーネントの一覧を理由として表示する。
#
# Before Claude runs `git commit`, returns permissionDecision "ask" so the user
# confirms the version bump of each affected component first.
#
# 入力 / Input: 標準入力の hook JSON（tool_input.command を使う）
#               hook JSON on stdin (uses tool_input.command)
# 出力 / Output: commit のときだけ hookSpecificOutput の JSON を標準出力に書く
#                hookSpecificOutput JSON on stdout, only for commits
# 終了コード / Exit status: 常に 0 / always 0
set -u

# Bash ツールに渡されたコマンド / the Bash command
CMD="$(jq -r '.tool_input.command // empty')"

# git commit を実行するコマンドでなければ何もしない（判定は pre-commit-doc-check.sh と同じ）
# do nothing unless the command runs `git commit` (same rule as pre-commit-doc-check.sh)
if ! grep -qE '(^|[;&|(])[[:space:]]*git([[:space:]]+-[Cc][[:space:]]+[^[:space:]]+)*[[:space:]]+commit([[:space:]]|$)' <<<"$CMD"; then
  exit 0
fi

# 確認画面に出す理由 / reason shown in the confirmation prompt
REASON='【バージョン確認】main へ入る変更なら、関連コンポーネントのバージョンインクリメント (major / minor / patch / 不要) をユーザーに確認済みか確かめてください。
対象: lcvgc = crates/lcvgc/Cargo.toml / lcvgc.nvim = ../lcvgc.nvim/lua/lcvgc/init.lua
[Version check] If this change goes to main, confirm the version bump of each affected component with the user.'

jq -n --arg reason "$REASON" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "ask",
    permissionDecisionReason: $reason
  }
}'
