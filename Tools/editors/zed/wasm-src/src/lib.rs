//! Jet Zed extension — language support only.

use zed_extension_api::{
    self as zed, settings::LspSettings, Command, Extension, LanguageServerId, Result, Worktree,
};

/// The language server id from extension.toml (`[language_servers.jet-lsp]`).
const SERVER_ID: &str = "jet-lsp";

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
        // Zed's worktree-trust gate decides whether this callback may start a
        // process at all. The server binary is, in order: the user's Zed
        // setting `lsp.jet-lsp.binary.path` (with its `arguments`/`env`), else
        // `jet` from the worktree's PATH. Nothing is read from the worktree's
        // files and no repository build output is guessed.
        let binary = LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let configured = binary.as_ref().and_then(|binary| binary.path.clone());
        let command = match configured {
            Some(path) => path,
            None => worktree.which("jet").ok_or_else(|| {
                "Jet language server: `jet` is not on PATH. Set the absolute path of the jet \
                 executable in Zed settings as `lsp.jet-lsp.binary.path`."
                    .to_string()
            })?,
        };
        let args = binary
            .as_ref()
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_else(|| vec!["self".to_string(), "lsp".to_string()]);
        let env = binary
            .and_then(|binary| binary.env)
            .map(|env| env.into_iter().collect())
            .unwrap_or_default();
        Ok(Command { command, args, env })
    }
}

zed::register_extension!(JetExtension);
