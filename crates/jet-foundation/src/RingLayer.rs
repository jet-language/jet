//! D-RINGLAYER1=A: runtime layer classification for core modules.
//!
//! Profiles form a total order: `core ⊂ alloc ⊂ hosted`. The compiler infers a
//! package's minimum runtime profile from `use core.*` imports and emitted helper usage,
//! and rejects imports/helpers above an optional `runtime:` ceiling in
//! `package.jet` (or explicit migration input).

use crate::Syntax;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Semantic edges in the emitted Prelude closure.
///
/// The graph lives beside `RuntimeLayer` so sema, target admission, and every
/// code-generation tier use one dependency authority. A namespace is not
/// automatically a dependency: pure URL and calendar helpers stop at their
/// own semantic part instead of inheriting an unrelated hosted service.
// BEGIN GENERATED CORE DEPENDENCIES
// Source: crates/jet-codegen/src/Prelude/Core.jet
// Source SHA-256: ff34cae9f8bde523fddf6a8095ff3e6daf64e808173032bb77cd388c0b79dd9a
const PRELUDE_DEPENDENCY_EDGES: &[(&str, &[&str])] = &[
    ("app", &["core.web"]),
    ("core.devtools", &["core"]),
    ("core.archive", &["core.encoding"]),
    ("core.archive.gzip", &["core.archive"]),
    ("core.archive.zstd", &["core.archive"]),
    ("core.args", &["core.text"]),
    ("core.auth", &["core.crypto", "core.net"]),
    ("core.compiler", &["core.text"]),
    ("core.compiler.lang", &["core.compiler"]),
    ("core.compute", &["core.math", "core.mem"]),
    ("core.compute.solve", &["core.compute"]),
    ("core.crypto", &["core"]),
    ("core.crypto.random", &["core.crypto"]),
    ("core.crypto.uuid", &["core.crypto", "core.encoding.hex"]),
    ("core.crypto.vault", &["core.crypto"]),
    ("core.data", &["core.encoding", "core.files"]),
    ("core.data.arrow", &["core.data"]),
    ("core.data.loader", &["core.data"]),
    ("core.data.stream", &["core.data.loader"]),
    ("core.data.plot", &["core.data"]),
    ("core.data.sketch.cms", &["core.data"]),
    ("core.data.sketch.hll", &["core.data"]),
    ("core.data.sketch.reservoir", &["core.data"]),
    ("core.data.sketch.tdigest", &["core.data"]),
    ("core.db", &["core.files", "core.net"]),
    ("core.email", &["core.net", "core.text"]),
    ("core.encoding", &["core"]),
    ("core.encoding.base32", &["core.encoding"]),
    ("core.encoding.base64", &["core.encoding"]),
    ("core.encoding.cbor", &["core.encoding"]),
    ("core.encoding.csv", &["core.encoding"]),
    ("core.encoding.hex", &["core.encoding"]),
    ("core.encoding.json", &["core.encoding"]),
    ("core.encoding.jsonl", &["core.encoding"]),
    ("core.encoding.toml", &["core.encoding"]),
    ("core.encoding.xml", &["core.encoding"]),
    ("core.encoding.yaml", &["core.encoding"]),
    ("core.event", &["core.mem"]),
    ("core.files", &["core.text"]),
    ("core.font", &["core.ui"]),
    ("core.game", &["core.math", "core.mem"]),
    ("core.game.raylib", &["core.game"]),
    ("core.http", &["core.net", "core.text"]),
    ("core.http.client", &["core.http"]),
    ("core.http.server", &["core.http"]),
    ("core.log", &["core.text"]),
    ("core.math.random", &["core.math"]),
    ("core.mem", &["core"]),
    ("core.mod", &["core.compiler", "core.files"]),
    ("core.net", &["core.text"]),
    ("core.net.tls", &["core.crypto.random", "core.net"]),
    ("core.net.ws", &["core.net"]),
    ("core.plugin", &["core.files", "core.process"]),
    ("core.prelude", &["core"]),
    ("core.process", &["core.args", "core.term"]),
    ("core.reactive", &["core.mem"]),
    ("core.reactive.loadable", &["core.reactive"]),
    ("core.reflect", &["core.text"]),
    ("core.regex", &["core.text"]),
    ("core.service", &["core.net", "core.tasks"]),
    ("core.sync", &["core.data", "core.tasks"]),
    ("core.sys", &["core.text"]),
    ("core.tasks", &["core.time"]),
    ("core.term", &["core.text"]),
    ("core.testing", &["core.files", "core.math.random", "core.time"]),
    ("core.text", &["core"]),
    ("core.text.fmt", &["core.text"]),
    ("core.time", &["core"]),
    ("core.ui", &["core.text"]),
    ("core.tui", &["core.ui", "core.text"]),
    ("core.ui.host", &["core.ui", "core.files"]),
    ("core.ui.host.clipboard", &["core.ui.host"]),
    ("core.ui.host.ime", &["core.ui.host"]),
    ("core.ui.host.drag_drop", &["core.ui.host"]),
    ("core.ui.host.shortcuts", &["core.ui.host"]),
    ("core.ui.host.accessibility", &["core.ui.host"]),
    ("core.units", &["core.math"]),
    ("core.watcher", &["core.files", "core.process"]),
    ("core.web", &["core.http"]),
    ("core.web.browser", &["core.web"]),
    ("core.web.devserver", &["core.web"]),
    ("core.web.forms", &["core.web"]),
    ("core.web.query", &["core.web"]),
    ("core.web.router", &["core.web"]),
    ("core.web.storage", &["core.web"]),
    ("core.web.storage.local", &["core.web.storage"]),
    ("core.web.storage.session", &["core.web.storage"]),
    ("core.web.store", &["core.web"]),
    ("core.web.table", &["core.web"]),
    ("core.web.virtual", &["core.web"]),
];

