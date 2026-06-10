//! Language Server Protocol implementation for Trust.
//!
//! Runs the same lower + lint pipeline as the CLI (`crates/trust/src/main.rs`)
//! on every textDocument/didOpen, didChange, didSave, and publishes diagnostics
//! back to the client. Also provides minimal hover info for named-arg call sites
//! and go-to-definition for local functions in the same file.
//!
//! This is the MVP; see RT-36 follow-ups for completion, code actions, etc.

use std::collections::HashMap;
use std::sync::Arc;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use trust_diag::{Diagnostic as RtDiag, Severity};
use trust_lower::named_args::CalleeRegistry;

/// Holds source text per Uri.
#[derive(Default)]
struct Documents {
    inner: HashMap<Url, Arc<String>>,
}

impl Documents {
    fn insert(&mut self, uri: Url, text: String) {
        self.inner.insert(uri, Arc::new(text));
    }
    fn get(&self, uri: &Url) -> Option<Arc<String>> {
        self.inner.get(uri).cloned()
    }
    fn remove(&mut self, uri: &Url) {
        self.inner.remove(uri);
    }
}

pub struct Backend {
    pub client: Client,
    docs: Arc<Mutex<Documents>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(Mutex::new(Documents::default())),
        }
    }

    async fn refresh(&self, uri: Url, text: String) {
        // Insert/update first.
        {
            let mut docs = self.docs.lock().await;
            docs.insert(uri.clone(), text.clone());
        }
        let diags = compute_diagnostics(&text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "trust-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "trust-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.refresh(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // FULL sync — last change's text is the new buffer contents.
        if let Some(change) = params.content_changes.pop() {
            self.refresh(params.text_document.uri, change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.refresh(params.text_document.uri, text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut docs = self.docs.lock().await;
        docs.remove(&params.text_document.uri);
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = match self.docs.lock().await.get(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };
        Ok(compute_hover(&text, pos))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = match self.docs.lock().await.get(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };
        Ok(compute_definition(&text, &uri, pos))
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (no LSP I/O) — testable.
// ---------------------------------------------------------------------------

/// Convert a byte offset into an LSP Position (line, UTF-16 column).
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    let offset = offset.min(source.len());
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let line_text = &source[line_start..offset];
    let character = line_text.encode_utf16().count() as u32;
    Position { line, character }
}

/// Convert an LSP Position (line, UTF-16 column) back to a byte offset.
pub fn position_to_offset(source: &str, pos: Position) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == pos.line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + ch.len_utf8();
        }
    }
    if current_line != pos.line {
        return source.len();
    }
    // Walk UTF-16 units within the line.
    let mut utf16_count = 0u32;
    let bytes_after_line_start = &source[line_start..];
    for (i, ch) in bytes_after_line_start.char_indices() {
        if utf16_count >= pos.character {
            return line_start + i;
        }
        utf16_count += ch.len_utf16() as u32;
        if ch == '\n' {
            return line_start + i;
        }
    }
    source.len()
}

fn rt_to_lsp_severity(s: Severity) -> DiagnosticSeverity {
    match s {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    }
}

/// Run the lower + lint pipeline against `source` and return LSP diagnostics.
///
/// Mirrors the CLI's `run_pipeline`. Unlike the CLI we never bail out — we want
/// to publish whatever we managed to compute. If lowering itself fails (the
/// token stream is unlexable, etc.) we surface that as a synthetic diagnostic.
pub fn compute_diagnostics(source: &str) -> Vec<Diagnostic> {
    let mut all: Vec<RtDiag> = Vec::new();

    let lower_out = match trust_lower::lower(source) {
        Ok(out) => out,
        Err(e) => {
            // Couldn't lex/lower — publish a top-of-file error so the user sees
            // *something*, but we can't run lints without a syn::File.
            let diag = Diagnostic {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 0,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("RLSP".to_string())),
                source: Some("trust".to_string()),
                message: format!("lowering failed: {e}"),
                ..Diagnostic::default()
            };
            return vec![diag];
        }
    };
    all.extend(lower_out.diagnostics);

    if let Ok(file) = syn::parse_str::<syn::File>(&lower_out.source) {
        let lint_report = trust_lints::lint_strict(&file, source, lower_out.strict_mode);
        all.extend(lint_report.diagnostics);
    }

    all.into_iter().map(|d| rt_diag_to_lsp(source, d)).collect()
}

fn rt_diag_to_lsp(source: &str, d: RtDiag) -> Diagnostic {
    let start = offset_to_position(source, d.span.start);
    let end = offset_to_position(source, d.span.end);
    let mut message = d.message.clone();
    if let Some(why) = &d.why {
        message.push_str("\n\nwhy: ");
        message.push_str(why);
    }
    if let Some(help) = &d.help {
        message.push_str("\n\nhelp: ");
        message.push_str(help);
    }
    Diagnostic {
        range: Range { start, end },
        severity: Some(rt_to_lsp_severity(d.severity)),
        code: Some(NumberOrString::String(d.rule.to_string())),
        source: Some("trust".to_string()),
        message,
        ..Diagnostic::default()
    }
}

// ---------------------------------------------------------------------------
// Hover & go-to-def: minimal source scans.
//
// We deliberately don't build a full AST index for the MVP. We do two cheap
// passes:
//   * a CalleeRegistry from the lowered token stream (gives param names per fn)
//   * a regex-free byte scan for `fn NAME(` to find local definitions
// Hover identifies the identifier at the cursor and either reports it as a
// named arg of an enclosing call, or as a call itself.
// ---------------------------------------------------------------------------

/// Identifier under the cursor, with its byte span in `source`.
struct WordAt {
    text: String,
    start: usize,
    end: usize,
}

fn word_at(source: &str, byte_offset: usize) -> Option<WordAt> {
    let bytes = source.as_bytes();
    if byte_offset > bytes.len() {
        return None;
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut start = byte_offset;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = byte_offset;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(WordAt {
        text: source[start..end].to_string(),
        start,
        end,
    })
}

pub fn compute_hover(source: &str, pos: Position) -> Option<Hover> {
    let offset = position_to_offset(source, pos);
    let word = word_at(source, offset)?;

    let tokens: TokenStream = source.parse().ok()?;
    let registry = CalleeRegistry::collect(&tokens);

    // Case 1: hovering on a param name in `f(name: value)`. We look at the
    // character immediately after the word — if it's `:` (and not `::`), and
    // some enclosing call has a callee in the registry containing this name,
    // report the named arg.
    let after = source[word.end..].trim_start_matches([' ', '\t']);
    if after.starts_with(':') && !after.starts_with("::") {
        if let Some(callee) = find_enclosing_call(source, word.start) {
            if let Some(params) = registry.fns.get(&callee) {
                if params.iter().any(|p| p == &word.text) {
                    let msg = format!(
                        "parameter `{}` of function `{}({})`",
                        word.text,
                        callee,
                        params.join(", ")
                    );
                    return Some(Hover {
                        contents: HoverContents::Scalar(MarkedString::String(msg)),
                        range: Some(span_to_range(source, word.start, word.end)),
                    });
                }
            }
        }
    }

    // Case 2: hovering on the callee identifier itself, with `(` next.
    if source[word.end..].trim_start().starts_with('(') {
        if let Some(params) = registry.fns.get(&word.text) {
            let msg = format!("fn {}({})", word.text, params.join(", "));
            return Some(Hover {
                contents: HoverContents::Scalar(MarkedString::String(msg)),
                range: Some(span_to_range(source, word.start, word.end)),
            });
        }
    }

    None
}

/// Find the name of the function whose argument list encloses `cursor`. We
/// scan backwards through the token stream of `source` to find the unmatched
/// open paren and then take the identifier immediately preceding it.
fn find_enclosing_call(source: &str, cursor: usize) -> Option<String> {
    // Operate at byte level. Skip strings/chars/line+block comments.
    let bytes = source.as_bytes();
    let mut i = 0usize;
    // Stack of (open_paren_offset, callee_name_opt) for parens currently open.
    let mut stack: Vec<Option<String>> = Vec::new();
    while i < bytes.len() && i < cursor {
        let b = bytes[i];
        match b {
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                i += 1;
            }
            b'\'' => {
                // crude char literal skip; lifetimes start with ' too but rarely
                // contain ')' so this is fine.
                i += 1;
                while i < bytes.len() && bytes[i] != b'\'' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                    } else {
                        i += 1;
                    }
                    if bytes.get(i - 1) == Some(&b'\n') {
                        // probably a lifetime — bail out of this skip
                        break;
                    }
                }
                if i < bytes.len() && bytes[i] == b'\'' {
                    i += 1;
                }
            }
            b'(' => {
                // Identify the identifier ending just before `i`.
                let mut j = i;
                while j > 0 && bytes[j - 1] == b' ' {
                    j -= 1;
                }
                let end = j;
                let mut start = end;
                while start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_')
                {
                    start -= 1;
                }
                let name = if start < end {
                    Some(source[start..end].to_string())
                } else {
                    None
                };
                stack.push(name);
                i += 1;
            }
            b')' => {
                stack.pop();
                i += 1;
            }
            _ => i += 1,
        }
    }
    stack.into_iter().rev().flatten().next()
}

