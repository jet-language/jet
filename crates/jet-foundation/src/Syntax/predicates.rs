/// D-MARKER-FAMILY1: is `name` a contract-plane (`@`) marker? The I7/R3
/// dispatch chokepoint — parser/formatter/sema/LSP ask here, never hand-roll
/// the move list.
pub fn is_contract_marker(name: &str) -> bool {
    CONTRACT_MARKERS.contains(&name)
}

/// D-MARKER-FAMILY1: is `name` a directive-plane (`#`) marker in the E0063
/// confusable set? Used to detect `@` written before a directive name.
pub fn is_directive_marker(name: &str) -> bool {
    DIRECTIVE_MARKERS.contains(&name)
}

/// D-DSLBLOCK1=A: is `name` one of the stdlib-owned DSL block markers allowed
/// to claim a checked syntax island?
pub fn is_stdlib_dsl_block_marker(name: &str) -> bool {
    STDLIB_DSL_BLOCK_MARKERS.contains(&name)
}

// D-UNITLIT1: unit-suffix numeric literals (`500ms`) are not an enumerable
// keyword — the lexer resolves a literal's identifier suffix against
// #UnitFamily members in scope (ATTR_UNIT_FAMILY, D-QUAL3). One fixed rule:
/// D-UNITLIT1: a literal suffix shaped `e` + digits is reserved for float
/// exponent notation (`1e5`) and may never resolve as a unit name.
pub const UNIT_SUFFIX_EXPONENT_RESERVED: &str = "e"; // D-UNITLIT1

// D-TRAILBLOCK1: no new token — `{` directly after a call's `)` parses as the
// trailing zero-parameter lambda argument. Parser-position rule, not lexical.
// D-DESTRUCT1: no new token — reuses the D-DOTCTOR1 `.{` sigil in pattern
// position and `..` (OP_RANGE) as the now-mandatory partial-pattern rest
// marker.
// D-CHAINCMP1: no new token — same-direction `<`/`<=`/`>`/`>=` chains are a
// parser/sema desugaring (`0 <= sev < 10` → `0 <= sev && sev < 10`, middle
// operand evaluated once).
// D-CLIFLAG1: the struct-level CLI-derive marker and field-level doc marker
// spellings ride D-CONTRACTCASE1/D-MARKERMOVE1 — constants land with them.
// D-EFFBUDGET1: `effects`/`allow`/`deny`/`grants` are pkg.jet manifest keys
// (Jetpack/PackageManifest), not language tokens; effect names reuse D-EFF4.
// D-ANY-JAI1 + D-VARARGBOUND1: reuses D-VARIADIC1 `...T`; multi-trait
// bounds use the owner-amended list form (`T: [A, B]`, `...items: [A, B]`).
// D-UFCS1 (B), D-POINTERCHAIN1 (A), D-ERRCTX1 (D): no typeable surface.

// ── Module name resolution helpers ───────────────────────────────────────────
//
// These are pure string functions used by both Sema and Codegen to identify
// compiler-known ("core") modules. They live here so that neither Sema nor
// Codegen need to depend on Loader (which does file I/O and belongs in the
// driver layer).

