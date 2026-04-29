//! Evaluator が `Block::Device` を評価したときに発火するイベント。
//!
//! `lcvgc` バイナリ側 (main.rs) はこの receiver を購読し、`MidirSink` を
//! 動的に構築・差し替えして `PlaybackDriver` の共有 sink マップを更新する。
//! Evaluator 自身は MIDI ポート接続ロジックを持たないため、core から
//! バイナリ層への一方向通知としてこの enum を介する。
//!
//! Event emitted by `Evaluator` after evaluating a `Block::Device`.
//! The `lcvgc` binary subscribes to the receiver, builds/swaps the
//! corresponding `MidirSink`, and updates the `PlaybackDriver`'s shared
//! sink map. Keeps MIDI-port plumbing out of `lcvgc-core`, flowing as a
//! one-way notification from core to the binary.

/// Device 定義の追加・更新通知 / Device definition upsert notification
///
/// 現状は `Upsert` 1 種類のみ。同名 device が再定義された場合も同じ variant
/// を emit し、port 同一性の判定（no-op か張り替えか）は受信側の責務。
/// 削除は今のところ対応しない（PR #54 の方針）。
///
/// Currently only `Upsert`. Same-name redefinitions also emit `Upsert`;
/// determining whether the port changed (no-op vs. rebuild) is the
/// receiver's responsibility. Deletion is intentionally out of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// 新規追加または同名再定義 / New device or same-name redefinition
    ///
    /// * `name` - DSL 上の論理 device 名 / logical device name from the DSL
    /// * `port` - 接続先 MIDI ポート名 / target MIDI port name
    Upsert { name: String, port: String },
}

/// Evaluator → 受信側へ `DeviceEvent` を送るための tx ハンドル。
/// `unbounded` を採用しているのは、device の eval 頻度が低くバックプレッシャ
/// が不要であり、Evaluator 側を `async` にしたくないため。
///
/// Sender handle for shipping `DeviceEvent`s from the Evaluator to the
/// receiver. `unbounded` keeps the Evaluator non-async; device eval is
/// rare enough that backpressure is unnecessary.
pub type DeviceEventTx = tokio::sync::mpsc::UnboundedSender<DeviceEvent>;

/// 対応する receiver 側ハンドル / Matching receiver handle
pub type DeviceEventRx = tokio::sync::mpsc::UnboundedReceiver<DeviceEvent>;
