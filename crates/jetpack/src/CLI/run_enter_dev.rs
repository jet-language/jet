use super::package_hangar_vendor::auto_clean_after_success;
use super::parse::Parsed;
use super::realize::{
    apply_locked_channels, classify_or_report, load_project_plan_with_selections, project_env_root,
    RunPlan,
};
use super::services_secrets_config::{
    find_jet_binary, find_project_entry, has_dev_or_run_entry, list_project_tasks,
    project_task_declared, project_task_metadata, run_lifecycle_hooks, run_lifecycle_hooks_clean,
    run_lifecycle_hooks_silent,
    validate_declared_secrets,
    wait_for_services_ready,
};
use super::trust_env_build::compose_env;
use super::workspace_sources::{cwd_table, load_workspace};
use crate::EnvFile;
use crate::EnvFiles;
use crate::EnvHook;
use crate::Bridge;
use crate::MemberSelect::{self, SelectRequest};
use jet_env_model::ModuleEval;
use crate::Output::Theme;
use crate::RefSpec;
use crate::Shell::{self, Env, ShellKind};
use crate::Store;
use crate::Syntax;
use crate::Trust;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `jetpack run [<ref>|<task>] [-- cmd…]`
///
/// D-JPK-TASKRUN1: a bare first positional that names a `#Job fn` in the
/// project entry runs that task (via `jet run --task <name> <entry>`). Package
/// refs (`source:pkg`, workspace members) keep the existing realize path.
pub(super) fn cmd_run(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let select_req = SelectRequest {
        packages: parsed.flags.workspace_members.clone(),
        affected: parsed.flags.affected,
        affected_since: parsed.flags.affected_since.clone(),
    };
    // D-JPK-SELECTOR1=C: workspace + selection flags → realize only those members.
    if project_dir.join(Syntax::WORKSPACE_FILE).exists() && select_req.is_restricting() {
        if let Some(result) = load_workspace(&project_dir) {
            return match result {
                Err(code) => code,
                Ok(plan) => match MemberSelect::select_members(&project_dir, &plan, &select_req) {
                    Ok(selected) if selected.is_empty() => {
                        theme.status("no workspace members matched the selection.");
                        0
                    }
                    Ok(_) => {
                        // Realize selected members through the build path.
                        let code = super::trust_env_build::cmd_build(theme, parsed);
                        if code != 0 {
                            return code;
                        }
                        match &parsed.command {
                            Some(cmd) if !cmd.is_empty() => {
                                let mut plan = match load_project_plan_with_selections(
                                    theme,
                                    parsed.flags.profile.as_deref(),
                                    parsed.flags.environment_profile.as_deref(),
                                ) {
                                    Ok(plan) => plan,
                                    Err(code) => return code,
                                };
                                if let Err(code) =
                                    apply_locked_channels(theme, &project_dir, &mut plan.table)
                                {
                                    return code;
                                }
                                let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                                    Ok(env) => env,
                                    Err(code) => return code,
                                };
                                run_visible_command(theme, &env, &plan.refs, cmd)
                            }
                            _ => 0,
                        }
                    }
                    Err(d) => {
                        theme.error_coded(&d.code, &d.what, &d.why, &d.fix);
                        2
                    }
                },
            };
        }
    }

    let entry = find_project_entry(&project_dir);
    let declared_tasks = list_project_tasks(&entry);

    // Prefer a project `#Job` over package-ref classification when the first
    // positional is a bare name (no `@source` suffix).
    if let Some(raw) = parsed.positional.first() {
        if !raw.contains(Syntax::REF_PROVIDER_AT) && declared_tasks.iter().any(|t| t == raw) {
            return run_project_task(theme, parsed, &roots, &project_dir, &entry, raw);
        }
    }

    // Collect the refs to realize plus the source table that resolves any
    // named sources: an explicit CLI ref (built-ins only), or the project pack.
    let mut explicit_package: Option<String> = None;
    let mut plan = match parsed.positional.first() {
        Some(raw) => match classify_or_report(theme, raw) {
            Ok(spec) => {
                explicit_package = Some(spec.short_name().to_string());
                RunPlan {
                    project_root: project_dir.clone(),
                    refs: vec![spec],
                    adapters: Vec::new(),
                    table: cwd_table(),
                    label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
                    prompt_path: ModuleEval::PromptPathMode::default(),
                    prompt_strip: ModuleEval::PromptStripMode::default(),
                    dev_services: Vec::new(),
                    secrets: Vec::new(),
                    environment: ModuleEval::EnvironmentFacts::default(),
                }
            }
            Err(_) => {
                // Bare unknown name + declared tasks → E1290 (list them).
                if !raw.contains(Syntax::REF_PROVIDER_AT) && !declared_tasks.is_empty() {
                    let list = declared_tasks.join(", ");
                    theme.error_coded(
                        "E1294",
                        &format!("no task named `{raw}`"),
                        "`jetpack run <name>` invokes a `#Job fn` in the project entry (D-JPK-TASKRUN1).",
                        "mark a function `#Job` to make it runnable, or check the spelling.",
                    );
                    theme.detail(&format!("declared tasks: {list}"));
                    return 2;
                }
                return 2;
            }
        },
        None => match load_project_plan_with_selections(
            theme,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(code) => return code,
        },
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let entry = find_project_entry(&project_dir);
    if let Err(code) = run_lifecycle_hooks(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.environment.lifecycle.on_enter,
        "on_enter",
    ) {
        return code;
    }

    let code = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => run_visible_command(theme, &env, &plan.refs, cmd),
        _ => {
            if let Some(program) = explicit_package {
                let cmd = vec![program];
                run_visible_command(theme, &env, &plan.refs, &cmd)
            } else {
                Shell::enter(theme, &env, ShellKind::detect())
            }
        }
    };
    if code == 0 {
        auto_clean_after_success(theme, &roots);
    }
    code
}

/// D-JPK-TASKRUN1: realize the project env (when present), then shell out to
/// `jet run --task <name> <entry> -- <task-args>` (D-JPK-DISPATCH1).
pub(super) fn run_project_task(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    task: &str,
) -> i32 {
    run_project_task_with_mode(theme, parsed, roots, project_dir, entry, task, false, false)
}

pub(super) fn run_project_task_with_mode(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    task: &str,
    clean: bool,
    silent: bool,
) -> i32 {
    if project_task_declared(entry, task) == Some(false) {
        let declared_tasks = list_project_tasks(entry);
        theme.error_coded(
            "E1294",
            &format!("no task named `{task}`"),
            "lifecycle task names must refer to a declared top-level #Job function.",
            "declare the task with #Job, or remove it from the environment lifecycle.",
        );
        if !declared_tasks.is_empty() {
            theme.detail(&format!("declared tasks: {}", declared_tasks.join(", ")));
        }
        return 2;
    }
    let metadata = project_task_metadata(entry, task).unwrap_or_default();
    if let Some(reason) = task_skip_reason(metadata.skip.as_ref()) {
        theme.status(&format!("skipping task {}: {}", theme.bold(task), reason));
        return 0;
    }

    let has_env = EnvFile::path_in(project_dir).is_file();
    // Env is optional for task-only projects. A task may still add package
    // metadata, in which case it is composed through the same RunPlan path as
    // `jet env` rather than by mutating PATH in this dispatcher.
    let mut plan = if has_env {
        match load_project_plan_with_selections(
            theme,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(code) => return code,
        }
    } else {
        empty_task_plan()
    };
    if let Err(code) = apply_locked_channels(theme, project_dir, &mut plan.table) {
        return code;
    }
    for raw in &metadata.packages {
        let spec = match RefSpec::classify_in(raw, &plan.table) {
            Ok(spec) => spec,
            Err(error) => {
                crate::Output::ref_error(theme, &error);
                return 2;
            }
        };
        if !plan.refs.iter().any(|existing| existing.raw == spec.raw) {
            plan.refs.push(spec);
        }
    }

    if let Err(code) = Trust::gate_with_environment(
        theme,
        &Trust::store_path(),
        project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        parsed.flags.trust,
    ) {
        return code;
    }
    if let Err(code) = validate_declared_secrets(theme, project_dir, &plan.secrets) {
        return code;
    }
    let mut env = if has_env || !plan.refs.is_empty() {
        match compose_env(theme, roots, &parsed.flags, &plan) {
            Ok(env) => env,
            Err(code) => return code,
        }
    } else {
        empty_task_env()
    };
    if let Some(authority) = &metadata.authority {
        env.vars
            .insert("JET_TASK_AUTHORITY".to_string(), authority.clone());
    }
    for (name, value) in &metadata.limits {
        let key = task_limit_env_name(name);
        env.vars.insert(key, value.clone());
    }

    let task_args: Vec<String> = parsed
        .positional
        .iter()
        .skip(1)
        .cloned()
        .chain(parsed.command.iter().flatten().cloned())
        .collect();

    let task_cwd = match task_path(project_dir, metadata.cwd.as_deref(), "cwd", false) {
        Ok(path) => path,
        Err(message) => {
            theme.error_coded(
                "E1330",
                &format!("task `{task}` has an unsafe cwd"),
                &message,
                "use a project-relative path without `..`.",
            );
            return 2;
        }
    };
    if !task_cwd.is_dir() {
        theme.error_coded(
            "E1330",
            &format!("task `{task}` has a non-directory cwd"),
            &format!("task cwd `{}` is not a directory", task_cwd.display()),
            "use a project-relative directory for `cwd`.",
        );
        return 2;
    }
    let mut jet_binary = find_jet_binary();
    let cache_key = if metadata.cache == crate::AST::TaskCachePolicy::Uncached {
        None
    } else {
        if metadata.outputs.is_empty() {
            theme.error_coded(
                "E1330",
                &format!("task `{task}` enables caching without outputs"),
                "a cached task needs at least one declared output so a later run can prove that the result still exists.",
                "add `outputs: [\"path\"]`, or use `cache: .Uncached`.",
            );
            return 2;
        }
        if let Err(message) = validate_cached_task_metadata(project_dir, &metadata) {
            theme.error_coded(
                "E1330",
                &format!("task `{task}` has unsafe cache declarations"),
                &message,
                "declare every project input and keep cached outputs separate from inputs, or use `cache: .Uncached`.",
            );
            return 2;
        }
        if let Err(message) = validate_cached_task_environment(&plan, &env) {
            theme.error_coded(
                "E1330",
                &format!("task `{task}` has an unsafe cached environment"),
                &message,
                "remove secret-bearing environment inputs, or use `cache: .Uncached`.",
            );
            return 2;
        }
        jet_binary = match resolve_task_jet_binary(&env) {
            Ok(path) => path,
            Err(message) => {
                theme.error_coded(
                    "E1330",
                    &format!("task `{task}` cannot resolve its compiler"),
                    &message,
                    "make the compiler named by the task environment available, or use `cache: .Uncached`.",
                );
                return 2;
            }
        };
        let key = match task_cache_key(
            project_dir,
            entry,
            task,
            Path::new(&jet_binary),
            &metadata,
            &task_args,
            &plan.refs,
            &plan.table,
            &task_environment_hash_with_env(
                &plan.refs,
                &plan.table,
                &plan.secrets,
                &plan.environment,
                &env,
            ),
        ) {
            Ok(key) => key,
            Err(message) => {
                theme.error_coded(
                    "E1330",
                    &format!("task `{task}` has invalid cache inputs or outputs"),
                    &message,
                    "use existing project-relative input and output paths.",
                );
                return 2;
            }
        };
        if task_cache_hit(project_dir, roots, &metadata, &key) {
            theme.status(&format!("task {} is up to date", theme.bold(task)));
            return 0;
        }
        Some(key)
    };

    theme.status(&format!(
        "running task {} ({})",
        theme.bold(task),
        theme.gray(&entry.display().to_string())
    ));

    let mut argv = vec![
        jet_binary,
        "run".to_string(),
        format!("--task={task}"),
        entry.to_string_lossy().into_owned(),
    ];
    if !task_args.is_empty() {
        argv.push("--".to_string());
        argv.extend(task_args);
    }
    let access_trace = cache_key
        .as_deref()
        .map(task_access_trace_path);
    let code = if let Some(trace_path) = access_trace.as_deref() {
        // A strict cache key must describe the complete task environment. The
        // ordinary direct-task path inherits host variables, so cached tasks
        // use the clean composed environment whose values are in the key.
        match run_task_with_access_trace(
            &env,
            &argv,
            &task_cwd,
            true,
            silent,
            trace_path,
        ) {
            Ok(code) => code,
            Err(message) => {
                theme.error_coded(
                    "E1330",
                    &format!("task `{task}` cannot prove strict cache access"),
                    &message,
                    "run the cached task on a host with file-access tracing, or use `cache: .Uncached`.",
                );
                return 2;
            }
        }
    } else if clean && silent {
        Shell::run_clean_command_in_silent(&env, &argv, Some(&task_cwd))
    } else if clean {
        Shell::run_clean_command_in(&env, &argv, Some(&task_cwd))
    } else if silent {
        Shell::run_command_in_silent(&env, &argv, Some(&task_cwd))
    } else {
        Shell::run_command_in(&env, &argv, Some(&task_cwd))
    };
    if code != 0 {
        if let Some(path) = access_trace.as_deref() {
            let _ = std::fs::remove_file(path);
        }
    }
    if code == 0 {
        if let Some(cache_key) = cache_key {
            let undeclared = access_trace
                .as_deref()
                .map(|path| {
                    let result = task_undeclared_accesses(project_dir, &task_cwd, entry, &metadata, path);
                    let _ = std::fs::remove_file(path);
                    result
                })
                .transpose();
            let undeclared = match undeclared {
                Ok(Some(paths)) => paths,
                Ok(None) => Vec::new(),
                Err(message) => {
                    theme.error_coded(
                        "E1330",
                        &format!("task `{task}` access proof failed"),
                        &message,
                        "fix the trace file or use `cache: .Uncached` for tasks that cannot be proven.",
                    );
                    return 1;
                }
            };
            if !undeclared.is_empty() {
                theme.error_coded(
                    "E1330",
                    &format!("task `{task}` read undeclared project files"),
                    &undeclared.join(", "),
                    "declare every read path in `inputs`, or use `cache: .Uncached`.",
                );
                return 1;
            }
            if !task_outputs_exist(project_dir, &metadata) {
                theme.error_coded(
                    "E1330",
                    &format!("task `{task}` did not produce its declared outputs"),
                    "a successful cached task must leave every declared output in place before its result can be recorded",
                    "write each declared output, or remove it from `outputs` and keep the task uncached",
                );
                return 1;
            }
            if let Err(error) = write_task_cache(project_dir, roots, &metadata, &cache_key) {
                theme.error(
                    "task completed but its cache record could not be written",
                    &error,
                    "fix permissions for `.jet/tasks` or the Jet state directory, then rerun the task.",
                );
                return 1;
            }
        }
        auto_clean_after_success(theme, roots);
    }
    code
}

