//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

mod types;
mod identity;
mod entry;
mod nixos_import;
mod nixos_import_live;
mod commands;
mod vm_commands;
mod load_validate;
mod generation;
mod kernel_bootstrap;
mod store_realize;
mod generation_files;
mod studio_projection;
mod root_projection;
mod etc_boot_facts;
mod system_facts;
mod user_flatpak_perf;
mod module_storage_workload;
mod theme_fleet_lifecycle;
mod desktop_store_vm;
mod installer_media;
mod initrd_overlay;
mod iso_vm_commands;
mod vm_proof;
mod nixos_backend;
mod activation_provenance;
mod options_rendering;
mod generations_activation;

pub use entry::{main, resolve_config_path, user_main};
pub use types::OSFlags;
pub(crate) use studio_projection::studio_pages_json;