/// Single canonical source of truth for all known Core modules (c45).
///
/// `is_known_core_module` and `core_modules_list` both derive from this slice.
/// `core_module_items` in Sema/CheckerCoreLib.rs has per-module item data and
/// cannot collapse here, but a drift-guard test (tests/corelib.rs) asserts its
/// key set equals this slice.
pub const KNOWN_CORE_MODULES: &[&str] = &[
    "core",
    "core.io",
    "core.env",
    // D-OSFACTS1=A: system facts and safe interrupt hook.
    "core.os",
    "core.process",
    "core.math",
    "core.random",
    "core.time",
    "core.tasks",
    // D-TESTKIT1=A: helpers under existing #Test syntax.
    "core.testing",
    "core.mem",
    // D-ALLOC-C (ratified 2026-06-19): wider allocator API bucket.
    "core.mem.alloc",
    // D-OPTGC1 / D-DEP-GC1: opt-in traced `Gc<T>` library.
    "core.gc",
    // D-SOLVER-LIB1=A: explicit finite solver state, no language backtracking.
    "core.solve",
    // D-DATA-SURFACE1=A: one beginner facade for typed tables, series, stats, and plots.
    "core.data",
    // E2-M7: streaming file handles and path helpers (D-IO1, D-IO2).
    "core.files",
    "core.path",
    // D-URL1=A: typed WHATWG-class URLs and MIME values.
    "core.url",
    "core.mime",
    // D-EMAIL1=A: typed message construction and native SMTP substrate.
    CORE_EMAIL_MODULE,
    // D-WATCH-SCOPE1: unified file/process/port watcher constructors.
    "core.watcher",
    // E2-M10: TCP/UDP sockets.
    "core.net",
    // D-DEFER1 option B: scope-exit guard (RAII cleanup via closure).
    "core.scope",
    // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing builder.
    "core.args",
    // D-TERM1 (ratified 2026-06-22): terminal direct-input — `term.read_key() -> Key`.
    "core.term",
    // D-ANY-JAI1 (c7jaiany §6, ratified 2026-07-01): runtime reflection floor —
    // `reflect.of(x) -> Value` with `.type_name()`/`.display()`/`.fields()`.
    "core.reflect",
    // D-ENC1 (ratified 2026-06-24): unified serialization library `core.encoding` with
    // per-format submodules. Supersedes `core.json` + `jet.{csv,toml,yaml}` (clean break).
    "core.encoding",
    "core.encoding.json",
    "core.encoding.jsonl",
    "core.encoding.csv",
    "core.encoding.toml",
    "core.encoding.yaml",
    "core.encoding.xml",
    "core.encoding.cbor",
    // D-UUIDENC1=A (ratified 2026-06-26): hex and base64 codecs (pure, no deps).
    "core.encoding.hex",
    "core.encoding.base64",
    "core.encoding.base32",
    // D-TEXTUNICODE1: std-only Unicode scalar helpers. Grapheme segmentation stays
    // future work because it needs a Unicode data table/engine.
    "core.text.unicode",
    // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — the constructors are
    // bare (no import needed); the modules exist for discoverability/docs.
    "core.binary",
    "core.text",
    // D-HUMANFMT1=A: Go-humanize-style helpers as ordinary library calls.
    "core.fmt",
    // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
    "core.uuid",
    // D-CORENS1: ring packages now spelled `core.*` (canonical user-facing name).
    // Most ring packages still dispatch through legacy `jet.*` keys; archive is
    // canonical end-to-end as `core.archive`.
    "core.log",
    "core.crypto",
    // D-RANDSPLIT1=A: CSPRNG submodule — `core.crypto.random.bytes(n)`.
    "core.crypto.random",
    // D-CRYPTOENV1=A: expert-only raw crypto primitives.
    "core.crypto.expert",
    // D-HTTPLIB1-4 (ratified 2026-06-26): HTTP client+server ring package.
    "core.http",
    // D-REGEXENGINE1=A: std-only linear regex in the generated prelude.
    "core.regex",
    // D-CORE-COMPRESS1=A / D-DEP-ARCHIVE1=A: zip/tar containers only.
    "core.archive",
    // D-RAYLIB1=A / D-GAME1=B: official first-party raylib graphics bridge.
    "core.raylib",
    // D-GAME1/2/3 + D-WD10 + D-GAME-*: stable headless game substrate.
    "core.game",
    // D-CORE-COMPRESS1=A / D-CODECS1: canonical stream codecs.
    // `flate2` (gzip) and `zstd` FFI bridges.
    "core.compress.gzip",
    "core.compress.zstd",
    // D-DEP-DB1: SQLite ring package via the `rusqlite` (bundled) crate FFI bridge.
    "core.db",
    // D-DEP-WASM1=A / D-PLUGIN1=B (c81): sandboxed WASM Component Model
    // plugin loader (wasmtime, runtime-side only, I6).
    "core.plugin",
    // D-REACT1=B (ratified 2026-06-22): opt-in reactive library — signals,
    // derived values, and effects. Pure std runtime (no external crate).
    "core.reactive",
    // D-EVENT1=D (ratified 2026-07-07): typed Event<T>/Hook<T,R> runtime family.
    "core.event",
    // D-HONESTNUM1=A (ratified 2026-06-26): Measurement<T> — value ± uncertainty
    // with standard uncertainty propagation. Pure float arithmetic; no external crates.
    "core.science.measurement",
    // D-CORE-NUMERIC1=A: precise numerics live in core.math.
    // D-PENDING1=B (ratified 2026-06-26): Loadable<T, E> — async UI state machine
    // (Idle / Loading / Loaded(T) / Failed(E)). Pure stdlib enum; no external crates.
    // D-CORENS2=A: loading state belongs to the reactive domain.
    "core.reactive.loadable",
    // D-FIDELITY-API1=A (ratified 2026-07-06): core.perf.Perf static API —
    // runtime-global quality/perf knob, with manual override/reset only.
    "core.perf",
    // D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): render-target backend trait seam.
    "core.ui",
    // D-FLAGSHIP-WEBAPI1=A: browser events, element reads, and storage for web slices.
    "core.web",
    "core.web.storage",
    "core.web.storage.local",
    "core.web.storage.session",
    // D-APPROX1=A (ratified 2026-06-26): approximate data structures under core.sketch.
    "core.sketch.hll",
    "core.sketch.tdigest",
    "core.sketch.reservoir",
    "core.sketch.cms",
    // D-TIMEDEPTH1=A (ratified 2026-06-26): civil-time constructors.
    "core.time.date",
    "core.time.datetime",
    // D-TTLVAL1=A: TTL-wrapped cache values.
    "core.time.expiring",
    // D-NETDEP1=A / D-HTTPLIB2=B (ratified 2026-06-26): full HTTP library.
    "core.http.client",
    "core.http.server",
    // D-NETTLSSTREAM1=A: verified TLS wraps the canonical core.net byte stream.
    "core.tls",
    // c-devserver (owner-directed 2026-07-01): a `.jet` file's own `jet dev`
    // behavior — a configurable server value (`for_app`/`.html`/`.port`/`.serve`).
    // D-CORENS2=A: dev-server configuration belongs to the web domain.
    "core.web.devserver",
    // U13 (D-JPK-SECRETCRYPTO1, card c9jetpackgates): `core.vault.get` reads a
    // secret decrypted from the project's encrypted repo file (`.jet/secrets.age`),
    // via an age-style crypto FFI bridge. D-CORE-SECRETS1=A also places
    // secret lifecycle (`Rotting<T>`) here; generic TTL remains core.time.expiring.
    "core.vault",
];

