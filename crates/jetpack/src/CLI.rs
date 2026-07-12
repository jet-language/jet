//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `Output::Theme`.

use super::Doctor;
use super::Output::Theme;
use std::path::PathBuf;

mod parse;
mod workspace_sources;
mod realize;
mod run_enter_dev;
mod services_secrets_config;
mod trust_env_build;
mod package_hangar_vendor;
mod update_search_info;
mod add_remove_push_image;
mod bridge_os_studio;
mod studio_server;
mod studio_transactions;
mod usage_tests;

use parse::Parsed;

pub use parse::main;
pub(crate) use realize::report_provider_error;

fn cmd_doctor(_theme: &Theme, parsed: &Parsed) -> i32 {
    if !parsed.positional.is_empty() || parsed.command.is_some() {
        eprintln!("jetpack doctor takes no positional arguments");
        return 2;
    }
    let report = Doctor::run(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        parsed.flags.online && !parsed.flags.offline,
    );
    if parsed.flags.json {
        println!("{}", report.to_json());
    } else {
        eprint!("{}", report.to_human());
    }
    report.exit_code()
}
