//! Activation marker for the Rustricted dialect (cargo-built crates).
//!
//! Add this crate as a dependency and invoke `rustricted_attrs::strict!{}`
//! at the top of any file you want the Rustricted lints to apply to. The
//! macro expands to nothing for `rustc`, so cargo builds proceed normally;
//! the Rustricted toolchain recognises the invocation and treats the file
//! as strict-mode.
//!
//! Single-file `rustricted check` invocations can still use `#![strict]`
//! directly without this crate — the toolchain accepts both activation
//! forms.
//!
//! # Why a function-like macro?
//!
//! `#![strict]` is a Rustricted-invented inner attribute. Stock `rustc`
//! rejects unknown inner attributes (proc-macro attributes do not work
//! at file inner position; only built-in or tool-registered attributes
//! do). A function-like macro at item position is the simplest stable
//! form that rustc accepts unchanged.

use proc_macro::TokenStream;

/// Marker macro for the Rustricted dialect. Expands to nothing.
///
/// Use at the top of a file:
///
/// ```ignore
/// rustricted_attrs::strict!{}
///
/// fn main() { /* this file is now Rustricted-strict */ }
/// ```
#[proc_macro]
pub fn strict(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
