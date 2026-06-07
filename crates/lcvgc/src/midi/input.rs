//! MIDI 入力購読モジュール。
//!
//! `midir::MidiInput::connect` を薄くラップし、指定した入力ポートに届く生
//! MIDI バイト列をコールバックへ流す。返す [`MidiInputSubscription`] が
//! 生存している間だけ購読が有効で、drop すると接続が閉じられる。
//!
//! `server` の `subscribe_midi_in` 処理から呼ばれ、コールバック内で
//! [`crate::midi::message::MidiMessage::from_bytes`] →
//! [`crate::midi::transcribe::message_to_dsl`] を経由して DSL トークンを
//! 組み立て、購読中クライアントへプッシュする。
//!
//! Thin wrapper over `midir::MidiInput::connect` that forwards raw MIDI bytes
//! from a named input port to a callback. The subscription stays active while
//! the returned [`MidiInputSubscription`] is alive; dropping it closes the
//! connection. Used by the server's `subscribe_midi_in` handling.

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::midi::MidiError;

/// MIDI 入力購読ハンドル。
///
/// 内部に `midir` の接続を保持する。この値を drop すると購読が解除される
/// （接続クローズ）。RAII により、購読のライフタイムを TCP 接続のスコープ等に
/// 紐付けられる。
///
/// Handle for an active MIDI input subscription. Holds the underlying `midir`
/// connection; dropping it unsubscribes (closes the connection). Lets callers
/// tie a subscription's lifetime to a scope (e.g. a TCP connection) via RAII.
pub struct MidiInputSubscription {
    /// `midir` の入力接続。フィールドとして保持することでコールバックが
    /// 動き続ける。`_` 始まりで「保持のためだけに存在する」ことを示す。
    /// The `midir` input connection; kept alive solely to keep the callback
    /// running (hence the leading underscore).
    _conn: MidiInputConnection<()>,
}

/// 名前で MIDI 入力ポートに接続し、受信バイト列を `on_message` へ流す。
///
/// SysEx・タイミングクロック・アクティブセンシングは [`Ignore::All`] で無視し、
/// ノート/CC 等のチャンネルボイスメッセージのみコールバックに届くようにする。
/// コールバックは `midir` の受信スレッド上で呼ばれるため、`Send + 'static` を
/// 要求し、内部でブロッキングや重い処理を行わないこと。
///
/// Connects to a MIDI input port by name and forwards received bytes to
/// `on_message`. SysEx, timing clock and active sensing are filtered out via
/// [`Ignore::All`] so only channel-voice messages reach the callback. The
/// callback runs on `midir`'s receive thread, hence `Send + 'static`; it must
/// not block or do heavy work.
///
/// # 引数 / Arguments
/// * `port_name` - 接続先の入力ポート名（`list_input_ports()` の戻り値のいずれか）
/// * `on_message` - 受信した生 MIDI バイト列を受け取るコールバック
///
/// # 戻り値 / Returns
/// 購読ハンドル。生存中は購読が有効。
/// A subscription handle; the subscription is active while it lives.
///
/// # Errors
/// 入力クライアント生成・ポート探索・接続のいずれかに失敗した場合
/// `MidiError` を返す。
pub fn subscribe<F>(port_name: &str, mut on_message: F) -> Result<MidiInputSubscription, MidiError>
where
    F: FnMut(&[u8]) + Send + 'static,
{
    let mut input =
        MidiInput::new("lcvgc-input").map_err(|e| MidiError::ConnectionError(e.to_string()))?;
    // ノート入力に不要なメッセージ（SysEx / Timing / ActiveSense）を捨てる。
    // Drop messages irrelevant to note input (SysEx / Timing / ActiveSense).
    input.ignore(Ignore::All);

    let ports = input.ports();
    let port = ports
        .iter()
        .find(|p| input.port_name(p).as_deref() == Ok(port_name))
        .cloned()
        .ok_or_else(|| MidiError::PortNotFound(port_name.to_string()))?;

    let conn = input
        .connect(
            &port,
            port_name,
            move |_timestamp, bytes, _| on_message(bytes),
            (),
        )
        .map_err(|e| MidiError::ConnectionError(e.to_string()))?;

    Ok(MidiInputSubscription { _conn: conn })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 存在しないポート名は `PortNotFound` を返す。
    /// `MidiInput::new` が ALSA シーケンサ初期化を要するため、MIDI サブシステムの
    /// ない CI/コンテナでは `ConnectionError` で先に失敗する。よって実環境向けに
    /// `#[ignore]`（`port.rs` の列挙テストと同じ扱い）。
    /// A nonexistent port name yields `PortNotFound`. Requires a working MIDI
    /// subsystem (`MidiInput::new`), so it is `#[ignore]`d like the enumeration
    /// tests in `port.rs`; without ALSA it fails earlier with `ConnectionError`.
    #[test]
    #[ignore] // 実MIDIサブシステムが必要 / requires a real MIDI subsystem
    fn subscribe_unknown_port_returns_port_not_found() {
        let result = subscribe("definitely-not-a-real-port-12345", |_bytes| {});
        match result {
            Err(MidiError::PortNotFound(name)) => {
                assert_eq!(name, "definitely-not-a-real-port-12345");
            }
            other => panic!("expected PortNotFound, got {:?}", other.map(|_| "Ok")),
        }
    }
}
