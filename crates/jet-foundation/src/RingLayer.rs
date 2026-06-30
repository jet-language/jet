//! D-RINGLAYER1=A: runtime layer classification for core modules.
//!
//! Layers form a total order: `core ⊂ alloc ⊂ std`. The compiler infers a
//! package's minimum layer from `use core.*` imports and emitted helper usage,
//! and rejects imports/helpers above an optional `layer:` ceiling in `pkg.jet`.

use crate::Syntax;

/// Minimum runtime capability a package needs: heap-free core, allocator, or full OS std.
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
    pub const STD: &'static str = "std";

    /// Parse a `layer:` manifest value (`core`, `alloc`, or `std`).
    pub fn parse_manifest(value: &str) -> Option<Self> {
        match value.trim() {
            Self::CORE => Some(RuntimeLayer::Core),
            Self::ALLOC => Some(RuntimeLayer::Alloc),
            Self::STD => Some(RuntimeLayer::Std),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeLayer::Core => Self::CORE,
            RuntimeLayer::Alloc => Self::ALLOC,
            RuntimeLayer::Std => Self::STD,
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
        "core" | "core.math" | "core.science.measurement" | "core.perf" | "core.scope"
            | "core.ui"
        | "core.encoding.hex" | "core.encoding.base64" | "jet.crypto" => RuntimeLayer::Core,

        // ── alloc: heap / growable data, no direct OS I/O ──────────────────
        "core.mem" | "core.mem.alloc" | "core.random" | "core.crypto.random" | "core.uuid"
        | "core.encoding" | "core.encoding.json" | "core.encoding.csv"
        | "core.encoding.toml" | "core.encoding.yaml" | "core.args"
        | "core.async.loadable" | "jet.reactive" | "core.sketch.hll"
        | "core.sketch.tdigest" | "core.sketch.cms" | "core.sketch.reservoir" | "jet.log"
        | "jet.regex" => RuntimeLayer::Alloc,

        // ── std: OS I/O, networking, processes ─────────────────────────────
        "core.fs" | "core.io" | "core.env" | "core.process" | "core.files" | "core.path"
        | "core.net" | "core.term" | "core.time" | "core.time.date" | "core.time.datetime"
        | "core.tasks" | "jet.http" | "core.http.client" | "core.http.server" | "core.archive"
        | "jet.db" => RuntimeLayer::Std,

        // Unknown modules default to std so new OS-facing modules stay conservative.
        other if Syntax::is_known_core_module(other) => RuntimeLayer::Std,
        _ => RuntimeLayer::Std,
    }
}

/// Classify a sema/codegen helper-usage key (`core.io::input`, `core.fs::read`, …)
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

/// E1006 — a `use core.*` import or emitted helper exceeds the package `layer:` ceiling.
pub fn layer_ceiling_exceeded(
    module: &str,
    needed: RuntimeLayer,
    ceiling: RuntimeLayer,
    span: Option<crate::Diagnostics::Span>,
    import_chain: Option<&str>,
) -> crate::Diagnostics::Diagnostic {
    let mut why = format!(
        "this package declares `layer: {}` in `{}`, which caps imports at the `{}` layer; `{module}` is a `{}` module",
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
            "`{module}` needs the `{}` runtime layer",
            needed.as_str()
        ),
        why,
        format!(
            "remove the import or helper use, raise the ceiling to `layer: {}` in `{}`, or use a `{}`-layer alternative",
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
        assert_eq!(core_usage_layer("core.math::__mathtypes__"), Some(RuntimeLayer::Core));
        assert_eq!(core_usage_layer("core::json"), Some(RuntimeLayer::Alloc));
        assert_eq!(core_usage_layer("core.reactive"), Some(RuntimeLayer::Alloc));
    }
}
