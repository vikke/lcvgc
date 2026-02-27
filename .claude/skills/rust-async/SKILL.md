---
name: rust-async
description: Rustの非同期プログラミングを支援するスキル。tokio/async-stdの使い方、async/awaitパターン、Send+Sync境界の問題解決、チャネル通信、タスクスポーン、非同期ストリーム、select/joinパターン、非同期トレイトをカバー。「async」「await」「tokio」「非同期」「並行」「spawn」「channel」「Send」「Sync」「Future」「ランタイム」など非同期に関する話題が出たら必ずこのスキルを使うこと。パフォーマンスのためにasyncを導入したいという相談にも使うこと。
---

# Rust 非同期プログラミングスキル

tokioを中心としたRust非同期プログラミングのパターンとベストプラクティス。

## tokio セットアップ

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
# features を個別指定する場合:
# tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "sync", "time", "io-util"] }
```

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 非同期処理
    Ok(())
}

// シングルスレッドランタイムの場合
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // ...
}
```

## 基本パターン

### タスクのスポーン

```rust
use tokio::task;

// 独立したタスク（Join Handle で結果を受け取れる）
let handle = task::spawn(async {
    expensive_computation().await
});
let result = handle.await?;

// 結果を待たないバックグラウンドタスク
task::spawn(async {
    background_logging().await;
});

// ブロッキング処理は spawn_blocking で
let result = task::spawn_blocking(|| {
    // CPU-heavy or blocking I/O
    std::thread::sleep(std::time::Duration::from_secs(1));
    42
}).await?;
```

### 複数タスクの並行実行

```rust
use tokio::try_join;

// すべて成功を待つ（1つでも失敗したら即エラー）
let (a, b, c) = try_join!(
    fetch_data("url1"),
    fetch_data("url2"),
    fetch_data("url3"),
)?;

// join! はエラーを伝播しない版
let (a, b) = tokio::join!(task_a(), task_b());
```

### select によるレース

```rust
use tokio::select;

select! {
    result = fetch_data() => {
        println!("データ取得完了: {result:?}");
    }
    _ = tokio::time::sleep(Duration::from_secs(5)) => {
        println!("タイムアウト");
    }
    _ = shutdown_signal() => {
        println!("シャットダウン要求");
    }
}
```

## チャネル通信

### mpsc（多対一）

最もよく使うパターン。プロデューサー→コンシューマーのメッセージパッシング：

```rust
use tokio::sync::mpsc;

#[derive(Debug)]
enum Command {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    Shutdown,
}

let (tx, mut rx) = mpsc::channel::<Command>(100);  // バッファサイズ100

// プロデューサー
let tx2 = tx.clone();  // 複数プロデューサーOK
task::spawn(async move {
    tx2.send(Command::NoteOn { note: 60, velocity: 127 }).await.unwrap();
});

// コンシューマー
task::spawn(async move {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::NoteOn { note, velocity } => { /* ... */ }
            Command::NoteOff { note } => { /* ... */ }
            Command::Shutdown => break,
        }
    }
});
```

### oneshot（一対一、一回きり）

リクエスト-レスポンスパターンに最適：

```rust
use tokio::sync::oneshot;

let (tx, rx) = oneshot::channel();

task::spawn(async move {
    let result = compute_something().await;
    let _ = tx.send(result);
});

let result = rx.await?;
```

### broadcast（一対多）

```rust
use tokio::sync::broadcast;

let (tx, _) = broadcast::channel::<String>(16);

let mut rx1 = tx.subscribe();
let mut rx2 = tx.subscribe();

tx.send("hello".to_string())?;
// rx1 と rx2 の両方が "hello" を受け取る
```

### watch（最新値の共有）

設定の変更通知などに：

```rust
use tokio::sync::watch;

let (tx, mut rx) = watch::channel(Config::default());

// 監視側
task::spawn(async move {
    while rx.changed().await.is_ok() {
        let config = rx.borrow().clone();
        apply_config(config);
    }
});

// 更新側
tx.send(new_config)?;
```

## Send + Sync 境界の問題解決

### よくあるエラーと対処

**`future is not Send`**:

```rust
// NG: Rc は Send でない
use std::rc::Rc;
async fn bad() {
    let rc = Rc::new(42);
    some_async_op().await;  // await を跨いで Rc が生存 → not Send
    println!("{rc}");
}

// OK: Arc を使う
use std::sync::Arc;
async fn good() {
    let arc = Arc::new(42);
    some_async_op().await;
    println!("{arc}");
}
```

**`MutexGuard` を await 越しに保持しない**:

```rust
use tokio::sync::Mutex;

// NG: std::sync::MutexGuard を await 越しに保持
async fn bad(data: &std::sync::Mutex<Vec<u8>>) {
    let mut guard = data.lock().unwrap();
    async_operation().await;  // デッドロックの危険
    guard.push(42);
}

// OK: ガードのスコープを限定
async fn good(data: &std::sync::Mutex<Vec<u8>>) {
    {
        let mut guard = data.lock().unwrap();
        guard.push(42);
    }  // ここでガード解放
    async_operation().await;
}

// OK: tokio::sync::Mutex（await 越しに保持可能だが低速）
async fn also_ok(data: &Mutex<Vec<u8>>) {
    let mut guard = data.lock().await;
    async_operation().await;
    guard.push(42);
}
```

### 判断基準: std::sync::Mutex vs tokio::sync::Mutex

- **ロック区間が短く await を含まない** → `std::sync::Mutex`（高速）
- **ロック区間に await がある** → `tokio::sync::Mutex`
- **読み取り多・書き込み少** → `tokio::sync::RwLock`

## 非同期トレイト

Rust 1.75+ で async fn in trait が安定化：

```rust
pub trait DataSource {
    async fn fetch(&self, key: &str) -> Result<Vec<u8>, Error>;
}

// dyn を使う場合は trait_variant が便利
// または手動で Box<dyn Future> を返す
pub trait DataSourceDyn: Send + Sync {
    fn fetch(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, Error>> + Send + '_>>;
}
```

## Graceful Shutdown パターン

```rust
use tokio::signal;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let server_handle = task::spawn(run_server(shutdown_rx.clone()));
    let worker_handle = task::spawn(run_worker(shutdown_rx));

    // Ctrl+C を待つ
    signal::ctrl_c().await?;
    println!("シャットダウン開始...");
    let _ = shutdown_tx.send(true);

    // タスクの終了を待つ（タイムアウト付き）
    let timeout = Duration::from_secs(10);
    tokio::select! {
        _ = server_handle => println!("サーバー停止"),
        _ = tokio::time::sleep(timeout) => println!("タイムアウト、強制終了"),
    }

    Ok(())
}

async fn run_server(mut shutdown: watch::Receiver<bool>) {
    loop {
        select! {
            _ = process_request() => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }
    cleanup().await;
}
```

## よくある落とし穴

- **async ブロック内で `.await` を忘れる** — Future は `.await` しないと実行されない
- **`tokio::spawn` 内のパニック** — JoinHandle を `.await` しないとパニックが無視される
- **バッファサイズ 0 の mpsc** — `mpsc::channel(0)` はエラー。最低 1 が必要
- **CPU-heavy な処理を async タスクで実行** — ランタイムをブロックする。`spawn_blocking` を使う
- **async fn の再帰** — Box で包む必要がある：`fn rec() -> BoxFuture<'static, ()>`
