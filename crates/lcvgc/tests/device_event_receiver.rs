//! `run_device_event_receiver_with_initial` の結合テスト
//!
//! Evaluator → mpsc → receiver → SharedSinks の経路全体が、build_sink closure
//! 経由で意図通りに動くことを確認する。本番ビルドでは `MidirSink` を生成する
//! closure を渡すが、ここでは `SharedMockSink` を返す closure に差し替え、
//! 同名同 port の no-op 判定や initial_ports による起動時 dedup などの分岐を
//! ハードに検証する。
//!
//! Integration tests for `run_device_event_receiver_with_initial`. Drives the
//! full Evaluator -> mpsc -> receiver -> SharedSinks path with a closure that
//! returns `SharedMockSink` instead of `MidirSink`, exercising the same-name +
//! same-port no-op, the initial_ports startup dedup, and graceful shutdown
//! when the sender is dropped.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use lcvgc::run_device_event_receiver_with_initial;
use lcvgc_core::engine::device_event::DeviceEvent;
use lcvgc_core::engine::evaluator::Evaluator;
use lcvgc_core::engine::midi_sink::SharedMockSink;
use lcvgc_core::engine::playback::{BoxedSink, SharedSinks, SinksNotify};
use tokio::sync::mpsc;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::Notify;

/// build_sink closure が返す anyhow 不要の軽量エラー型
///
/// 本テストでは builder が失敗するケースは扱わないため、`!` 相当のダミーで
/// 十分。`Display` を実装するために単純な struct とする。
///
/// Lightweight error type returned by the test build_sink closures. None of
/// the cases here actually fail, but the closure signature requires a
/// `Display`-implementing error type.
#[derive(Debug)]
struct NeverError;

impl std::fmt::Display for NeverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("never")
    }
}

/// テスト用に共有 sink マップと notify を作成する
///
/// Allocates a fresh `SharedSinks` and `SinksNotify` pair for a single test.
fn fresh_sinks_and_notify() -> (SharedSinks, SinksNotify) {
    let sinks: SharedSinks = Arc::new(TokioMutex::new(HashMap::new()));
    let notify: SinksNotify = Arc::new(Notify::new());
    (sinks, notify)
}

/// receiver の処理完了を notify で待ち受ける（タイムアウト付き）
///
/// `apply_device_event` が成功すると `notify.notify_one()` が呼ばれるので、
/// 1 秒以内に通知が来なければテストは失敗とみなす。
///
/// Awaits `notify` with a 1-second timeout so a missing notification fails
/// the test rather than hanging forever.
async fn wait_notified(notify: &SinksNotify) {
    tokio::time::timeout(Duration::from_secs(1), notify.notified())
        .await
        .expect("receiver did not signal within 1s");
}

/// Evaluator から流れた `DeviceEvent::Upsert` が receiver で受信され、
/// SharedSinks に sink が insert されることを確認する
///
/// Verifies that an `Upsert` produced by the Evaluator reaches the receiver
/// and inserts a sink into the shared map.
#[tokio::test]
async fn evaluator_emits_upsert_received_and_sink_inserted() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    // build_sink: 呼ばれたら新しい SharedMockSink を Box にして返す
    // build_sink: returns a fresh `SharedMockSink` boxed as `BoxedSink`.
    let build_sink = |_name: &str, _port: &str| -> Result<BoxedSink, NeverError> {
        Ok(Box::new(SharedMockSink::new()))
    };

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        HashMap::new(),
        build_sink,
    ));

    // Evaluator を共有 Mutex に詰めて、device_event_tx を登録してから device を eval
    // Wrap Evaluator in a shared Mutex, wire up the tx, then evaluate `device`.
    let evaluator = Arc::new(TokioMutex::new(Evaluator::new(120.0)));
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(tx);
        ev.eval_source("device foo { port px }\n").expect("eval ok");
    }

    // receiver が処理完了するまで待つ
    wait_notified(&notify).await;

    // sinks マップに "foo" が入っていること
    let map = sinks.lock().await;
    assert!(map.contains_key("foo"), "sinks に foo が登録されていない");
    drop(map);

    // 後始末: Evaluator の tx を drop（lock 越しに reset 用 API は無いので
    // Evaluator ごと drop する）。
    drop(evaluator);
    // receiver は tx の最後の参照が drop されると終了する
    let _ = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
}

