// Trust VS Code extension — a thin LSP client around `trust-lsp`.
//
// The server runs the same lower + lint pipeline as `trust check`,
// publishing diagnostics live plus hover and go-to-definition for local
// functions. This extension's only jobs are: find the binary, start the
// client on Rust files, and surface a useful error when the binary is
// missing.

import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

/** Resolve the trust-lsp binary: explicit setting → PATH → ~/.cargo/bin. */
function findServer(): string | undefined {
  const configured = vscode.workspace
    .getConfiguration("trust")
    .get<string>("serverPath");
  if (configured && configured.length > 0) {
    return fs.existsSync(configured) ? configured : undefined;
  }

  const exe = process.platform === "win32" ? "trust-lsp.exe" : "trust-lsp";
  const dirs = (process.env.PATH ?? "").split(path.delimiter);
  dirs.push(path.join(os.homedir(), ".cargo", "bin"));
  for (const dir of dirs) {
    if (dir.length === 0) {
      continue;
    }
    const candidate = path.join(dir, exe);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

export async function activate(context: vscode.ExtensionContext) {
  const server = findServer();
  if (!server) {
    const pick = await vscode.window.showWarningMessage(
      "Trust: could not find the `trust-lsp` binary. Install it with " +
        "`cargo install trust-lsp` (or build from source and set " +
        "`trust.serverPath`).",
      "Open settings"
    );
    if (pick === "Open settings") {
      vscode.commands.executeCommand(
        "workbench.action.openSettings",
        "trust.serverPath"
      );
    }
    return;
  }

  const serverOptions: ServerOptions = {
    command: server,
    args: [],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "rust" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/Cargo.toml"),
    },
  };

  client = new LanguageClient(
    "trust",
    "Trust Language Server",
    serverOptions,
    clientOptions
  );
  context.subscriptions.push({ dispose: () => client?.stop() });
  await client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
