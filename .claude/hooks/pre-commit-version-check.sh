#!/bin/bash
# PreToolUse Hook: git commit 検知時にバージョンインクリメント確認をリマインド
TOOL_INPUT_STR="$TOOL_INPUT"
if echo "$TOOL_INPUT_STR" | grep -q 'git commit'; then
  cat <<'MSG'
【バージョン確認必須】コミット前に、今回の変更に関連するコンポーネントのバージョンインクリメントについてユーザーに確認してください。

対象コンポーネント:
- lcvgc: crates/lcvgc/Cargo.toml
- lcvgc-core: crates/lcvgc-core/Cargo.toml
- lcvgc.nvim: ../lcvgc.nvim/lua/lcvgc/init.lua

各コンポーネントについて以下を確認:
1. 今回の変更で影響を受けるか
2. 影響がある場合: major / minor / patch のどれをインクリメントするか
3. 影響がない場合: スキップ
MSG
fi