/// 同名 + 同 port での再定義は no-op（builder が再呼び出しされない）
///
/// Same-name + same-port redefinitions must short-circuit, so the builder is
/// only invoked once.
#[tokio::test]
async fn same_name_same_port_redefinition_is_noop() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    // builder の呼び出し回数を観測するためのカウンタ
    // Counter for observing how many times the builder ran.
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_for_builder = Arc::clone(&call_count);
    let build_sink = move |_name: &str, _port: &str| -> Result<BoxedSink, NeverError> {
        call_count_for_builder.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(SharedMockSink::new()))
    };

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        HashMap::new(),
        build_sink,
    ));

    let evaluator = Arc::new(TokioMutex::new(Evaluator::new(120.0)));
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(tx);
    }

    // 1 回目: 新規追加 → builder が呼ばれる
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source("device foo { port px }\n").expect("eval ok");
    }
    wait_notified(&notify).await;

    // 2 回目: 同 port 再定義 → no-op
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source("device foo { port px }\n").expect("eval ok");
    }
    // no-op の場合は notify が来ないので、わずかに待ってから判定する
    // No-op events do not notify, so just sleep briefly before assertion.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // sinks には "foo" 1 件、builder は 1 回だけ呼ばれている
    let map = sinks.lock().await;
    assert_eq!(map.len(), 1, "sinks には foo の 1 件のみ存在");
    assert!(map.contains_key("foo"));
    drop(map);
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "同名同 port 再定義で builder が再呼び出しされている",
    );

    drop(evaluator);
    let _ = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
}

/// 同名 + 異 port での再定義は sink を作り直す（builder が 2 回呼ばれる）
///
/// Same-name + different-port redefinitions must rebuild the sink, invoking
/// the builder once for each unique port.
#[tokio::test]
async fn same_name_different_port_rebuilds_sink() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    // builder の (name, port) ペアを順序込みで記録する
    // Records the (name, port) tuples passed to the builder, in order.
    let calls: Arc<StdMutex<Vec<(String, String)>>> = Arc::new(StdMutex::new(Vec::new()));
    let calls_for_builder = Arc::clone(&calls);
    let build_sink = move |name: &str, port: &str| -> Result<BoxedSink, NeverError> {
        calls_for_builder
            .lock()
            .expect("calls poisoned")
            .push((name.to_string(), port.to_string()));
        Ok(Box::new(SharedMockSink::new()))
    };

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        HashMap::new(),
        build_sink,
    ));

    let evaluator = Arc::new(TokioMutex::new(Evaluator::new(120.0)));
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(tx);
    }

    // 1 回目: foo / pa
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source("device foo { port pa }\n").expect("eval ok");
    }
    wait_notified(&notify).await;

    // 2 回目: foo / pb（port 違い）
    {
        let mut ev = evaluator.lock().await;
        ev.eval_source("device foo { port pb }\n").expect("eval ok");
    }
    wait_notified(&notify).await;

    // 記録に ("foo","pa") と ("foo","pb") が順に並んでいる
    let recorded = calls.lock().expect("calls poisoned").clone();
    assert_eq!(
        recorded,
        vec![
            ("foo".to_string(), "pa".to_string()),
            ("foo".to_string(), "pb".to_string()),
        ],
        "builder の呼び出し履歴が期待と異なる",
    );

    // sinks には "foo" 1 件のみ（差し替えなので件数は増えない）
    let map = sinks.lock().await;
    assert_eq!(map.len(), 1, "差し替えなので件数は 1");
    assert!(map.contains_key("foo"));
    drop(map);

    drop(evaluator);
    let _ = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
}

