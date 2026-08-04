use super::parse::Parsed;
use super::realize::load_project_plan_with_selections;
use super::trust_env_build::compose_env;
use jet_env_model::ModuleEval;
use crate::Output::Theme;
use crate::RuntimePolicy;
use crate::Secrets;
use crate::Services;
use crate::Shell::Env;
use crate::Store;
use crate::Syntax;
use crate::Trust;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// U13: before `jet env`/`jet dev` enters a trusted project environment, every
/// declared `secrets: ["name", …]` entry must exist in `.jet/secrets.age`.
/// Values stay inside `Secrets::get` and are dropped immediately; this is a
/// presence check, not env-var injection.
pub(super) fn validate_declared_secrets(
    theme: &Theme,
    project_dir: &Path,
    names: &[String],
) -> Result<(), i32> {
    for name in names {
        match Secrets::get(project_dir, name) {
            Ok(Some(_)) => {}
            Ok(None) => {
                theme.error_coded(
                    "E1263",
                    &format!("no secret named `{name}`"),
                    "this environment declares that secret, but the encrypted store doesn't have an entry with this name.",
                    &format!("set it first with `jetpack secrets set {name} <value>`, or check the spelling."),
                );
                return Err(2);
            }
            Err(msg) => {
                theme.error(&format!("couldn't read `{name}`"), &msg, "");
                return Err(2);
            }
        }
    }
    Ok(())
}

/// U12: bring up every enabled dev `services:` entry and block until each is
/// healthy (or E1261 on timeout) before `jetpack dev` runs the project's
/// `fn dev()`/`fn run()`. Takes the composed `Env` so a catalog binary (e.g.
/// `redis-server`, realized alongside the project's own packages) resolves
/// on PATH the same way the project's own command does.
pub(super) fn wait_for_services_ready(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    env: &Env,
    services: &[ModuleEval::DevServicePlan],
) -> Result<(), i32> {
    let selected = services
        .iter()
        .enumerate()
        .filter_map(|(index, service)| service.enable.then_some(index))
        .collect::<Vec<_>>();
    let result = Services::up_ordered_with(
        project_dir,
        env,
        services,
        &selected,
        service_health_timeout(),
        |svc| {
            if let Some(field) = Services::unknown_field(svc) {
                theme.error_coded(
                    "E1262",
                    &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
                    "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
                    "rename it to one of `enable`, `ports`, `run`, `shutdown`, `data_dir`, `ready`, `restart`, `watch`, `after`, `before_start`, or `sockets`, or remove it.",
                );
                return Err(format!("service `{}` has an unsupported field", svc.name));
            }
            if let Err(()) = run_before_start_tasks(theme, parsed, roots, project_dir, entry, svc) {
                return Err(format!("before_start task for service `{}` failed", svc.name));
            }
            theme.detail(&format!(
                "waiting for service `{}` to become healthy…",
                svc.name
            ));
            Ok(())
        },
    );
    if let Err(error) = result {
        report_service_start_error(theme, "couldn't start dev services", &error);
        return Err(2);
    }
    Ok(())
}

fn report_service_start_error(theme: &Theme, title: &str, error: &str) {
    if error.contains("did not become healthy")
        || error.contains("restart limit exhausted")
        || error.contains("restart did not produce a running process")
    {
        let name = error
            .strip_prefix("service `")
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, _)| name)
            .unwrap_or("unknown");
        theme.error_coded(
            "E1261",
            &format!("service `{name}` never became healthy"),
            "jetpack waited for the service's typed readiness contract and it did not pass in time.",
            &format!("inspect `jetpack services logs {name}` and fix the service's run or readiness declaration."),
        );
    } else {
        theme.error(
            title,
            error,
            "inspect the service logs and readiness declarations before retrying.",
        );
    }
}

fn run_before_start_tasks(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    service: &ModuleEval::DevServicePlan,
) -> Result<(), ()> {
    for task in &service.before_start {
        let task_parsed = Parsed {
            flags: parsed.flags.clone(),
            positional: vec![task.clone()],
            command: None,
        };
        let code = super::run_enter_dev::run_project_task(
            theme,
            &task_parsed,
            roots,
            project_dir,
            entry,
            task,
        );
        if code != 0 {
            theme.error(
                &format!("service `{}` prerequisite task `{task}` failed", service.name),
                "the service was not started because its declared finite task did not complete.",
                "fix the task, then run the service again.",
            );
            return Err(());
        }
    }
    Ok(())
}

/// Run typed lifecycle actions through the same composed environment used by
/// commands and services. The normal short form names a checked `#Job fn`;
/// only an explicit record command uses the expert trust-gated hook escape.
pub(super) fn run_lifecycle_hooks(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    env: &Env,
    hooks: &[ModuleEval::HookSpec],
    phase: &str,
) -> Result<(), i32> {
    run_lifecycle_hooks_with_mode(
        theme, parsed, roots, project_dir, entry, env, hooks, phase, false, false, false,
    )
}