fn span_to_range(source: &str, start: usize, end: usize) -> Range {
    Range {
        start: offset_to_position(source, start),
        end: offset_to_position(source, end),
    }
}

pub fn compute_definition(
    source: &str,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = position_to_offset(source, pos);
    let word = word_at(source, offset)?;
    // Only treat as a call site if `(` follows.
    if !source[word.end..].trim_start().starts_with('(') {
        // also allow plain identifier — still useful to jump to a fn definition
    }
    let (def_start, def_end) = find_local_fn_def(source, &word.text)?;
    let range = span_to_range(source, def_start, def_end);
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range,
    }))
}

/// Find `fn NAME` in the source via a token-tree walk; return the byte span of
/// the NAME identifier. Recurses into groups so methods/trait fns are found.
fn find_local_fn_def(source: &str, name: &str) -> Option<(usize, usize)> {
    let tokens: TokenStream = source.parse().ok()?;
    find_fn_in_tokens(tokens, name)
}

fn find_fn_in_tokens(tokens: TokenStream, name: &str) -> Option<(usize, usize)> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut prev_was_fn_keyword = false;
    for tt in &trees {
        match tt {
            TokenTree::Ident(id) => {
                let s = id.to_string();
                if prev_was_fn_keyword && s == name {
                    let span = id.span();
                    if let (Some(start), Some(end)) = byte_range_of_span(&span) {
                        return Some((start, end));
                    }
                }
                prev_was_fn_keyword = s == "fn";
            }
            TokenTree::Group(g) => {
                prev_was_fn_keyword = false;
                if let Some(found) = find_fn_in_tokens(g.stream(), name) {
                    return Some(found);
                }
            }
            TokenTree::Punct(p) => {
                // a leading `pub` / attributes don't break the `fn NAME` window
                // because they appear *before* `fn`. But e.g. `;` between
                // statements does break it. We treat any non-ident as a
                // separator just to keep this honest.
                let _ = p.spacing(); // touch field
                prev_was_fn_keyword = false;
            }
            TokenTree::Literal(_) => {
                prev_was_fn_keyword = false;
            }
        }
    }
    None
}

