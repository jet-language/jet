use super::commands::{
    cmd_build, cmd_check, cmd_generations, cmd_image, cmd_init, cmd_lift, cmd_migrate, cmd_plan,
    cmd_proof, cmd_rollback,
};
use super::generation::build_generation;
use super::generations_activation::{
    activate_generation, find_rollback_generation, latest_generation_for, print_help,
};
use super::load_validate::load_user_profile_target;
use super::nixos_import::cmd_import;
use super::options_rendering::render_user_profile_json;
use super::types::{OsFlags, Target};
use super::vm_commands::cmd_vm;
use crate::Output::Theme;
use crate::Syntax;
use crate::JSON;
use std::fs;
use std::path::PathBuf;

pub fn main(theme: &Theme, verb: Option<&str>, args: &[String], flags: &OsFlags) -> i32 {
    match verb {
        Some(v) if v == Syntax::OS_VERB_CHECK => cmd_check(theme, args),
        Some(v) if v == Syntax::OS_VERB_PLAN => cmd_plan(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_PROOF => cmd_proof(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_BUILD => cmd_build(theme, args, flags, false),
        Some(v) if v == Syntax::OS_VERB_SWITCH => cmd_build(theme, args, flags, true),
        Some(v) if v == Syntax::OS_VERB_ROLLBACK => cmd_rollback(theme, args),
        Some(v) if v == Syntax::OS_VERB_GENERATIONS => cmd_generations(args),
        Some(v) if v == Syntax::OS_VERB_INIT => cmd_init(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_LIFT => cmd_lift(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_IMPORT => cmd_import(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_MIGRATE => cmd_migrate(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_IMAGE => cmd_image(theme, args, flags),
        Some(v) if v == Syntax::OS_VERB_VM => cmd_vm(theme, args, flags),
        Some("help" | "--help" | "-h") => {
            print_help();
            0
        }
        Some(other) => {
            theme.error(
                &format!("`{other}` is not a jetos verb"),
                &format!("jetos verbs are: {}.", Syntax::OS_VERBS.join(", ")),
                "run `jet os help`.",
            );
            2
        }
        None => {
            print_help();
            2
        }
    }
}

pub fn user_main(theme: &Theme, verb: Option<&str>, args: &[String], flags: &OsFlags) -> i32 {
    let Some(action) = verb else {
        theme.error(
            "user needs an action",
            "D-JOS-USERAPPLY1=A: standalone user profiles support plan, build, switch, rollback, and prove.",
            "run `jetos user plan <name>`.",
        );
        return 2;
    };
    if !Syntax::USER_VERBS.contains(&action) {
        theme.error(
            &format!("`{action}` is not a jetos user action"),
            &format!("jetos user actions are: {}.", Syntax::USER_VERBS.join(", ")),
            "run `jetos user plan <name>`.",
        );
        return 2;
    }
    let user = args.first().map_or("", String::as_str);
    if user.is_empty() {
        theme.error(
            "user action needs a profile name",
            "D-JOS-USERENV1=A: `user.<name>` or `users.<name>` declares a per-user environment profile.",
            "run `jetos user plan nate`.",
        );
        return 2;
    }
    let target = Target {
        config: default_config_path(),
        host: user.to_string(),
    };
    let Some((plan, system)) = load_user_profile_target(theme, &target, user) else {
        return 2;
    };
    match action {
        "plan" => {
            let profile = render_user_profile_json(&system, user);
            if flags.json {
                println!("{profile}");
            } else {
                theme.ok(&format!("jetos user plan for {}", theme.bold(user)));
                println!("{profile}");
            }
            0
        }
        "build" => {
            let Some(gen) = build_generation(theme, &plan, &system, flags, &target.config) else {
                return 2;
            };
            theme.ok(&format!(
                "built user {} generation {}",
                theme.bold(user),
                theme.bold(&gen.name)
            ));
            theme.detail(&format!("{}", gen.path.join("users").join(user).display()));
            0
        }
        "switch" => {
            let Some(gen) = build_generation(theme, &plan, &system, flags, &target.config) else {
                return 2;
            };
            if let Err(e) = activate_generation(&gen) {
                theme.error(
                    "could not activate the user generation",
                    &format!("updating the current/default pointers failed: {e}"),
                    "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                );
                return 2;
            }
            theme.ok(&format!(
                "activated user {} generation {}",
                theme.bold(user),
                theme.bold(&gen.name)
            ));
            0
        }
        "rollback" => {
            let requested = args.get(1).map(String::as_str);
            let Some(gen) = find_rollback_generation(&system.name, requested) else {
                theme.error(
                    "no user generation is available for rollback",
                    &format!(
                        "no recorded jetos generation with profile `{user}` exists for `{}`.",
                        system.name
                    ),
                    "run `jetos user build <name>` or `jetos user switch <name>` first.",
                );
                return 2;
            };
            match activate_generation(&gen) {
                Ok(()) => {
                    theme.ok(&format!(
                        "rolled back user {user} to {}",
                        theme.bold(&gen.name)
                    ));
                    0
                }
                Err(e) => {
                    theme.error(
                        "could not activate the rollback generation",
                        &format!("updating the current/default pointers failed: {e}"),
                        "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                    );
                    2
                }
            }
        }
        "prove" => {
            let Some(gen) = latest_generation_for(&system.name) else {
                theme.error(
                    "jetos user proof is missing",
                    &format!(
                        "no built generation exists for profile `{user}` on `{}`.",
                        system.name
                    ),
                    "run `jetos user build <name>` first.",
                );
                return 2;
            };
            let profile = gen.path.join("users").join(user).join("profile.json");
            let proof = gen.path.join("users").join(user).join("proof.txt");
            if !profile.is_file() || !proof.is_file() {
                theme.error(
                    "jetos user proof is incomplete",
                    &format!(
                        "generation `{}` lacks the profile/proof files for `{user}`.",
                        gen.name
                    ),
                    "rebuild the user generation.",
                );
                return 2;
            }
            if flags.json {
                println!(
                    "{{\"user\":{},\"host\":{},\"generation\":{},\"profile\":{},\"proof\":{}}}",
                    JSON::quote(user),
                    JSON::quote(&system.name),
                    JSON::quote(&gen.name),
                    JSON::quote(&profile.display().to_string()),
                    JSON::quote(&proof.display().to_string())
                );
            } else {
                theme.ok(&format!(
                    "jetos user proof for {user} generation {}",
                    gen.name
                ));
                println!("{}", fs::read_to_string(proof).unwrap_or_default());
            }
            0
        }
        _ => unreachable!("checked user action"),
    }
}

pub fn resolve_config_path(prefix: Option<&str>) -> PathBuf {
    match prefix {
        Some("") | None => default_config_path(),
        Some(path) => PathBuf::from(path),
    }
}

pub(super) fn default_config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(Syntax::CONFIG_FILE)
}
