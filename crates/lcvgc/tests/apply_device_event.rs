//! `apply_device_event` の結合テスト（PR #54）
//!
//! 実 MIDI ポートを伴う `MidirSink` ではなく、`SharedMockSink` を `BoxedSink`
//! に詰めて build closure から返すことで、sink マップの差し替え挙動・
//! AllNotesOff 送出・notify 通知・builder 失敗時の sink クリア挙動を検証する。
//!
//! Integration tests for `apply_device_event` (PR #54). Uses `SharedMockSink`
//! returned from a sink-builder closure instead of a real MIDI-backed
//! `MidirSink`, so the suite verifies sink map swap behaviour, AllNotesOff
//! emission to the old sink, notifier wakeups, and the build-failure cleanup
//! semantics without requiring an actual MIDI port.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tokio::time::timeout;

use lcvgc::{apply_device_event, DeviceApplyOutcome};
use lcvgc_core::engine::device_event::DeviceEvent;
use lcvgc_core::engine::midi_sink::SharedMockSink;
use lcvgc_core::engine::playback::{BoxedSink, SharedSinks, SinksNotify};
use lcvgc_core::midi::message::MidiMessage;

/// 空の `SharedSinks` を生成するテスト用ヘルパ。
/// Helper that creates an empty `SharedSinks` for tests.
fn empty_shared_sinks() -> SharedSinks {
    Arc::new(Mutex::new(HashMap::new()))
}

/// 新しい `SinksNotify` を生成するテスト用ヘルパ。
/// Helper that creates a fresh `SinksNotify` for tests.
fn new_notify() -> SinksNotify {
    Arc::new(Notify::new())
}

/// 指定 `SharedMockSink` を呼び出し毎に返す build closure を作る。
///
/// 返された `Arc<Mutex<Vec<(String, String)>>>` には builder が呼び出された
/// 際の `(name, port)` 引数が記録される。テスト側で「何回呼ばれたか」「どの
/// 引数で呼ばれたか」を検証するのに使う。
///
/// Build a closure that returns the given `SharedMockSink` on every call.
/// The accompanying `Arc<Mutex<Vec<(String, String)>>>` records each
/// `(name, port)` invocation for assertions in the test body.
type BuildLog = Arc<std::sync::Mutex<Vec<(String, String)>>>;

/// 成功 builder closure を構築するヘルパ。
/// Build a closure that always succeeds and returns clones of `handle`.
fn make_success_builder(
    handle: SharedMockSink,
) -> (
    impl Fn(&str, &str) -> Result<BoxedSink, Infallible>,
    BuildLog,
) {
    let log: BuildLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log_for_closure = log.clone();
    let closure = move |name: &str, port: &str| -> Result<BoxedSink, Infallible> {
        log_for_closure
            .lock()
            .expect("build log poisoned")
            .push((name.to_string(), port.to_string()));
        Ok(Box::new(handle.clone()))
    };
    (closure, log)
}

/// 常に失敗する builder closure を構築するヘルパ。
/// Build a closure that always returns `Err("connect failed")`.
fn make_failing_builder() -> (
    impl Fn(&str, &str) -> Result<BoxedSink, &'static str>,
    BuildLog,
) {
    let log: BuildLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log_for_closure = log.clone();
    let closure = move |name: &str, port: &str| -> Result<BoxedSink, &'static str> {
        log_for_closure
            .lock()
            .expect("build log poisoned")
            .push((name.to_string(), port.to_string()));
        Err("connect failed")
    };
    (closure, log)
}

/// 1. 空の sinks に Upsert を適用すると、新 sink が登録され notify が発火する。
///
/// Verifies that applying `Upsert` against empty sinks inserts the new sink
/// and triggers `notify_one()`.
#[tokio::test]
async fn apply_inserts_new_sink_and_notifies() {
    let sinks = empty_shared_sinks();
    let notify = new_notify();
    let new_handle = SharedMockSink::new();
    let (builder, build_log) = make_success_builder(new_handle.clone());

    apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "port1".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // sinks に "synth_a" が登録されている / sink registered under "synth_a"
    {
        let map = sinks.lock().await;
        assert!(map.contains_key("synth_a"), "新 sink が登録されているはず");
        assert_eq!(map.len(), 1, "他の device は登録されていないはず");
    }

    // builder が (name, port) で 1 回呼ばれている / builder invoked once with (name, port)
    {
        let log = build_log.lock().expect("build log poisoned");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], ("synth_a".to_string(), "port1".to_string()));
    }

    // notify_one() が発火しているので即座に notified() が完了する
    // notify_one() must have fired so notified() resolves immediately
    timeout(Duration::from_millis(50), notify.notified())
        .await
        .expect("notify_one() が発火しているはず（timeout）");

    // 新 sink には apply 時点で MIDI イベントは送られていない
    // The new sink should not have received any MIDI messages from apply itself
    assert!(
        new_handle.snapshot().is_empty(),
        "apply は build 直後に新 sink へ MIDI を送らないはず"
    );
}

