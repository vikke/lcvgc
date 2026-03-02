// LSPモジュールには将来利用予定のコード（goto_def, hover, symbols, diatonic等）が含まれる
#![allow(dead_code)]

mod analyzer;
mod backend;
mod completion;
mod diagnostic;
mod diatonic;
mod goto_def;
mod hover;
mod span_parser;
mod symbols;

use tower_lsp::{LspService, Server};

use crate::backend::Backend;

/// lcvgc-lsp エントリポイント
///
/// LSPサーバーをstdio通信で起動する
#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
