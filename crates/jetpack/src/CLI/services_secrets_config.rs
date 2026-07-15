use super::parse::Parsed;
use super::realize::load_project_plan;
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
    project_dir: &Path,
    env: &Env,
    services: &[ModuleEval::DevServicePlan],
) -> Result<(), i32> {
    for svc in services {
        if !svc.enable {
            continue;
        }
        theme.detail(&format!(
            "waiting for service `{}` to become healthy…",
            svc.name
        ));
        bring_up_one(theme, project_dir, env, svc).map_err(|_| 2)?;
    }
    Ok(())
}

/// Bring up one enabled service and block until it's healthy: E1262 for an
/// unrecognized field, a plain error if it can't be started at all, E1261 on
/// a readiness timeout. Shared by `wait_for_services_ready` (the `jet dev`
/// health gate) and `cmd_services`'s `up` verb so the two never drift.
fn bring_up_one(
    theme: &Theme,
    project_dir: &Path,
    env: &Env,
    svc: &ModuleEval::DevServicePlan,
) -> Result<(), ()> {
    if let Some(field) = Services::unknown_field(svc) {
        theme.error_coded(
            "E1262",
            &format!("service `{}` has a field jetpack doesn't recognize: `{field}`", svc.name),
            "a dev-supervised `Service` stays open at parse time, but jetpack's dev-runtime tier is the only consumer of its fields — an unrecognized key is almost always a typo.",
            "rename it to one of `enable`, `ports`, `init`, `shutdown`, `data_dir`, `ready`, or remove it.",
        );
        return Err(());
    }
    if let Err(msg) = Services::up_one(project_dir, env, svc) {
        theme.error(&format!("couldn't start service `{}`", svc.name), &msg, "");
        return Err(());
    }
    if Services::wait_healthy(project_dir, svc, service_health_timeout()) {
        Ok(())
    } else {
        theme.error_coded(
            "E1261",
            &format!("service `{}` never became healthy", svc.name),
            "jetpack waited for its readiness contract (`ready:`, else a TCP probe on its first `ports:` entry, else a bare process-alive check) and it never passed.",
            &format!("check `jetpack services logs {}` for what it printed, and confirm its `init`/`ready` commands are correct.", svc.name),
        );
        Err(())
    }
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

    let plan = match load_project_plan(theme) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
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
            for svc in &targets {
                if !svc.enable {
                    theme.detail(&format!("service `{}` is disabled, skipping", svc.name));
                    continue;
                }
                if bring_up_one(theme, &project_dir, &env, svc).is_err() {
                    return 2;
                }
                theme.ok(&format!("service `{}` is up", svc.name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_DOWN => {
            for svc in &targets {
                if let Err(error) = Services::down_one(&project_dir, svc) {
                    theme.error(&format!("couldn't stop service `{}`", svc.name), &error, "");
                    return 2;
                }
                theme.ok(&format!("service `{}` is down", svc.name));
            }
            0
        }
        v if v == Syntax::SERVICES_VERB_HEALTH => {
            let mut all_healthy = true;
            for svc in &targets {
                let (label, healthy) = match Services::health_one(&project_dir, svc) {
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
    let plan = match load_project_plan(theme) {
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

    let roots = Store::resolve();
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let project_dir = std::env::current_dir().unwrap_or_default();
    let samples = match Services::measure_readiness(&project_dir, &env, service, 20) {
        Ok(samples) => samples,
        Err(message) => {
            theme.error(
                &format!("couldn't measure service `{name}`"),
                &message,
                "check the service init, shutdown, and readiness declarations.",
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

/// The project's entry file for the bare (no-file) `jetpack dev`: `.jet/main.jet`
/// if present, else `main.jet` — the same convention `jet run`/`jet build` use
/// for a bare project (`Source/main.rs`'s `find_project_entry`). Duplicated by
/// hand rather than shared: jetpack and jet are separate binaries by design
/// (D-JPK-DISPATCH1), so deleting either still leaves the other's own commands
/// working.
pub(super) fn find_project_entry(project_dir: &Path) -> PathBuf {
    let dot_jet = project_dir
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(format!("main.{}", Syntax::FILE_EXT));
    if dot_jet.is_file() {
        return dot_jet;
    }
    project_dir.join(format!("main.{}", Syntax::FILE_EXT))
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

/// D-JPK-TASKRUN1: top-level `#Task fn` names in the project entry (sorted).
/// Parse failure → empty list (real diagnostics surface when jet compiles).
pub(super) fn list_project_tasks(file: &Path) -> Vec<String> {
    let Ok(src) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let (toks, diags) = crate::Lexer::lex(&src);
    if !diags.is_empty() {
        return Vec::new();
    }
    let Ok(prog) = crate::Parser::parse(&toks) else {
        return Vec::new();
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
    names
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