const PRELUDE_NAMESPACE_ONLY: &[&str] = &[
    "core.mem.scope",
    "core.net.mime",
    "core.net.url",
    "core.time.expiring",
];
// END GENERATED CORE DEPENDENCIES

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
/// One source-level runtime fact for the generated Prelude closure.
///
/// The registry is deliberately separate from semantic module classification:
/// source names identify the implementation that was emitted, while
/// `RuntimeLayer` remains the admission fact. `shared` marks a source whose
/// semantic kernel is consumed by both hosted and portable emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PreludeSourceFact {
    pub name: &'static str,
    pub layer: RuntimeLayer,
    pub shared: bool,
}

impl PreludeSourceFact {
    pub const fn new(name: &'static str, layer: RuntimeLayer, shared: bool) -> Self {
        Self {
            name,
            layer,
            shared,
        }
    }
}

/// Canonical source registry for the generated runtime closure. A source may
/// only be emitted when its checked artifact layer admits it; unknown source
/// names are intentionally absent instead of being inferred from a target
/// triple or module spelling.
pub const PRELUDE_SOURCE_REGISTRY: &[PreludeSourceFact] = &[
    PreludeSourceFact::new("Prelude/Core/PortableCore.rs", RuntimeLayer::Core, true),
    PreludeSourceFact::new(
        "Prelude/Core/EmbeddedHardware.rs::portable",
        RuntimeLayer::Core,
        true,
    ),
    PreludeSourceFact::new("Prelude/PortableAlloc.rs", RuntimeLayer::Alloc, true),
    PreludeSourceFact::new("Prelude/TargetAdapters.rs", RuntimeLayer::Core, true),
    PreludeSourceFact::new("Prelude/Core.rs", RuntimeLayer::Std, false),
    PreludeSourceFact::new("Prelude/ProgramAllocator.rs", RuntimeLayer::Std, false),
    PreludeSourceFact::new("Prelude/Scheduler.rs", RuntimeLayer::Std, false),
    PreludeSourceFact::new("Prelude/Core/EmbeddedHardware.rs::host", RuntimeLayer::Std, false),
];

/// Look up a source-level runtime fact without applying a fallback policy.
pub fn prelude_source_fact(name: &str) -> Option<PreludeSourceFact> {
    PRELUDE_SOURCE_REGISTRY
        .iter()
        .copied()
        .find(|fact| fact.name == name)
}