/// 2. 既存 sink がある状態で Upsert を適用すると、旧 sink に AllNotesOff が
///    16 件送られた後、新 sink に差し替わる。
///
/// Verifies that an `Upsert` against an existing entry sends 16 AllNotesOff
/// messages (one per channel) to the old sink before swapping in the new one.
#[tokio::test]
async fn apply_to_existing_sink_sends_all_notes_off_to_old_sink() {
    let sinks = empty_shared_sinks();
    let notify = new_notify();

    // 旧 sink を予め登録 / pre-register an old sink
    let old_handle = SharedMockSink::new();
    {
        let mut map = sinks.lock().await;
        map.insert("synth_a".to_string(), Box::new(old_handle.clone()));
    }

    // 新 sink は別 instance / use a distinct instance for the new sink
    let new_handle = SharedMockSink::new();
    let (builder, build_log) = make_success_builder(new_handle.clone());

    apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "port_changed".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // 旧 sink には channel 0..15 の AllNotesOff (CC#123, value=0) が記録されている
    // Old sink must have recorded AllNotesOff (CC#123 value=0) for channels 0..15
    let old_msgs = old_handle.snapshot();
    assert_eq!(
        old_msgs.len(),
        16,
        "旧 sink には 16 channel 分の AllNotesOff が送られるはず"
    );
    for (idx, msg) in old_msgs.iter().enumerate() {
        match msg {
            MidiMessage::ControlChange { channel, cc, value } => {
                assert_eq!(
                    *channel as usize, idx,
                    "channel は 0..15 を順に網羅するはず"
                );
                assert_eq!(*cc, 123, "AllNotesOff の CC 番号は 123");
                assert_eq!(*value, 0, "AllNotesOff の value は 0");
            }
            other => panic!("旧 sink への送出は ControlChange のはず: {other:?}"),
        }
    }

    // 新 sink には何も送られていない（apply は build 直後に MIDI を送らない）
    // The new sink should not have received anything from apply itself
    assert!(
        new_handle.snapshot().is_empty(),
        "新 sink は build 直後には空のはず"
    );

    // builder は新しい port 名で 1 回だけ呼ばれている / builder called once with new port
    {
        let log = build_log.lock().expect("build log poisoned");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], ("synth_a".to_string(), "port_changed".to_string()));
    }

    // sinks マップから取り出した sink は新 handle と同一 buffer を共有しているはず。
    // 新 sink を `BoxedSink` 経由で send() させ、new_handle.snapshot() で観測できる
    // ことを以て identity を確認する。
    //
    // Confirm the sink stored in the map is the new handle by sending a probe
    // message through the boxed sink and observing it through `new_handle`.
    let probe = MidiMessage::NoteOn {
        channel: 7,
        note: 42,
        velocity: 99,
    };
    {
        let mut map = sinks.lock().await;
        let sink = map
            .get_mut("synth_a")
            .expect("新 sink が登録されているはず");
        sink.send(&probe).expect("send は成功するはず");
    }

    // probe は new_handle 側に積まれ、old_handle 側には積まれない
    // The probe lands on the new handle and never on the old handle
    let new_msgs = new_handle.snapshot();
    assert_eq!(
        new_msgs.len(),
        1,
        "probe は新 sink にだけ届くはず（new_handle と sinks の sink が同一）"
    );
    assert_eq!(new_msgs[0], probe);
    assert_eq!(
        old_handle.snapshot().len(),
        16,
        "旧 sink には probe が届いていないはず（差し替え後は使われない）"
    );
}

/// 3. builder が失敗するケース：旧 sink には AllNotesOff が送られた上で
///    drop され、sinks マップから当該 device が削除されたまま残る。
///
/// Verifies the documented build-failure semantics: the old sink still
/// receives the AllNotesOff barrage and is dropped, and the sinks map ends
/// up *without* the device entry (build failure means disconnect).
#[tokio::test]
async fn apply_with_failing_builder_logs_warn_and_clears_sink() {
    let sinks = empty_shared_sinks();
    let notify = new_notify();

    // 旧 sink を予め登録 / pre-register an old sink
    let old_handle = SharedMockSink::new();
    {
        let mut map = sinks.lock().await;
        map.insert("synth_a".to_string(), Box::new(old_handle.clone()));
    }

    let (builder, build_log) = make_failing_builder();

    apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "broken_port".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // 旧 sink には AllNotesOff 16 件が届いている（drop 前のクリーンアップ）
    // Old sink received the full 16-channel AllNotesOff barrage before drop
    assert_eq!(
        old_handle.snapshot().len(),
        16,
        "旧 sink には AllNotesOff が 16 件送られているはず"
    );

    // builder が (name, port) で 1 回呼ばれて失敗している / builder was attempted once
    {
        let log = build_log.lock().expect("build log poisoned");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], ("synth_a".to_string(), "broken_port".to_string()));
    }

    // 現実装の semantics: build 失敗時、map から remove したまま再 insert しない
    // ため "synth_a" キーは消える（= 切断扱い）。
    //
    // Documented semantics: on build failure the device key remains removed
    // (treated as a disconnect) — we assert that here so future regressions
    // are caught explicitly.
    {
        let map = sinks.lock().await;
        assert!(
            !map.contains_key("synth_a"),
            "build 失敗時、sinks マップから device は削除されたまま残る仕様"
        );
        assert!(map.is_empty(), "他に entry が無いことも確認");
    }
}

