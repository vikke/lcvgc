/// リクエストハンドラーモジュール
/// Request handler module
pub mod handler;
/// プロトコル定義モジュール（リクエスト・レスポンス型）
/// Protocol definition module (request/response types)
pub mod protocol;

use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::engine::evaluator::Evaluator;
use crate::midi::input::{subscribe as subscribe_midi_input, MidiInputSubscription};
use crate::midi::message::MidiMessage;
use crate::midi::transcribe::message_to_dsl;
use handler::handle_request;
use protocol::{Request, Response, ServerEvent};

/// `Response` を JSON 行へシリアライズし、出力集約チャネルへ送る。
///
/// 送信先 writer タスクが既に終了している場合（クライアント切断後など）は
/// 送信エラーを黙って捨てる。受信ループ側は reader の EOF で別途終了するため、
/// ここでの失敗を伝播させる必要はない。
///
/// Serializes a `Response` to a JSON line and pushes it to the outbound
/// channel, ignoring send errors (e.g. after the writer task has stopped).
fn send_response(out_tx: &UnboundedSender<String>, resp: &Response) {
    let json = serde_json::to_string(resp).unwrap_or_default();
    let _ = out_tx.send(json);
}

/// 指定ポートの MIDI 入力購読を開始し、受信ノートを `midi_in_event` として
/// 出力集約チャネルへ流す購読ハンドルを返す。
///
/// `midir` の受信スレッド上で動くコールバック内では、生バイト列を
/// [`MidiMessage::from_bytes`] → [`message_to_dsl`] で DSL トークン化し、
/// 発音イベント（NoteOn vel>0）のみを [`ServerEvent::MidiInEvent`] として
/// `out_tx` に送る。`out_tx.send` は非同期ランタイム外からでも安全に呼べる。
///
/// Starts a MIDI input subscription on `port`, forwarding received notes as
/// `midi_in_event` lines to the outbound channel. The `midir` receive-thread
/// callback transcribes raw bytes via [`MidiMessage::from_bytes`] →
/// [`message_to_dsl`] and emits only note-on events. `out_tx.send` is safe to
/// call off the async runtime.
///
/// # Errors
/// ポート接続に失敗した場合は `MidiError` を返す。
fn start_midi_subscription(
    port: &str,
    out_tx: UnboundedSender<String>,
) -> Result<MidiInputSubscription, crate::midi::MidiError> {
    subscribe_midi_input(port, move |bytes| {
        let Some(msg) = MidiMessage::from_bytes(bytes) else {
            return;
        };
        let Some(dsl) = message_to_dsl(&msg) else {
            return;
        };
        let note = match msg {
            MidiMessage::NoteOn { note, .. } => note,
            _ => return,
        };
        let event = ServerEvent::MidiInEvent {
            dsl,
            note,
            raw: bytes.to_vec(),
        };
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = out_tx.send(json);
        }
    })
}

/// 1 クライアント接続を処理する。
///
/// 受信ループ（reader）と送信（writer）を分離し、全出力を `mpsc<String>` の
/// 出力集約チャネル経由で 1 本の writer タスクへ直列化する。これにより、
/// リクエストへの 1:1 レスポンスと、MIDI 入力購読由来の非同期プッシュ
/// （[`ServerEvent::MidiInEvent`]）を、行が混ざらないよう同一接続へ書き込める。
///
/// MIDI 入力購読ハンドルは本関数のスコープに保持し、接続終了時に drop して
/// 自動的に購読解除する（1 接続 1 購読、再 subscribe で張り替え）。
///
/// Handles a single client connection. The read loop and writes are decoupled:
/// every outbound line goes through an `mpsc<String>` to a single writer task,
/// so 1:1 responses and async MIDI-input pushes interleave on the same
/// connection without corrupting lines. The subscription handle is scoped to
/// this function and dropped (auto-unsubscribe) when the connection ends.
async fn handle_connection(evaluator: Arc<Mutex<Evaluator>>, stream: TcpStream) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // 出力集約チャネル: レスポンスと非同期イベントを 1 本の writer へ直列化。
    // Outbound channel: serialize responses and async events onto one writer.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();

    // writer タスク: 受け取った JSON 行を到着順に書き出す。
    // Writer task: writes received JSON lines in arrival order.
    let writer_task = tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            if writer
                .write_all(format!("{}\n", line).as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // このコネクションの MIDI 入力購読ハンドル（drop で購読解除）。
    // This connection's MIDI input subscription handle (drop = unsubscribe).
    let mut midi_sub: Option<MidiInputSubscription> = None;

    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<Request>(&line) {
            Ok(Request::SubscribeMidiIn { port }) => {
                let resp = match start_midi_subscription(&port, out_tx.clone()) {
                    Ok(sub) => {
                        // 旧購読は新ハンドル代入時に drop され解除される。
                        // Any previous subscription is dropped on reassignment.
                        midi_sub = Some(sub);
                        info!("MIDI入力購読開始: {}", port);
                        Response::ok(format!("subscribed: {}", port))
                    }
                    Err(e) => {
                        warn!("MIDI入力購読失敗: {} ({})", port, e);
                        Response::err(e.to_string())
                    }
                };
                send_response(&out_tx, &resp);
            }
            Ok(Request::UnsubscribeMidiIn) => {
                // 購読ハンドルを drop して解除する（未購読でも冪等に成功）。
                // Drop the handle to unsubscribe (idempotent if not subscribed).
                midi_sub = None;
                send_response(&out_tx, &Response::ok("unsubscribed"));
            }
            Ok(request) => {
                let response = handle_request(&evaluator, request).await;
                send_response(&out_tx, &response);
            }
            Err(e) => {
                send_response(&out_tx, &Response::err(format!("Invalid JSON: {}", e)));
            }
        }
    }

    // クライアント切断: 購読を解除し、out_tx を落として writer を畳む。
    // Disconnect: unsubscribe, drop out_tx to end the writer task.
    drop(midi_sub);
    drop(out_tx);
    let _ = writer_task.await;
}