/// Return the admitted layer for one registered source.
pub fn prelude_source_layer(name: &str) -> Option<RuntimeLayer> {
    prelude_source_fact(name).map(|fact| fact.layer)
}

/// Classify a compiler-known core module path to its minimum runtime layer.
pub fn core_module_layer(module: &str) -> Option<RuntimeLayer> {
    Some(layer_of(module))
}

fn layer_of(module: &str) -> RuntimeLayer {
    match module {
        // ── core: no heap, no OS ─────────────────────────────────────────
        "core" | "core.prelude" | "core.math" | "core.units" | "core.perf"
        | "core.mem.scope" => RuntimeLayer::Core,

        // ── alloc: owned values and allocation-backed, non-OS operations ───
        "core.mem"
        | "core.math.random"
        | "core.crypto"
        | "core.crypto.random"
        | "core.crypto.uuid"
        | "core.crypto.vault"
        | "core.encoding"
        | "core.encoding.json"
        | "core.encoding.jsonl"
        | "core.encoding.csv"
        | "core.encoding.toml"
        | "core.encoding.yaml"
        | "core.encoding.xml"
        | "core.encoding.cbor"
        | "core.encoding.hex"
        | "core.encoding.base64"
        | "core.encoding.base32"
        | "core.text"
        | "core.text.fmt"
        | "core.reflect"
        | "core.compiler"
        | "core.game"
        | "core.reactive.loadable"
        | "core.event"
        | "core.compute"
        | "core.compute.solve"
        | "core.time.expiring"
        | "core.reactive"
        | "core.data"
        | "core.data.plot"
        | "core.data.sketch.hll"
        | "core.data.sketch.tdigest"
        | "core.data.sketch.cms"
        | "core.data.sketch.reservoir"
        | "core.log"
        | "core.regex"
        | "core.net.url"
        | "core.net.mime" => RuntimeLayer::Alloc,

        // ── hosted: OS, platform, terminal, and provider adapters ──────────
        "core.args"
        | "core.term"
        | "core.sys"
        | "core.process"
        | "core.files"
        | "core.watcher"
        | "core.net"
        | "core.net.tls"
        | "core.net.ws"
        | "core.time"
        | "core.tasks"
        | "core.http"
        | "core.http.client"
        | "core.http.server"
        | "core.archive"
        | "core.archive.gzip"
        | "core.archive.zstd"
        | "core.game.raylib"
        | "core.db"
        | "core.plugin"
        | "core.testing"
        | "core.mod"
        | "core.email"
        | "core.auth"
        | "core.sync"
        | "core.service"
        | "core.ui"
        | "core.web"
        | "core.web.browser"
        | "core.web.storage"
        | "core.web.storage.local"
        | "core.web.storage.session"
        | "core.web.devserver"
        | "app" => RuntimeLayer::Std,

        // Unknown modules default to hosted so new OS-facing modules stay
        // conservative until their semantic part is entered in this ledger.
        other if Syntax::is_known_core_module(other) => RuntimeLayer::Std,
        _ => RuntimeLayer::Std,
    }
}

