//! Jetpack Phase 1 command dispatch (D-JPK2/9).
//!
//! `jetpack run/build/list/clean/add/remove`. Independent from the `jet`
//! binary (D-JPK1). All user-facing output flows through `Output::Theme`.

use super::Bridge;
use super::BuildDebug;
use super::Components;
use super::Discovery;
use super::Doctor;
use super::Image;
use super::ManifestTOML;
use super::Output::{self, Theme};
use super::Overlay;
use super::Provider::{self, ProviderError};
use super::RefSpec::{self, ProviderKind};
use super::RuntimePolicy;
use super::Secrets;
use super::Services;
use super::Shell::{self, Env, ShellKind};
use super::Store::{self, Roots};
use super::Trust;
use super::{
    EnvFile, ModuleEval, RefSpec::RefError, SemanticLock, WorkspaceFile, WorkspaceLock, JSON,
};
use crate::{Lock, Syntax};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

include!("CLI/parse.rs");
include!("CLI/workspace_sources.rs");
include!("CLI/realize.rs");
include!("CLI/run_enter_dev.rs");
include!("CLI/services_secrets_config.rs");
include!("CLI/trust_env_build.rs");
include!("CLI/package_hangar_vendor.rs");
include!("CLI/update_search_info.rs");
include!("CLI/add_remove_push_image.rs");
include!("CLI/bridge_os_studio.rs");
include!("CLI/studio_server.rs");
include!("CLI/studio_transactions.rs");
include!("CLI/usage_tests.rs");

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
