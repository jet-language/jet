//! D-BROWSER-AUTO1=A (#1187): `jetpack browser lock|provision|resolve|list`.

use super::parse::Parsed;
use super::realize::{classify_or_report, RunPlan};
use super::trust_env_build::compose_env;
use crate::BrowserLock;
use crate::Output::Theme;
use crate::RefSpec;
use crate::Store;
use crate::Syntax;
use jet_env_model::ModuleEval;
use std::path::PathBuf;

pub(super) fn cmd_browser(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::BROWSER_VERB_LOCK => browser_lock(theme, parsed),
        Some(v) if v == Syntax::BROWSER_VERB_PROVISION => browser_provision(theme, parsed),
        Some(v) if v == Syntax::BROWSER_VERB_RESOLVE => browser_resolve(theme, parsed),
        Some(v) if v == Syntax::BROWSER_VERB_LIST => browser_list(theme),
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a jetpack browser verb"),
                &format!(
                    "`jetpack browser` verbs are: {}.",
                    Syntax::BROWSER_VERBS.join(", ")
                ),
                "try `jetpack browser lock chromium --binary /path/to/chromium`.",
            );
            2
        }
        None => {
            theme.error(
                "`jetpack browser` needs a verb",
                &format!(
                    "verbs are: {} — lock or provision a project-pinned browser binary (D-BROWSER-AUTO1).",
                    Syntax::BROWSER_VERBS.join(", ")
                ),
                "try `jetpack browser list` or `jetpack browser lock chromium --binary ./chromium`.",
            );
            2
        }
    }
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn flag_value(parsed: &Parsed, name: &str) -> Option<String> {
    match name {
        n if n == Syntax::BROWSER_FLAG_BINARY => parsed.flags.browser_binary.clone(),
        n if n == Syntax::BROWSER_FLAG_VERSION => parsed.flags.browser_version.clone(),
        n if n == Syntax::BROWSER_FLAG_PROTOCOL => parsed.flags.browser_protocol.clone(),
        _ => None,
    }
    .or_else(|| {
        let mut i = 0;
        while i + 1 < parsed.positional.len() {
            if parsed.positional[i] == name {
                return Some(parsed.positional[i + 1].clone());
            }
            i += 1;
        }
        None
    })
}

fn browser_lock(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(engine) = parsed.positional.get(1).map(String::as_str) else {
        theme.error(
            "`jetpack browser lock` needs an engine",
            &format!("engines are: {}.", Syntax::BROWSER_ENGINES.join(", ")),
            "try `jetpack browser lock chromium --binary /usr/bin/chromium`.",
        );
        return 2;
    };
    let Some(binary) = flag_value(parsed, Syntax::BROWSER_FLAG_BINARY) else {
        theme.error(
            "`jetpack browser lock` needs --binary <path>",
            "locking pins an exact on-disk browser for automation; PATH lookups are rejected.",
            "try `jetpack browser lock chromium --binary /nix/store/.../bin/chromium`.",
        );
        return 2;
    };
    let version = flag_value(parsed, Syntax::BROWSER_FLAG_VERSION)
        .unwrap_or_else(|| BrowserLock::read_version_label(std::path::Path::new(&binary)));
    let protocol = flag_value(parsed, Syntax::BROWSER_FLAG_PROTOCOL)
        .unwrap_or_else(|| Syntax::BROWSER_DEFAULT_PROTOCOL.to_string());
    match BrowserLock::lock_binary(
        &project_root(),
        engine,
        std::path::Path::new(&binary),
        &version,
        &protocol,
        &format!("locked from {}", binary),
    ) {
        Ok(locked) => {
            theme.status(&format!(
                "locked {} {} -> {} ({})",
                theme.bold(&locked.engine),
                theme.bold(&locked.version),
                locked.binary,
                locked.envelope.output_hash
            ));
            0
        }
        Err(error) => {
            theme.error("browser lock failed", &error.to_string(), "fix the binary path and retry.");
            2
        }
    }
}

