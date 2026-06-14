//! Jet Zed extension — launches `jet lsp` with the same discovery order as the
//! VS Code extension (debug binary in the compiler repo, then `jet` on PATH).

use zed_extension_api::{self as zed, Command, Extension, LanguageServerId, Result, Worktree};

struct JetExtension;

impl Extension for JetExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let command = find_jet_binary(worktree)?;
        Ok(Command {
            command,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

fn find_jet_binary(worktree: &Worktree) -> Result<String> {
    let root = worktree.root_path();
    let debug_bin = format!("{root}/target/debug/jet");

    // Developing the compiler itself: prefer the cargo-built debug binary.
    if worktree.read_text_file("flake.nix").is_ok() {
        return Ok(debug_bin);
    }

    if let Some(path) = worktree.which("jet") {
        return Ok(path);
    }

    Ok(debug_bin)
}

zed::register_extension!(JetExtension);
