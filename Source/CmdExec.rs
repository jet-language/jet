//! `jet self exec`: the application-facing workspace executor.
//!
//! The command is a thin CLI adapter. It resolves the executable and checks
//! the declared workspace grants here, then hands the child to the same native
//! isolation boundary used by hermetic build actions. It never falls back to a
//! plain `Command` launch.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jet::Comptime::Build::{self, NativeSandboxError};

use crate::OutputMode;

#[derive(Default)]
struct ExecOptions {
    workspace: Option<String>,
    executable_grant: Option<String>,
    reads: Vec<String>,
    writes: Vec<String>,
    unsupported: Option<String>,
}

fn fail(what: impl Into<String>, fix: impl Into<String>, json: bool) -> ! {
    crate::emit_cli_report(
        "E2104",
        what.into(),
        "the executor accepts only explicit workspace and authority inputs before the `--` command separator".to_string(),
        fix.into(),
        json,
    );
    std::process::exit(jet::ExitCodes::USAGE)
}

fn reject_unsupported_grant(grant: &str, json: bool) -> ! {
    crate::emit_cli_report(
        "E2105",
        format!("executor grant `{grant}` cannot be enforced by the native child boundary"),
        "Jet does not convert an unenforced expert grant into ambient authority".to_string(),
        "use the workspace read/build-write grants, or wait for the host backend that enforces this grant".to_string(),
        json,
    );
    std::process::exit(jet::ExitCodes::USER_ERROR)
}

fn next_value(args: &[String], index: &mut usize, flag: &str, json: bool) -> String {
    *index += 1;
    args.get(*index)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| {
            fail(
                format!("`{flag}` needs a value"),
                format!("write `{flag} <value>`"),
                json,
            )
        })
}

fn parse_options(args: &[String], json: bool) -> (ExecOptions, Vec<String>) {
    let separator = args.iter().position(|arg| arg == "--").unwrap_or_else(|| {
        fail(
            "`jet self exec` needs a `--` command separator",
            "write `jet self exec --workspace <dir> -- <program> [args]`",
            json,
        )
    });
    let mut options = ExecOptions::default();
    let mut index = 1;
    while index < separator {
        let arg = &args[index];
        match arg.as_str() {
            "--workspace" => options.workspace = Some(next_value(args, &mut index, arg, json)),
            "--exec" => options.executable_grant = Some(next_value(args, &mut index, arg, json)),
            "--read" => options.reads.push(next_value(args, &mut index, arg, json)),
            "--write" => options.writes.push(next_value(args, &mut index, arg, json)),
            "--host" | "--secret" | "--timeout" | "--output-limit" => {
                options.unsupported = Some(arg.clone());
                index += 1;
                if index < separator && !args[index].starts_with('-') {
                    index += 1;
                }
                continue;
            }
            "--json" | "--quiet" => {}
            value if value.starts_with("--workspace=") => {
                let value = value.trim_start_matches("--workspace=");
                if value.is_empty() {
                    fail(
                        "`--workspace=` needs a value",
                        "write `--workspace <dir>`",
                        json,
                    );
                }
                options.workspace = Some(value.to_string());
            }
            value if value.starts_with("--exec=") => {
                let value = value.trim_start_matches("--exec=");
                if value.is_empty() {
                    fail("`--exec=` needs a value", "write `--exec <path>`", json);
                }
                options.executable_grant = Some(value.to_string());
            }
            value if value.starts_with("--read=") => {
                let value = value.trim_start_matches("--read=");
                if value.is_empty() {
                    fail("`--read=` needs a value", "write `--read <path>`", json);
                }
                options.reads.push(value.to_string());
            }
            value if value.starts_with("--write=") => {
                let value = value.trim_start_matches("--write=");
                if value.is_empty() {
                    fail("`--write=` needs a value", "write `--write <path>`", json);
                }
                options.writes.push(value.to_string());
            }
            value
                if value.starts_with("--host=")
                    || value.starts_with("--secret=")
                    || value.starts_with("--timeout=")
                    || value.starts_with("--output-limit=") =>
            {
                options.unsupported = Some(value.split('=').next().unwrap_or(value).to_string());
            }
            _ => fail(
                format!("unknown `jet self exec` option `{arg}`"),
                "use `--workspace`, optional exact `--exec`/`--read`/`--write`, then `-- <program> [args]`",
                json,
            ),
        }
        index += 1;
    }
    let command = args[separator + 1..].to_vec();
    if command.is_empty() {
        fail(
            "the executor command is empty",
            "add `<program> [args]` after `--`",
            json,
        );
    }
    (options, command)
}

fn real_directory(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    let canonical = path.canonicalize().map_err(|error| {
        format!(
            "workspace `{}` is not a readable directory: {error}",
            path.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!("workspace `{}` is not a directory", path.display()));
    }
    Ok(canonical)
}

fn secure_output_directory(workspace: &Path) -> Result<PathBuf, String> {
    let jet_dir = workspace.join(".jet");
    let output = jet_dir.join("build");
    for path in [&jet_dir, &output] {
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "workspace output path `{}` is not a real directory",
                    path.display()
                ));
            }
        }
    }
    fs::create_dir_all(&output).map_err(|error| {
        format!(
            "cannot prepare workspace output `{}`: {error}",
            output.display()
        )
    })?;
    output.canonicalize().map_err(|error| {
        format!(
            "cannot resolve workspace output `{}`: {error}",
            output.display()
        )
    })
}