fn browser_provision(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.get(1) else {
        theme.error(
            "`jetpack browser provision` needs a package ref",
            "provision realizes one browser package and locks its binary into `.jet/lock`.",
            "try `jetpack browser provision chromium@nixpkgs`.",
        );
        return 2;
    };
    let Ok(spec) = classify_or_report(theme, raw) else {
        return 2;
    };
    let engine = match BrowserLock::normalize_engine(spec.short_name()) {
        Ok(engine) => engine,
        Err(error) => {
            // Allow chromium@nixpkgs where short_name is chromium.
            theme.error(
                "browser provision needs a supported engine package",
                &error.to_string(),
                &format!(
                    "package short name must be one of: {}.",
                    Syntax::BROWSER_ENGINES.join(", ")
                ),
            );
            return 2;
        }
    };
    let roots = Store::resolve();
    let plan = RunPlan {
        refs: vec![spec.clone()],
        adapters: Vec::new(),
        table: RefSpec::SourceTable::empty(),
        label: Syntax::JETPACK_PROMPT_LABEL.to_string(),
        prompt_path: ModuleEval::PromptPathMode::default(),
        prompt_strip: ModuleEval::PromptStripMode::default(),
        dev_services: Vec::new(),
        secrets: Vec::new(),
        environment: ModuleEval::EnvironmentFacts::default(),
    };
    let env = match compose_env(theme, &roots, &parsed.flags, &plan) {
        Ok(env) => env,
        Err(code) => return code,
    };
    let Some(lease) = env.cache_leases.first() else {
        theme.error(
            &format!("`{}` realized with no output to lock", spec.raw),
            "browser provision needs a package output that contains an engine binary.",
            "pick chromium, firefox, or webkit from a realizable source.",
        );
        return 2;
    };
    let output = PathBuf::from(lease.original_output());
    let binary = match BrowserLock::find_engine_binary(&output, engine) {
        Ok(path) => path,
        Err(error) => {
            theme.error(
                "could not find the browser binary in the realized package",
                &error.to_string(),
                "the package must ship an executable under bin/.",
            );
            return 2;
        }
    };
    let version = flag_value(parsed, Syntax::BROWSER_FLAG_VERSION)
        .unwrap_or_else(|| BrowserLock::read_version_label(&binary));
    let protocol = flag_value(parsed, Syntax::BROWSER_FLAG_PROTOCOL)
        .unwrap_or_else(|| Syntax::BROWSER_DEFAULT_PROTOCOL.to_string());
    match BrowserLock::lock_binary(
        &project_root(),
        engine,
        &binary,
        &version,
        &protocol,
        &format!("{} via jetpack browser provision", spec.raw),
    ) {
        Ok(locked) => {
            theme.status(&format!(
                "provisioned {} {} -> {} ({})",
                theme.bold(&locked.engine),
                theme.bold(&locked.version),
                locked.binary,
                locked.envelope.output_hash
            ));
            0
        }
        Err(error) => {
            theme.error(
                "browser provision lock failed",
                &error.to_string(),
                "the realized binary could not be pinned; check permissions on `.jet/lock`.",
            );
            2
        }
    }
}

fn browser_resolve(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(engine) = parsed.positional.get(1).map(String::as_str) else {
        theme.error(
            "`jetpack browser resolve` needs an engine",
            &format!("engines are: {}.", Syntax::BROWSER_ENGINES.join(", ")),
            "try `jetpack browser resolve chromium`.",
        );
        return 2;
    };
    match BrowserLock::resolve(&project_root(), engine) {
        Ok(locked) => {
            println!(
                "engine={}\nversion={}\nbinary={}\nprotocol={}\noutput-hash={}\nplatform={}\nsize={}",
                locked.engine,
                locked.version,
                locked.binary,
                locked.protocol,
                locked.envelope.output_hash,
                locked.envelope.platform,
                locked.size
            );
            0
        }
        Err(error) => {
            theme.error(
                "browser resolve failed",
                &error.to_string(),
                "run `jetpack browser lock` or `jetpack browser provision` first.",
            );
            2
        }
    }
}

fn browser_list(theme: &Theme) -> i32 {
    let browsers = BrowserLock::list(&project_root());
    if browsers.is_empty() {
        theme.status("no locked browsers yet.");
        return 0;
    }
    println!("ENGINE    VERSION  PROTOCOL     HASH");
    for browser in browsers {
        println!(
            "{:<9} {:<8} {:<12} {}",
            browser.engine,
            if browser.version.is_empty() {
                "-"
            } else {
                &browser.version
            },
            browser.protocol,
            browser.envelope.output_hash
        );
    }
    0
}