/// Get `(start_byte, end_byte)` for a proc_macro2 Span. On stable, Span only
/// exposes `start()`/`end()` LineColumn pairs — but with `syn`'s feature set
/// we get `byte_range`. proc_macro2 exposes `byte_range` since 1.0.66.
fn byte_range_of_span(span: &proc_macro2::Span) -> (Option<usize>, Option<usize>) {
    let r = span.byte_range();
    (Some(r.start), Some(r.end))
}

// Silence unused-import warning from `Spacing` / `Delimiter` if any path drops
// them later.
#[allow(dead_code)]
fn _force_use(_d: Delimiter, _s: Spacing) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_to_position_basic() {
        let src = "abc\ndef\n";
        assert_eq!(
            offset_to_position(src, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            offset_to_position(src, 4),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            offset_to_position(src, 6),
            Position {
                line: 1,
                character: 2
            }
        );
    }

    #[test]
    fn position_offset_roundtrip() {
        let src = "hello\nworld\n";
        for off in 0..src.len() {
            let p = offset_to_position(src, off);
            let back = position_to_offset(src, p);
            assert_eq!(back, off, "off={off} pos={p:?}");
        }
    }

    #[test]
    fn diagnostics_strict_unwrap() {
        let src = "#![strict]\nfn main() { let x: Option<u32> = Some(1); let _ = x.unwrap(); }\n";
        let diags = compute_diagnostics(src);
        assert!(
            diags
                .iter()
                .any(|d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "R0001")),
            "expected R0001, got {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn hover_named_arg() {
        let src = "fn make_rect(width: u32, height: u32) -> u32 { width * height }\n\
                   fn main() { let _ = make_rect(width: 10, height: 5); }\n";
        // Position the cursor on `width` in the call site.
        let needle = "make_rect(width";
        let offset = src.find(needle).unwrap() + "make_rect(".len() + 2; // mid 'width'
        let pos = offset_to_position(src, offset);
        let hover = compute_hover(src, pos).expect("expected hover");
        let s = match hover.contents {
            HoverContents::Scalar(MarkedString::String(s)) => s,
            _ => panic!("unexpected hover form"),
        };
        assert!(s.contains("parameter `width`"), "got: {s}");
        assert!(s.contains("make_rect"), "got: {s}");
    }

    #[test]
    fn hover_callee_signature() {
        let src = "fn make_rect(width: u32, height: u32) -> u32 { width * height }\n\
                   fn main() { let _ = make_rect(10, 5); }\n";
        let offset = src.rfind("make_rect(").unwrap() + 2;
        let pos = offset_to_position(src, offset);
        let hover = compute_hover(src, pos).expect("expected hover");
        let s = match hover.contents {
            HoverContents::Scalar(MarkedString::String(s)) => s,
            _ => panic!("unexpected hover form"),
        };
        assert!(s.contains("fn make_rect"), "got: {s}");
        assert!(s.contains("width"), "got: {s}");
    }

    #[test]
    fn definition_local_fn() {
        let src = "fn helper(x: u32) -> u32 { x + 1 }\nfn main() { let _ = helper(2); }\n";
        let call_offset = src.rfind("helper(").unwrap() + 1;
        let pos = offset_to_position(src, call_offset);
        let uri = Url::parse("file:///fake.rs").unwrap();
        let def = compute_definition(src, &uri, pos).expect("expected def");
        let loc = match def {
            GotoDefinitionResponse::Scalar(l) => l,
            _ => panic!("expected scalar"),
        };
        // Should point at the `helper` identifier on line 0.
        assert_eq!(loc.range.start.line, 0);
    }
}