fn task_access_trace_path(cache_key: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jet-task-access-{}-{}.log",
        std::process::id(),
        &cache_key[..cache_key.len().min(24)]
    ))
}

fn resolve_task_jet_binary(env: &Env) -> Result<String, String> {
    let requested = find_jet_binary();
    if let Some(stable) = env
        .cache_leases
        .iter()
        .find_map(|lease| lease.executable(&requested))
    {
        return Ok(stable.to_string_lossy().into_owned());
    }
    Ok(resolve_executable_path(&requested)?.to_string_lossy().into_owned())
}

fn run_task_with_access_trace(
    env: &Env,
    argv: &[String],
    cwd: &Path,
    clean: bool,
    silent: bool,
    trace_path: &Path,
) -> Result<i32, String> {
    if !cfg!(target_os = "linux") {
        return Err("strict cached task access tracing is currently supported only on Linux".to_string());
    }
    let base_path = if clean {
        crate::Platform::clean_path().to_string()
    } else {
        std::env::var("PATH").unwrap_or_default()
    };
    let composed_path = env.composed_path(&base_path);
    let tracer = resolve_executable_path_in("strace", &composed_path)?;
    let _ = std::fs::remove_file(trace_path);
    let mut traced = vec![
        tracer.to_string_lossy().into_owned(),
        "-f".to_string(),
        "-qq".to_string(),
        "-e".to_string(),
        "trace=%file".to_string(),
        "-o".to_string(),
        trace_path.to_string_lossy().into_owned(),
    ];
    traced.extend_from_slice(argv);
    let code = if clean && silent {
        Shell::run_clean_command_in_silent(env, &traced, Some(cwd))
    } else if clean {
        Shell::run_clean_command_in(env, &traced, Some(cwd))
    } else if silent {
        Shell::run_command_in_silent(env, &traced, Some(cwd))
    } else {
        Shell::run_command_in(env, &traced, Some(cwd))
    };
    if !trace_path.is_file() {
        return Err("file-access tracer completed without producing an access log".to_string());
    }
    Ok(code)
}

fn task_undeclared_accesses(
    project_dir: &Path,
    task_cwd: &Path,
    entry: &Path,
    metadata: &crate::AST::TaskMetadata,
    trace_path: &Path,
) -> Result<Vec<String>, String> {
    let trace = std::fs::read_to_string(trace_path)
        .map_err(|error| format!("couldn't read file-access trace: {error}"))?;
    let project_root = project_dir
        .canonicalize()
        .map_err(|error| format!("couldn't resolve project root for access proof: {error}"))?;
    let entry = entry
        .canonicalize()
        .map_err(|error| format!("couldn't resolve task entry for access proof: {error}"))?;
    let declared = metadata
        .inputs
        .iter()
        .map(|input| task_path(project_dir, Some(input), "input", true))
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = metadata
        .outputs
        .iter()
        .map(|output| task_path(project_dir, Some(output), "output", true))
        .collect::<Result<Vec<_>, _>>()?;
    let mut unexpected = BTreeSet::new();
    for line in trace.lines() {
        for raw in strace_paths(line) {
            let path = raw.as_path();
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                task_cwd.join(path)
            };
            let Some(path) = normalize_trace_path(&path) else {
                unexpected.insert(path.to_string_lossy().replace('\\', "/"));
                continue;
            };
            let path = if path.exists() {
                path.canonicalize().unwrap_or(path)
            } else {
                path
            };
            if !path.starts_with(&project_root)
                || path.starts_with(project_root.join(".git"))
                || path.starts_with(project_root.join("target"))
                || path == entry
                || declared.iter().any(|allowed| path == *allowed || path.starts_with(allowed))
                || outputs.iter().any(|allowed| path == *allowed || path.starts_with(allowed))
            {
                continue;
            }
            unexpected.insert(path.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(unexpected.into_iter().collect())
}

fn normalize_trace_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            std::path::Component::Normal(_)
            | std::path::Component::Prefix(_)
            | std::path::Component::RootDir => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized)
}

fn strace_paths(line: &str) -> Vec<PathBuf> {
    let Some(open) = line.find('(') else {
        return Vec::new();
    };
    let Some(name) = line[..open].split_whitespace().last() else {
        return Vec::new();
    };
    let args = strace_argument_tokens(&line[open + 1..]);
    let indices: Vec<usize> = match name {
        "execve" | "open" | "creat" | "stat" | "lstat" | "access"
        | "readlink" | "unlink" | "rmdir" | "truncate" | "chmod" | "chown"
        | "lchown" | "mknod" | "mkdir" | "chdir" | "getxattr" | "lgetxattr"
        | "setxattr" | "lsetxattr" | "listxattr" | "llistxattr" | "removexattr" => {
            vec![0]
        }
        "execveat" | "openat" | "openat2" | "statx" | "newfstatat" | "fstatat64"
        | "faccessat" | "faccessat2" | "readlinkat" | "unlinkat" | "mkdirat"
        | "fchmodat" | "fchownat" | "utimensat" => vec![1],
        "rename" | "renameat2" | "link" | "linkat" => vec![0, 1, 3],
        "renameat" => vec![1, 3],
        "symlink" => vec![0, 1],
        "symlinkat" => vec![0, 2],
        "pivot_root" => vec![0, 1],
        _ => (0..args.len()).collect(),
    };
    indices
        .into_iter()
        .filter_map(|index| {
            args.get(index)
                .and_then(|argument| strace_quoted_argument(argument))
        })
        .filter_map(decode_strace_string)
        .filter(|path| !path.is_empty() && path.as_slice() != b"?")
        .map(strace_path_from_bytes)
        .collect()
}

/// Return top-level syscall arguments. Nested strings, such as `execve`'s
/// argv array, are intentionally kept inside one argument so they cannot be
/// mistaken for file paths.
fn strace_argument_tokens(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match *byte {
            b'"' => quoted = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                tokens.push(&line[start..index]);
                start = index + 1;
            }
            b')' if depth == 0 => {
                tokens.push(&line[start..index]);
                break;
            }
            _ => {}
        }
    }
    tokens
}

fn strace_quoted_argument(argument: &str) -> Option<&str> {
    let argument = argument.trim();
    let bytes = argument.as_bytes();
    if bytes.first().copied() != Some(b'"') {
        return None;
    }
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Some(&argument[1..index]);
        }
    }
    None
}

