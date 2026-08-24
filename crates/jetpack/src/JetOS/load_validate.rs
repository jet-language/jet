use super::entry::default_config_path;
use super::options_rendering::user_names;
use super::types::Target;
use crate::Output::Theme;
use crate::Package::PackageFacts;
use crate::Syntax;
use jet_env_model::ModuleEval::{
    self, env_plan_from_package_outputs, project_package_outputs, EnvPlan, SystemPlan,
};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn parse_target_or_report(theme: &Theme, raw: Option<&str>) -> Option<Target> {
    let raw = raw.unwrap_or("");
    if raw.trim().is_empty() {
        theme.error_coded(
            "E0979",
            "`jet os` needs a host to apply",
            "D-JPK-REF1: a bare host selects `system.<host>` in ./config.jet; `host@root` selects an exact external root.",
            "write `jet os switch laptop` or `jet os switch laptop@../machines`.",
        );
        return None;
    }
    let Some((host, root)) = raw.split_once(Syntax::OS_HOST_SELECTOR) else {
        return Some(Target {
            config: config_for_root(
                default_config_path()
                    .parent()
                    .unwrap_or_else(|| Path::new(".")),
            ),
            host: raw.to_string(),
        });
    };
    if host.trim().is_empty() {
        theme.error_coded(
            "E0979",
            "`jet os` needs a host to apply",
            "D-JPK-REF1: `host@root` uses the text before `@` as the host name.",
            "write `jet os switch laptop` or `jet os switch laptop@../machines`.",
        );
        return None;
    }
    if root.trim().is_empty() {
        theme.error_coded(
            "E0979",
            &format!("`jet os` target `{raw}` needs a root after `@`"),
            "D-JPK-REF1: `host@root` uses the text after `@` as the external config root.",
            "write `jet os switch laptop` or `jet os switch laptop@../machines`.",
        );
        return None;
    }
    let path = PathBuf::from(root);
    let config = if path.is_dir() {
        config_for_root(&path)
    } else {
        path
    };
    Some(Target {
        config,
        host: host.to_string(),
    })
}

fn config_for_root(root: &Path) -> PathBuf {
    let package = root.join(Syntax::PACKAGE_FILE);
    if package.is_file() {
        package
    } else {
        root.join(Syntax::CONFIG_FILE)
    }
}

pub(super) fn load_target(theme: &Theme, target: &Target) -> Option<(EnvPlan, SystemPlan)> {
    let plan = load_plan(theme, target)?;
    let Some(system) = plan.systems.iter().find(|s| s.name == target.host).cloned() else {
        let mut systems: Vec<String> = plan.systems.iter().map(|s| s.name.clone()).collect();
        systems.sort();
        let known = if systems.is_empty() {
            "this config defines no systems".to_string()
        } else {
            format!("available systems: {}", systems.join(", "))
        };
        theme.error_coded(
            "E0980",
            &format!("`{}` is not a system in this config", target.host),
            &known,
            "define `system.<host>: { ... }`, or select one of the systems above.",
        );
        return None;
    };
    if !validate_system_options(theme, &system) {
        return None;
    }
    Some((plan, system))
}

pub(super) fn load_user_generation_target(
    theme: &Theme,
    target: &Target,
    user: &str,
) -> Option<(EnvPlan, SystemPlan)> {
    let plan = load_plan(theme, target)?;
    let Some(system) = plan
        .systems
        .iter()
        .find(|s| user_names(s).iter().any(|name| name == user))
        .cloned()
    else {
        let mut users = plan.systems.iter().flat_map(user_names).collect::<Vec<_>>();
        users.sort();
        users.dedup();
        let known = if users.is_empty() {
            "this config defines no user generations".to_string()
        } else {
            format!("available users: {}", users.join(", "))
        };
        theme.error(
            &format!("`{user}` is not a user generation in this config"),
            &known,
            "define `user.<name>.*` or `users.<name>.*` options on a system, then rerun `jetos user plan <name>`.",
        );
        return None;
    };
    if !validate_system_options(theme, &system) {
        return None;
    }
    Some((plan, system))
}

pub(super) fn load_plan(theme: &Theme, target: &Target) -> Option<EnvPlan> {
    if target.config.file_name().and_then(|name| name.to_str()) == Some(Syntax::PACKAGE_FILE) {
        return load_package_plan(theme, target);
    }
    let src = match fs::read_to_string(&target.config) {
        Ok(src) => src,
        Err(_) => {
            theme.error_coded(
                "E0981",
                "the jetos config file does not exist",
                &format!(
                    "`jet os` tried to load `{}` for host `{}`.",
                    target.config.display(),
                    target.host
                ),
                "create the config file, or pass an explicit root after the `@`.",
            );
            return None;
        }
    };
    let source_base = std::env::var_os("JETOS_STUDIO_SOURCE_BASE").map(PathBuf::from);
    let base = source_base
        .as_deref()
        .unwrap_or_else(|| target.config.parent().unwrap_or_else(|| Path::new(".")));
    let plan = match ModuleEval::evaluate_env(&src, base) {
        Ok(plan) => plan,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(
                    &target.config.to_string_lossy(),
                    &src,
                    std::slice::from_ref(&d)
                )
            );
            return None;
        }
    };
    Some(plan)
}

fn load_package_plan(theme: &Theme, target: &Target) -> Option<EnvPlan> {
    let root = target
        .config
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let facts = match PackageFacts::load(root) {
        Some(Ok(facts)) => facts,
        Some(Err(error)) => {
            theme.error(
                "the Package graph is invalid",
                &format!(
                    "{} could not load package.jet: {error}",
                    target.config.display()
                ),
                "repair package.jet or its composed Config files, then run the command again.",
            );
            return None;
        }
        None => {
            theme.error_coded(
                "E0981",
                "the jetos package file does not exist",
                &format!(
                    "jet os tried to load {} for host {}.",
                    target.config.display(),
                    target.host
                ),
                "create package.jet, or pass an explicit root after the @.",
            );
            return None;
        }
    };
    let projection = match project_package_outputs(&facts) {
        Ok(projection) => projection,
        Err(error) => {
            theme.error(
                "the Package graph cannot project to JetOS",
                &format!("package output projection failed: {error}"),
                "repair the named System or Fleet output, then run the command again.",
            );
            return None;
        }
    };
    Some(env_plan_from_package_outputs(
        Syntax::PACKAGE_FILE,
        projection,
    ))
}

pub(super) fn validate_system_options(theme: &Theme, system: &SystemPlan) -> bool {
    if let Some(bad) = system.options.iter().find(|o| {
        let ns = o.key.split('.').next().unwrap_or("");
        !Syntax::OS_OPTION_NAMESPACES.contains(&ns)
    }) {
        theme.error_coded(
            "E1277",
            &format!("`{}` uses a retired jetos option namespace", bad.key),
            "D-JPK-OSNS1=B, D-JOS-SYSTEMTREE1=A, and Epoch 7 jetos surface decisions: jetos option keys start with full-word namespaces such as `filesystem`, `network`, `packages`, `services`, `users`, `user`, `apps`, `performance`, `storage`, `theme`, `workload`, `hardware`, `groups`, `secrets`, `boot`, `kernel`, `init`, `health`, or `deploy`.",
            "rename the option namespace, for example `net.hostName` becomes `network.hostName`.",
        );
        return false;
    }
    true
}
