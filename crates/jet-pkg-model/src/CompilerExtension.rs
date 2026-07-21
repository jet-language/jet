//! D-DX5-HOOK1=A (Tower #549) — compiler-extension v1 host surface.
//!
//! Reuses the shipped WASM Component Model substrate owned here
//! (`WASMTIME_CRATE_SPEC` + Component Model loader pattern in
//! `Prelude/CompilerExtension.rs`), with a **compiler-specific** WIT world
//! that stays distinct from:
//! - application `target: plugin` / `core.plugin` (world `jetplugin`, D-PLUGIN1)
//! - PATH-discovered `jet-*` helpers (D-DX5)
//!
//! V1 contract (ratified): post-sema typed read-only snapshot in → validated
//! findings/edit proposals out. The host remains the only semantic authority;
//! plugins cannot mutate compiler state or expose rustc (I2/I3).

use crate::FFI::WASMTIME_CRATE_SPEC;

/// Closed set of Jet plugin mechanisms (I8 — one semantic role each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMechanism {
    /// D-DX5: `jet-<cmd>` executables discovered on PATH.
    PathHelper,
    /// D-PLUGIN1 / D-DEP-WASM1: application `target: plugin` + `core.plugin`.
    ApplicationPlugin,
    /// D-DX5-HOOK1=A: compiler-extension WASM component (this module).
    CompilerExtension,
}

/// Wire protocol version for the typed post-sema snapshot contract.
pub const PROTOCOL_VERSION: u32 = 1;

/// First hook stage: after sema, typed facts only.
pub const STAGE: &str = "typed";

/// Component Model world name — fixed for every compiler-extension component.
/// Distinct from application plugins' fixed world `jetplugin` (D-PLUGIN-EXPORT1).
pub const WORLD_NAME: &str = "compiler-extension-v1";

/// WIT package identity for the compiler-extension world.
pub const PACKAGE_NAME: &str = "jet:compiler-extension@0.1.0";

/// Application `target: plugin` world name (D-PLUGIN1 / D-PLUGIN-EXPORT1).
/// Kept here so callers can assert the two worlds never collide (I8).
pub const APPLICATION_PLUGIN_WORLD: &str = "jetplugin";

/// Required guest export that receives the versioned snapshot and returns a
/// validated response payload (findings / proposed edits as opaque bytes in
/// the host runtime; typed decode is host-owned).
pub const ANALYZE_EXPORT: &str = "analyze";

/// Capabilities a v1 component may negotiate. Later stages extend this set
/// rather than inventing a second plugin system (D-DX5-HOOK1 hybrid law).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    ReadTypes,
    ReadSymbols,
    ReadEffects,
    ReadSpans,
    ReadProvenance,
    EmitFinding,
    ProposeEdit,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::ReadTypes => "read_types",
            Capability::ReadSymbols => "read_symbols",
            Capability::ReadEffects => "read_effects",
            Capability::ReadSpans => "read_spans",
            Capability::ReadProvenance => "read_provenance",
            Capability::EmitFinding => "emit_finding",
            Capability::ProposeEdit => "propose_edit",
        }
    }

    /// V1 floor: typed observation + findings/edits.
    pub fn v1_defaults() -> &'static [Capability] {
        &[
            Capability::ReadTypes,
            Capability::ReadSymbols,
            Capability::ReadEffects,
            Capability::ReadSpans,
            Capability::ReadProvenance,
            Capability::EmitFinding,
            Capability::ProposeEdit,
        ]
    }
}

/// This host's mechanism identity.
pub fn mechanism() -> PluginMechanism {
    PluginMechanism::CompilerExtension
}

/// Same wasmtime crate pin as application `core.plugin` (D-DEP-WASM1=A).
/// Compiler-extension host must not invent a second loader dependency.
pub fn wasm_substrate_crate_spec() -> (&'static str, &'static str) {
    WASMTIME_CRATE_SPEC
}

/// Hand-written wasmtime Component Model host runtime (include_str substrate,
/// same ownership pattern as `Prelude/Plugin.rs` for application plugins).
pub fn runtime_source() -> &'static str {
    include_str!("Prelude/CompilerExtension.rs")
}

/// Canonical `.wit` world text for a compiler-extension component.
/// Snapshot and response travel as `list<u8>` so the wire schema can version
/// independently of the WIT shape; the host validates decoded payloads.
pub fn wit_world() -> String {
    format!(
        "package {PACKAGE_NAME};\n\n\
         world {WORLD_NAME} {{\n\
         \t/// Versioned typed post-sema snapshot bytes (host-owned schema).\n\
         \texport {ANALYZE_EXPORT}: func(snapshot: list<u8>) -> list<u8>;\n\
         }}\n"
    )
}

/// True when `world` names the application-plugin world, not this host.
pub fn is_application_plugin_world(world: &str) -> bool {
    world == APPLICATION_PLUGIN_WORLD
}

/// True when `world` names this compiler-extension world.
pub fn is_compiler_extension_world(world: &str) -> bool {
    world == WORLD_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dx5_hook1_world_is_distinct_from_application_plugin_and_path_helpers() {
        assert_eq!(mechanism(), PluginMechanism::CompilerExtension);
        assert_ne!(WORLD_NAME, APPLICATION_PLUGIN_WORLD);
        assert!(is_compiler_extension_world(WORLD_NAME));
        assert!(is_application_plugin_world(APPLICATION_PLUGIN_WORLD));
        assert!(!is_compiler_extension_world(APPLICATION_PLUGIN_WORLD));
        assert!(!is_application_plugin_world(WORLD_NAME));
        // PATH jet-* is a different mechanism entirely (D-DX5), not a WIT world.
        assert_ne!(mechanism(), PluginMechanism::PathHelper);
        assert_ne!(mechanism(), PluginMechanism::ApplicationPlugin);
    }

    #[test]
    fn host_reuses_jet_pkg_model_wasmtime_substrate() {
        assert_eq!(wasm_substrate_crate_spec(), WASMTIME_CRATE_SPEC);
        assert_eq!(wasm_substrate_crate_spec(), ("wasmtime", "26"));
        let runtime = runtime_source();
        assert!(
            runtime.contains("wasmtime::component"),
            "compiler-extension host must use the Component Model substrate"
        );
        assert!(
            runtime.contains("jet_compiler_extension_load"),
            "host entry must be compiler-extension-specific"
        );
        assert!(
            !runtime.contains("jet_plugin_load"),
            "must not reuse application core.plugin entry points"
        );
        assert!(
            runtime.contains(WORLD_NAME),
            "runtime must name the compiler-extension world"
        );
        assert!(
            !runtime.contains("\"jetplugin\""),
            "runtime must not bind the application plugin world name"
        );
    }

    #[test]
    fn v1_wit_world_and_protocol_match_dx5_hook1() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(STAGE, "typed");
        let wit = wit_world();
        assert!(wit.contains(&format!("package {PACKAGE_NAME};")));
        assert!(wit.contains(&format!("world {WORLD_NAME}")));
        assert!(wit.contains(&format!("export {ANALYZE_EXPORT}:")));
        assert!(wit.contains("list<u8>"));
        assert!(!wit.contains("world jetplugin"));
        assert!(Capability::v1_defaults().contains(&Capability::ReadTypes));
        assert!(Capability::v1_defaults().contains(&Capability::EmitFinding));
        assert_eq!(Capability::ReadSymbols.as_str(), "read_symbols");
    }
}
