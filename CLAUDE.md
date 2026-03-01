## serena

- **厳守**: claude code を起動したら、activate & onbording を自動で行なう事。
- **厳守**: claude code でpromptを指示されたら、promptを実行する前に、activate & onbording を自動で行なう事。

## 思考

- **厳守**: 仕様書等大きなファイルを読んで大きなタスク実行を行なう場合は、planモードで何をどうするかを綿密に事前に計画する事。
    - planで決まった事は @TODO.md として `- [ ]` でリスト化する事。compaction されても何をするのかわかるように出力する事。
    - リストが完了したら、チェックする事。
    - 出力が必要な作業の場合、 @./tmp/ に direcotry を作成し、そこに出力して行く。
- 迷いが発生した場合は、こちらに聞く事。その際、3～5パターンの案を提示し、推奨するパターンを提示する事。他の選ばれなかったバターンは選ばれなかった理由と強みを出力し、こちらの選択の為の情報を出す事。
- **厳守**: ベストなAの方法と、次善策のBがあった場合、すぐにBに行かず、Aで行く方法をしつこく調査する事。Bに行きたい場合は、こちらに相談する事。

## 行動

- 調査や分析は、sub agent を起動し、そちらに任せる事。sub agent は、 @./tmp/ に調査や分析を行なった結果を markdown で出力すること。
- branch の運用
    - promptからbranchの名前を考え、まずbranchを作成し、そこのbranchで作業する事。
    - 作業が全て終わったら、mainにmergeするか、こちらに聞く事。 **絶対守る事**: 勝手に merge しない事。
- `git worktree` の活用
    - 複数のsub agentで並列処理を行なう場合は、 `git worktree` を使う事。
    - 作業結果を `copy` で、`worktree` から `current directory` に取り込まない。
    - 一度、conventional commits で、タイトル、本文付きでcommitし、それから、作業元の branch が rebase で取り込む事。
        - この場合は、作業を止めたくないので、一番良いと思うcommit messageを採用して。
    - 作った `git worktree` は `git worktree remove -f ` で最後に削除する事。
- commit message
    - `commit message` は日本語で3種類程度提案する事。
    - 推奨するパターンを提示する事。他の選ばれなかったバターンは選ばれなかった理由と強みを出力し、こちらの選択の為の情報を出す事。
    - conventional commits を守る事。
    - タイトルと本文で構成する事。

## 実装

- **厳守**: 単一責務設計を意識し、細かい単機能の集合体での実装を行なう事。
- **厳守**: まず、 test code を実装し、それが動く product code を実装する事。
- 実装が、並列で行なえる物は、sub agent で実装させる。 sub agent 毎に `git worktree` を作成し、実装が終わったら、current branch に rebase する事。

## ドキュメント

- 日本語で書いた物は、英語でも書く事。skill を使っての翻訳で良い。
- ファイル名は以下の通り
    - 日本語ドキュメント: *.ja.md
    - 英語ドキュメント: *.md
- 翻訳は、日本語400行程度に対して、1 sub agentを起動して並列実行で処理を行なう事。1 sub agent 1 section を担当させ、`git worktree` を上手く使って作業分担する事。

## **厳守**: 再発防止注意事項

- 失敗をした場合には、このセクションにどうしたら繰替えさないかを追記していくこと。
- コミット前にビルド・リント（Rust: `cargo build --release`、Lua: `luacheck`等）で警告ゼロを確認する事。警告が残っている状態でコミットしない事。
- コミット前にテスト（Rust: `cargo test`、Lua: plenary test等）で全テストパスを確認する事。
- モジュール構成を変更したら、関連ファイルとの整合性を必ず確認する事。
