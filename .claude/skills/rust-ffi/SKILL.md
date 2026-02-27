---
name: rust-ffi
description: RustのFFI（Foreign Function Interface）を支援するスキル。C/C++ライブラリのバインディング生成（bindgen）、Rustライブラリのcbindgen公開、WindowsのDLL/COM連携、WSL2↔Windows IPC、共有メモリ、名前付きパイプなどをカバー。「FFI」「バインディング」「bindgen」「cbindgen」「extern」「C言語連携」「DLL」「Windows連携」「WSL」「IPC」「共有メモリ」「名前付きパイプ」などの話題が出たら必ずこのスキルを使うこと。クロスプラットフォーム通信の設計にも使うこと。
---

# Rust FFI スキル

Rustと他言語の連携、およびプロセス間通信のパターンガイド。

## C ライブラリの呼び出し（Rust → C）

### bindgen による自動生成

```toml
[build-dependencies]
bindgen = "0.71"

[dependencies]
libc = "0.2"
```

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_function("midi_.*")     // 必要な関数だけ
        .allowlist_type("MidiMessage")
        .generate()
        .expect("bindgen failed");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
```

```rust
// src/lib.rs
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
```

### 手動 extern 宣言

小規模な場合は手動で書く方がシンプル：

```rust
extern "C" {
    fn midi_open(port: libc::c_int) -> *mut MidiHandle;
    fn midi_send(handle: *mut MidiHandle, data: *const u8, len: libc::size_t) -> libc::c_int;
    fn midi_close(handle: *mut MidiHandle);
}

// 安全なラッパー
pub struct MidiPort {
    handle: *mut MidiHandle,
}

impl MidiPort {
    pub fn open(port: i32) -> Result<Self, MidiError> {
        // SAFETY: midi_open は有効なポート番号で呼ばれ、NULL を返す場合がある
        let handle = unsafe { midi_open(port as libc::c_int) };
        if handle.is_null() {
            return Err(MidiError::OpenFailed(port));
        }
        Ok(Self { handle })
    }

    pub fn send(&self, data: &[u8]) -> Result<(), MidiError> {
        // SAFETY: handle は open で有効性を確認済み、data はスライスで範囲保証
        let ret = unsafe { midi_send(self.handle, data.as_ptr(), data.len()) };
        if ret < 0 {
            return Err(MidiError::SendFailed(ret));
        }
        Ok(())
    }
}

impl Drop for MidiPort {
    fn drop(&mut self) {
        // SAFETY: handle は初期化時に有効性を確認済み、二重解放は Drop が防ぐ
        unsafe { midi_close(self.handle) };
    }
}

// Send は手動で実装する必要がある場合がある
// SAFETY: MidiHandle の操作はスレッドセーフ（Cライブラリのドキュメントで確認）
unsafe impl Send for MidiPort {}
```

## Rust ライブラリの公開（C → Rust）

### cbindgen によるヘッダー生成

```toml
[lib]
crate-type = ["cdylib", "staticlib"]

[build-dependencies]
cbindgen = "0.28"
```

```rust
// build.rs
fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(cbindgen::Language::C)
        .generate()
        .expect("cbindgen failed")
        .write_to_file("include/my_lib.h");
}
```

```rust
// src/lib.rs

