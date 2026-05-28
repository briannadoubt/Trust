//! Entry point for the Trust language server.
//!
//! Speaks LSP over stdio. Wire your editor to this binary; see README.

trust_attrs::strict! {}

use trust_lsp::Backend;
use tower_lsp::{LspService, Server};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
