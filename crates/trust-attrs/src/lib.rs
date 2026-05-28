//! Activation marker for the Trust dialect (cargo-built crates).
//!
//! Add this crate as a dependency and invoke `trust_attrs::strict!{}`
//! at the top of any file you want the Trust lints to apply to. The
//! macro expands to nothing for `rustc`, so cargo builds proceed normally;
//! the Trust toolchain recognises the invocation and treats the file
//! as strict-mode.
//!
//! Single-file `trust check` invocations can still use `#![strict]`
//! directly without this crate — the toolchain accepts both activation
//! forms.
//!
//! # Why a function-like macro?
//!
//! `#![strict]` is a Trust-invented inner attribute. Stock `rustc`
//! rejects unknown inner attributes (proc-macro attributes do not work
//! at file inner position; only built-in or tool-registered attributes
//! do). A function-like macro at item position is the simplest stable
//! form that rustc accepts unchanged.

use proc_macro::TokenStream;

/// Marker macro for the Trust dialect. Expands to nothing.
///
/// Use at the top of a file:
///
/// ```ignore
/// trust_attrs::strict!{}
///
/// fn main() { /* this file is now Trust-strict */ }
/// ```
#[proc_macro]
pub fn strict(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
