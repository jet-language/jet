//! Compiler output types.
use crate::Diagnostics::Diagnostic;
use crate::Lock;
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
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs seen during sema.
    /// Each entry: relative path + sha256 of the bytes at compile time.
    /// Written to `.jet/lock` by the build driver for reproducibility.
    pub comptime_inputs: Vec<Lock::ComptimeInput>,
    /// D-WEBBACKEND1 (c123 M2): web target artifacts when `--target=web`.
    pub web: Option<crate::Codegen::WebArtifacts>,
    /// D-WASM1: partition report when `--target=web`.
    pub web_partition_report: Option<String>,
    /// D-PLUGIN1=B / D-DEP-WASM1=A (c81): plugin guest artifacts when
    /// `--target=sandbox` (the `.wit` world + wasm32 guest Rust source).
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

