---
name: write-comment
description: 編集された code に対して適切なフォーマットで、日本語、英語のコメントを記載する。
---

lua, rust 等、様々な言語があるが、それぞれの official なコメントの書き方で記述内容を記載する事。
- module, class, method, 変数、定数、enum等、プログラムを構成する単位をアイテムと呼ぶ事にする。
- private な アイテムにもコメントは記載する。
- 日本語、英語を両方表記する。
  - e.g.
    ```
    /// tracing初期化
    ///
    /// init tracing
    ///
    /// # Arguments
    /// * `log_level` - ログレベル文字列 (e.g. "info", "debug") / string of log level(e.g. "info", "debug")
    fn init_tracing(log_level: &str) {

    ```

    ```
    /// ファイル変更イベントの種類
    ///
    /// kind of events when file changed
    #[derive(Debug, Clone, PartialEq)]
    pub enum FileChangeEvent {
        /// ファイルが変更された / changed file.
        Modified(PathBuf),
        /// ファイルが作成された / created file
        Created(PathBuf),
    }
    ```
