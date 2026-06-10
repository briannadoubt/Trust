# Trust for VS Code

Language support for [Trust](https://github.com/briannadoubt/trust) — the
strict Rust dialect for the bugs LLMs actually ship. Wraps `trust-lsp`:
live diagnostics (the full R-rule set), hover, and go-to-definition for
local functions, on every Rust file.

## Setup

1. Install the server:

   ```sh
   cargo install trust-lsp          # once 0.1 is on crates.io
   # or, from a source checkout:
   cargo build -p trust-lsp --release
   ```

2. Install this extension (from the marketplace once published, or
   `npm install && npm run package` here and install the `.vsix`).

The extension finds `trust-lsp` on `PATH` (including `~/.cargo/bin`). A
custom location can be set with `trust.serverPath`.

## Settings

| Setting | Default | Meaning |
| ------- | ------- | ------- |
| `trust.serverPath` | `""` | Explicit path to the `trust-lsp` binary; empty means search `PATH`. |
| `trust.trace.server` | `off` | LSP traffic tracing (`off` / `messages` / `verbose`). |

## Development

```sh
npm install
npm run compile     # or: npm run watch
```

Press F5 in VS Code with this folder open to launch an Extension
Development Host.

## Not yet done

- **Bundled platform binaries** (darwin-arm64/x64, linux-x64/arm64,
  win-x64): planned for the release pipeline so install is one click with
  no cargo step. Until then the extension requires `trust-lsp` on `PATH`.
- Marketplace publishing (happens with the 0.1 push).
