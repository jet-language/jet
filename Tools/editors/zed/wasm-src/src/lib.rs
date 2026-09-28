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
        _worktree: &Worktree,
    ) -> Result<Command> {
        // Zed's worktree-trust gate controls whether this language-server
        // callback may start a process. Keep the command identity literal so
        // an opened worktree cannot select `./jet` (or another PATH entry).
        Ok(Command {
            command: "jet".to_string(),
            args: vec!["self".to_string(), "lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(JetExtension);