fn decode_strace_string(raw: &str) -> Option<Vec<u8>> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let character = raw[index..].chars().next()?;
            let width = character.len_utf8();
            decoded.extend_from_slice(&bytes[index..index + width]);
            index += width;
            continue;
        }
        index += 1;
        let escaped = *bytes.get(index)?;
        match escaped {
            b'\\' => decoded.push(b'\\'),
            b'"' => decoded.push(b'"'),
            b'a' => decoded.push(0x07),
            b'b' => decoded.push(0x08),
            b'f' => decoded.push(0x0c),
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b't' => decoded.push(b'\t'),
            b'v' => decoded.push(0x0b),
            b'?' => decoded.push(b'?'),
            b'x' => {
                let high = hex_digit(*bytes.get(index + 1)?)?;
                let low = hex_digit(*bytes.get(index + 2)?)?;
                decoded.push(high * 16 + low);
                index += 2;
            }
            b'0'..=b'7' => {
                let mut value = escaped - b'0';
                let mut consumed = 1;
                while consumed < 3 {
                    let Some(next @ b'0'..=b'7') = bytes.get(index + consumed).copied() else {
                        break;
                    };
                    value = value * 8 + next - b'0';
                    consumed += 1;
                }
                decoded.push(value);
                index += consumed - 1;
            }
            other => decoded.push(other),
        }
        index += 1;
    }
    Some(decoded)
}

#[cfg(unix)]
fn strace_path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn strace_path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn empty_task_plan() -> RunPlan {
    RunPlan {
        project_root: std::env::current_dir().unwrap_or_default(),
        refs: Vec::new(),
        adapters: Vec::new(),
        table: RefSpec::SourceTable::empty(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        environment: ModuleEval::EnvironmentFacts::default(),
    }
}

fn empty_task_env() -> Env {
    Env {
        bin_dirs: Vec::new(),
        vars: std::collections::BTreeMap::new(),
        unset_vars: Vec::new(),
        refs: Vec::new(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        cache_leases: Vec::new(),
    }
}

fn task_limit_env_name(name: &str) -> String {
    let mut out = String::from("JET_TASK_LIMIT_");
    for ch in name.chars() {
        out.push(if ch.is_ascii_alphanumeric() { ch.to_ascii_uppercase() } else { '_' });
    }
    out
}

fn task_skip_reason(skip: Option<&crate::AST::TaskSkip>) -> Option<String> {
    skip.and_then(|rule| rule.reason_for_host(&crate::Envelope::host_platform()))
}

fn task_path(
    project_dir: &Path,
    raw: Option<&str>,
    field: &str,
    allow_missing: bool,
) -> Result<PathBuf, String> {
    let relative = raw.unwrap_or(".");
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(format!("task {field} `{relative}` must stay inside the project"));
    }
    let root = project_dir
        .canonicalize()
        .map_err(|error| format!("couldn't resolve project root: {error}"))?;
    let candidate = project_dir.join(path);
    if !allow_missing && !candidate.exists() {
        return Err(format!("task {field} `{relative}` does not exist"));
    }
    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|error| format!("couldn't resolve task {field} `{relative}`: {error}"))?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| format!("task {field} `{relative}` has no parent"))?
            .canonicalize()
            .map_err(|error| format!("couldn't resolve task {field} parent: {error}"))?;
        let name = candidate
            .file_name()
            .ok_or_else(|| format!("task {field} `{relative}` has no file name"))?;
        parent.join(name)
    };
    if !resolved.starts_with(&root) {
        return Err(format!("task {field} `{relative}` escapes the project"));
    }
    Ok(resolved)
}

fn task_cache_key(
    project_dir: &Path,
    entry: &Path,
    task: &str,
    compiler_path: &Path,
    metadata: &crate::AST::TaskMetadata,
    task_args: &[String],
    refs: &[RefSpec::RefSpec],
    table: &RefSpec::SourceTable,
    environment_hash: &str,
) -> Result<String, String> {
    let mut identity = String::from("jet-task-cache-v2\n");
    identity.push_str(task);
    identity.push('\n');
    identity.push_str(&format!("compiler={}\n", env!("CARGO_PKG_VERSION")));
    identity.push_str(&format!(
        "compiler-build={}\n",
        crate::SHA256::sha256_file_hex(&compiler_path)
            .map_err(|error| format!("couldn't hash compiler `{}`: {error}", compiler_path.display()))?
    ));
    identity.push_str(&format!("platform={}\n", crate::Envelope::host_platform()));
    identity.push_str(
        &crate::SHA256::sha256_file_hex(entry)
            .map_err(|error| format!("couldn't hash task entry: {error}"))?,
    );
    identity.push('\n');
    identity.push_str(&format!("args={task_args:?}\n"));
    identity.push_str(&format!("environment={environment_hash}\n"));
    identity.push_str(&format!("packages={:?}\n", metadata.packages));
    identity.push_str(&format!("inputs={:?}\n", metadata.inputs));
    identity.push_str(&format!("outputs={:?}\n", metadata.outputs));
    identity.push_str(&format!("skip={:?}\n", metadata.skip));
    identity.push_str(&format!("cwd={:?}\n", metadata.cwd));
    identity.push_str(&format!("cache={:?}\nauthority={:?}\n", metadata.cache, metadata.authority));
    for (name, value) in &metadata.limits {
        identity.push_str(&format!("limit={name}:{value}\n"));
    }
    for reference in refs {
        identity.push_str("ref=");
        identity.push_str(&reference.raw);
        identity.push('\n');
    }
    for (name, upstream, provider) in table.declarations() {
        identity.push_str(&format!("source={name}:{upstream}:{}\n", provider.label()));
    }
    for relative in [Syntax::ENV_FILE, "jetpack.toml", Syntax::UNIFIED_LOCK_FILE] {
        let path = project_dir.join(relative);
        if path.is_file() {
            identity.push_str(&format!("project-input={relative}:{}\n", crate::SHA256::sha256_file_hex(&path).map_err(|error| format!("couldn't hash `{relative}`: {error}"))?));
        }
    }
    if metadata.inputs.is_empty() {
        identity.push_str("inputs=<none>\n");
    }
    for input in &metadata.inputs {
        let path = task_path(project_dir, Some(input), "input", false)?;
        let digest = if path.is_dir() {
            crate::SHA256::tree_hash(&path)
        } else if path.is_file() {
            crate::SHA256::sha256_file_hex(&path)
                .map_err(|error| format!("couldn't hash task input `{input}`: {error}"))?
        } else {
            return Err(format!("task input `{input}` is not a regular file or directory"));
        };
        identity.push_str(&format!("input={input}:{digest}\n"));
    }
    for output in &metadata.outputs {
        let path = task_path(project_dir, Some(output), "output", true)?;
        identity.push_str(&format!("output={output}:{}\n", path.display()));
    }
    if metadata.cache != crate::AST::TaskCachePolicy::Uncached {
        identity.push_str(&format!(
            "project-scope={}\n",
            task_project_scope_fingerprint(project_dir, metadata)?
        ));
    }
    Ok(crate::SHA256::sha256_hex(identity.as_bytes()))
}

fn task_environment_hash_with_env(
    refs: &[RefSpec::RefSpec],
    table: &RefSpec::SourceTable,
    secrets: &[String],
    facts: &ModuleEval::EnvironmentFacts,
    env: &Env,
) -> String {
    task_environment_hash_with_vars(refs, table, secrets, facts, &env.vars)
}

fn task_environment_hash_with_vars(
    refs: &[RefSpec::RefSpec],
    table: &RefSpec::SourceTable,
    secrets: &[String],
    facts: &ModuleEval::EnvironmentFacts,
    vars: &BTreeMap<String, String>,
) -> String {
    let mut identity = Trust::environment_definition_hash(refs, table, secrets, facts);
    identity.push_str("\n--task-active-environment--\n");
    identity.push_str(facts.active_environment.as_deref().unwrap_or("<none>"));
    identity.push('\n');
    for module in &facts.active_environment_provenance {
        identity.push_str("provenance=");
        identity.push_str(module);
        identity.push('\n');
    }
    identity.push_str("--task-source-files--\n");
    for source in &facts.source_files {
        identity.push_str(source);
        identity.push('\n');
    }
    identity.push_str("--task-environment-values--\n");
    for (name, value) in vars {
        identity.push_str(name);
        identity.push('=');
        identity.push_str(&crate::SHA256::sha256_hex(value.as_bytes()));
        identity.push('\n');
    }
    crate::SHA256::sha256_hex(identity.as_bytes())
}

fn resolve_executable_path(program: &str) -> Result<PathBuf, String> {
    let search = std::env::var_os("PATH")
        .ok_or_else(|| format!("couldn't resolve executable `{program}`: PATH is unavailable"))?;
    resolve_executable_path_in(program, &search.to_string_lossy())
}

fn resolve_executable_path_in(program: &str, search: &str) -> Result<PathBuf, String> {
    let path = Path::new(program);
    if path.is_absolute() || path.components().count() > 1 {
        return path
            .canonicalize()
            .map_err(|error| format!("couldn't resolve executable `{program}`: {error}"));
    }
    for directory in std::env::split_paths(search) {
        let candidate = directory.join(program);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|error| format!("couldn't resolve executable `{program}`: {error}"));
        }
    }
    Err(format!("couldn't resolve executable `{program}` through PATH"))
}