/// TCPサーバーを起動し、JSON-over-TCPプロトコルでリクエストを受け付ける
/// Starts a TCP server that accepts requests via JSON-over-TCP protocol
///
/// # 引数 / Arguments
/// * `evaluator` - 共有されたDSL評価エンジン / Shared DSL evaluator engine
/// * `port` - リッスンするTCPポート番号 / TCP port number to listen on
///
/// # 戻り値 / Returns
/// サーバーが停止した場合のResult / Result when the server stops
///
/// # エラー / Errors
/// TCPバインドやI/Oエラー時にエラーを返す / Returns error on TCP bind or I/O failures
pub async fn run_server(
    evaluator: Arc<Mutex<Evaluator>>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    info!("lcvgc server listening on port {}", port);

    loop {
        let (stream, addr) = listener.accept().await?;
        info!("Client connected: {}", addr);
        let ev = evaluator.clone();

        tokio::spawn(async move {
            handle_connection(ev, stream).await;
            info!("Client disconnected: {}", addr);
        });
    }
}

#[cfg(test)]
mod tests {
    // `super::*` brings tokio's `AsyncBufReadExt` / `AsyncWriteExt` / `BufReader`
    // / `TcpStream` / `TcpListener` into scope (re-imported from the parent's
    // private `use`s), so no additional imports are needed here.
    use super::*;

    /// 実際の `handle_connection` を 1 接続だけ駆動するテストサーバーを立て、
    /// 接続済みクライアントストリームを返す。これにより、本番経路（出力集約 +
    /// writer タスク + subscribe 分岐）をそのまま検証できる。
    /// Spins up a test server that drives the real `handle_connection` for one
    /// connection and returns a connected client stream.
    async fn connect_to_handler() -> TcpStream {
        let evaluator = Arc::new(Mutex::new(Evaluator::new(120.0)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(evaluator, stream).await;
        });

        TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap()
    }

    /// `stream` に 1 行送り、1 行の JSON レスポンスを受け取ってパースする。
    /// Sends one request line and reads back one parsed JSON response line.
    async fn round_trip(stream: &mut TcpStream, request_line: &str) -> serde_json::Value {
        stream
            .write_all(format!("{}\n", request_line).as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[tokio::test]
    async fn server_accepts_eval_request() {
        let mut stream = connect_to_handler().await;
        let response = round_trip(&mut stream, r#"{"type":"eval","source":"tempo 140"}"#).await;
        assert_eq!(response["success"], true);
    }

    #[tokio::test]
    async fn server_handles_invalid_json() {
        let mut stream = connect_to_handler().await;
        let response = round_trip(&mut stream, "not json").await;
        assert_eq!(response["success"], false);
        assert!(response["error"].as_str().unwrap().contains("Invalid JSON"));
    }

    /// `unsubscribe_midi_in` は未購読でも冪等に成功レスポンスを返す
    /// （ハードウェア非依存）。
    /// `unsubscribe_midi_in` succeeds idempotently even when not subscribed.
    #[tokio::test]
    async fn unsubscribe_midi_in_is_idempotent_success() {
        let mut stream = connect_to_handler().await;
        let response = round_trip(&mut stream, r#"{"type":"unsubscribe_midi_in"}"#).await;
        assert_eq!(response["success"], true);
        assert_eq!(response["message"], "unsubscribed");
    }

    /// 存在しない（または MIDI サブシステム不在の）ポートへの購読要求は、
    /// 接続を切らずにエラーレスポンスを返す。
    /// Subscribing to an unavailable port returns an error response without
    /// tearing down the connection.
    #[tokio::test]
    async fn subscribe_midi_in_bad_port_returns_error() {
        let mut stream = connect_to_handler().await;
        let response = round_trip(
            &mut stream,
            r#"{"type":"subscribe_midi_in","port":"no-such-midi-port-zzz"}"#,
        )
        .await;
        assert_eq!(response["success"], false);
        assert!(response["error"].is_string());

        // 接続は生きており、後続の通常リクエストにも応答できる。
        // The connection stays alive and still serves normal requests.
        let status = round_trip(&mut stream, r#"{"type":"status"}"#).await;
        assert_eq!(status["success"], true);
    }
}
