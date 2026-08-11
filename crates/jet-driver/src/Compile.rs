//! Compiler output types: CompileOutput, Capabilities, bundle_uses_unsafe.
use crate::Diagnostics::Diagnostic;
use crate::Lock;
use crate::AST;
use crate::FFI;

/// Result of a successful compile: generated Rust plus any lint warnings.
#[derive(Debug)]
pub struct CompileOutput {
    pub rust: String,
    pub lints: Vec<Diagnostic>,
    /// Built FFI bridge when the program declares `extern rust` (M7).
    pub ffi: Option<FFI::FfiLink>,
    /// Native C-library linker args (S59 / E2-M14), ready for `rustc`.
    pub clinks: Vec<String>,
    /// D-TOOL5 (E2-M11): capability flags inferred from the generated code.
    pub capabilities: Capabilities,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs seen during sema.
    /// Each entry: relative path + sha256 of the bytes at compile time.
    /// Written to `.jet/lock` by the build driver for reproducibility.
    pub comptime_inputs: Vec<Lock::ComptimeInput>,
    /// D-WEBBACKEND1 (c123 M2): web target artifacts when `--target=web`.
    pub web: Option<crate::Codegen::WebArtifacts>,
    /// D-WASM1: partition report when `--target=web`.
    pub web_partition_report: Option<String>,
    /// D-PLUGIN1=B / D-DEP-WASM1=A (c81): plugin guest artifacts when
    /// `--target=plugin` (the `.wit` world + wasm32 guest Rust source).
    pub plugin: Option<crate::Codegen::PluginArtifacts>,
    /// D-LIB-EXPORT1=C: native Library Rust wrappers and foreign projections.
    pub library: Option<crate::Codegen::LibraryArtifacts>,
    /// D-LIB-NAME1=A / D-LIB-DYNTRUST1=A: the checked output configuration
    /// carried beside the generated projections for the native build adapter.
    pub library_config: Option<crate::LibraryExport::LibraryConfig>,
    /// D-RINGLAYER1=A M2: minimum runtime profile inferred from imports + helpers.
    pub inferred_layer: crate::Syntax::RuntimeLayer,
    /// D-RINGLAYER1=A: optional `runtime:` ceiling from `pkg.jet`.
    pub layer_ceiling: Option<crate::Syntax::RuntimeLayer>,
}

/// D-TOOL5 (E2-M11, ratified as option C): capability summary emitted by `jet build`.
#[derive(Debug, Default)]
pub struct Capabilities {
    pub uses_network: bool,
    pub uses_file_io: bool,
    pub uses_unsafe: bool,
    pub uses_ffi: bool,
    pub uses_crypto: bool,
    pub uses_concurrency: bool,
}

impl Capabilities {
    pub fn from_sema(
        used_core: &std::collections::HashSet<String>,
        has_unsafe: bool,
        has_ffi: bool,
    ) -> Self {
        let any = |prefixes: &[&str]| {
            used_core
                .iter()
                .any(|k| prefixes.iter().any(|p| k.starts_with(p)))
        };
        Capabilities {
            uses_network: any(&["core.net", "core.http", "core.watcher::port"]),
            uses_file_io: any(&["core.io", "core.files", "core.path", "core.watcher"]),
            uses_unsafe: has_unsafe || any(&["core.mem"]),
            uses_ffi: has_ffi,
            uses_crypto: any(&["core.crypto", "core.auth"]),
            uses_concurrency: any(&["core.tasks", "core.time", "core.watcher"]),
        }
    }

    pub fn from_rust(rust: &str) -> Self {
        Capabilities {
            uses_network: rust.contains("jet_net_")
                || rust.contains("jet_http_")
                || rust.contains("jet_watcher_port"),
            uses_file_io: rust.contains("jet_fs_")
                || rust.contains("jet_io_")
                || rust.contains("jet_watcher_"),
            uses_unsafe: rust.contains("unsafe {") || rust.contains("unsafe fn"),
            uses_ffi: rust.contains("extern \"C\"") || rust.contains("jet_ffi"),
            uses_crypto: rust.contains("jet_crypto_"),
            uses_concurrency: rust.contains("jet_tasks_")
                || rust.contains("jet_time_")
                || rust.contains("jet_watcher_"),
        }
    }

    pub fn summary(&self) -> String {
        let mut caps = Vec::new();
        if self.uses_network {
            caps.push("network");
        }
        if self.uses_file_io {
            caps.push("file-io");
        }
        if self.uses_crypto {
            caps.push("crypto");
        }
        if self.uses_concurrency {
            caps.push("concurrency");
        }
        if self.uses_ffi {
            caps.push("ffi");
        }
        if self.uses_unsafe {
            caps.push("unsafe");
        }
        if caps.is_empty() {
            "capabilities: none".to_string()
        } else {
            format!("capabilities: {}", caps.join(", "))
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"network\":{},\"file_io\":{},\"unsafe\":{},\"ffi\":{},\"crypto\":{},\"concurrency\":{}}}",
            self.uses_network,
            self.uses_file_io,
            self.uses_unsafe,
            self.uses_ffi,
            self.uses_crypto,
            self.uses_concurrency,
        )
    }
}

/// True when the program contains an `#Unsafe fn` anywhere.
pub fn bundle_uses_unsafe(bundle: &AST::ProgramBundle) -> bool {
    use AST::Item;
    bundle.modules.iter().any(|m| {
        m.items.iter().any(|it| match it {
            Item::Func(f) => f.is_unsafe,
            Item::Struct(s) => {
                s.methods.iter().any(|x| x.is_unsafe)
                    || s.trait_impls
                        .iter()
                        .any(|b| b.methods.iter().any(|x| x.is_unsafe))
            }
            Item::Enum(e) => {
                e.methods.iter().any(|x| x.is_unsafe)
                    || e.trait_impls
                        .iter()
                        .any(|b| b.methods.iter().any(|x| x.is_unsafe))
            }
            Item::Impl(i) => i.methods.iter().any(|x| x.is_unsafe),
            _ => false,
        })
    })
}