/// Helper-level exceptions keep pure/fixed operations in Core while a mixed
/// module remains conservatively classified at its owning module layer.
fn helper_layer(module: &str, helper: &str) -> Option<RuntimeLayer> {
    if helper.is_empty() {
        return None;
    }
    match module {
        "core.math" | "core.units" | "core.perf" | "core.mem.scope" => {
            Some(RuntimeLayer::Core)
        }
        "core.mem" => matches!(
            helper,
            "Ptr"
                | "from_addr"
                | "volatile_read"
                | "volatile_write"
                | "address_of"
                | "pin"
                | "Pin"
        )
        .then_some(RuntimeLayer::Core),
        "core.text" => matches!(
            helper,
            "Cursor"
                | "caseless_eq"
                | "display_width"
                | "scalar_count"
                | "byte_count"
                | "is_alphabetic"
                | "is_numeric"
                | "is_whitespace"
                | "is_ascii"
                | "starts_any"
                | "ends_any"
        )
        .then_some(RuntimeLayer::Core),
        "core.time" => matches!(
            helper,
            "from_unix_ms"
                | "from_unix_seconds"
                | "from_unix_microseconds"
                | "from_unix_nanoseconds"
                | "from_timestamp"
                | "days_in_month"
                | "is_leap_year"
                | "period"
                | "period_days"
                | "period_months"
                | "period_years"
                | "utc"
        )
        .then_some(RuntimeLayer::Core),
        "core.net" => matches!(
            helper,
            "ip_addr"
                | "ip_to_string"
                | "ip_is_ipv4"
                | "socket_addr"
                | "socket_addr_parse"
                | "socket_host"
                | "socket_port"
                | "socket_to_string"
                | "error_operation"
                | "error_address"
                | "error_name"
                | "error_message"
                | "error_os_code"
        )
        .then_some(RuntimeLayer::Alloc),
        "core.net.url" | "core.net.mime" => Some(RuntimeLayer::Alloc),
        "core.files" => matches!(
            helper,
            "typed_head"
                | "path_from"
                | "path_join"
                | "path_parent"
                | "path_extension"
                | "path_stem"
                | "path_normalize"
        )
        .then_some(RuntimeLayer::Alloc),
        "core.crypto" => match helper {
            "__nominal__"
            | "Secret"
            | "SigningKey"
            | "VerifyKey"
            | "X25519SecretKey"
            | "X25519PublicKey"
            | "SharedSecret"
            | "Signature"
            | "Digest256"
            | "Digest512"
            | "sha256"
            | "blake3"
            | "sha512"
            | "constant_time_equal_bytes"
            | "constant_time_equal"
            | "verify"
            | "sign" => Some(RuntimeLayer::Core),
            "file_seal" | "file_open" => Some(RuntimeLayer::Std),
            _ => None,
        },
        "core.archive" => matches!(
            helper,
            "zip_compress"
                | "zip_decompress"
                | "crc32"
                | "adler32"
                | "deflate"
                | "inflate"
                | "zip_names_json"
                | "tar_names_json"
        )
        .then_some(RuntimeLayer::Alloc),
        "core.email" => matches!(
            helper,
            "Address"
                | "Message"
                | "Attachment"
                | "Envelope"
                | "RecipientPolicy"
                | "Limits"
                | "address"
                | "attachment"
                | "message"
                | "envelope"
                | "serialize"
        )
        .then_some(RuntimeLayer::Alloc),
        _ => None,
    }
}

/// Classify a sema/codegen helper-usage key (`core.term::input`, `core.files::read`, …)
/// to its minimum runtime layer.
pub fn core_usage_layer(usage: &str) -> Option<RuntimeLayer> {
    if let Some(rest) = usage.strip_prefix("core::") {
        return Some(match rest {
            "json" | "bytes" => RuntimeLayer::Alloc,
            "from_bytes" | "from_bytes_lossy" => RuntimeLayer::Core,
            "elapsed_millis" => RuntimeLayer::Std,
            _ => RuntimeLayer::Std,
        });
    }
    let (module, helper) = usage
        .split_once("::")
        .map_or((usage, ""), |(module, helper)| (module, helper));
    let module_layer = core_module_layer(module)?;
    Some(helper_layer(module, helper).unwrap_or(module_layer))
}

/// Return the complete, deterministic layer map for every semantic Prelude
/// part reachable from `roots`. Closure markers are provenance metadata and
/// therefore never become runtime requirements.
pub fn classify_prelude_closure<I, S>(roots: I) -> BTreeMap<String, RuntimeLayer>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut pending = BTreeSet::new();
    for root in roots {
        let root = root.as_ref();
        if !is_prelude_closure_marker(root) {
            pending.insert(root.to_owned());
        }
    }

    let mut closure = BTreeMap::new();
    while let Some(current) = pending.iter().next().cloned() {
        pending.remove(&current);
        if closure.contains_key(&current) {
            continue;
        }
        let layer = core_usage_layer(&current)
            .or_else(|| core_module_layer(&current))
            .unwrap_or(RuntimeLayer::Std);
        closure.insert(current.clone(), layer);
        for dependency in prelude_dependencies(&current) {
            if !closure.contains_key(&dependency) {
                pending.insert(dependency);
            }
        }
    }
    closure
}