/// initial_ports に登録済みの (name, port) と同一の Upsert は dedup される
///
/// Startup events whose (name, port) match `initial_ports` must be filtered
/// without invoking the builder.
#[tokio::test]
async fn initial_ports_dedups_startup_event() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_for_builder = Arc::clone(&call_count);
    let build_sink = move |_name: &str, _port: &str| -> Result<BoxedSink, NeverError> {
        call_count_for_builder.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(SharedMockSink::new()))
    };

    // initial_ports に "foo" -> "px" を入れた状態で起動
    // Seed `initial_ports` with foo -> px before spawning the receiver.
    let mut initial_ports = HashMap::new();
    initial_ports.insert("foo".to_string(), "px".to_string());

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        initial_ports,
        build_sink,
    ));

    let evaluator = Arc::new(TokioMutex::new(Evaluator::new(120.0)));
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(tx);
        ev.eval_source("device foo { port px }\n").expect("eval ok");
    }

    // dedup されるので notify は来ない。短く待ってから状態をチェックする。
    // No notification is expected because the event is deduplicated.
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        0,
        "initial_ports と同 port なら builder は呼ばれない",
    );

    // initial_ports は current_ports の seed 用であり、sinks マップは別管理。
    // dedup された場合は sinks に何も insert されない。
    // initial_ports only seeds current_ports; the sinks map stays untouched
    // when the event is deduplicated.
    let map = sinks.lock().await;
    assert!(
        map.is_empty(),
        "dedup の場合、sinks に新たな insert は発生しない",
    );
    drop(map);

    drop(evaluator);
    let _ = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
}

/// initial_ports と name が同じでも port が異なれば dedup されない
///
/// `initial_ports` does not block events whose port differs from the seed
/// value; the builder is still invoked.
#[tokio::test]
async fn initial_ports_does_not_block_different_port() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    let calls: Arc<StdMutex<Vec<(String, String)>>> = Arc::new(StdMutex::new(Vec::new()));
    let calls_for_builder = Arc::clone(&calls);
    let build_sink = move |name: &str, port: &str| -> Result<BoxedSink, NeverError> {
        calls_for_builder
            .lock()
            .expect("calls poisoned")
            .push((name.to_string(), port.to_string()));
        Ok(Box::new(SharedMockSink::new()))
    };

    let mut initial_ports = HashMap::new();
    initial_ports.insert("foo".to_string(), "px".to_string());

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        initial_ports,
        build_sink,
    ));

    let evaluator = Arc::new(TokioMutex::new(Evaluator::new(120.0)));
    {
        let mut ev = evaluator.lock().await;
        ev.set_device_event_tx(tx);
        ev.eval_source("device foo { port py }\n").expect("eval ok");
    }
    wait_notified(&notify).await;

    let recorded = calls.lock().expect("calls poisoned").clone();
    assert_eq!(
        recorded,
        vec![("foo".to_string(), "py".to_string())],
        "port 違いの場合は builder が 1 回呼ばれる",
    );

    let map = sinks.lock().await;
    assert!(map.contains_key("foo"));
    drop(map);

    drop(evaluator);
    let _ = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
}

/// tx を drop すると receiver loop が抜けて終了する
///
/// Dropping the sender closes the channel and the receiver loop must exit.
#[tokio::test]
async fn dropping_tx_terminates_receiver() {
    let (tx, rx) = mpsc::unbounded_channel::<DeviceEvent>();
    let (sinks, notify) = fresh_sinks_and_notify();

    let build_sink = |_name: &str, _port: &str| -> Result<BoxedSink, NeverError> {
        Ok(Box::new(SharedMockSink::new()))
    };

    let receiver_handle = tokio::spawn(run_device_event_receiver_with_initial(
        rx,
        Arc::clone(&sinks),
        Arc::clone(&notify),
        HashMap::new(),
        build_sink,
    ));

    // 直ちに tx を drop → rx.recv().await が None を返し、loop が抜ける
    // Drop the sender immediately: `rx.recv()` returns None and the loop ends.
    drop(tx);

    let result = tokio::time::timeout(Duration::from_secs(1), receiver_handle).await;
    assert!(
        result.is_ok(),
        "tx drop 後 1 秒以内に receiver が終了していない",
    );
    let join_result = result.unwrap();
    assert!(join_result.is_ok(), "receiver タスクが panic で終了した");
}
