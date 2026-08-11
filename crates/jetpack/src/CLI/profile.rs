//! D-JPK-PROFILE1=D: source-backed package-generation planning.
//!
//! This command is intentionally read-only. It exercises the shared module
//! evaluator and resolver; generation publication, activation, rollback, and
//! user composition consume the same plan in their later slices.

use super::parse::Parsed;
use super::realize::project_env_root;
use crate::Output::Theme;
use crate::{EnvFile, JSON, Syntax};
use jet_env_model::ModuleEval;

pub(super) fn cmd_profile(theme: &Theme, parsed: &Parsed) -> i32 {
    match parsed.positional.first().map(String::as_str) {
        Some(v) if v == Syntax::PROFILE_VERB_PLAN => profile_plan(theme, parsed),
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a package-generation verb"),
                &format!("`jet profile` verbs are: {}.", Syntax::PROFILE_VERBS.join(", ")),
                "try `jet profile plan <name>`.",
            );
            2
        }
        None => {
            theme.error(
                "`jet profile` needs a verb",
                &format!("verbs are: {}.", Syntax::PROFILE_VERBS.join(", ")),
                "try `jet profile plan <name>`.",
            );
            2
        }
    }
}

fn profile_plan(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(name) = parsed.positional.get(1) else {
        theme.error(
            "`jet profile plan` needs a generation name",
            "planning resolves one source-backed `profile.<name>` declaration and its parents",
            "try `jet profile plan dev`.",
        );
        return 2;
    };
    if parsed.positional.len() != 2 || parsed.command.is_some() {
        theme.error(
            "`jet profile plan` accepts one generation name",
            "planning is read-only and has no trailing command",
            "run `jet profile plan <name> --json` for machine-readable output",
        );
        return 2;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = project_env_root(&cwd);
    let path = EnvFile::path_in(&project_dir);
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(error) => {
            theme.error(
                &format!("couldn't read {}", path.display()),
                &error.to_string(),
                "create an env.jet with a `module profile.<name> { … }` declaration",
            );
            return 2;
        }
    };
    let plan = match ModuleEval::evaluate_package_profile(&src, &project_dir, name) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(Syntax::ENV_FILE, &src, std::slice::from_ref(&diagnostic))
            );
            return 2;
        }
    };
    if parsed.flags.json {
        println!(
            "{{\"name\":{},\"selected_profiles\":[{}],\"applied\":[{}],\"sources\":[{}],\"packages\":[{}],\"collisions\":{{{}}}}}",
            JSON::quote(&plan.name),
            quote_strings(&plan.selected_profiles),
            quote_strings(&plan.applied),
            quote_strings(&plan.sources),
            plan.packages
                .iter()
                .map(|package| {
                    format!(
                        "{{\"raw\":{},\"target\":{},\"source\":{},\"upstream\":{},\"provider\":{},\"channel\":{},\"declared_by\":[{}]}}",
                        JSON::quote(&package.raw),
                        JSON::quote(&package.target),
                        JSON::quote(&package.source),
                        package
                            .upstream
                            .as_deref()
                            .map(JSON::quote)
                            .unwrap_or_else(|| "null".to_string()),
                        JSON::quote(&package.provider),
                        package
                            .channel
                            .as_deref()
                            .map(JSON::quote)
                            .unwrap_or_else(|| "null".to_string()),
                        quote_strings(&package.declared_by),
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
            plan.collisions
                .iter()
                .map(|(path, provider)| {
                    format!("{}:{}", JSON::quote(path), JSON::quote(provider))
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        return 0;
    }
    theme.ok(&format!("package generation {} planned", theme.bold(&plan.name)));
    theme.detail(&format!("applied: {}", plan.applied.join(" -> ")));
    if !plan.sources.is_empty() {
        theme.detail(&format!("declared by: {}", plan.sources.join(", ")));
    }
    for package in &plan.packages {
        let channel = package
            .channel
            .as_deref()
            .map(|value| format!("#{value}"))
            .unwrap_or_default();
        let upstream = package
            .upstream
            .as_deref()
            .map(|value| format!(" -> {value}"))
            .unwrap_or_default();
        theme.detail(&format!(
            "package {}  [{}{} via {}]  ({})",
            package.raw,
            package.source,
            channel,
            package.provider,
            package.declared_by.join(", ")
        ));
        if !upstream.is_empty() {
            theme.detail(&format!("  source{upstream}"));
        }
    }
    for (path, provider) in &plan.collisions {
        theme.detail(&format!("collision {path} <- {provider}"));
    }
    0
}

fn quote_strings(values: &[String]) -> String {
    values
        .iter()
        .map(|value| JSON::quote(value))
        .collect::<Vec<_>>()
        .join(",")
}