pub fn is_known_core_module(name: &str) -> bool {
    if KNOWN_CORE_MODULES.contains(&name) {
        return true;
    }
    // D-CORENS1: internal dispatch key `jet.<ring>` (from normalize_core_module)
    // is valid for ring modules that have not been canonicalized end to end.
    if let Some(ring) = name.strip_prefix("jet.") {
        if ring == "raylib" {
            return false;
        }
        return is_ring_module(ring);
    }
    false
}

pub fn core_modules_list() -> String {
    KNOWN_CORE_MODULES.join(", ")
}

/// Normalize a module import name to a canonical core-module path, or `None`
/// if the import is not a core/ring module.
///
/// D-CORENS-CANON1: `core.<ring>` is the only user-facing spelling. Ring modules
/// still normalize to the internal `jet.<ring>` key used by sema dispatch.
pub fn normalize_core_module(name: &str) -> Option<String> {
    if name == CORE_SHORT {
        return Some(CORE_SHORT.to_string());
    }
    if name == CORE_CANONICAL {
        return Some(CORE_SHORT.to_string());
    }
    // Some ring modules still use internal `jet.<ring>` keys until their
    // package cleanup lands. Canonicalized modules stay `core.*` end to end.
    if let Some(ring) = name.strip_prefix("core.") {
        if matches!(ring, "archive" | "raylib") {
            return Some(name.to_string());
        }
        if is_ring_module(ring) {
            return Some(format!("jet.{ring}"));
        }
        return Some(format!("core.{ring}"));
    }
    None
}

/// E2-M9: ring module names that resolve as compiler-known modules.
pub fn is_ring_module(name: &str) -> bool {
    matches!(
        name,
        "log" | "crypto" | "http" | "regex" | "reactive" | "archive" | "raylib" | "db"
            // D-DEP-WASM1=A (c81): `core.plugin` / internal `jet.plugin` — the
            // wasmtime-backed plugin loader (`Plugin.load`/`.call`).
            | "plugin"
    )
}

/// The env var that names the active realized toolchain object directory
/// (D-JPK-TOOLCHAIN1 / #179). Tests set it to a fixture; #179's realizer points
/// it at the hangar object. The object carries prebuilt ring artifacts under
/// `<dir>/ring/<name>` (D-JPK-RINGSHIP1=C).
pub const TOOLCHAIN_OBJECT_ENV: &str = "JET_TOOLCHAIN_FIXTURE";

/// D-JPK-TOOLCHAIN1=A (#179): re-exec guard marker. Before `jet` execs a
/// version-pinned toolchain, it sets this env var to the pinned version. The
/// child, seeing its own version match the marker, runs natively and never
/// re-realizes or re-execs — this breaks the exec loop and lets the pinned
/// toolchain run the program directly.
pub const TOOLCHAIN_EXEC_MARKER_ENV: &str = "JET_TOOLCHAIN_EXEC";

/// D-JPK-RINGSHIP1=C: is this ring lib present as a realized hangar object for
/// the active toolchain? True when the active toolchain object carries a
/// prebuilt artifact for `name` on this platform; false otherwise (the loader
/// then falls back to the compiler-embedded template — rung-0 magic preserved).
pub fn is_ring_module_staged(name: &str) -> bool {
    staged_ring_artifact(name).is_some()
}

/// The prebuilt ring artifact path for `name` in the active toolchain object, or
/// `None` when there is no active object or it carries no artifact for `name`.
pub fn staged_ring_artifact(name: &str) -> Option<std::path::PathBuf> {
    if !is_ring_module(name) {
        return None;
    }
    let dir = std::env::var_os(TOOLCHAIN_OBJECT_ENV)?;
    let artifact = std::path::Path::new(&dir).join("ring").join(name);
    artifact.exists().then_some(artifact)
}