fn validate_cached_task_metadata(
    project_dir: &Path,
    metadata: &crate::AST::TaskMetadata,
) -> Result<(), String> {
    if metadata.cache == crate::AST::TaskCachePolicy::Uncached {
        return Ok(());
    }
    if metadata.inputs.is_empty() {
        return Err(
            "strict cached tasks must declare at least one project input; undeclared reads cannot be proven safe".to_string(),
        );
    }
    let outputs = metadata
        .outputs
        .iter()
        .map(|output| -> Result<(&String, PathBuf), String> {
            reject_cache_sensitive_path(output, "output")?;
            let path = task_path(project_dir, Some(output), "output", true)?;
            reject_cache_sensitive_resolved_path(project_dir, &path, output, "output")?;
            Ok((output, path))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let inputs = metadata
        .inputs
        .iter()
        .map(|input| -> Result<(&String, PathBuf), String> {
            reject_cache_sensitive_path(input, "input")?;
            let path = task_path(project_dir, Some(input), "input", true)?;
            reject_cache_sensitive_resolved_path(project_dir, &path, input, "input")?;
            reject_cache_sensitive_descendants(&path, input, "input")?;
            Ok((input, path))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (output, output_path) in outputs {
        if inputs.iter().any(|(_, input)| {
            input == &output_path || input.starts_with(&output_path) || output_path.starts_with(input)
        }) {
            return Err(format!(
                "cached task input `{}` overlaps output `{output}`",
                inputs
                    .iter()
                    .find(|(_, input)| {
                        input == &output_path
                            || input.starts_with(&output_path)
                            || output_path.starts_with(input)
                    })
                    .map(|(name, _)| name.as_str())
                    .unwrap_or("<unknown>")
            ));
        }
    }
    for (input_name, input_path) in &inputs {
        if !input_path.exists() {
            return Err(format!("task input `{input_name}` does not exist"));
        }
    }
    Ok(())
}

fn validate_cached_task_environment(plan: &RunPlan, env: &Env) -> Result<(), String> {
    if !plan.secrets.is_empty() {
        return Err(
            "strict cached tasks cannot use declared secrets; secret values are never cache inputs"
                .to_string(),
        );
    }
    if let Some(dotenv) = plan
        .environment
        .lifecycle
        .dotenv
        .iter()
        .find(|dotenv| !dotenv.secrets.is_empty())
    {
        return Err(format!(
            "strict cached tasks cannot use secret dotenv variables from `{}`",
            dotenv.file
        ));
    }
    if let Some(name) = env.vars.keys().find(|name| is_sensitive_environment_name(name)) {
        return Err(format!(
            "strict cached tasks cannot use secret-bearing environment variable `{name}`"
        ));
    }
    Ok(())
}

fn reject_cache_sensitive_path(relative: &str, field: &str) -> Result<(), String> {
    if is_cache_sensitive_path(Path::new(relative)) {
        return Err(format!(
            "cached task {field} `{relative}` is secret-bearing or Jet state"
        ));
    }
    Ok(())
}

fn reject_cache_sensitive_resolved_path(
    project_dir: &Path,
    path: &Path,
    declared: &str,
    field: &str,
) -> Result<(), String> {
    let root = project_dir
        .canonicalize()
        .map_err(|error| format!("couldn't resolve cached task project root: {error}"))?;
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| format!("cached task {field} `{declared}` escapes the project"))?;
    if is_cache_sensitive_path(relative) {
        return Err(format!(
            "cached task {field} `{declared}` resolves into secret-bearing or Jet state `{}`",
            relative.display()
        ));
    }
    Ok(())
}

fn reject_cache_sensitive_descendants(
    root: &Path,
    relative: &str,
    field: &str,
) -> Result<(), String> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)
        .map_err(|error| format!("couldn't inspect cached task {field} `{relative}`: {error}"))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let child = Path::new(relative).join(entry.file_name());
        if is_cache_sensitive_path(&child) {
            return Err(format!(
                "cached task {field} `{relative}` contains secret-bearing or Jet state `{}`",
                child.display()
            ));
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err(format!(
                "strict cached task {field} `{relative}` contains unsupported symlink `{}`",
                child.display()
            ));
        }
        if file_type.is_dir() {
            reject_cache_sensitive_descendants(&entry.path(), &child.to_string_lossy(), field)?;
        }
    }
    Ok(())
}

fn task_project_scope_fingerprint(
    project_dir: &Path,
    metadata: &crate::AST::TaskMetadata,
) -> Result<String, String> {
    let mut files = Vec::new();
    collect_task_scope_files(project_dir, project_dir, metadata, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut identity = String::from("jet-task-project-scope-v1\n");
    for (path, digest) in files {
        identity.push_str(&path);
        identity.push('=');
        identity.push_str(&digest);
        identity.push('\n');
    }
    Ok(crate::SHA256::sha256_hex(identity.as_bytes()))
}

fn collect_task_scope_files(
    root: &Path,
    directory: &Path,
    metadata: &crate::AST::TaskMetadata,
    files: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("couldn't read task cache scope `{}`: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?;
        if is_cache_sensitive_path(relative)
            || relative.components().next().is_some_and(|component| {
                matches!(component, std::path::Component::Normal(name) if name == ".git" || name == "target")
            }) || metadata.outputs.iter().any(|output| {
            let output = Path::new(output);
            relative == output || relative.starts_with(output)
        }) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_task_scope_files(root, &path, metadata, files)?;
        } else if file_type.is_file() {
            files.push((
                relative.to_string_lossy().replace('\\', "/"),
                crate::SHA256::sha256_file_hex(&path)
                    .map_err(|error| format!("couldn't hash task scope `{}`: {error}", relative.display()))?,
            ));
        } else if file_type.is_symlink() {
            return Err(format!(
                "strict cached task scope contains unsupported symlink `{}`",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn is_cache_sensitive_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        matches!(component, std::path::Component::Normal(name) if is_cache_sensitive_component(name))
    })
}

fn is_cache_sensitive_component(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return true;
    };
    let name = name.to_ascii_lowercase();
    name == ".jet"
        || name == ".env"
        || name.starts_with(".env.")
        || ["secret", "credential", "token", "password"]
            .iter()
            .any(|word| name.contains(word))
}

fn is_sensitive_environment_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .split(['_', '-', '.'])
        .any(|part| {
            matches!(
                part,
                "secret"
                    | "secrets"
                    | "credential"
                    | "credentials"
                    | "token"
                    | "tokens"
                    | "password"
                    | "passwords"
                    | "key"
                    | "keys"
            )
        })
}

fn task_cache_path(
    project_dir: &Path,
    roots: &Store::Roots,
    metadata: &crate::AST::TaskMetadata,
    key: &str,
) -> PathBuf {
    match metadata.cache {
        crate::AST::TaskCachePolicy::Local => project_dir.join(Syntax::SOURCE_ROOT_DIR).join("tasks").join(format!("{key}.done")),
        crate::AST::TaskCachePolicy::Shared => roots.root.join("tasks").join(format!("{key}.done")),
        crate::AST::TaskCachePolicy::Uncached => PathBuf::new(),
    }
}

fn task_outputs_exist(project_dir: &Path, metadata: &crate::AST::TaskMetadata) -> bool {
    metadata
        .outputs
        .iter()
        .all(|output| task_path(project_dir, Some(output), "output", false).is_ok())
}

fn task_cache_hit(
    project_dir: &Path,
    roots: &Store::Roots,
    metadata: &crate::AST::TaskMetadata,
    key: &str,
) -> bool {
    if metadata.cache == crate::AST::TaskCachePolicy::Uncached || !task_outputs_exist(project_dir, metadata) {
        return false;
    }
    let path = task_cache_path(project_dir, roots, metadata, key);
    matches!(std::fs::read_to_string(path), Ok(value) if value.trim() == key)
}

fn write_task_cache(
    project_dir: &Path,
    roots: &Store::Roots,
    metadata: &crate::AST::TaskMetadata,
    key: &str,
) -> Result<(), String> {
    let path = task_cache_path(project_dir, roots, metadata, key);
    let parent = path
        .parent()
        .ok_or_else(|| "task cache has no parent directory".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = parent.join(format!(".{key}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, format!("{key}\n")).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

fn run_visible_command(theme: &Theme, env: &Env, refs: &[RefSpec::RefSpec], cmd: &[String]) -> i32 {
    if let Some(program) = cmd.first() {
        let ref_label = refs
            .first()
            .map(|r| r.raw.as_str())
            .unwrap_or("project env");
        let arg_note = if cmd.len() == 1 { " (no args)" } else { "" };
        theme.status(&format!(
            "running {} -> {}{}",
            theme.bold(ref_label),
            theme.bold(program),
            theme.gray(arg_note)
        ));
    }
    Shell::run_command(env, cmd)
}

/// `jetpack enter [-- cmd]` — realize the project environment and drop into its
/// shell (Scale-2; U §8). Unlike `run`, `enter` is project-scoped: it never
/// takes an explicit ref, it always composes the env declared by the project
/// `env.jet`. The `-- cmd` form runs a one-off command in that env, then exits.
///
/// U16 additions: `-p <pkg>...` folds ad-hoc nixpkgs packages into the plan
/// (same trust gate, same realize path, as a manifest ref); `--flake` forces
/// (and the absence of any declared `env.*` module otherwise triggers) a
/// foreign `flake.nix`/`devenv.nix` fallback that uses Jetpack's bounded native
/// projection instead of composing a second shell model.
pub(super) fn cmd_enter(theme: &Theme, parsed: &Parsed) -> i32 {
    // D-ENVHOOK1=A: `jet env hook <shell>` / `jet env export <shell>` route
    // through `jetpack enter` (D-JPK-DISPATCH1) as reserved first-positional
    // subverbs of `jet env`. The bare `jet env` shell-entry (no positional, or
    // a `-p`/`--flake`/`-- cmd` form) is untouched.
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::ENV_HOOK_VERB => return cmd_env_hook(theme, parsed),
        Some(v) if v == Syntax::ENV_EXPORT_VERB => return cmd_env_export(theme, parsed),
        Some(v) if v == Syntax::ENV_TEST_VERB => return cmd_env_test(theme, parsed),
        Some(v) if v == Syntax::ENV_SYNC_VERB => return cmd_env_sync(theme, parsed),
        Some(v) if v == Syntax::ENV_INFO_VERB => return cmd_env_info(theme, parsed),
        _ => {}
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);

    // U16: a project's own `env.*` always wins; the foreign-flake fallback
    // only kicks in when it declares none, or when `--flake` forces it. An
    // explicit `-p` request is an active signal of intent and must never be
    // silently discarded by the passive auto-detect fallback — only
    // `--flake` (an equally explicit signal) can still force the foreign
    // shell over ad-hoc packages.
    let foreign = foreign_flake_path(&project_dir);
    let auto_detect_wants_foreign = foreign.is_some()
        && !project_declares_env(&project_dir)
        && parsed.flags.packages.is_empty();
    if parsed.flags.flake || auto_detect_wants_foreign {
        let Some(flake_path) = foreign else {
            theme.error(
                "no foreign flake here",
                &format!(
                    "`--flake` was passed but no {}/{} was found in this directory.",
                    Syntax::FOREIGN_FLAKE_FILE,
                    Syntax::FOREIGN_DEVENV_FILE
                ),
                "remove --flake to use the project's own env.*, or add a flake.nix.",
            );
            return 2;
        };
        return enter_foreign_flake(theme, &project_dir, &flake_path, parsed);
    }

    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    // U16: `-p` needs no manifest at all — a project with no `env.jet` and at
    // least one ad-hoc package still gets a (package-only) shell instead of
    // the usual "nothing to do" refusal, which only applies when neither is
    // present.
    let has_env_file = EnvFile::path_in(&project_dir).is_file();
    let mut plan = if has_env_file || parsed.flags.packages.is_empty() {
        match load_project_plan_with_selections(
            theme,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(code) => return code,
        }
    } else {
        RunPlan {
            project_root: project_dir.clone(),
            refs: Vec::new(),
            adapters: Vec::new(),
            table: RefSpec::SourceTable::empty(),
            label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
            prompt_path: ModuleEval::PromptPathMode::default(),
            prompt_strip: ModuleEval::PromptStripMode::default(),
            dev_services: Vec::new(),
            secrets: Vec::new(),
            environment: ModuleEval::EnvironmentFacts::default(),
        }
    };

    // U16: ad-hoc `-p` packages become ordinary nixpkgs `RefSpec`s, folded
    // into the same plan as any manifest-declared package — same realize
    // path, same trust gate, no separate machinery.
    for name in &parsed.flags.packages {
        plan.refs.push(RefSpec::RefSpec {
            source: RefSpec::Source::Nixpkgs,
            package: name.clone(),
            raw: format!(
                "{}{}{}",
                name,
                Syntax::REF_PROVIDER_AT,
                Syntax::REF_SOURCE_NIXPKGS
            ),
        });
    }
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    // U19: `jet env` never runs a project function (the invariant this card
    // confirms), but it DOES realize the project's own declared packages —
    // first entry to a repo whose env is trust-sensitive gates on it.
    if let Err(code) = Trust::gate_with_environment(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        parsed.flags.trust,
    ) {
        return code;
    }

    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let entry = find_project_entry(&project_dir);
    if let Err(code) = run_lifecycle_hooks(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.environment.lifecycle.on_enter,
        "on_enter",
    ) {
        return code;
    }

    let code = match &parsed.command {
        Some(cmd) if !cmd.is_empty() => Shell::run_command(&env, cmd),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    };
    if code == 0 {
        auto_clean_after_success(theme, &roots);
    }
    code
}

/// `jet env test [-- command]`: run lifecycle hooks and checks with a clean
/// child environment. A command is optional; without one the declared checks
/// are the complete operation.
fn cmd_env_test(theme: &Theme, parsed: &Parsed) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let roots = Store::resolve();
    let mut plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }
    if let Err(code) = Trust::gate_with_environment(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        parsed.flags.trust,
    ) {
        return code;
    }
    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let entry = find_project_entry(&project_dir);
    if let Err(code) = run_lifecycle_hooks_clean(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.environment.lifecycle.on_enter,
        "on_enter",
    ) {
        return code;
    }
    if let Err(code) = run_lifecycle_hooks_clean(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.environment.lifecycle.checks,
        "check",
    ) {
        return code;
    }
    if let Some(command) = parsed.command.as_ref().filter(|command| !command.is_empty()) {
        return Shell::run_clean_command(&env, command);
    }
    theme.ok("environment checks passed in a clean process");
    0
}

/// `jet env sync`: show and optionally apply the complete managed-file plan.
fn cmd_env_sync(theme: &Theme, parsed: &Parsed) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let mut plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }
    let file_plan = match EnvFiles::plan(&project_dir, &plan.environment.files) {
        Ok(plan) => plan,
        Err(error) => {
            theme.error(
                "couldn't plan managed environment files",
                &error,
                "resolve the reported path or ownership conflict, then run `jet env sync` again.",
            );
            return 2;
        }
    };
    let source_snapshot = (!plan.environment.files.is_empty())
        .then(|| file_plan.source_snapshot_hash());
    if let Err(code) = Trust::gate_with_environment_and_snapshot(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        source_snapshot.as_deref(),
        parsed.flags.trust,
    ) {
        return code;
    }
    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }
    for action in &file_plan.actions {
        let verb = match action.kind {
            EnvFiles::FileActionKind::Create => "create",
            EnvFiles::FileActionKind::ReplaceOwned => "update",
            EnvFiles::FileActionKind::Preserve => "preserve",
            EnvFiles::FileActionKind::Unchanged => "unchanged",
        };
        let detail = if action.sensitive { " sensitive" } else { "" };
        theme.status(&format!(
            "{verb} {} ({}, sha256={}){detail}",
            action.destination,
            action.mode.as_str(),
            action.digest
        ));
    }
    if !file_plan.has_changes() {
        theme.ok("managed environment files are up to date");
        return 0;
    }
    if !theme.confirm_apply(parsed.flags.assume_yes) {
        return 0;
    }
    match file_plan.apply() {
        Ok(report) => {
            theme.ok(&format!(
                "managed environment files synced ({} applied, {} preserved)",
                report.applied, report.preserved
            ));
            0
        }
        Err(error) => {
            theme.error("managed environment file sync failed", &error, "no partial file change was retained.");
            2
        }
    }
}