/// Run lifecycle tasks for shell activation without allowing task stdout to
/// corrupt the generated export script.
pub(super) fn run_lifecycle_hooks_silent(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    env: &Env,
    hooks: &[ModuleEval::HookSpec],
    phase: &str,
) -> Result<(), i32> {
    run_lifecycle_hooks_with_mode(
        theme, parsed, roots, project_dir, entry, env, hooks, phase, false, false, true,
    )
}

/// Run lifecycle hooks with only the declared environment visible. This is
/// used by `jet env test`: checks must not accidentally pass because a host
/// variable or host-installed executable leaked into the process.
pub(super) fn run_lifecycle_hooks_clean(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    env: &Env,
    hooks: &[ModuleEval::HookSpec],
    phase: &str,
) -> Result<(), i32> {
    run_lifecycle_hooks_with_mode(
        theme, parsed, roots, project_dir, entry, env, hooks, phase, true, true, true,
    )
}

fn run_lifecycle_hooks_with_mode(
    theme: &Theme,
    parsed: &Parsed,
    roots: &Store::Roots,
    project_dir: &Path,
    entry: &Path,
    env: &Env,
    hooks: &[ModuleEval::HookSpec],
    phase: &str,
    clean: bool,
    strict: bool,
    silent: bool,
) -> Result<(), i32> {
    for hook in hooks {
        if let ModuleEval::HookAction::Task(task) = &hook.action {
            let task_parsed = Parsed {
                flags: parsed.flags.clone(),
                positional: vec![task.clone()],
                command: None,
            };
            let code = super::run_enter_dev::run_project_task_with_mode(
                theme,
                &task_parsed,
                roots,
                project_dir,
                entry,
                task,
                clean,
                silent,
            );
            if code != 0 {
                theme.error(
                    &format!("lifecycle {phase} task `{task}` failed"),
                    &format!("the task returned exit code {code}"),
                    "fix the task or remove it from the environment lifecycle.",
                );
                return Err(code);
            }
            continue;
        }
        if !hook.trusted {
            if !strict {
                theme.detail(&format!(
                    "skipping untrusted lifecycle {phase} hook `{}`",
                    hook.name
                ));
                continue;
            }
            theme.error_coded(
                "E1329",
                &format!("lifecycle hook '{}' is not trusted", hook.name),
                "background environment hooks execute project commands and require an explicit trusted record.",
                "set trusted: true after reviewing the hook, then approve the environment again.",
            );
            return Err(2);
        }
        let cwd = hook
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute()
                    || path
                        .components()
                        .any(|component| component == std::path::Component::ParentDir)
                {
                    None
                } else {
                    Some(project_dir.join(path))
                }
            });
        let Some(cwd) = cwd.flatten().or_else(|| {
            if hook.cwd.is_some() {
                None
            } else {
                Some(project_dir.to_path_buf())
            }
        }) else {
            theme.error(
                &format!("lifecycle {phase} hook '{}' has an unsafe cwd", hook.name),
                "hook working directories must stay inside the project",
                "use a project-relative path without `..`.",
            );
            return Err(2);
        };
        let project_root = match project_dir.canonicalize() {
            Ok(root) => root,
            Err(error) => {
                theme.error(
                    &format!("couldn't resolve lifecycle {phase} hook '{}' project root", hook.name),
                    &error.to_string(),
                    "run the hook from an existing project directory.",
                );
                return Err(2);
            }
        };
        let cwd = match cwd.canonicalize() {
            Ok(cwd) if cwd.starts_with(&project_root) && cwd.is_dir() => cwd,
            Ok(cwd) => {
                theme.error(
                    &format!("lifecycle {phase} hook '{}' has an unsafe cwd", hook.name),
                    &format!("`{}` resolves outside the project", cwd.display()),
                    "use a project-relative directory that does not escape through a symlink.",
                );
                return Err(2);
            }
            Err(error) => {
                theme.error(
                    &format!("lifecycle {phase} hook '{}' has an unusable cwd", hook.name),
                    &error.to_string(),
                    "use an existing project-relative directory.",
                );
                return Err(2);
            }
        };
        let ModuleEval::HookAction::Command(command) = &hook.action else {
            unreachable!("task lifecycle actions return above");
        };
        let mut command = crate::Platform::shell_command(command);
        command
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if clean {
            command.env_clear();
            env.apply_clean_to(&mut command);
        } else {
            env.apply_to(&mut command);
        }
        let output = command.output().map_err(|error| {
            theme.error(
                &format!("couldn't run {phase} hook '{}'", hook.name),
                &error.to_string(),
                "check the hook command and its working directory.",
            );
            2
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            theme.error(
                &format!("lifecycle {phase} hook '{}' failed", hook.name),
                stderr.trim(),
                "fix the hook or remove it from the environment lifecycle.",
            );
            return Err(2);
        }
    }
    Ok(())
}

