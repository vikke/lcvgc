use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::evaluator::Evaluator;

use super::protocol::{Request, Response};

/// リクエストを処理してレスポンスを返す
pub async fn handle_request(evaluator: &Arc<Mutex<Evaluator>>, request: Request) -> Response {
    match request {
        Request::Eval { source } => {
            let mut ev = evaluator.lock().await;
            match ev.eval_source(&source) {
                Ok(results) => {
                    let msg = results
                        .iter()
                        .map(|r| format!("{:?}", r))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Response::ok(msg)
                }
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::Load { path } => {
            let mut ev = evaluator.lock().await;
            match ev.load_file(&path) {
                Ok(results) => Response::ok(format!("{} blocks evaluated", results.len())),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::Status => {
            let ev = evaluator.lock().await;
            Response::ok(format!(
                "BPM: {:.1}, State: {:?}",
                ev.bpm(),
                ev.state().state()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_eval_success() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Eval {
            source: "tempo 140".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        assert!(resp.message.unwrap().contains("TempoChanged"));
    }

    #[tokio::test]
    async fn handle_eval_parse_error() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Eval {
            source: "invalid !@# syntax".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn handle_load_not_found() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Load {
            path: "/nonexistent/file.cvg".into(),
        };
        let resp = handle_request(&ev, req).await;
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn handle_status() {
        let ev = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let req = Request::Status;
        let resp = handle_request(&ev, req).await;
        assert!(resp.success);
        let msg = resp.message.unwrap();
        assert!(msg.contains("BPM: 120.0"));
        assert!(msg.contains("Stopped"));
    }
}
