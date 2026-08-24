//! Jet Zed extension — launches `jet self lsp` and exposes the native Jet DAP
//! adapter with the same executable discovery order as the VS Code extension.

use std::path::Path;

use zed_extension_api::{
    self as zed, Command, DebugAdapterBinary, DebugConfig, DebugRequest, DebugScenario,
    DebugTaskDefinition, Extension, LanguageServerId, LaunchRequest, Result,
    StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest, Worktree,
};

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

    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &Worktree,
    ) -> Result<DebugAdapterBinary> {
        if adapter_name != "jet" {
            return Err(format!("unsupported Jet debug adapter `{adapter_name}`"));
        }
        let value: zed::serde_json::Value = zed::serde_json::from_str(&config.config)
            .map_err(|error| format!("Jet debug configuration is not valid JSON: {error}"))?;
        let request = request_kind(&value)?;
        let source = value
            .get("program")
            .and_then(zed::serde_json::Value::as_str)
            .filter(|path| path.ends_with(".jet"))
            .map(str::to_string)
            .or_else(|| {
                matches!(&request, StartDebuggingRequestArgumentsRequest::Attach)
                    .then(|| value.get("map").and_then(zed::serde_json::Value::as_str))
                    .flatten()
                    .and_then(|map| map_source(worktree, map).ok())
            })
            .ok_or_else(|| {
                "Jet DAP needs a .jet source file, or an attach map that identifies one".to_string()
            })?;
        let command = user_provided_debug_adapter_path
            .or_else(|| find_jet_binary(worktree).ok())
            .ok_or_else(|| "Jet executable is not available for Zed debugging".to_string())?;
        Ok(DebugAdapterBinary {
            command: Some(command),
            arguments: vec!["debug".to_string(), "--dap".to_string(), source],
            envs: Vec::new(),
            cwd: Some(worktree.root_path()),
            connection: None,
            request_args: StartDebuggingRequestArguments {
                configuration: config.config,
                request,
            },
        })
    }

    fn dap_request_kind(
        &mut self,
        adapter_name: String,
        config: zed::serde_json::Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest> {
        if adapter_name != "jet" {
            return Err(format!("unsupported Jet debug adapter `{adapter_name}`"));
        }
        request_kind(&config)
    }

    fn dap_config_to_scenario(&mut self, config: DebugConfig) -> Result<DebugScenario> {
        if config.adapter != "jet" {
            return Err(format!(
                "unsupported Jet debug adapter `{}`",
                config.adapter
            ));
        }
        let value = match config.request {
            DebugRequest::Launch(LaunchRequest {
                program,
                cwd,
                args,
                envs,
            }) => launch_config(program, cwd, args, envs, config.stop_on_entry),
            DebugRequest::Attach(_) => {
                return Err(
                    "Jet local attach needs an explicit native `program`, `.jetmap` `map`, and `processId` task configuration"
                        .to_string(),
                )
            }
        }?;
        Ok(DebugScenario {
            label: config.label,
            adapter: config.adapter,
            build: None,
            config: zed::serde_json::to_string(&value)
                .map_err(|error| format!("cannot encode Jet debug configuration: {error}"))?,
            tcp_connection: None,
        })
    }
}

fn request_kind(value: &zed::serde_json::Value) -> Result<StartDebuggingRequestArgumentsRequest> {
    match value
        .get("request")
        .and_then(zed::serde_json::Value::as_str)
    {
        Some("launch") => Ok(StartDebuggingRequestArgumentsRequest::Launch),
        Some("attach") => Ok(StartDebuggingRequestArgumentsRequest::Attach),
        Some(other) => Err(format!("unsupported Jet debug request `{other}`")),
        None if value.get("processId").is_some() => {
            Ok(StartDebuggingRequestArgumentsRequest::Attach)
        }
        None if value.get("program").is_some() => Ok(StartDebuggingRequestArgumentsRequest::Launch),
        None => Err("Jet debug configuration needs `request` and `program`".to_string()),
    }
}

fn launch_config(
    program: String,
    cwd: Option<String>,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    stop_on_entry: Option<bool>,
) -> Result<zed::serde_json::Value> {
    let mut fields = vec![
        (
            "request",
            zed::serde_json::Value::String("launch".to_string()),
        ),
        ("program", zed::serde_json::Value::String(program)),
        (
            "args",
            zed::serde_json::Value::Array(
                args.into_iter()
                    .map(zed::serde_json::Value::String)
                    .collect(),
            ),
        ),
    ];
    if let Some(cwd) = cwd {
        fields.push(("cwd", zed::serde_json::Value::String(cwd)));
    }
    if !envs.is_empty() {
        let mut env = zed::serde_json::Map::new();
        for (key, value) in envs {
            env.insert(key, zed::serde_json::Value::String(value));
        }
        fields.push(("env", zed::serde_json::Value::Object(env)));
    }
    if let Some(stop_on_entry) = stop_on_entry {
        fields.push(("stopOnEntry", zed::serde_json::Value::Bool(stop_on_entry)));
    }
    let mut object = zed::serde_json::Map::new();
    for (key, value) in fields {
        object.insert(key.to_string(), value);
    }
    Ok(zed::serde_json::Value::Object(object))
}

fn map_source(worktree: &Worktree, map_path: &str) -> Result<String> {
    let root = worktree.root_path();
    let relative = Path::new(map_path)
        .strip_prefix(&root)
        .unwrap_or_else(|_| Path::new(map_path));
    let text = worktree.read_text_file(&relative.to_string_lossy())?;
    let value: zed::serde_json::Value = zed::serde_json::from_str(&text)
        .map_err(|error| format!("Jet debugger map is not valid JSON: {error}"))?;
    let source = value
        .get("jet_file")
        .and_then(zed::serde_json::Value::as_str)
        .filter(|source| !source.is_empty())
        .ok_or_else(|| "Jet debugger map does not identify a Jet source file".to_string())?;
    if Path::new(source).is_absolute() {
        Ok(source.to_string())
    } else {
        Ok(Path::new(&root).join(source).to_string_lossy().into_owned())
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
