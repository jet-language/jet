//! jetos realization tier (Epoch 7).
//!
//! `jet os` is the user-facing command. The implementation lives in the
//! Jetpack engine because it reuses Jetpack's source table, provider boundary,
//! hangar, and trust/runtime policy.

mod activation_provenance;
mod commands;
mod desktop_store_vm;
mod entry;
mod etc_boot_facts;
mod generation;
mod generation_files;
mod generations_activation;
mod identity;
mod initrd_overlay;
mod installer_media;
mod iso_vm_commands;
mod kernel_bootstrap;
mod load_validate;
mod module_storage_workload;
mod nixos_backend;
mod nixos_import;
mod nixos_import_live;
mod options_rendering;
mod root_projection;
mod store_realize;
mod studio_projection;
mod system_facts;
mod theme_fleet_lifecycle;
mod types;
mod user_flatpak_perf;
mod vm_commands;
mod vm_proof;

pub use entry::{main, resolve_config_path, user_main};
pub(crate) use studio_projection::studio_pages_json;
pub use types::OSFlags;