/// 4. builder 失敗時に notify が発火しないことを timeout で明示的に検証する。
///
/// Asserts via `tokio::time::timeout` that `notify_one()` is *not* fired when
/// the builder returns `Err`.
#[tokio::test]
async fn apply_emits_no_notify_on_build_failure() {
    let sinks = empty_shared_sinks();
    let notify = new_notify();

    // 旧 sink あり/なしどちらでも notify は発火しないが、3 のテストとの差別化として
    // ここでは「旧 sink 無し（純粋な新規追加が失敗）」を扱う。
    //
    // Cover the "no prior sink, build still fails" path to complement test #3.
    let (builder, _build_log) = make_failing_builder();

    apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "broken_port".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // notify_one() が呼ばれていなければ notified() は 50ms で完了しない
    // notified() must NOT resolve within 50ms because notify_one() never ran
    let waited = timeout(Duration::from_millis(50), notify.notified()).await;
    assert!(
        waited.is_err(),
        "build 失敗時は notify_one() が呼ばれないはず（timeout になるべき）"
    );

    // 旧 sink が無いのでマップは空のまま / map stays empty (no prior sink)
    let map = sinks.lock().await;
    assert!(map.is_empty(), "旧 sink 無し + build 失敗なら sinks は空");
}

/// 5. build 成功時、戻り値が `DeviceApplyOutcome::Connected { name }` であることを検証する。
///
/// Verifies that on a successful build the function returns
/// `DeviceApplyOutcome::Connected { name }` carrying the device name verbatim
/// (PR #55). The receiver loop relies on this variant to clear any prior
/// connection error recorded against that device on the Evaluator.
#[tokio::test]
async fn apply_returns_connected_outcome_on_success() {
    let sinks = empty_shared_sinks();
    let notify = new_notify();
    let new_handle = SharedMockSink::new();
    let (builder, _build_log) = make_success_builder(new_handle.clone());

    let outcome = apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "port_ok".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // 戻り値は Connected variant で、name には Upsert で渡した device 名が入る
    // The outcome must be the `Connected` variant carrying the upserted name
    match outcome {
        DeviceApplyOutcome::Connected { name } => assert_eq!(name, "synth_a"),
        other => panic!("expected Connected, got {other:?}"),
    }
}

/// 6. build 失敗時、戻り値が `DeviceApplyOutcome::Failed { name, port, message }`
///    で、`message` に builder の `Display` 出力が含まれることを検証する。
///
/// Verifies that on builder failure the function returns
/// `DeviceApplyOutcome::Failed { name, port, message }` with the builder's
/// `Display` output forwarded into `message`, so the receiver loop can call
/// `Evaluator::record_device_connection_error(name, port, message)` directly.
#[tokio::test]
async fn apply_returns_failed_outcome_on_build_error() {
    /// 専用エラー型: builder の `Display` 出力を outcome の `message` に
    /// パススルーできることを確認するためのテスト用型。
    /// Test-local error whose `Display` output is asserted against
    /// `DeviceApplyOutcome::Failed::message`.
    #[derive(Debug)]
    struct BuildErr;
    impl std::fmt::Display for BuildErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("build failed: connection refused")
        }
    }

    let sinks = empty_shared_sinks();
    let notify = new_notify();

    // BuildErr を返す builder を局所定義（既存ヘルパは &'static str 用）
    // Inline builder returning `BuildErr`; the existing helper is wired for
    // `&'static str` errors so we roll a dedicated one here.
    let builder = |_name: &str, _port: &str| -> Result<BoxedSink, BuildErr> { Err(BuildErr) };

    let outcome = apply_device_event(
        DeviceEvent::Upsert {
            name: "synth_a".to_string(),
            port: "port_x".to_string(),
        },
        &sinks,
        &notify,
        &builder,
    )
    .await;

    // Failed variant で name / port が透過されており、message に builder の
    // Display 出力（"connection refused"）が含まれている
    // Failed variant carries name/port verbatim and message contains the
    // builder's Display output ("connection refused").
    match outcome {
        DeviceApplyOutcome::Failed {
            name,
            port,
            message,
        } => {
            assert_eq!(name, "synth_a");
            assert_eq!(port, "port_x");
            assert!(
                message.contains("connection refused"),
                "message='{message}'"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
