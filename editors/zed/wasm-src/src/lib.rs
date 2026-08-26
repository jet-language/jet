//! Jet Zed extension — language support only.

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
            args: vec!["self".to_string(), "lsp".to_string()],
            env: Default::default(),
        })
    }
}

fn find_jet_binary(worktree: &Worktree) -> Result<String> {
    if let Some(path) = worktree.which("jet") {
        return Ok(path);
    }

    Err("Jet language server `jet` was not found on PATH".into())
}

zed::register_extension!(JetExtension);
