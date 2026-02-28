/// ファイル変更監視によるホットリロード機構
///
/// DSLファイル(.cvg)の変更を検知し、自動的にre-evalを実行する。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::engine::evaluator::Evaluator;

/// ファイル変更イベントの種類
#[derive(Debug, Clone, PartialEq)]
pub enum FileChangeEvent {
    /// ファイルが変更された
    Modified(PathBuf),
    /// ファイルが作成された
    Created(PathBuf),
}

/// ホットリロードの設定
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// デバウンス時間（短時間の連続変更をまとめる）
    pub debounce_ms: u64,
    /// 監視対象の拡張子
    pub extensions: Vec<String>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 200,
            extensions: vec!["cvg".to_string(), "lcvgc".to_string()],
        }
    }
}

/// ファイルウォッチャー
pub struct FileWatcher {
    config: WatcherConfig,
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<FileChangeEvent>,
}

impl FileWatcher {
    /// 指定パスの監視を開始する
    pub fn new(path: &Path, config: WatcherConfig) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel(32);
        let extensions = config.extensions.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    let dominated = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    );
                    if !dominated {
                        return;
                    }

                    for path in &event.paths {
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("");
                        if extensions.iter().any(|e| e == ext) {
                            let change = if matches!(event.kind, EventKind::Create(_)) {
                                FileChangeEvent::Created(path.clone())
                            } else {
                                FileChangeEvent::Modified(path.clone())
                            };
                            let _ = tx.blocking_send(change);
                        }
                    }
                }
                Err(e) => {
                    // ログだけ出して続行
                    let _ = e;
                }
            }
        })?;

        watcher.watch(path, RecursiveMode::Recursive)?;

        Ok(Self {
            config,
            _watcher: watcher,
            rx,
        })
    }

    /// 変更イベントを受信する（デバウンス付き）
    pub async fn next_change(&mut self) -> Option<FileChangeEvent> {
        // 最初のイベントを待つ
        let event = self.rx.recv().await?;

        // デバウンス: 短時間内の追加イベントをスキップ
        let debounce = Duration::from_millis(self.config.debounce_ms);
        tokio::time::sleep(debounce).await;

        // バッファに溜まったイベントを消費し、最後のものを返す
        let mut latest = event;
        while let Ok(e) = self.rx.try_recv() {
            latest = e;
        }

        Some(latest)
    }
}

/// ホットリロードループを起動する
///
/// ファイル変更を検知したらevaluatorにre-evalを実行する。
pub async fn run_hot_reload(
    evaluator: Arc<Mutex<Evaluator>>,
    watch_path: PathBuf,
    config: WatcherConfig,
) {
    let path = watch_path.clone();
    let mut watcher = match FileWatcher::new(&path, config) {
        Ok(w) => w,
        Err(e) => {
            error!("ファイル監視の開始に失敗: {}", e);
            return;
        }
    };

    info!("ホットリロード開始: {}", watch_path.display());

    while let Some(event) = watcher.next_change().await {
        let file_path = match &event {
            FileChangeEvent::Modified(p) | FileChangeEvent::Created(p) => p.clone(),
        };

        info!("ファイル変更検知: {}", file_path.display());

        let source = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                warn!("ファイル読み込み失敗: {}: {}", file_path.display(), e);
                continue;
            }
        };

        let mut ev = evaluator.lock().await;
        match ev.eval_source(&source) {
            Ok(results) => {
                info!(
                    "ホットリロード成功: {} ブロック評価 ({})",
                    results.len(),
                    file_path.display()
                );
                debug!("評価結果: {:?}", results);
            }
            Err(e) => {
                warn!("ホットリロード失敗: {}: {}", file_path.display(), e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = WatcherConfig::default();
        assert_eq!(config.debounce_ms, 200);
        assert!(config.extensions.contains(&"cvg".to_string()));
        assert!(config.extensions.contains(&"lcvgc".to_string()));
    }

    #[test]
    fn file_change_event_equality() {
        let a = FileChangeEvent::Modified(PathBuf::from("test.cvg"));
        let b = FileChangeEvent::Modified(PathBuf::from("test.cvg"));
        assert_eq!(a, b);

        let c = FileChangeEvent::Created(PathBuf::from("test.cvg"));
        assert_ne!(a, c);
    }

    #[tokio::test]
    async fn watcher_on_nonexistent_path_returns_error() {
        let result = FileWatcher::new(
            Path::new("/nonexistent/path"),
            WatcherConfig::default(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn watcher_on_valid_path_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let result = FileWatcher::new(dir.path(), WatcherConfig::default());
        assert!(result.is_ok());
    }
}