fn resolve_executable(command: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(command);
    if candidate.is_absolute() || command.contains('/') || command.contains('\\') {
        if candidate.is_file() {
            return Ok(candidate.to_path_buf());
        }
        return Err(format!("cannot resolve executable `{command}`"));
    }
    let path = std::env::var_os("PATH")
        .ok_or_else(|| "cannot resolve executable without a PATH snapshot".to_string())?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "executable `{command}` was not found in the captured PATH"
    ))
}

fn grant_path(path: &str, workspace: &Path) -> Result<PathBuf, String> {
    let path = Path::new(path);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    candidate.canonicalize().map_err(|error| {
        format!(
            "cannot resolve authority path `{}`: {error}",
            candidate.display()
        )
    })
}

pub(crate) fn run_exec(raw: &[String], mode: OutputMode) -> ! {
    let (options, command) = parse_options(raw, mode.json);
    let workspace = options.workspace.as_deref().unwrap_or_else(|| {
        fail(
            "`--workspace` is required",
            "write `--workspace <dir>`",
            mode.json,
        )
    });
    let workspace = match real_directory(workspace) {
        Ok(path) => path,
        Err(detail) => {
            crate::emit_cli_report(
                "E2105",
                detail,
                "the executor cannot bind a non-directory workspace".to_string(),
                "choose an existing workspace directory".to_string(),
                mode.json,
            );
            std::process::exit(jet::ExitCodes::USER_ERROR)
        }
    };
    if let Some(grant) = options.unsupported.as_deref() {
        reject_unsupported_grant(grant, mode.json);
    }
    let executable = match resolve_executable(&command[0]) {
        Ok(path) => path,
        Err(detail) => {
            crate::emit_cli_report(
                "E2105",
                detail,
                "the executor reviews executable identity before launch".to_string(),
                "use an executable present in the captured PATH".to_string(),
                mode.json,
            );
            std::process::exit(jet::ExitCodes::USER_ERROR)
        }
    };
    if let Some(grant) = options.executable_grant.as_deref() {
        let grant = match Path::new(grant).canonicalize() {
            Ok(path) => path,
            Err(error) => reject_unsupported_grant(&format!("Exec:{grant} ({error})"), mode.json),
        };
        let executable_identity = match executable.canonicalize() {
            Ok(path) => path,
            Err(error) => reject_unsupported_grant(
                &format!("Exec:{} ({error})", executable.display()),
                mode.json,
            ),
        };
        if grant != executable_identity {
            reject_unsupported_grant(&format!("Exec:{}", grant.display()), mode.json);
        }
    }
    for path in &options.reads {
        let path = match grant_path(path, &workspace) {
            Ok(path) => path,
            Err(detail) => {
                crate::emit_cli_report(
                    "E2105",
                    detail,
                    "an authority path must resolve before launch".to_string(),
                    "grant a real path inside the workspace".to_string(),
                    mode.json,
                );
                std::process::exit(jet::ExitCodes::USER_ERROR)
            }
        };
        if path != workspace {
            reject_unsupported_grant(&format!("FS.Read:{}", path.display()), mode.json);
        }
    }
    let output = match secure_output_directory(&workspace) {
        Ok(path) => path,
        Err(detail) => {
            crate::emit_cli_report(
                "E2105",
                detail,
                "the executor refuses symlinked or invalid output roots".to_string(),
                "repair `.jet/build` and retry".to_string(),
                mode.json,
            );
            std::process::exit(jet::ExitCodes::USER_ERROR)
        }
    };
    for path in &options.writes {
        let path = match grant_path(path, &workspace) {
            Ok(path) => path,
            Err(detail) => {
                crate::emit_cli_report(
                    "E2105",
                    detail,
                    "an authority path must resolve before launch".to_string(),
                    "grant the private `.jet/build` output directory".to_string(),
                    mode.json,
                );
                std::process::exit(jet::ExitCodes::USER_ERROR)
            }
        };
        if path != output {
            reject_unsupported_grant(&format!("FS.Write:{}", path.display()), mode.json);
        }
    }
    let status = Build::native_sandbox_status();
    if !status.available {
        crate::emit_cli_report(
            "E2105",
            format!(
                "authority-bound execution refused before spawn: {}",
                status.reason
            ),
            "the host must expose the #398 native isolation boundary".to_string(),
            "install or enable the supported native sandbox backend".to_string(),
            mode.json,
        );
        std::process::exit(jet::ExitCodes::USER_ERROR)
    }
    let args = command[1..].to_vec();
    let result = Build::run_native_sandboxed(
        &executable,
        &args,
        &workspace,
        Some(&output),
        &BTreeMap::new(),
        false,
    );
    match result {
        Ok(result) => {
            print!("{}", String::from_utf8_lossy(&result.output.stdout));
            eprint!("{}", String::from_utf8_lossy(&result.output.stderr));
            std::process::exit(
                result
                    .output
                    .status
                    .code()
                    .unwrap_or(jet::ExitCodes::USER_ERROR),
            );
        }
        Err(NativeSandboxError::Unsupported(detail)) | Err(NativeSandboxError::Io(detail)) => {
            crate::emit_cli_report(
                "E2105",
                format!("authority-bound execution refused before spawn: {detail}"),
                "Jet never falls back to an unsandboxed child".to_string(),
                "enable the native isolation backend and retry".to_string(),
                mode.json,
            );
            std::process::exit(jet::ExitCodes::USER_ERROR)
        }
    }
}