/// U12: the readiness-poll ceiling `wait_for_services_ready`/`cmd_services`'s
/// `up` verb allow a service before reporting E1261. Not yet user-configurable
/// (no ratified field for it) — 15s comfortably covers a local dev dependency
/// (redis, a small HTTP mock, …) starting cold. `JETPACK_SERVICE_HEALTH_TIMEOUT_MS`
/// overrides it for tests that need to exercise the E1261 timeout path quickly
/// (mirrors `JETPACK_ROOT`/`JETPACK_FIXTURES`'s test-only env-var escape hatch).
fn service_health_timeout() -> std::time::Duration {
    std::env::var("JETPACK_SERVICE_HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(15))
}

fn selected_service_order(
    services: &[ModuleEval::DevServicePlan],
    name: Option<&str>,
) -> Result<Vec<usize>, String> {
    let order = Services::dependency_order(services)?;
    let Some(name) = name else { return Ok(order) };
    let Some(target) = services.iter().position(|service| service.name == name) else {
        return Err(format!("service `{name}` is not declared"));
    };
    if !services[target].enable {
        return Err(format!("service `{name}` is disabled"));
    }
    let mut needed = std::collections::BTreeSet::new();
    let mut pending = vec![target];
    while let Some(index) = pending.pop() {
        if !needed.insert(index) {
            continue;
        }
        for dependency in Services::dependency_names(&services[index]) {
            if let Some(index) = services.iter().position(|service| service.name == dependency) {
                pending.push(index);
            }
        }
    }
    Ok(order
        .into_iter()
        .filter(|index| needed.contains(index))
        .collect())
}

/// `jetpack services up|down|health|logs [<name>]` (U12). With no `<name>`,
/// every declared dev service is targeted; `logs` requires exactly one name.
pub(super) fn cmd_services(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(verb) = parsed.positional.first().cloned() else {
        theme.error(
            "`jetpack services` needs a verb",
            &format!("known verbs: {}.", Syntax::SERVICES_VERBS.join(", ")),
            "try `jetpack services up`.",
        );
        return 2;
    };
    let name = parsed.positional.get(1).cloned();

    let plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
    if matches!(
        verb.as_str(),
        v if v == Syntax::SERVICES_VERB_UP
            || v == Syntax::SERVICES_VERB_RESTART
            || v == Syntax::SERVICES_VERB_WATCH
            || v == Syntax::SERVICES_VERB_HEALTH
            || v == Syntax::SERVICES_VERB_WAIT
    ) {
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
    }
    let targets: Vec<&ModuleEval::DevServicePlan> = plan
        .dev_services
        .iter()
        .filter(|s| name.as_deref().is_none_or(|n| n == s.name))
        .collect();
    if targets.is_empty() {
        let headline = match &name {
            Some(n) => format!("no dev service named `{n}`"),
            None => "this project declares no dev `services:`".to_string(),
        };
        theme.error(
            &headline,
            "declare one under an `env.<name> { services: { … } }` role-module (U12).",
            "",
        );
        if name.is_none() {
            let presets = Services::catalog_presets()
                .into_iter()
                .map(|preset| format!("{} ({})", preset.name, preset.package))
                .collect::<Vec<_>>();
            theme.detail(&format!("available typed presets: {}", presets.join(", ")));
        }
        return 2;
    }

    match verb.as_str() {
        v if v == Syntax::SERVICES_VERB_LOGS => {
            let Some(n) = &name else {
                theme.error(
                    "`jetpack services logs` needs a service name",
                    "logs are per service.",
                    "try `jetpack services logs <name>`.",
                );
                return 2;
            };
            print!("{}", Services::logs(&project_dir, n));
            0
        }
        v if v == Syntax::SERVICES_VERB_UP => {
            let roots = Store::resolve();
            if roots.dev_mode {
                theme.detail(&theme.gray("user-owned hangar active"));
            }
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            let order = match selected_service_order(&plan.dev_services, name.as_deref()) {
                Ok(order) => order,
                Err(error) => {
                    theme.error(
                        "service dependency graph is invalid",
                        &error,
                        "declare each dependency once, keep it enabled, and remove dependency cycles.",
                    );
                    return 2;
                }
            };
            let entry = find_project_entry(&project_dir);
            let started = match Services::up_ordered_with(
                &project_dir,
                &env,
                &plan.dev_services,
                &order,
                service_health_timeout(),
                |svc| {
                    if let Some(field) = Services::unknown_field(svc) {
                        theme.error_coded(
                            "E1262",
                            &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
                            "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
                            "rename it to one of `enable`, `ports`, `run`, `shutdown`, `data_dir`, `ready`, `restart`, `watch`, `after`, `before_start`, or `sockets`, or remove it.",
                        );
                        return Err(format!("service `{}` has an unsupported field", svc.name));
                    }
                    run_before_start_tasks(
                        theme,
                        parsed,
                        &roots,
                        &project_dir,
                        &entry,
                        svc,
                    )
                    .map_err(|_| format!("before_start task for service `{}` failed", svc.name))
                },
            ) {
                Ok(started) => started,
                Err(error) => {
                    report_service_start_error(theme, "couldn't start services", &error);
                    return 2;
                }
            };
            for index in started {
                theme.ok(&format!("service `{}` is up", plan.dev_services[index].name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_RESTART => {
            let roots = Store::resolve();
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            let order = match selected_service_order(&plan.dev_services, name.as_deref()) {
                Ok(order) => order,
                Err(error) => {
                    theme.error("service dependency graph is invalid", &error, "remove dependency cycles and unknown dependencies.");
                    return 2;
                }
            };
            let entry = find_project_entry(&project_dir);
            let restarted = match Services::restart_ordered_with(
                &project_dir,
                &env,
                &plan.dev_services,
                &order,
                service_health_timeout(),
                |svc| {
                    if let Some(field) = Services::unknown_field(svc) {
                        theme.error_coded(
                            "E1262",
                            &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
                            "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
                            "rename it to one of `enable`, `ports`, `run`, `shutdown`, `data_dir`, `ready`, `restart`, `watch`, `after`, `before_start`, or `sockets`, or remove it.",
                        );
                        return Err(format!("service `{}` has an unsupported field", svc.name));
                    }
                    run_before_start_tasks(
                        theme,
                        parsed,
                        &roots,
                        &project_dir,
                        &entry,
                        svc,
                    )
                    .map_err(|_| format!("before_start task for service `{}` failed", svc.name))
                },
            ) {
                Ok(restarted) => restarted,
                Err(error) => {
                    theme.error(
                        "couldn't restart services",
                        &error,
                        "check the service logs, readiness declarations, and restart policy.",
                    );
                    return 2;
                }
            };
            for index in restarted {
                theme.ok(&format!("service `{}` restarted", plan.dev_services[index].name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_DOWN => {
            let order = match selected_service_order(&plan.dev_services, name.as_deref()) {
                Ok(order) => order,
                Err(error) => {
                    theme.error(
                        "service dependency graph is invalid",
                        &error,
                        "declare each dependency once, keep it enabled, and remove dependency cycles.",
                    );
                    return 2;
                }
            };
            if let Err(error) = Services::down_ordered(&project_dir, &plan.dev_services, &order) {
                theme.error("couldn't stop services", &error, "inspect the service logs and state.");
                return 2;
            }
            for index in order.iter().rev() {
                theme.ok(&format!("service `{}` is down", plan.dev_services[*index].name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_HEALTH => {
            let roots = Store::resolve();
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            if parsed.flags.json {
                let mut all_healthy = true;
                let records = targets
                    .iter()
                    .map(|svc| {
                        let (health, healthy) = match Services::health_one_with_env(
                            &project_dir,
                            Some(&env),
                            svc,
                        ) {
                            Services::Health::Disabled => ("disabled", true),
                            Services::Health::NotRunning => ("not-running", false),
                            Services::Health::Unhealthy => ("unhealthy", false),
                            Services::Health::Healthy => ("healthy", true),
                        };
                        all_healthy &= healthy;
                        let lifecycle = Services::lifecycle_json(&project_dir, &svc.name)
                            .unwrap_or_else(|| "null".to_string());
                        format!(
                            "{{\"service\":{},\"health\":{},\"lifecycle\":{lifecycle}}}",
                            crate::JSON::quote(&svc.name),
                            crate::JSON::quote(health),
                        )
                    })
                    .collect::<Vec<_>>();
                println!("[{}]", records.join(","));
                return if all_healthy { 0 } else { 1 };
            }
            let mut all_healthy = true;
            for svc in &targets {
                let (label, healthy) = match Services::health_one_with_env(
                    &project_dir,
                    Some(&env),
                    svc,
                ) {
                    Services::Health::Disabled => ("disabled", true),
                    Services::Health::NotRunning => ("not running", false),
                    Services::Health::Unhealthy => ("unhealthy", false),
                    Services::Health::Healthy => ("healthy", true),
                };
                all_healthy &= healthy;
                if healthy {
                    theme.ok(&format!("service `{}`: {label}", svc.name));
                } else {
                    theme.error(&format!("service `{}`: {label}", svc.name), "", "");
                }
            }
            if all_healthy {
                0
            } else {
                1
            }
        }
        v if v == Syntax::SERVICES_VERB_WAIT => {
            let roots = Store::resolve();
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            let order = match selected_service_order(&plan.dev_services, name.as_deref()) {
                Ok(order) => order,
                Err(error) => {
                    theme.error("service dependency graph is invalid", &error, "remove dependency cycles and unknown dependencies.");
                    return 2;
                }
            };
            for index in order {
                let svc = &plan.dev_services[index];
                if !Services::wait_healthy_with_env(
                    &project_dir,
                    Some(&env),
                    svc,
                    service_health_timeout(),
                ) {
                    theme.error_coded(
                        "E1261",
                        &format!("service `{}` is not ready", svc.name),
                        "jetpack waited for the service's typed readiness contract and it did not pass.",
                        &format!("start it with `jetpack services up {}` or inspect its logs.", svc.name),
                    );
                    return 1;
                }
                theme.ok(&format!("service `{}` is ready", svc.name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_WATCH => {
            let roots = Store::resolve();
            let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
                Ok(env) => env,
                Err(code) => return code,
            };
            let order = match selected_service_order(&plan.dev_services, name.as_deref()) {
                Ok(order) => order,
                Err(error) => {
                    theme.error("service dependency graph is invalid", &error, "remove dependency cycles and unknown dependencies.");
                    return 2;
                }
            };
            let entry = find_project_entry(&project_dir);
            let changed = match Services::watch_once_ordered_with(
                &project_dir,
                &env,
                &plan.dev_services,
                &order,
                service_health_timeout(),
                |svc| {
                    if let Some(field) = Services::unknown_field(svc) {
                        theme.error_coded(
                            "E1262",
                            &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
                            "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
                            "rename it to one of `enable`, `ports`, `run`, `shutdown`, `data_dir`, `ready`, `restart`, `watch`, `after`, `before_start`, or `sockets`, or remove it.",
                        );
                        return Err(format!("service `{}` has an unsupported field", svc.name));
                    }
                    run_before_start_tasks(
                        theme,
                        parsed,
                        &roots,
                        &project_dir,
                        &entry,
                        svc,
                    )
                    .map_err(|_| format!("before_start task for service `{}` failed", svc.name))
                },
            ) {
                Ok(changed) => changed,
                Err(error) => {
                    theme.error(
                        "couldn't watch services",
                        &error,
                        "declare project-relative files in `watch` and inspect the service lifecycle error.",
                    );
                    return 2;
                }
            };
            for index in order {
                if changed.contains(&index) {
                    theme.ok(&format!("service `{}` restarted after a watched-file change", plan.dev_services[index].name));
                } else if !plan.dev_services[index].watch.is_empty() {
                    theme.detail(&format!("service `{}` watch baseline is current", plan.dev_services[index].name));
                }
            }
            0
        }
        other => {
            theme.error(
                &format!("`{other}` is not a `jetpack services` verb"),
                &format!("known verbs: {}.", Syntax::SERVICES_VERBS.join(", ")),
                "try `jetpack services up`.",
            );
            2
        }
    }
}

/// Compiler-internal ServiceProbe transport. This is deliberately absent from
/// help/completions: Jetpack owns service realization and measurement, while
/// `jet budget` owns the typed report. The versioned row is validated by the
/// compiler before any sample enters a report.
pub(super) fn cmd_service_probe(theme: &Theme, parsed: &Parsed) -> i32 {
    let [name] = parsed.positional.as_slice() else {
        theme.error(
            "internal service probe needs exactly one service name",
            "the compiler and Jetpack use this operation as a version-checked machine protocol.",
            "use `jet budget check` or `jet dev`; do not invoke the internal operation directly.",
        );
        return 2;
    };
    let plan = match load_project_plan_with_selections(
        theme,
        parsed.flags.profile.as_deref(),
        parsed.flags.environment_profile.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let Some(service) = plan.dev_services.iter().find(|service| service.name == *name) else {
        theme.error(
            &format!("no dev service named `{name}`"),
            "the ServiceProbe budget names a service absent from this project's env.jet.",
            "name an enabled service declared under the active env role.",
        );
        return 2;
    };
    if !service.enable {
        theme.error(
            &format!("service `{name}` is disabled"),
            "a disabled service has no startup-readiness workload to measure.",
            "enable the service or remove its ServiceProbe budget.",
        );
        return 2;
    }

    let project_dir = std::env::current_dir().unwrap_or_default();
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

    let roots = Store::resolve();
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let samples = match Services::measure_readiness(&project_dir, &env, service, 20) {
        Ok(samples) => samples,
        Err(message) => {
            theme.error(
                &format!("couldn't measure service `{name}`"),
                &message,
            "check the service run, shutdown, and readiness declarations.",
            );
            return 2;
        }
    };
    let encoded_name: String = name.bytes().map(|byte| format!("{byte:02x}")).collect();
    print!("JETSERVICE1\t{encoded_name}");
    for sample in samples {
        print!("\t{sample}");
    }
    println!();
    0
}

/// `jetpack secrets keygen|set|get|recipients` (U13, D-JPK-SECRETCRYPTO1).
pub(super) fn cmd_secrets(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(verb) = parsed.positional.first().cloned() else {
        theme.error(
            "`jetpack secrets` needs a verb",
            &format!("known verbs: {}.", Syntax::SECRETS_VERBS.join(", ")),
            "try `jetpack secrets keygen`.",
        );
        return 2;
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
    match verb.as_str() {
        v if v == Syntax::SECRETS_VERB_KEYGEN => {
            let force = parsed
                .positional
                .iter()
                .any(|a| a == Syntax::SECRETS_FLAG_FORCE);
            match Secrets::keygen(force) {
                Ok((path, recipient)) => {
                    theme.ok(&format!("wrote identity to `{}`", path.display()));
                    theme.detail(&format!("recipient: {recipient}"));
                    theme.detail("add it with `jetpack secrets recipients add <recipient>`");
                    0
                }
                Err(msg) => {
                    theme.error("couldn't generate a secrets identity", &msg, "");
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_RECIPIENTS => {
            let Some(sub) = parsed.positional.get(1).cloned() else {
                theme.error(
                    "`jetpack secrets recipients` needs a verb",
                    &format!(
                        "known verbs: {}.",
                        Syntax::SECRETS_RECIPIENTS_VERBS.join(", ")
                    ),
                    "try `jetpack secrets recipients list`.",
                );
                return 2;
            };
            match sub.as_str() {
                v if v == Syntax::SECRETS_RECIPIENTS_VERB_ADD => {
                    let Some(recipient) = parsed.positional.get(2) else {
                        theme.error(
                            "`jetpack secrets recipients add` needs a recipient",
                            "an `age1...` public key.",
                            "try `jetpack secrets recipients add age1...`.",
                        );
                        return 2;
                    };
                    match Secrets::add_recipient(&project_dir, recipient) {
                        Ok(true) => theme.ok(&format!("added recipient `{recipient}`")),
                        Ok(false) => {
                            theme.detail(&format!("recipient `{recipient}` already present"))
                        }
                        Err(error) => {
                            theme.error("couldn't add secrets recipient", &error, "");
                            return 2;
                        }
                    }
                    0
                }
                v if v == Syntax::SECRETS_RECIPIENTS_VERB_LIST => {
                    match Secrets::list_recipients(&project_dir) {
                        Ok(recipients) => {
                            for recipient in recipients {
                                println!("{recipient}");
                            }
                        }
                        Err(error) => {
                            theme.error("couldn't list secrets recipients", &error, "");
                            return 2;
                        }
                    }
                    0
                }
                other => {
                    theme.error(
                        &format!("`{other}` is not a `jetpack secrets recipients` verb"),
                        &format!(
                            "known verbs: {}.",
                            Syntax::SECRETS_RECIPIENTS_VERBS.join(", ")
                        ),
                        "try `jetpack secrets recipients list`.",
                    );
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_SET => {
            let (Some(name), Some(value)) = (parsed.positional.get(1), parsed.positional.get(2))
            else {
                theme.error(
                    "`jetpack secrets set` needs a name and a value",
                    "",
                    "try `jetpack secrets set db_password hunter2`.",
                );
                return 2;
            };
            match Secrets::set(&project_dir, name, value) {
                Ok(()) => {
                    theme.ok(&format!("set `{name}`"));
                    0
                }
                Err(msg) => {
                    theme.error(&format!("couldn't set `{name}`"), &msg, "");
                    2
                }
            }
        }
        v if v == Syntax::SECRETS_VERB_GET => {
            let Some(name) = parsed.positional.get(1) else {
                theme.error(
                    "`jetpack secrets get` needs a name",
                    "",
                    "try `jetpack secrets get db_password`.",
                );
                return 2;
            };
            match Secrets::get(&project_dir, name) {
                Ok(Some(value)) => {
                    println!("{value}");
                    0
                }
                Ok(None) => {
                    theme.error_coded(
                        "E1263",
                        &format!("no secret named `{name}`"),
                        "the encrypted store doesn't have an entry with this name.",
                        &format!("set it first with `jetpack secrets set {name} <value>`, or check the spelling."),
                    );
                    2
                }
                Err(msg) => {
                    theme.error(&format!("couldn't read `{name}`"), &msg, "");
                    2
                }
            }
        }
        other => {
            theme.error(
                &format!("`{other}` is not a `jetpack secrets` verb"),
                &format!("known verbs: {}.", Syntax::SECRETS_VERBS.join(", ")),
                "try `jetpack secrets keygen`.",
            );
            2
        }
    }
}

/// The sibling `jet` binary next to the running `jetpack` process, falling
/// back to a bare PATH lookup. Mirrors `Source/EngineDispatch.rs`'s
/// same-directory-then-PATH search in the other direction; jetpack never
/// links the compiler in-process (D-JPK-DISPATCH1), so this is the one place
/// it hands off to it.
pub(super) fn find_jet_binary() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let name = if cfg!(windows) { "jet.exe" } else { "jet" };
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    Syntax::BINARY_NAME.to_string()
}

/// Project entry for bare `jetpack dev`, matching `jet`'s run-first convention.
/// Kept local because jetpack and jet are separate binaries (D-JPK-DISPATCH1).
pub(super) fn find_project_entry(project_dir: &Path) -> PathBuf {
    match package_output_entry(project_dir) {
        Ok(Some(entry)) => return entry,
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!(" fix: repair the typed Package output or point at a `.jet` file directly");
            std::process::exit(2);
        }
    }
    let default = project_dir.join(Syntax::DEFAULT_ENTRY_FILE);
    if default.is_file() {
        return default;
    }
    let src_default = project_dir.join("src").join(Syntax::DEFAULT_ENTRY_FILE);
    if src_default.is_file() {
        return src_default;
    }
    if let Some(Ok(manifest)) = crate::PackageManifest::PackManifest::load(project_dir) {
        let named = project_dir.join(format!("{}.{}", manifest.package.name, Syntax::FILE_EXT));
        if named.is_file() {
            return named;
        }
    }
    for legacy in [
        project_dir.join("src").join(Syntax::LEGACY_ENTRY_FILE),
        project_dir.join(Syntax::LEGACY_ENTRY_FILE),
        project_dir
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(Syntax::LEGACY_ENTRY_FILE),
    ] {
        if legacy.is_file() {
            return legacy;
        }
    }
    default
}

/// D-ENV-PACKAGE1 / #1003: a canonical Package output is the first entry
/// selection rule. Legacy `run.jet` remains the fallback for projects that do
/// not declare a typed Package output.
fn package_output_entry(project_dir: &Path) -> Result<Option<PathBuf>, String> {
    let Some(package) = jet_pkg_model::Package::PackageFacts::load(project_dir) else {
        return Ok(None);
    };
    let package = match package {
        Ok(package) => package,
        Err(_error)
            if !project_dir.join(Syntax::PACKAGE_FILE).is_file()
                && crate::PackageManifest::PackManifest::load(project_dir)
                    .is_some_and(|manifest| manifest.is_ok()) =>
        {
            // A legacy `pkg.jet` manifest still owns package identity and
            // publish metadata, but it is not a typed Package output. Let
            // the normal entry-file fallback handle that project shape.
            return Ok(None);
        }
        Err(error) => {
        let source = if project_dir.join(Syntax::PACKAGE_FILE).is_file() {
            project_dir.join(Syntax::PACKAGE_FILE)
        } else {
            project_dir.join(Syntax::PAYLOAD_FILE)
        };
            return Err(format!("typed Package `{}` is invalid: {error}", source.display()));
        }
    };
    package.resolve_run_entry(project_dir)
}

/// Whether `file` defines a top-level `fn dev()` or `fn run()` (U19's
/// dev-with-fallback rule, E1254 otherwise). A parse failure just means "no"
/// here — the real diagnostics surface a moment later when the compiler
/// actually loads the file.
pub(super) fn has_dev_or_run_entry(file: &Path) -> bool {
    let Ok(src) = std::fs::read_to_string(file) else {
        return false;
    };
    let (toks, diags) = crate::Lexer::lex(&src);
    if !diags.is_empty() {
        return false;
    }
    let Ok(prog) = crate::Parser::parse(&toks) else {
        return false;
    };
    prog.items
        .iter()
        .any(|i| matches!(i, crate::AST::Item::Func(f) if f.name == "dev" || f.name == "run"))
}

/// D-JPK-TASKRUN1: top-level `#Job fn` names in the project entry (sorted).
/// Parse failure → empty list (real diagnostics surface when jet compiles).
pub(super) fn list_project_tasks(file: &Path) -> Vec<String> {
    project_task_names(file).unwrap_or_default()
}

/// Return task names when the entry can be parsed. `None` preserves the
/// compiler's own diagnostic path for unreadable or malformed entries.
fn project_task_names(file: &Path) -> Option<Vec<String>> {
    let Ok(src) = std::fs::read_to_string(file) else {
        return None;
    };
    let (toks, diags) = crate::Lexer::lex(&src);
    if !diags.is_empty() {
        return None;
    }
    let Ok(prog) = crate::Parser::parse(&toks) else {
        return None;
    };
    let mut names: Vec<String> = prog
        .items
        .iter()
        .filter_map(|i| match i {
            crate::AST::Item::Func(f) if f.is_task => Some(f.name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    Some(names)
}

/// Distinguish a parsed entry with no matching task from an entry whose
/// syntax must be diagnosed by the compiler handoff.
pub(super) fn project_task_declared(file: &Path, task: &str) -> Option<bool> {
    project_task_names(file).map(|names| names.iter().any(|name| name == task))
}

/// D-TASK-META1: return the checked static metadata for one task. A parse
/// failure is intentionally treated as absence here; the compiler invocation
/// below remains the source of the complete diagnostic.
pub(super) fn project_task_metadata(
    file: &Path,
    task: &str,
) -> Option<crate::AST::TaskMetadata> {
    let src = std::fs::read_to_string(file).ok()?;
    let (toks, diags) = crate::Lexer::lex(&src);
    if !diags.is_empty() {
        return None;
    }
    let prog = crate::Parser::parse(&toks).ok()?;
    prog.items.iter().find_map(|item| match item {
        crate::AST::Item::Func(function) if function.name == task && function.is_task => {
            function.task_metadata.clone()
        }
        _ => None,
    })
}

/// `jetpack config trust add/list/remove` (U19) — durable glob/prefix patterns
/// that pre-authorize matching projects with no per-hash prompt at all.
pub(super) fn cmd_config(theme: &Theme, parsed: &Parsed) -> i32 {
    let group = parsed.positional.first().map(String::as_str);
    if group != Some(Syntax::CONFIG_VERB_TRUST) && group != Some(Syntax::CONFIG_VERB_SANDBOX) {
        theme.error(
            &format!("`jetpack config {}` isn't a command", group.unwrap_or("")),
            "jetpack config manages the env/dev trust store and sandbox fallback policy.",
            "try `jetpack config trust list` or `jetpack config sandbox status`.",
        );
        return 2;
    }
    let store = Trust::store_path();
    match parsed.positional.get(1).map(String::as_str) {
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_REQUIRE =>
        {
            match RuntimePolicy::write_sandbox_policy(RuntimePolicy::SandboxPolicy::Require) {
                Ok(path) => theme.status(&format!("sandbox fallback refused: {}", path.display())),
                Err(e) => {
                    theme.error(
                        "couldn't write sandbox policy",
                        &format!("{e}"),
                        "check permissions on your ~/.jet directory.",
                    );
                    return 1;
                }
            }
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_ALLOW =>
        {
            match RuntimePolicy::write_sandbox_policy(RuntimePolicy::SandboxPolicy::AllowFallback) {
                Ok(path) => theme.status(&format!("sandbox fallback allowed: {}", path.display())),
                Err(e) => {
                    theme.error(
                        "couldn't write sandbox policy",
                        &format!("{e}"),
                        "check permissions on your ~/.jet directory.",
                    );
                    return 1;
                }
            }
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_SANDBOX)
                && v == Syntax::CONFIG_SANDBOX_VERB_STATUS =>
        {
            let status = RuntimePolicy::detect_sandbox();
            let policy = RuntimePolicy::read_sandbox_policy();
            let policy_label = match policy {
                RuntimePolicy::SandboxPolicy::AllowFallback => "allow",
                RuntimePolicy::SandboxPolicy::Require => "require",
            };
            theme.status(&format!(
                "sandbox: {:?} via {} (policy: {policy_label})",
                status.level, status.mechanism
            ));
            theme.detail(&status.reason);
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST) && v == Syntax::CONFIG_TRUST_VERB_ADD =>
        {
            let Some(pattern) = parsed.positional.get(2) else {
                theme.error(
                    "`jetpack config trust add` needs a pattern",
                    "a pattern pre-authorizes matching projects by path prefix/glob.",
                    "try `jetpack config trust add ~/work/*`.",
                );
                return 2;
            };
            let added = Trust::add_pattern(&store, pattern);
            theme.status(&if added {
                format!("trusted: {pattern}")
            } else {
                format!("already trusted: {pattern}")
            });
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST) && v == Syntax::CONFIG_TRUST_VERB_LIST =>
        {
            let entries = Trust::list_entries(&store);
            if entries.is_empty() {
                theme.status("no trust entries yet.");
            } else {
                for e in entries {
                    theme.detail(&e);
                }
            }
            0
        }
        Some(v)
            if group == Some(Syntax::CONFIG_VERB_TRUST)
                && v == Syntax::CONFIG_TRUST_VERB_REMOVE =>
        {
            let Some(pattern) = parsed.positional.get(2) else {
                theme.error(
                    "`jetpack config trust remove` needs a pattern",
                    "removes a previously trusted path pattern.",
                    "try `jetpack config trust remove ~/work/*`.",
                );
                return 2;
            };
            let removed = Trust::remove_pattern(&store, pattern);
            theme.status(&if removed {
                format!("removed: {pattern}")
            } else {
                format!("not found: {pattern}")
            });
            0
        }
        _ if group == Some(Syntax::CONFIG_VERB_TRUST) => {
            theme.error(
                "`jetpack config trust` needs a verb",
                &format!(
                    "the trust verbs are: {}.",
                    Syntax::CONFIG_TRUST_VERBS.join(", ")
                ),
                "try `add <pattern>`, `list`, or `remove <pattern>`.",
            );
            2
        }
        _ if group == Some(Syntax::CONFIG_VERB_SANDBOX) => {
            theme.error(
                "`jetpack config sandbox` needs a verb",
                &format!(
                    "the sandbox verbs are: {}.",
                    Syntax::CONFIG_SANDBOX_VERBS.join(", ")
                ),
                "try `require`, `allow`, or `status`.",
            );
            2
        }
        _ => {
            theme.error(
                &format!("`jetpack config {}` isn't a command", group.unwrap_or("")),
                "jetpack config manages trust and sandbox fallback policy.",
                "try `jetpack config trust list` or `jetpack config sandbox status`.",
            );
            2
        }
    }
}