/// エンジンを初期化する。失敗時は NULL を返す。
#[no_mangle]
pub extern "C" fn engine_new(sample_rate: u32) -> *mut Engine {
    match Engine::new(sample_rate) {
        Ok(engine) => Box::into_raw(Box::new(engine)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// エンジンを破棄する。NULL ポインタは安全に無視される。
#[no_mangle]
pub extern "C" fn engine_free(ptr: *mut Engine) {
    if !ptr.is_null() {
        // SAFETY: ptr は engine_new で作成され、一度だけ free される
        unsafe { drop(Box::from_raw(ptr)) };
    }
}

/// MIDI メッセージを送信する。成功時 0、失敗時 -1。
#[no_mangle]
pub extern "C" fn engine_send_midi(
    ptr: *mut Engine,
    data: *const u8,
    len: usize,
) -> i32 {
    if ptr.is_null() || data.is_null() {
        return -1;
    }
    // SAFETY: ptr の有効性は上で確認、data と len はスライスに変換
    let engine = unsafe { &mut *ptr };
    let slice = unsafe { std::slice::from_raw_parts(data, len) };

    match engine.send_midi(slice) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
```

## 文字列の受け渡し

```rust
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// C → Rust: *const c_char を &str に
#[no_mangle]
pub extern "C" fn set_name(name: *const c_char) -> i32 {
    if name.is_null() {
        return -1;
    }
    // SAFETY: name は null でないことを確認済み
    let c_str = unsafe { CStr::from_ptr(name) };
    match c_str.to_str() {
        Ok(s) => {
            // s を使う
            0
        }
        Err(_) => -1,  // 不正な UTF-8
    }
}

// Rust → C: String を *mut c_char に
#[no_mangle]
pub extern "C" fn get_name() -> *mut c_char {
    let name = String::from("hello");
    match CString::new(name) {
        Ok(c_string) => c_string.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// C 側で使い終わったら解放
#[no_mangle]
pub extern "C" fn free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        // SAFETY: ptr は get_name で作成された CString
        unsafe { drop(CString::from_raw(ptr)) };
    }
}
```

## WSL2 ↔ Windows IPC パターン

lcvgc のようなWSL2上のRustプログラムとWindows側のアプリケーションを接続するパターン。

### TCP ソケット（最もシンプル）

WSL2とWindowsホストは同一ネットワーク上にあるので TCP で通信可能：

```rust
// WSL2側（サーバー）
use tokio::net::TcpListener;

async fn start_server() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:9000").await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_client(stream));
    }
}
```

Windows側から `localhost:9000` で接続（WSL2のポートフォワーディング経由）。

### 名前付きパイプ（Windows側）

Windows ネイティブアプリとの連携：

```rust
// Windows側でのみコンパイル
#[cfg(windows)]
mod pipe {
    use tokio::net::windows::named_pipe::{ServerOptions, ClientOptions};

    pub async fn create_pipe_server() -> anyhow::Result<()> {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(r"\\.\pipe\lcvgc")?;

        server.connect().await?;
        // server を読み書き
        Ok(())
    }
}
```

### Unix ドメインソケット（WSL内部通信）

WSL2 内のプロセス間通信に最適（TCP より高速）：

```rust
use tokio::net::UnixListener;

async fn start_unix_server() -> anyhow::Result<()> {
    let _ = std::fs::remove_file("/tmp/lcvgc.sock");
    let listener = UnixListener::bind("/tmp/lcvgc.sock")?;

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_connection(stream));
    }
}
```

### プロトコル設計のベストプラクティス

IPCの上に載せるプロトコルは、シリアライゼーションフォーマットを統一する：

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
enum Message {
    NoteOn { note: u8, velocity: u8, channel: u8 },
    NoteOff { note: u8, channel: u8 },
    ControlChange { cc: u8, value: u8, channel: u8 },
    EvalCode(String),
    Response(Result<String, String>),
}

// メッセージの送受信（長さプレフィクス方式）
async fn send_message<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    msg: &Message,
) -> anyhow::Result<()> {
    let data = serde_json::to_vec(msg)?;
    let len = (data.len() as u32).to_le_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&data).await?;
    Ok(())
}

async fn recv_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> anyhow::Result<Message> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;
    Ok(serde_json::from_slice(&data)?)
}
```

## FFI の安全性チェックリスト

- [ ] すべての `unsafe` ブロックに `// SAFETY:` コメントがある
- [ ] NULL ポインタのチェックがすべてのパブリック関数にある
- [ ] メモリの所有権が明確（誰が allocate し、誰が free するか）
- [ ] スレッドセーフティが文書化されている
- [ ] パニックが FFI 境界を越えない（`catch_unwind` で捕捉）
- [ ] 文字列は UTF-8 バリデーション済みまたは CStr で扱っている
