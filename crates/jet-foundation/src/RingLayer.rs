//! D-RINGLAYER1=A: runtime layer classification for core modules.
//!
//! Profiles form a total order: `core ⊂ alloc ⊂ hosted`. The compiler infers a
//! package's minimum runtime profile from `use core.*` imports and emitted helper usage,
//! and rejects imports/helpers above an optional `runtime:` ceiling in `pkg.jet`.

use crate::Syntax;

/// Minimum runtime capability a package needs: heap-free core, allocator, or hosted OS runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RuntimeLayer {
    #[default]
    Core = 0,
    Alloc = 1,
    Std = 2,
}

impl RuntimeLayer {
    pub const CORE: &'static str = "core";
    pub const ALLOC: &'static str = "alloc";
    pub const HOSTED: &'static str = "hosted";

    /// Parse a `runtime:` manifest value (`core`, `alloc`, or `hosted`).
    pub fn parse_manifest(value: &str) -> Option<Self> {
        match value.trim() {
            Self::CORE => Some(RuntimeLayer::Core),
            Self::ALLOC => Some(RuntimeLayer::Alloc),
            Self::HOSTED => Some(RuntimeLayer::Std),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeLayer::Core => Self::CORE,
            RuntimeLayer::Alloc => Self::ALLOC,
            RuntimeLayer::Std => Self::HOSTED,
        }
    }
}

/// Classify a compiler-known core module path to its minimum runtime layer.
/// Accepts user-facing `core.*` and legacy internal `jet.*` ring keys.
pub fn core_module_layer(module: &str) -> Option<RuntimeLayer> {
    let key = Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    Some(layer_of_normalized(&key))
}

fn layer_of_normalized(module: &str) -> RuntimeLayer {
    match module {
        // ── core: no heap, no OS ─────────────────────────────────────────
        "core"
        | "core.math"
        | "core.science.measurement"
        | "core.perf"
        | "core.scope"
        | "core.ui"
        | "core.web"
        | "core.web.storage"
        | "core.web.storage.local"
        | "core.web.storage.session"
        | "core.encoding.hex"
        | "core.encoding.base64"
        | "jet.crypto" => RuntimeLayer::Core,

        // ── alloc: heap / growable data, no direct OS I/O ──────────────────
        "core.mem"
        | "core.mem.alloc"
        | "core.random"
        | "core.crypto.random"
        | "core.uuid"
        | "core.encoding"
        | "core.encoding.json"
        | "core.encoding.csv"
        | "core.encoding.toml"
        | "core.encoding.yaml"
        | "core.text.unicode"
        | "core.args"
        | "core.reflect"
        | "core.game"
        | "core.async.loadable"
        | "core.event"
        | "core.solve"
        | "core.time.expiring"
        | "core.secrets"
        | "jet.reactive"
        | "core.sketch.hll"
        | "core.sketch.tdigest"
        | "core.sketch.cms"
        | "core.sketch.reservoir"
        | "jet.log"
        | "jet.regex" => RuntimeLayer::Alloc,

        // ── hosted: OS I/O, networking, processes ──────────────────────────
        "core.io" | "core.env" | "core.process" | "core.files" | "core.path"
        | "core.net" | "core.term" | "core.time" | "core.time.date" | "core.time.datetime"
        | "core.tasks" | "jet.http" | "core.http.client" | "core.http.server" | "core.archive"
        | "core.raylib" | "core.compress.gzip" | "core.compress.zstd" | "jet.db"
        // D-DEP-WASM1=A (c81): the plugin loader embeds wasmtime — same OS-facing
        // posture as jet.db's embedded rusqlite.
        | "jet.plugin" => RuntimeLayer::Std,

        // Unknown modules default to std so new OS-facing modules stay conservative.
        other if Syntax::is_known_core_module(other) => RuntimeLayer::Std,
        _ => RuntimeLayer::Std,
    }
}

/// Classify a sema/codegen helper-usage key (`core.io::input`, `core.files::read`, …)
/// to its minimum runtime layer.
pub fn core_usage_layer(usage: &str) -> Option<RuntimeLayer> {
    if let Some(rest) = usage.strip_prefix("core::") {
        return Some(match rest {
            "json" => RuntimeLayer::Alloc,
            "bytes" => RuntimeLayer::Alloc,
            "from_bytes" | "to_u8" => RuntimeLayer::Core,
            "elapsed_millis" => RuntimeLayer::Std,
            _ => RuntimeLayer::Std,
        });
    }
    let (module, _) = usage.split_once("::").unwrap_or((usage, ""));
    core_module_layer(module)
}

/// E1006 — a `use core.*` import or emitted helper exceeds the package `runtime:` ceiling.
pub fn layer_ceiling_exceeded(
    module: &str,
    needed: RuntimeLayer,
    ceiling: RuntimeLayer,
    span: Option<crate::Diagnostics::Span>,
    import_chain: Option<&str>,
) -> crate::Diagnostics::Diagnostic {
    let mut why = format!(
        "this package declares `runtime: {}` in `{}`, which caps imports at the `{}` runtime profile; `{module}` needs `{}`",
        ceiling.as_str(),
        Syntax::PAYLOAD_FILE,
        ceiling.as_str(),
        needed.as_str(),
    );
    if let Some(chain) = import_chain {
        why.push_str(&format!("; import chain: {chain}"));
    }
    crate::Diagnostics::Diagnostic::error(
        "E1006",
        format!(
            "`{module}` needs `runtime: {}`",
            needed.as_str()
        ),
        why,
        format!(
            "remove the import or helper use, raise the ceiling to `runtime: {}` in `{}`, or use a `{}` runtime alternative",
            needed.as_str(),
            Syntax::PAYLOAD_FILE,
            ceiling.as_str(),
        ),
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Syntax::KNOWN_CORE_MODULES;

    #[test]
    fn every_known_core_module_has_layer() {
        for &name in KNOWN_CORE_MODULES {
            assert!(
                core_module_layer(name).is_some(),
                "missing layer for {name}"
            );
        }
    }

    #[test]
    fn ring_imports_normalize_to_layer() {
        assert_eq!(core_module_layer("core.log"), Some(RuntimeLayer::Alloc));
        assert_eq!(core_module_layer("core.http"), Some(RuntimeLayer::Std));
        assert_eq!(core_module_layer("core.math"), Some(RuntimeLayer::Core));
    }

    #[test]
    fn helper_usage_keys_map_to_layer() {
        assert_eq!(core_usage_layer("core.io::input"), Some(RuntimeLayer::Std));
        assert_eq!(
            core_usage_layer("core.math::__mathtypes__"),
            Some(RuntimeLayer::Core)
        );
        assert_eq!(core_usage_layer("core::json"), Some(RuntimeLayer::Alloc));
        assert_eq!(core_usage_layer("core.reactive"), Some(RuntimeLayer::Alloc));
    }
}
