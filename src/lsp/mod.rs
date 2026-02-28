pub mod analyzer;
pub mod backend;
pub mod completion;
pub mod diagnostic;
pub mod diatonic;
pub mod goto_def;
pub mod hover;
pub mod span_parser;
pub mod symbols;

use tower_lsp::{LspService, Server};

use crate::lsp::backend::Backend;

/// LSPサーバーをstdio通信で起動
pub async fn run_lsp() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