/// Expand a sema usage set with every reachable semantic Prelude part.
pub fn expand_prelude_closure(used: &mut HashSet<String>) {
    let closure = classify_prelude_closure(used.iter());
    used.extend(closure.into_keys());
}

/// Compute the closure identity input in stable order. Callers hash this
/// canonical list together with target/provider facts for artifact identity.
pub fn prelude_closure_keys<I, S>(roots: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    classify_prelude_closure(roots).into_keys().collect()
}

fn is_prelude_closure_marker(usage: &str) -> bool {
    usage.starts_with("__core_source::") || usage.starts_with("__core_intrinsic::")
}

fn prelude_dependencies(usage: &str) -> Vec<String> {
    let (module, helper) = usage
        .split_once("::")
        .map_or((usage, ""), |(module, helper)| (module, helper));
    let mut dependencies = PRELUDE_DEPENDENCY_EDGES
        .iter()
        .find_map(|(name, dependencies)| (*name == module).then(|| {
            dependencies
                .iter()
                .map(|dependency| (*dependency).to_owned())
                .collect::<Vec<_>>()
        }))
        .unwrap_or_default();
    if !helper.is_empty() && module != "core" {
        dependencies.push(module.to_owned());
    }

    match usage {
        "core::json" => dependencies.push("core.encoding.json".to_string()),
        "core::bytes" => dependencies.push("core.encoding".to_string()),
        "core.crypto::file_seal" | "core.crypto::file_open" => {
            dependencies.push("core.files".to_string())
        }
        _ => {}
    }

    if !PRELUDE_NAMESPACE_ONLY.contains(&module) {
        if let Some((parent, _)) = module.rsplit_once('.') {
            if parent.starts_with("core") {
                dependencies.push(parent.to_owned());
            }
        }
    }
    dependencies.sort_unstable();
    dependencies.dedup();
    dependencies
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
        Syntax::PACKAGE_FILE,
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
            Syntax::PACKAGE_FILE,
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
    fn core_imports_use_layer() {
        assert_eq!(core_module_layer("core.log"), Some(RuntimeLayer::Alloc));
        assert_eq!(core_module_layer("core.http"), Some(RuntimeLayer::Std));
        assert_eq!(core_module_layer("core.math"), Some(RuntimeLayer::Core));
    }

    #[test]
    fn helper_usage_keys_map_to_layer() {
        assert_eq!(
            core_usage_layer("core.term::input"),
            Some(RuntimeLayer::Std)
        );
        assert_eq!(
            core_usage_layer("core.math::__mathtypes__"),
            Some(RuntimeLayer::Core)
        );
        assert_eq!(core_usage_layer("core::json"), Some(RuntimeLayer::Alloc));
        assert_eq!(core_usage_layer("core.reactive"), Some(RuntimeLayer::Alloc));
    }

    #[test]
    fn prelude_closure_is_transitive_and_ignores_provenance() {
        let closure = classify_prelude_closure([
            "core.crypto::file_seal",
            "__core_intrinsic::core.crypto::file_seal",
        ]);
        assert_eq!(
            closure.get("core.crypto::file_seal"),
            Some(&RuntimeLayer::Std)
        );
        assert_eq!(closure.get("core.files"), Some(&RuntimeLayer::Std));
        assert_eq!(closure.get("core.crypto"), Some(&RuntimeLayer::Alloc));
        assert!(!closure.contains_key("__core_intrinsic::core.crypto::file_seal"));
    }

    #[test]
    fn pure_nested_parts_do_not_inherit_hosted_namespace() {
        let closure = classify_prelude_closure(["core.net.url::parse"]);
        assert_eq!(
            closure.get("core.net.url::parse"),
            Some(&RuntimeLayer::Alloc)
        );
        assert!(!closure.contains_key("core.net"));
    }
}