/// `jet env info`: disclose the selected profile and the typed environment
/// facts without realizing packages or executing lifecycle commands.
fn cmd_env_info(theme: &Theme, parsed: &Parsed) -> i32 {
    let plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let profile = plan
        .environment
        .selected_profile
        .as_ref()
        .map(|profile| profile.name.as_str())
        .unwrap_or("<none>");
    let selected_profiles = plan
        .environment
        .selected_profile
        .as_ref()
        .map(|profile| profile.selected_profiles.clone())
        .unwrap_or_default();
    let applied_profiles = plan
        .environment
        .selected_profile
        .as_ref()
        .map(|profile| profile.applied.clone())
        .unwrap_or_default();
    let packages = plan
        .refs
        .iter()
        .map(|reference| reference.raw.as_str())
        .collect::<Vec<_>>();
    let entry = find_project_entry(&plan.project_root);
    let mut tasks = list_project_tasks(&entry);
    for hook in plan
        .environment
        .lifecycle
        .on_enter
        .iter()
        .chain(plan.environment.lifecycle.checks.iter())
    {
        if let ModuleEval::HookAction::Task(name) = &hook.action {
            tasks.push(name.clone());
        }
    }
    tasks.sort();
    tasks.dedup();
    let mut variable_sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add_variable = |name: &str, source: String| {
        let sources = variable_sources.entry(name.to_string()).or_default();
        if !sources.iter().any(|existing| existing == &source) {
            sources.push(source);
        }
    };
    if let Some(profile) = &plan.environment.selected_profile {
        for name in profile.variables.keys() {
            add_variable(name, "profile".to_string());
        }
    }
    for name in plan.environment.language_expansion.variables.keys() {
        add_variable(name, "language".to_string());
    }
    for dotenv in &plan.environment.lifecycle.dotenv {
        for name in &dotenv.allow {
            add_variable(name, format!("dotenv:{}", dotenv.file));
        }
        for name in &dotenv.secrets {
            add_variable(name, format!("dotenv-secret:{}", dotenv.file));
        }
    }
    for name in &plan.environment.lifecycle.unset {
        add_variable(name, "unset".to_string());
    }
    if parsed.flags.json {
        let quote_list = |values: &[&str]| {
            values
                .iter()
                .map(|value| crate::JSON::quote(value))
                .collect::<Vec<_>>()
                .join(",")
        };
        let quote_strings = |values: &[String]| {
            values
                .iter()
                .map(|value| crate::JSON::quote(value))
                .collect::<Vec<_>>()
                .join(",")
        };
        let files = plan
            .environment
            .files
            .iter()
            .map(|file| file.destination.as_str())
            .collect::<Vec<_>>();
        let dotenv = plan
            .environment
            .lifecycle
            .dotenv
            .iter()
            .map(|item| {
                format!(
                    "{{\"file\":{},\"allow\":[{}],\"secrets\":[{}]}}",
                    crate::JSON::quote(&item.file),
                    item.allow
                        .iter()
                        .map(|name| crate::JSON::quote(name))
                        .collect::<Vec<_>>()
                        .join(","),
                    item.secrets
                        .iter()
                        .map(|name| crate::JSON::quote(name))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
            .collect::<Vec<_>>();
        let languages = plan
            .environment
            .languages
            .iter()
            .map(|language| {
                let extras = language
                    .extra_packages
                    .iter()
                    .map(|package| crate::JSON::quote(package))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"name\":{},\"enable\":{},\"version\":{},\"channel\":{},\"venv\":{},\"extra\":[{}]}}",
                    crate::JSON::quote(&language.name),
                    language.enable,
                    language
                        .version
                        .as_deref()
                        .map(|value| crate::JSON::quote(value))
                        .unwrap_or_else(|| "null".to_string()),
                    language
                        .channel
                        .as_deref()
                        .map(|value| crate::JSON::quote(value))
                        .unwrap_or_else(|| "null".to_string()),
                    language.venv,
                    extras,
                )
            })
            .collect::<Vec<_>>();
        let catalog = ModuleEval::LanguagePackCatalog::builtin();
        let language_catalog = catalog
            .names()
            .into_iter()
            .filter_map(|name| {
                let pack = catalog.get(&name)?.clone();
                Some(format!(
                    "{{\"name\":{},\"fingerprint\":{}}}",
                    crate::JSON::quote(&pack.name),
                    crate::JSON::quote(&pack.fingerprint()),
                ))
            })
            .collect::<Vec<_>>()
            .join(",");
        let language_packs = plan
            .environment
            .language_packs
            .iter()
            .map(|pack| {
                format!(
                    "{{\"name\":{},\"fingerprint\":{},\"packages\":[{}],\"venv_packages\":[{}],\"host\":{},\"platforms\":[{}],\"license\":{},\"required_tools\":[{}]}}",
                    crate::JSON::quote(&pack.name),
                    crate::JSON::quote(&pack.fingerprint()),
                    quote_strings(&pack.packages),
                    quote_strings(&pack.venv_packages),
                    crate::JSON::quote(&pack.host),
                    quote_strings(&pack.platforms),
                    crate::JSON::quote(&pack.license),
                    quote_strings(&pack.required_tools),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let language_projections = plan
            .environment
            .language_projections
            .iter()
            .map(|projection| {
                let selection = &projection.selection;
                let version = selection
                    .version
                    .as_deref()
                    .map(crate::JSON::quote)
                    .unwrap_or_else(|| "null".to_string());
                let channel = selection
                    .channel
                    .as_deref()
                    .map(crate::JSON::quote)
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    "{{\"name\":{},\"enable\":{},\"version\":{},\"channel\":{},\"venv\":{},\"pack_fingerprint\":{},\"host\":{},\"platform\":{},\"license\":{},\"missing_tools\":[{}],\"included\":[{}],\"omitted\":[{}],\"changed\":[{}]}}",
                    crate::JSON::quote(&selection.name),
                    selection.enable,
                    version,
                    channel,
                    selection.venv,
                    crate::JSON::quote(&projection.pack.fingerprint()),
                    crate::JSON::quote(&projection.host),
                    crate::JSON::quote(&projection.platform),
                    crate::JSON::quote(&projection.license),
                    quote_strings(&projection.missing_tools),
                    quote_strings(&projection.included),
                    quote_strings(&projection.omitted),
                    quote_strings(&projection.changed),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let integrations = plan
            .environment
            .integrations
            .iter()
            .map(|integration| {
                let option_keys = integration
                    .options
                    .keys()
                    .map(|key| crate::JSON::quote(key))
                    .collect::<Vec<_>>()
                    .join(",");
                let packages = integration
                    .packages
                    .iter()
                    .map(|value| crate::JSON::quote(value))
                    .collect::<Vec<_>>()
                    .join(",");
                let files = integration
                    .files
                    .iter()
                    .map(|file| crate::JSON::quote(&file.destination))
                    .collect::<Vec<_>>()
                    .join(",");
                let strings = |values: &[String]| {
                    values
                        .iter()
                        .map(|value| crate::JSON::quote(value))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                format!(
                    "{{\"kind\":{},\"name\":{},\"preset\":{},\"option_keys\":[{}],\"packages\":[{}],\"files\":[{}],\"tasks\":[{}],\"providers\":[{}],\"host_checks\":[{}],\"secrets\":[{}],\"grants\":[{}],\"losses\":[{}]}}",
                    crate::JSON::quote(integration.kind.as_str()),
                    crate::JSON::quote(&integration.name),
                    crate::JSON::quote(&integration.preset),
                    option_keys,
                    packages,
                    files,
                    strings(&integration.tasks),
                    strings(&integration.providers),
                    strings(&integration.host_checks),
                    strings(&integration.secrets),
                    strings(&integration.grants),
                    strings(&integration.losses),
                )
            })
            .collect::<Vec<_>>();
        let package_profiles = plan
            .environment
            .package_profiles
            .iter()
            .map(|profile| {
                let collisions = profile
                    .collisions
                    .iter()
                    .map(|(path, provider)| {
                        format!("{}:{}", crate::JSON::quote(path), crate::JSON::quote(provider))
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"name\":{},\"extends\":[{}],\"packages\":[{}],\"collisions\":{{{}}},\"sources\":[{}]}}",
                    crate::JSON::quote(&profile.name),
                    quote_list(&profile.extends.iter().map(String::as_str).collect::<Vec<_>>()),
                    quote_list(&profile.packages.iter().map(String::as_str).collect::<Vec<_>>()),
                    collisions,
                    quote_list(&profile.sources.iter().map(String::as_str).collect::<Vec<_>>()),
                )
            })
            .collect::<Vec<_>>();
        let services = plan
            .dev_services
            .iter()
            .map(|service| {
                let ports = service
                    .ports
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let after = service
                    .after
                    .iter()
                    .map(|name| crate::JSON::quote(name))
                    .collect::<Vec<_>>()
                    .join(",");
                let readiness = service.ready.is_some()
                    || service.ready_probe.is_some()
                    || !service.ports.is_empty();
                format!(
                    "{{\"name\":{},\"enabled\":{},\"ports\":[{}],\"readiness\":{},\"after\":[{}]}}",
                    crate::JSON::quote(&service.name),
                    service.enable,
                    ports,
                    readiness,
                    after,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let variables = variable_sources
            .iter()
            .map(|(name, sources)| {
                format!(
                    "{{\"name\":{},\"sources\":[{}]}}",
                    crate::JSON::quote(name),
                    sources
                        .iter()
                        .map(|source| crate::JSON::quote(source))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"profile\":{},\"selected_profiles\":[{}],\"applied_profiles\":[{}],\"profiles\":[{}],\"package_profiles\":[{}],\"environments\":[{}],\"active_environment\":{},\"active_environment_provenance\":[{}],\"sources\":[{}],\"language_catalog\":{{\"source\":\"jet-env-model builtin\",\"fingerprint\":{},\"packs\":[{}]}},\"languages\":[{}],\"language_packs\":[{}],\"language_projections\":[{}],\"packages\":[{}],\"services\":[{}],\"tasks\":[{}],\"variables\":[{}],\"files\":[{}],\"dotenv\":[{}],\"integrations\":[{}]}}",
            crate::JSON::quote(profile),
            quote_list(&selected_profiles.iter().map(String::as_str).collect::<Vec<_>>()),
            quote_list(&applied_profiles.iter().map(String::as_str).collect::<Vec<_>>()),
            quote_list(&plan.environment.profiles.iter().map(|item| item.name.as_str()).collect::<Vec<_>>()),
            package_profiles.join(","),
            quote_list(&plan.environment.environment_names.iter().map(String::as_str).collect::<Vec<_>>()),
            plan.environment
                .active_environment
                .as_deref()
                .map(crate::JSON::quote)
                .unwrap_or_else(|| "null".to_string()),
            quote_list(
                &plan
                    .environment
                    .active_environment_provenance
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ),
            quote_list(&plan.environment.source_files.iter().map(String::as_str).collect::<Vec<_>>()),
            crate::JSON::quote(&catalog.fingerprint()),
            language_catalog,
            languages.join(","),
            language_packs,
            language_projections,
            quote_list(&packages),
            services,
            quote_list(&tasks.iter().map(String::as_str).collect::<Vec<_>>()),
            variables,
            quote_list(&files),
            dotenv.join(","),
            integrations.join(","),
        );
        return 0;
    }
    theme.status(&format!("profile: {profile}"));
    let applied = plan
        .environment
        .selected_profile
        .as_ref()
        .map(|profile| profile.applied.join(" -> "))
        .unwrap_or_else(|| "<none>".to_string());
    let selected = if selected_profiles.is_empty() {
        "<none>".to_string()
    } else {
        selected_profiles.join(", ")
    };
    theme.detail(&format!("selected profiles: {selected}"));
    theme.detail(&format!("profiles: {applied}"));
    theme.detail(&format!(
        "package profiles: {}",
        if plan.environment.package_profiles.is_empty() {
            "<none>".to_string()
        } else {
            plan.environment
                .package_profiles
                .iter()
                .map(|profile| profile.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    theme.detail(&format!(
        "active environment: {} (from {})",
        plan.environment
            .active_environment
            .as_deref()
            .unwrap_or("<none>"),
        if plan.environment.active_environment_provenance.is_empty() {
            "<none>".to_string()
        } else {
            plan.environment.active_environment_provenance.join(", ")
        }
    ));
    let languages = plan
        .environment
        .languages
        .iter()
        .map(|language| {
            let mut label = language.name.clone();
            if !language.enable {
                label.push_str(" (disabled)");
            }
            if let Some(version) = &language.version {
                label.push_str(&format!("@{version}"));
            }
            if let Some(channel) = &language.channel {
                label.push_str(&format!(" [{channel}]"));
            }
            if language.venv {
                label.push_str(" +venv");
            }
            label
        })
        .collect::<Vec<_>>();
    let catalog = ModuleEval::LanguagePackCatalog::builtin();
    theme.detail(&format!(
        "language catalog: {} ({})",
        catalog.names().join(", "),
        catalog.fingerprint()
    ));
    theme.detail(&format!("languages: {}", if languages.is_empty() { "<none>".to_string() } else { languages.join(", ") }));
    let expanded = plan
        .environment
        .language_projections
        .iter()
        .map(|projection| {
            format!(
                "{}: +{} -{}",
                projection.selection.name,
                if projection.included.is_empty() { "<none>".to_string() } else { projection.included.join(",") },
                if projection.omitted.is_empty() { "<none>".to_string() } else { projection.omitted.join(",") },
            )
        })
        .collect::<Vec<_>>();
    theme.detail(&format!("language projections: {}", if expanded.is_empty() { "<none>".to_string() } else { expanded.join("; ") }));
    theme.detail(&format!("packages: {}", if packages.is_empty() { "<none>".to_string() } else { packages.join(", ") }));
    let services = plan
        .dev_services
        .iter()
        .map(|service| {
            let state = if service.enable { "enabled" } else { "disabled" };
            format!("{} ({state})", service.name)
        })
        .collect::<Vec<_>>();
    theme.detail(&format!(
        "services: {}",
        if services.is_empty() { "<none>".to_string() } else { services.join(", ") }
    ));
    theme.detail(&format!(
        "tasks: {}",
        if tasks.is_empty() { "<none>".to_string() } else { tasks.join(", ") }
    ));
    let variables = variable_sources
        .iter()
        .map(|(name, sources)| format!("{} [{}]", name, sources.join(",")))
        .collect::<Vec<_>>();
    theme.detail(&format!(
        "variables: {}",
        if variables.is_empty() { "<none>".to_string() } else { variables.join(", ") }
    ));
    theme.detail(&format!("managed files: {}", if plan.environment.files.is_empty() { "<none>".to_string() } else { plan.environment.files.iter().map(|file| file.destination.as_str()).collect::<Vec<_>>().join(", ") }));
    let integrations = plan
        .environment
        .integrations
        .iter()
        .map(|integration| {
            if integration.name.is_empty() {
                integration.kind.as_str().to_string()
            } else {
                format!("{} ({})", integration.name, integration.kind.as_str())
            }
        })
        .collect::<Vec<_>>();
    theme.detail(&format!("integrations: {}", if integrations.is_empty() { "<none>".to_string() } else { integrations.join(", ") }));
    0
}

/// D-ENVHOOK1=A: `jet env hook <shell>` — print the opt-in shell hook the user
/// installs once. Pure text (no realize, no trust): installing the hook is a
/// safe editor action; the trust gate only fires later, on the first activation
/// of an untrusted env.
fn cmd_env_hook(theme: &Theme, parsed: &Parsed) -> i32 {
    match EnvHook::parse_shell(parsed.positional.get(1).map(String::as_str)) {
        Some(kind) => {
            print!("{}", EnvHook::render_hook(kind));
            0
        }
        None => {
            theme.error(
                "unknown shell for `jet env hook`",
                &format!(
                    "the auto-activation hook is available for: {}.",
                    Syntax::ENV_HOOK_SHELLS.join(", ")
                ),
                "try `jet env hook bash`, `jet env hook zsh`, or `jet env hook fish`.",
            );
            2
        }
    }
}

/// D-ENVHOOK1=A: `jet env export <shell>` — the hook's private per-prompt
/// callback. Emits (to stdout) the shell statements that load the nearest
/// `env.jet` into the current shell, or unload it when the shell has left that
/// directory tree. Realize/trust/compose reuse the exact same path as `jet env`
/// (`compose_env` + `Trust::gate`), so there is one env engine (I8). All
/// human-facing output (ledger rows, the trust prompt) goes to stderr via
/// `Theme`; stdout carries only shell code for the hook to `eval`.
fn cmd_env_export(theme: &Theme, parsed: &Parsed) -> i32 {
    use std::io::IsTerminal;

    let Some(kind) = EnvHook::parse_shell(parsed.positional.get(1).map(String::as_str)) else {
        // An unknown shell from an installed hook is not worth a diagnostic on
        // every prompt — emit nothing and let the shell keep its environment.
        return 0;
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let disabled = std::env::var_os(Syntax::ENV_DISABLE_VAR)
        .is_some_and(|v| !v.is_empty());
    let target = if disabled {
        None
    } else {
        EnvHook::find_env_root(&cwd)
    };
    let target_s = target.as_ref().map(|p| p.to_string_lossy().into_owned());
    let active_s = std::env::var(Syntax::ENV_HOOK_ACTIVE_DIR_VAR)
        .ok()
        .filter(|s| !s.is_empty());
    let active_hash = std::env::var(Syntax::ENV_HOOK_ACTIVE_HASH_VAR)
        .ok()
        .filter(|s| !s.is_empty());
    let target_hash = target
        .as_ref()
        .and_then(|root| {
            EnvHook::definition_fingerprint_with_selections(
                root,
                parsed.flags.profile.as_deref(),
                parsed.flags.environment_profile.as_deref(),
            )
        });

    // Nothing changed since the last prompt — stay silent so the hook is a
    // no-op on the vast majority of prompts (and never re-realizes).
    if target_s == active_s && target_hash == active_hash {
        return 0;
    }

    // A changed definition does not always mean an immediate reload. `Never`
    // keeps the current activation, while `Watch` records the first changed
    // hash and coalesces prompt callbacks until its debounce window expires.
    let mut watched_reload_ready = false;
    if target_s == active_s && target_hash != active_hash {
        if let (Some(root), Some(hash)) = (target.as_ref(), target_hash.as_deref()) {
            match EnvHook::reload_policy_with_environment_profile(
                root,
                parsed.flags.environment_profile.as_deref(),
            ) {
                ModuleEval::ReloadPolicy::Never => return 0,
                ModuleEval::ReloadPolicy::Prompt => {}
                ModuleEval::ReloadPolicy::Watch { debounce_ms, .. } => {
                    match EnvHook::watch_reload_ready(root, hash, debounce_ms) {
                        Ok(true) => watched_reload_ready = true,
                        Ok(false) => return 0,
                        Err(error) => {
                            theme.detail(&format!("environment reload is waiting: {error}"));
                            return 0;
                        }
                    }
                }
            }
        }
    }

    // The PATH to restore on unload / build on top of when activating: the
    // saved pre-env PATH if an env is currently live, else the live PATH.
    let base_path = if active_s.is_some() {
        std::env::var(Syntax::ENV_HOOK_OLD_PATH_VAR)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
    } else {
        std::env::var("PATH").unwrap_or_default()
    };

    // Do not emit an unload until the replacement environment is fully
    // realized, trusted, composed, and entered. A failed reload must leave
    // the caller's previous activation usable.
    let mut script = String::new();

    if let Some(root_s) = &target_s {
        let root = PathBuf::from(root_s);
        // Realize the target env with `root` as cwd so the existing
        // cwd-relative plan/realize path composes it exactly like `jet env`
        // would from inside it. This process exits immediately after, so
        // changing its own cwd affects nothing else.
        let _ = std::env::set_current_dir(&root);
        let roots = Store::resolve();
        let mut plan = match load_project_plan_with_selections(
            theme,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        ) {
            Ok(plan) => plan,
            Err(_) => {
                // Malformed / foreign-only env: keep the previous activation.
                return 0;
            }
        };
        if apply_locked_channels(theme, &root, &mut plan.table).is_err() {
            return 0;
        }

        // D-JPK-GRANTCMD1 trust law: the first activation of an untrusted,
        // trust-sensitive env prompts (interactive) or is refused with a hint
        // (non-interactive). A trusted or non-sensitive env activates silently.
        let store = Trust::store_path();
        let hash = Trust::environment_definition_hash(
            &plan.refs,
            &plan.table,
            &plan.secrets,
            &plan.environment,
        );
        let sensitive = Trust::is_trust_sensitive_ext(&plan.refs, !plan.secrets.is_empty())
            || !plan.environment.lifecycle.on_enter.is_empty()
            || !plan.environment.lifecycle.checks.is_empty();
        let trusted = !sensitive
            || Trust::is_environment_trusted(
                &store,
                &root,
                &hash,
                &plan.refs,
                &plan.secrets,
                &plan.environment,
            );
        if !trusted {
            if std::io::stdin().is_terminal() {
                if Trust::gate_with_environment(
                    theme,
                    &store,
                    &root,
                    &plan.refs,
                    &plan.table,
                    &plan.secrets,
                    &plan.environment,
                    parsed.flags.trust,
                )
                .is_err()
                {
                    return 0;
                }
            } else {
                theme.detail(&format!(
                    "{} here is not trusted — run `jet env` to approve it once",
                    Syntax::ENV_FILE
                ));
                return 0;
            }
        }

        let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
            Ok(env) => env,
            Err(_) => {
                return 0;
            }
        };
        // Reconcile the source graph after realization. If a prompt races an
        // edit, retain the prior activation and let the next prompt retry the
        // new hash instead of emitting a plan assembled from mixed revisions.
        if EnvHook::definition_fingerprint_with_selections(
            &root,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        )
        .as_deref()
            != target_hash.as_deref()
        {
            return 0;
        }
        let plan_hash = target_hash.clone().unwrap_or_default();
        let entry = find_project_entry(&root);
        if run_lifecycle_hooks_silent(
            theme,
            parsed,
            &roots,
            &root,
            &entry,
            &env,
            &plan.environment.lifecycle.on_enter,
            "on_enter",
        )
        .is_err()
        {
            return 0;
        }
        if EnvHook::definition_fingerprint_with_selections(
            &root,
            parsed.flags.profile.as_deref(),
            parsed.flags.environment_profile.as_deref(),
        )
        .as_deref()
            != target_hash.as_deref()
        {
            return 0;
        }
        if active_s.is_some() {
            script.push_str(&EnvHook::render_unload(kind, &base_path));
        }
        let composed_path = env.composed_path(&base_path);
        let activation = match EnvHook::render_activate(
            kind,
            &EnvHook::Activation {
                base_path,
                composed_path,
                refs: env.refs.join(" "),
                root: root_s.clone(),
                vars: env.vars.clone(),
                unset: env.unset_vars.clone(),
                plan_hash,
            },
        ) {
            Ok(script) => script,
            Err(error) => {
                theme.detail(&format!("environment activation was rejected: {error}"));
                return 0;
            }
        };
        script.push_str(&activation);
        if watched_reload_ready {
            EnvHook::clear_watch_reload(&root);
        }
    } else if active_s.is_some() {
        script.push_str(&EnvHook::render_unload(kind, &base_path));
    }

    print!("{script}");
    0
}

/// The foreign flake/devenv file in `dir`, if either exists. `flake.nix` wins
/// when both are present (Nix's own name for the concept jetpack is bridging
/// from; `devenv.nix` is devenv's flake-backed variant of the same file).
pub(super) fn foreign_flake_path(dir: &Path) -> Option<PathBuf> {
    let flake = dir.join(Syntax::FOREIGN_FLAKE_FILE);
    if flake.is_file() {
        return Some(flake);
    }
    let devenv = dir.join(Syntax::FOREIGN_DEVENV_FILE);
    if devenv.is_file() {
        return Some(devenv);
    }
    None
}

/// Whether the project's own manifest already declares an environment —
/// either the typed `module env.*` surface or the Phase-1 `pkg.*` directive
/// surface. U16's foreign-flake auto-detection only fires when this is
/// false; a malformed typed surface still counts as "has env" (its author
/// clearly meant to declare one, so this never masks that error by silently
/// falling through to a foreign flake instead).
pub(super) fn project_declares_env(dir: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(dir)) else {
        return false;
    };
    if ModuleEval::is_module_surface(&src) {
        return ModuleEval::evaluate_env(&src, dir)
            .map(|p| {
                !p.environment_names.is_empty()
                    || !p.lifecycle.dotenv.is_empty()
                    || !p.lifecycle.unset.is_empty()
                    || !p.lifecycle.on_enter.is_empty()
                    || !p.lifecycle.checks.is_empty()
                    || p.lifecycle.reload_explicit
                    || !p.profiles.is_empty()
                    || !p.languages.is_empty()
                    || !p.files.is_empty()
                    || !p.dev_services.is_empty()
                    || !p.package_refs.is_empty()
                    || p.prompt.is_some()
                    || !p.secrets.is_empty()
            })
            .unwrap_or(true);
    }
    let ef = EnvFile::parse(&src);
    !ef.packages.is_empty() || ef.default_source.is_some() || !ef.named.is_empty()
}

/// U16: enter a foreign flake's default devShell through the same bounded
/// native evaluator as `jet bridge flake`. The projection is deliberately
/// loss-recording: package lists become ordinary nixpkgs refs and fields with
/// no `env.*` meaning are reported as L0204. Jetpack never delegates this
/// product path to an installed `nix` binary.
fn enter_foreign_flake(
    theme: &Theme,
    project_dir: &Path,
    flake_path: &Path,
    parsed: &Parsed,
) -> i32 {
    if let Err(code) = Trust::gate_flake(
        theme,
        &Trust::store_path(),
        project_dir,
        flake_path,
        parsed.flags.trust,
    ) {
        return code;
    }
    let flake_dir = flake_path.parent().unwrap_or(project_dir);
    let facts = match Bridge::read_devshell_facts(flake_dir, None) {
        Ok(facts) => facts,
        Err(error) => {
            crate::CLI::report_provider_error(theme, &error);
            return 1;
        }
    };
    for field in &facts.unmapped {
        theme.warning_coded(
            "L0204",
            &format!("`{field}` in `{}` has no `env.*` equivalent yet", flake_path.display()),
            "the bounded native foreign-flake projection preserves unsupported fields as loss facts instead of executing them",
            "declare the effect explicitly in `env.*` if the project needs it",
        );
    }
    let mut plan = empty_task_plan();
    plan.refs = facts
        .packages
        .into_iter()
        .map(|package| RefSpec::RefSpec {
            raw: format!("{package}@{}", Syntax::REF_SOURCE_NIXPKGS),
            source: RefSpec::Source::Nixpkgs,
            package,
        })
        .collect();
    theme.status(&format!(
        "entering native projection of foreign shell: {}",
        theme.bold(&flake_path.display().to_string())
    ));
    let roots = Store::resolve();
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    match &parsed.command {
        Some(command) if !command.is_empty() && parsed.flags.pure => {
            Shell::run_clean_command(&env, command)
        }
        Some(command) if !command.is_empty() => Shell::run_command(&env, command),
        _ if parsed.flags.pure => Shell::enter_clean(theme, &env, ShellKind::detect()),
        _ => Shell::enter(theme, &env, ShellKind::detect()),
    }
}

/// `jetpack dev` — U19 project-level dev (distinct from the already-shipped
/// `jet dev <file.jet>` file-watch interpreter loop, D-DEV4, which this never
/// touches). Realizes the project's declared env — today `load_project_plan`
/// already merges every `env.*` contribution into one plan, which is
/// `env(base + env.dev)` for the common case of a project that only declares
/// `module env.dev { … }` — gates on trust, waits for services through
/// `wait_for_services_ready`, then runs the project's
/// `fn dev()` or falls back to `fn run()` by re-invoking `jet dev <entry>`
/// inside the composed env. Running Jet source is the compiler's job, never
/// jetpack's (D-JPK-DISPATCH1) — this shells out to the sibling `jet` binary
/// exactly the way `-- cmd` already shells out to an arbitrary command.
pub(super) fn cmd_dev(theme: &Theme, parsed: &Parsed) -> i32 {
    let roots = Store::resolve();
    if roots.dev_mode {
        theme.detail(&theme.gray(&format!(
            "user-owned hangar: using {}",
            roots.root.display()
        )));
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let entry = find_project_entry(&project_dir);
    if !has_dev_or_run_entry(&entry) {
        theme.error_coded(
            "E1254",
            "this project has no `jet dev` entry",
            &format!(
                "`jet dev` runs the entry file's top-level `fn dev()` if it defines one, else \
                 `fn run()` (U19); `{}` defines neither",
                entry.display()
            ),
            "add `fn dev() { … }` (a custom dev command) or `fn run() { … }` (the default) to the entry file.",
        );
        return 2;
    }

    let mut plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if let Err(code) = apply_locked_channels(theme, &project_dir, &mut plan.table) {
        return code;
    }

    if let Err(code) = Trust::gate_with_environment(
        theme,
        &Trust::store_path(),
        &project_dir,
        &plan.refs,
        &plan.table,
        &plan.secrets,
        &plan.environment,
        parsed.flags.trust,
    ) {
        return code;
    }

    if let Err(code) = validate_declared_secrets(theme, &project_dir, &plan.secrets) {
        return code;
    }

    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    if let Err(code) = run_lifecycle_hooks(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.environment.lifecycle.on_enter,
        "on_enter",
    ) {
        return code;
    }

    if let Err(code) = wait_for_services_ready(
        theme,
        parsed,
        &roots,
        &project_dir,
        &entry,
        &env,
        &plan.dev_services,
    ) {
        return code;
    }

    theme.status(&format!(
        "running {}",
        theme.bold(&entry.display().to_string())
    ));
    let mut cmd = vec![
        find_jet_binary(),
        Syntax::DEV_SUBCOMMAND.to_string(),
        entry.to_string_lossy().into_owned(),
    ];
    // Any leftover positional token (e.g. `--watch=off`) is a flag `jet dev
    // <file>` itself understands — bare `jetpack dev` takes no file argument
    // of its own, so everything here is pass-through.
    cmd.extend(parsed.positional.iter().cloned());
    Shell::run_command(&env, &cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_cache_key_changes_when_task_arguments_change() {
        let root = std::env::temp_dir().join(format!(
            "jet-task-cache-key-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        std::fs::write(&entry, "#Job fn build() {}\n").unwrap();
        let metadata = crate::AST::TaskMetadata::default();
        let table = RefSpec::SourceTable::empty();
        let compiler = resolve_executable_path(&find_jet_binary()).unwrap();

        let first = task_cache_key(&root, &entry, "build", &compiler, &metadata, &["one".to_string()], &[], &table, "environment-a")
            .unwrap();
        let second = task_cache_key(&root, &entry, "build", &compiler, &metadata, &["two".to_string()], &[], &table, "environment-a")
            .unwrap();
        assert_ne!(first, second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_platform_skip_only_skips_outside_its_declared_host() {
        let linux = crate::AST::TaskSkip::UnlessPlatform {
            platform: "Linux".to_string(),
        };
        let macos = crate::AST::TaskSkip::UnlessPlatform {
            platform: "MacOS".to_string(),
        };
        assert!(linux.reason_for_host("aarch64-macos").is_some());
        assert!(linux.reason_for_host("x86_64-linux").is_none());
        assert!(macos.reason_for_host("x86_64-linux").is_some());
        assert!(macos.reason_for_host("aarch64-macos").is_none());
        assert_eq!(
            task_skip_reason(Some(&crate::AST::TaskSkip::Always("manual".to_string())))
                .as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn strict_cached_tasks_need_declared_inputs_and_reject_overlap() {
        let root = std::env::temp_dir().join(format!(
            "jet-task-cache-declarations-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("input.txt"), "input\n").unwrap();
        let mut metadata = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        assert!(validate_cached_task_metadata(&root, &metadata)
            .unwrap_err()
            .contains("must declare at least one project input"));
        metadata.inputs = vec!["input.txt".to_string()];
        assert!(validate_cached_task_metadata(&root, &metadata).is_ok());
        metadata.inputs = vec!["out.txt".to_string()];
        assert!(validate_cached_task_metadata(&root, &metadata)
            .unwrap_err()
            .contains("overlaps output"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn strict_cached_tasks_reject_undeclared_project_access() {
        let root = std::env::temp_dir().join(format!(
            "jet-task-access-proof-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        let input = root.join("input.txt");
        let hidden = root.join("hidden.jet");
        let secret = root.join("secret.txt");
        let state_dir = root.join(".jet");
        let state_secret = state_dir.join("credentials");
        let renamed = root.join("other.jet");
        let trace = root.join("access.log");
        std::fs::write(&entry, "#Job fn build() {}\n").unwrap();
        std::fs::write(&input, "input\n").unwrap();
        std::fs::write(&hidden, "hidden\n").unwrap();
        std::fs::write(&secret, "secret\n").unwrap();
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(&state_secret, "secret\n").unwrap();
        let nested = root.join("workspace/credentials/prod");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("config"), "secret\n").unwrap();
        std::fs::write(
            &trace,
            format!(
                "123 openat(AT_FDCWD, \"{}\", O_RDONLY) = 3\n123 openat(AT_FDCWD, \"{}\", O_RDONLY) = 4\n123 openat(AT_FDCWD, \"{}\", O_RDONLY) = 5\n123 openat(AT_FDCWD, \"{}\", O_RDONLY) = 6\n123 rename(\"{}\", \"{}\") = 0\n",
                input.display(),
                hidden.display(),
                secret.display(),
                state_secret.display(),
                input.display(),
                renamed.display()
            ),
        )
        .unwrap();
        let metadata = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            inputs: vec!["input.txt".to_string()],
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        let accesses = task_undeclared_accesses(&root, &root, &entry, &metadata, &trace).unwrap();
        assert_eq!(
            accesses,
            vec![
                state_secret.to_string_lossy().replace('\\', "/"),
                hidden.to_string_lossy().replace('\\', "/"),
                renamed.to_string_lossy().replace('\\', "/"),
                secret.to_string_lossy().replace('\\', "/")
            ]
        );
        let sensitive = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            inputs: vec![".jet/credentials".to_string()],
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        assert!(validate_cached_task_metadata(&root, &sensitive)
            .unwrap_err()
            .contains("secret-bearing or Jet state"));
        let nested_sensitive = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            inputs: vec!["workspace".to_string()],
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        assert!(validate_cached_task_metadata(&root, &nested_sensitive)
            .unwrap_err()
            .contains("secret-bearing or Jet state"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strace_paths_use_file_arguments_and_decode_escapes() {
        assert_eq!(
            strace_paths(
                r#"123 execve("/project/jet", ["jet", "run", "--task=build"], 0x0) = 0"#
            ),
            vec![PathBuf::from("/project/jet")]
        );
        assert_eq!(
            strace_paths(r#"123 rename("src", "odd\\name") = 0"#),
            vec![PathBuf::from("src"), PathBuf::from("odd\\name")]
        );
        assert_eq!(
            strace_paths(r#"123 openat(AT_FDCWD, "space\x20name", O_RDONLY) = 3"#),
            vec![PathBuf::from("space name")]
        );
        assert_eq!(
            strace_paths(r#"123 statx(AT_FDCWD, "hidden.txt", AT_STATX_SYNC_AS_STAT, STATX_ALL, {}) = 0"#),
            vec![PathBuf::from("hidden.txt")]
        );
        assert_eq!(
            strace_paths(r#"123 linkat(AT_FDCWD, "src", AT_FDCWD, "dest", 0) = 0"#),
            vec![PathBuf::from("src"), PathBuf::from("dest")]
        );
        assert_eq!(
            strace_paths(r#"123 open("caf\303\251", O_RDONLY) = 3"#),
            vec![PathBuf::from("café")]
        );
        assert_eq!(
            normalize_trace_path(Path::new("/project/work/../../outside")),
            Some(PathBuf::from("/outside"))
        );
    }

    #[test]
    fn strict_cached_tasks_reject_secret_environment_names() {
        let mut env = empty_task_env();
        for name in ["API_KEY", "PRIVATE_KEY", "AWS_ACCESS_KEY_ID"] {
            env.vars.insert(name.to_string(), "not-for-cache".to_string());
            let error = validate_cached_task_environment(&empty_task_plan(), &env)
                .unwrap_err();
            assert!(error.contains("secret-bearing environment variable"), "{name}: {error}");
            env.vars.clear();
        }
    }

    #[test]
    fn task_cache_key_changes_when_an_unlisted_project_file_changes() {
        let root = std::env::temp_dir().join(format!(
            "jet-task-cache-scope-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        std::fs::write(&entry, "#Job fn build() {}\n").unwrap();
        std::fs::write(root.join("undeclared.txt"), "one\n").unwrap();
        let metadata = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            inputs: vec!["main.jet".to_string()],
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        let table = RefSpec::SourceTable::empty();
        let compiler = resolve_executable_path(&find_jet_binary()).unwrap();
        let first = task_cache_key(&root, &entry, "build", &compiler, &metadata, &[], &[], &table, "environment-a").unwrap();
        std::fs::write(root.join("undeclared.txt"), "two\n").unwrap();
        let second = task_cache_key(&root, &entry, "build", &compiler, &metadata, &[], &[], &table, "environment-a").unwrap();
        assert_ne!(first, second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_cache_key_changes_when_environment_profile_facts_change() {
        let root = std::env::temp_dir().join(format!(
            "jet-task-cache-environment-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("main.jet");
        std::fs::write(&entry, "#Job fn build() {}\n").unwrap();
        let compiler = std::env::current_exe().unwrap();
        let metadata = crate::AST::TaskMetadata {
            cache: crate::AST::TaskCachePolicy::Local,
            inputs: vec!["main.jet".to_string()],
            outputs: vec!["out.txt".to_string()],
            ..Default::default()
        };
        let table = RefSpec::SourceTable::default();
        let first = task_cache_key(
            &root,
            &entry,
            "build",
            &compiler,
            &metadata,
            &[],
            &[],
            &table,
            "profile=dev;var=MODE=one",
        )
        .unwrap();
        let second = task_cache_key(
            &root,
            &entry,
            "build",
            &compiler,
            &metadata,
            &[],
            &[],
            &table,
            "profile=dev;var=MODE=two",
        )
        .unwrap();
        assert_ne!(first, second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_environment_hash_changes_when_active_environment_changes() {
        let table = RefSpec::SourceTable::empty();
        let mut first = ModuleEval::EnvironmentFacts::default();
        first.active_environment = Some("dev".to_string());
        let mut second = first.clone();
        second.active_environment = Some("ci".to_string());
        assert_ne!(
            task_environment_hash_with_vars(&[], &table, &[], &first, &BTreeMap::new()),
            task_environment_hash_with_vars(&[], &table, &[], &second, &BTreeMap::new())
        );
    }

    #[test]
    fn task_environment_hash_changes_when_allowed_dotenv_value_changes() {
        let table = RefSpec::SourceTable::empty();
        let facts = ModuleEval::EnvironmentFacts::default();
        let mut first = BTreeMap::new();
        first.insert("PORT".to_string(), "8080".to_string());
        let mut second = first.clone();
        second.insert("PORT".to_string(), "8081".to_string());
        assert_ne!(
            task_environment_hash_with_vars(&[], &table, &[], &facts, &first),
            task_environment_hash_with_vars(&[], &table, &[], &facts, &second)
        );
    }
}
