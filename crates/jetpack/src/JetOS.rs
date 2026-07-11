//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

use super::ModuleEval::{self, EnvPlan, ImageKind, ServicePlan, SystemPlan, VmTestPlan};
use super::Output::Theme;
use super::{Provider, RefSpec, Store, JSON};
use crate::Syntax;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

include!("JetOS/types.rs");
include!("JetOS/identity.rs");
include!("JetOS/entry.rs");
include!("JetOS/nixos_import.rs");
include!("JetOS/nixos_import_live.rs");
include!("JetOS/commands.rs");
include!("JetOS/vm_commands.rs");
include!("JetOS/load_validate.rs");
include!("JetOS/generation.rs");
include!("JetOS/kernel_bootstrap.rs");
include!("JetOS/store_realize.rs");
include!("JetOS/generation_files.rs");
include!("JetOS/studio_projection.rs");
include!("JetOS/root_projection.rs");
include!("JetOS/etc_boot_facts.rs");
include!("JetOS/system_facts.rs");
include!("JetOS/user_flatpak_perf.rs");
include!("JetOS/module_storage_workload.rs");
include!("JetOS/theme_fleet_lifecycle.rs");
include!("JetOS/desktop_store_vm.rs");
include!("JetOS/installer_media.rs");
include!("JetOS/initrd_overlay.rs");
include!("JetOS/iso_vm_commands.rs");
include!("JetOS/vm_proof.rs");
include!("JetOS/nixos_backend.rs");
include!("JetOS/activation_provenance.rs");
include!("JetOS/options_rendering.rs");
include!("JetOS/generations_activation.rs");
