//! Read-only MCP environment resources served over the existing LSP transport.
//!
//! The resource is deliberately a projection of the public `jet env info
//! --json` path.  That keeps package, service, task, file, variable, grant,
//! and integration facts owned by the typed Jetpack model instead of creating
//! a second environment evaluator in the language server.

use std::path::{Path, PathBuf};
use std::process::Command;

use jet_foundation::JSON::{parse_json, MAX_PROTOCOL_MESSAGE_BYTES};

pub(crate) const ENVIRONMENT_RESOURCE_URI: &str = "jet://environment";
const RESOURCE_MIME_TYPE: &str = "application/json";
const MAX_RESOURCE_TEXT_BYTES: usize = MAX_PROTOCOL_MESSAGE_BYTES / 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadError {
    NotFound,
    Unavailable,
}

pub(crate) fn list_json(workspace_roots: &[String]) -> String {
    let resources = environment_root(workspace_roots)
        .filter(|root| root.join(crate::Syntax::ENV_FILE).is_file())
        .map(|_| {
            format!(
                "{{\"uri\":\"{}\",\"name\":\"Jet environment\",\"description\":\"Read-only typed environment facts\",\"mimeType\":\"{}\"}}",
                ENVIRONMENT_RESOURCE_URI, RESOURCE_MIME_TYPE
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    format!("{{\"resources\":[{}]}}", resources.join(","))
}

pub(crate) fn read_json(workspace_roots: &[String], uri: &str) -> Result<String, ReadError> {
    if uri != ENVIRONMENT_RESOURCE_URI {
        return Err(ReadError::NotFound);
    }
    let Some(root) = environment_root(workspace_roots) else {
        return Err(ReadError::NotFound);
    };
    if !root.join(crate::Syntax::ENV_FILE).is_file() {
        return Err(ReadError::NotFound);
    }

    let executable = std::env::current_exe().map_err(|_| ReadError::Unavailable)?;
    let output = Command::new(executable)
        .args(["env", "info", "--json", "--no-color"])
        .current_dir(&root)
        .output()
        .map_err(|_| ReadError::Unavailable)?;
    if !output.status.success() || output.stdout.len() > MAX_RESOURCE_TEXT_BYTES {
        return Err(ReadError::Unavailable);
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| ReadError::Unavailable)?
        .trim()
        .to_string();
    if text.is_empty() || parse_json(&text).is_err() {
        return Err(ReadError::Unavailable);
    }
    Ok(text)
}

fn environment_root(workspace_roots: &[String]) -> Option<PathBuf> {
    let start = workspace_roots
        .first()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    jetpack::EnvHook::find_env_root(Path::new(&start))
}