pub fn is_legacy_std_import(name: &str) -> bool {
    name == "std" || name.starts_with("std.") || name == "jet.std" || name.starts_with("jet.std.")
}

/// D-ALLOC1/D-ALLOC-C: allocator opaque types → jet_mem Rust types.
pub fn alloc_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Arena" => Some("jet_mem::JetArena"),
        "Bump" => Some("jet_mem::JetBump"),
        "Pool" => Some("jet_mem::JetPool"),
        "Fixed" => Some("jet_mem::JetFixed"),
        _ => None,
    }
}

/// D-ARGS1: ArgsSpec/ParsedArgs → prelude Rust types.
pub fn args_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "ArgsSpec" => Some("JetArgsSpec"),
        "ParsedArgs" => Some("JetParsedArgs"),
        _ => None,
    }
}

/// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s handle types are top-level
/// prelude structs, same shape as `args_handle_rust_type` above.
pub fn reflect_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Value" => Some("JetReflectValue"),
        "Field" => Some("JetReflectField"),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): `binary.Reader`/`text.Cursor` handle types are
/// top-level prelude structs, same shape as `reflect_handle_rust_type`
/// above — including the caller's `!type_names.contains(name)` collision
/// guard, since "Reader"/"Cursor" are plausible user type names.
pub fn binary_text_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Reader" => Some("JetReader"),
        "Cursor" => Some("JetCursor"),
        TYPE_BIT_SET => Some("JetBitSet"),
        TYPE_BYTE_BUFFER => Some("JetByteBuffer"),
        _ => None,
    }
}

/// True when a crate spec string needs an explicit version (i.e. it's not
/// "std" and doesn't already contain `@`).
pub fn crate_spec_needs_version(spec: &str) -> bool {
    spec != "std" && spec.split_once('@').is_none()
}

/// D-EFFBUDGET1 (ratified 2026-07-01): the `effects { allow: […], deny: […] }`
/// block in `pkg.jet` that turns on whole-dependency-graph effect enforcement.
/// Manifest keys only — no language grammar (§0.4 DO-NOT).
pub const MANIFEST_BLOCK_EFFECTS: &str = "effects"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `allow:` field inside `effects { … }` — the closed list of
/// effect names the whole dependency graph may use.
pub const EFFECTS_FIELD_ALLOW: &str = "allow"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `deny:` field inside `effects { … }` — effect names the
/// dependency graph must never use.
pub const EFFECTS_FIELD_DENY: &str = "deny"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `grants { "dep": [Effect] }` block — an audited
/// per-dependency escape from the `effects:` budget, recorded in the lockfile.
pub const MANIFEST_BLOCK_GRANTS: &str = "grants"; // D-EFFBUDGET1
/// D-JPK-GRANTSCHEMA1=A: source-reviewed trust policy lives under
/// `policy: { trust: { … } }` in `pkg.jet`. Manifest keys only, no language
/// grammar.
pub const MANIFEST_BLOCK_POLICY: &str = "policy"; // D-JPK-GRANTSCHEMA1
pub const POLICY_FIELD_TRUST: &str = "trust"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_FIELD_DEFAULT: &str = "default"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_FIELD_CI: &str = "ci"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_FIELD_PROMPT: &str = "prompt"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_FIELD_SERVICES: &str = "services"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_DECISION_PROMPT: &str = "prompt"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_DECISION_DENY: &str = "deny"; // D-JPK-GRANTSCHEMA1
pub const POLICY_TRUST_DECISION_ALLOW: &str = "allow"; // D-JPK-GRANTSCHEMA1
/// D-LINTPOLICY1=A (the override law, card #505): the `policy: { lints: { … } }`
/// sub-block, joining `trust` under the one `policy:` namespace
/// (D-JPK-POLICYSURFACE1). Warnings never fail a build by default (I1 memory/
/// type safety is never in scope here); a team opts into a wall by naming
/// lint codes here. Manifest keys only, no language grammar.
pub const POLICY_FIELD_LINTS: &str = "lints"; // D-LINTPOLICY1
/// D-LINTPOLICY1: the `deny:` field inside `policy.lints { … }` — lint codes
/// (e.g. `L0504`) that fail the build when they fire, instead of warning.
pub const LINTS_FIELD_DENY: &str = "deny"; // D-LINTPOLICY1

/// Levenshtein edit distance between two strings (used for "did you mean?" suggestions).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}
use super::{
    CONTRACT_MARKERS, CORE_CANONICAL, CORE_EMAIL_MODULE, CORE_SHORT, DIRECTIVE_MARKERS,
    STDLIB_DSL_BLOCK_MARKERS, TYPE_BIT_SET, TYPE_BYTE_BUFFER,
};
