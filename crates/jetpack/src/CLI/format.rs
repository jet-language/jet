//! Environment-backed project formatting (D-ECO12 / D-FMTPROJECT1).
//!
//! Native Jet formatting remains in the driver. This module owns only the
//! language path: it discovers files, realizes the typed formatter package
//! through the normal environment plan, stages the complete batch, and lets
//! the existing composed-process seam run the formatter.

use super::parse::Parsed;
use super::realize::{apply_locked_channels, load_project_plan_with_selections, project_env_root};
use super::services_secrets_config::validate_declared_secrets;
use super::trust_env_build::compose_env;
use crate::Output::Theme;
use crate::Shell;
use crate::Store;
use jet_foundation::ExitCodes;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const IGNORED_DIRS: &[&str] = &[".git", ".jet", "target", "build", "vendor", "node_modules"];

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let base = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        for attempt in 0..32u32 {
            let path = base.join(format!("jet-fmt-{}-{stamp}-{attempt}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("could not allocate a unique formatter staging directory".to_string())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct StagedFile {
    source: PathBuf,
    staged: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

/// Run the external formatter declared by the active typed environment.
pub(super) fn cmd_fmt(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(language) = parsed.flags.fmt_language.as_deref() else {
        theme.error_coded(
            "E2104",
            "jetpack fmt needs --lang <language>",
            "the engine route is reserved for the environment formatter passthrough",
            "run jet fmt --lang <language>, or run jet fmt for native Jet files",
        );
        return ExitCodes::USAGE;
    };
    let Some(language) = valid_language(language) else {
        theme.error_coded(
            "E2104",
            "formatter language is invalid",
            "--lang is a file-language selector, not a shell command or path",
            "pass a non-empty language name such as --lang nix",
        );
        return ExitCodes::USAGE;
    };
    if parsed.positional.iter().any(|path| path == "-") {
        theme.error_coded(
            "E2104",
            "environment formatter passthrough does not accept stdin",
            "the external formatter receives staged project files so a failed batch makes no source writes",
            "pass .nix paths or let jet fmt --lang nix discover the project files",
        );
        return ExitCodes::USAGE;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_dir = project_env_root(&cwd);
    let mut plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.preset.as_deref(),
        parsed.flags.environment.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }
    let Some(formatter) = plan.environment.lifecycle.formatter.as_ref() else {
        theme.error_coded(
            "E1340",
            &format!("no formatter is declared for language {language}"),
            "jet fmt --lang delegates through the selected environment typed formatter fact",
            "declare one package, for example formatter: pkgs.nixfmt, in the selected environment",
        );
        return ExitCodes::USER_ERROR;
    };
    let formatter_package = formatter.package.clone();
    let Some(formatter_ref) = plan.refs.iter().find(|spec| spec.raw == formatter_package) else {
        theme.error_coded(
            "E1340",
            "the environment formatter is not in the realization plan",
            &format!("typed formatter package {formatter_package} was not classified"),
            "check the formatter package reference and retry",
        );
        return ExitCodes::USER_ERROR;
    };
    let formatter_program = formatter_ref.short_name().to_string();

    if let Err(code) = crate::Trust::gate_with_environment(
        theme,
        &crate::Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        parsed.flags.trust,
    ) {
        return code;
    }
    if let Err(code) = validate_declared_secrets(
        theme,
        &project_dir,
        &plan.secrets,
        plan.environment.active_environment.as_deref(),
    ) {
        return code;
    }

    let files = match collect_files(&cwd, &project_dir, &parsed.positional, language) {
        Ok(files) => files,
        Err(error) => {
            theme.error_coded(
                "E1340",
                "could not discover formatter input files",
                &error,
                "fix the named path or run the formatter from the project root",
            );
            return ExitCodes::USAGE;
        }
    };
    let files = if parsed.flags.fmt_changed {
        let changed = match changed_files(&project_dir) {
            Ok(changed) => changed,
            Err(error) => {
                theme.error_coded(
                    "E1340",
                    "--changed needs a readable Git worktree",
                    &error,
                    "run this command inside a Git worktree or remove --changed",
                );
                return ExitCodes::USAGE;
            }
        };
        files
            .into_iter()
            .filter(|path| {
                relative_display(&project_dir, path).is_some_and(|rel| changed.contains(&rel))
            })
            .collect::<Vec<_>>()
    } else {
        files
    };

    let roots = Store::resolve();
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let staged = match stage_and_run(theme, &env, &formatter_program, &project_dir, &files) {
        Ok(staged) => staged,
        Err(code) => return code,
    };
    let changed = staged
        .iter()
        .filter(|file| file.before != file.after)
        .collect::<Vec<_>>();
    let check_only = parsed.flags.fmt_check || parsed.flags.fmt_dry_run || parsed.flags.fmt_diff;
    if changed.is_empty() {
        if parsed.flags.json {
            println!("{}", json_ok());
        }
        return ExitCodes::OK;
    }
    if check_only {
        report_changed(
            theme,
            &project_dir,
            &changed,
            parsed.flags.json,
            parsed.flags.fmt_diff || parsed.flags.fmt_dry_run,
        );
        return ExitCodes::USER_ERROR;
    }
    for file in changed {
        if let Err(error) = fs::write(&file.source, &file.after) {
            theme.error_coded(
                "E1340",
                &format!("could not write {}", file.source.display()),
                &error.to_string(),
                "fix the file permissions and rerun the formatter",
            );
            return ExitCodes::USAGE;
        }
    }
    if parsed.flags.json {
        println!("{}", json_ok());
    }
    ExitCodes::OK
}

fn valid_language(raw: &str) -> Option<&str> {
    let language = raw.trim().trim_start_matches('.');
    (!language.is_empty()
        && language != "."
        && !language.chars().any(|ch| ch.is_whitespace())
        && !language.contains('/')
        && !language.contains('\\'))
    .then_some(language)
}

fn collect_files(
    cwd: &Path,
    project_dir: &Path,
    explicit: &[String],
    language: &str,
) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if explicit.is_empty() {
        walk_files(project_dir, language, &mut files);
    } else {
        for raw in explicit {
            let path = Path::new(raw);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if metadata.is_dir() {
                walk_files(&path, language, &mut files);
            } else if metadata.is_file() && matches_language(&path, language) {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk_files(dir: &Path, language: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let ignored = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| IGNORED_DIRS.contains(&name));
            if !ignored {
                walk_files(&path, language, files);
            }
        } else if kind.is_file() && matches_language(&path, language) {
            files.push(path);
        }
    }
}

fn matches_language(path: &Path, language: &str) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some(language)
}

fn stage_and_run(
    theme: &Theme,
    env: &Shell::Env,
    program: &str,
    project_dir: &Path,
    files: &[PathBuf],
) -> Result<Vec<StagedFile>, i32> {
    let temp = match TempDir::new() {
        Ok(temp) => temp,
        Err(error) => {
            theme.error_coded(
                "E1340",
                "could not stage formatter inputs",
                &error,
                "fix the temporary directory and retry",
            );
            return Err(ExitCodes::USAGE);
        }
    };
    let mut staged = Vec::with_capacity(files.len());
    let mut command = vec![program.to_string()];
    for (index, source) in files.iter().enumerate() {
        let before = match fs::read(source) {
            Ok(bytes) => bytes,
            Err(error) => {
                theme.error_coded(
                    "E1340",
                    &format!("could not read {}", source.display()),
                    &error.to_string(),
                    "fix the file permissions and retry",
                );
                return Err(ExitCodes::USAGE);
            }
        };
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed("input"));
        let staged_path = temp.path().join(format!("{index:04}-{name}"));
        if let Err(error) = fs::write(&staged_path, &before) {
            theme.error_coded(
                "E1340",
                "could not stage formatter inputs",
                &error.to_string(),
                "fix the temporary directory and retry",
            );
            return Err(ExitCodes::USAGE);
        }
        command.push(staged_path.to_string_lossy().into_owned());
        staged.push(StagedFile {
            source: source.clone(),
            staged: staged_path,
            before,
            after: Vec::new(),
        });
    }
    if !files.is_empty() {
        let code = Shell::run_command_in_silent(env, &command, Some(project_dir));
        if code != ExitCodes::OK {
            theme.error_coded(
                "E1340",
                &format!("environment formatter {program} failed"),
                &format!("the formatter returned exit code {code}; source files were not written"),
                "fix the formatter configuration or run the realized formatter directly",
            );
            return Err(code);
        }
    }
    for file in &mut staged {
        match fs::read(&file.staged) {
            Ok(after) => file.after = after,
            Err(error) => {
                theme.error_coded(
                    "E1340",
                    "environment formatter did not produce readable output",
                    &error.to_string(),
                    "check the formatter package and retry",
                );
                return Err(ExitCodes::USER_ERROR);
            }
        }
    }
    Ok(staged)
}

fn relative_display(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn changed_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let commands = [
        vec!["diff", "--name-only", "HEAD"],
        vec!["diff", "--name-only", "--cached"],
        vec!["ls-files", "--others", "--exclude-standard"],
    ];
    let mut changed = BTreeSet::new();
    for args in commands {
        let output = Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() && args.first() == Some(&"diff") {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let path = line.trim().replace('\\', "/");
            if !path.is_empty() {
                changed.insert(path);
            }
        }
    }
    Ok(changed)
}

fn report_changed(_theme: &Theme, root: &Path, files: &[&StagedFile], json: bool, show_diff: bool) {
    let entries = files
        .iter()
        .filter_map(|file| {
            relative_display(root, &file.source).map(|path| {
                let diff = String::from_utf8(file.before.clone())
                    .ok()
                    .zip(String::from_utf8(file.after.clone()).ok())
                    .map(|(before, after)| {
                        jet_codegen::Formatter::unified_diff(&path, &before, &after)
                    });
                (path, diff)
            })
        })
        .collect::<Vec<_>>();
    if json {
        let files = if show_diff {
            entries
                .iter()
                .map(|(path, diff)| {
                    format!(
                        "{{\"path\":{},\"diff\":{}}}",
                        crate::JSON::quote(path),
                        crate::JSON::quote(diff.as_deref().unwrap_or(""))
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        } else {
            entries
                .iter()
                .map(|(path, _)| crate::JSON::quote(path))
                .collect::<Vec<_>>()
                .join(",")
        };
        println!(
            "{{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"dirty\",\"files\":[{}]}}",
            files
        );
        return;
    }
    for (path, diff) in entries {
        println!("{path}");
        if show_diff {
            if let Some(diff) = diff {
                print!("{diff}");
            }
        }
    }
}

fn json_ok() -> &'static str {
    "{\"schema_version\":1,\"command\":\"fmt\",\"status\":\"ok\"}"
}
