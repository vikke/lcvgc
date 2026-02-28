pub mod handler;
pub mod protocol;

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::engine::evaluator::Evaluator;
use handler::handle_request;
use protocol::Response;

/// TCPサーバーを起動し、JSON-over-TCPプロトコルでリクエストを受け付ける
pub async fn run_server(
    evaluator: Arc<Mutex<Evaluator>>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    eprintln!("lcvgc server listening on port {}", port);

    loop {
        let (stream, addr) = listener.accept().await?;
        eprintln!("Client connected: {}", addr);
        let ev = evaluator.clone();

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str(&line) {
                    Ok(request) => handle_request(&ev, request).await,
                    Err(e) => Response::err(format!("Invalid JSON: {}", e)),
                };

                let json = serde_json::to_string(&response).unwrap_or_default();
                if writer
                    .write_all(format!("{}\n", json).as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            eprintln!("Client disconnected: {}", addr);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn server_accepts_eval_request() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let ev = evaluator.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            if let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str(&line) {
                    Ok(request) => handle_request(&ev, request).await,
                    Err(e) => Response::err(format!("Invalid JSON: {}", e)),
                };
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(format!("{}\n", json).as_bytes()).await;
            }
        });

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        stream
            .write_all(b"{\"type\":\"eval\",\"source\":\"tempo 140\"}\n")
            .await
            .unwrap();

        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(response["success"], true);
    }

    #[tokio::test]
    async fn server_handles_invalid_json() {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let ev = evaluator.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            if let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str(&line) {
                    Ok(request) => handle_request(&ev, request).await,
                    Err(e) => Response::err(format!("Invalid JSON: {}", e)),
                };
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(format!("{}\n", json).as_bytes()).await;
            }
        });

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        stream.write_all(b"not json\n").await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(response["success"], false);
        assert!(response["error"].as_str().unwrap().contains("Invalid JSON"));
    }
}
