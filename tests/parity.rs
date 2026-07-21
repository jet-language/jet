//! Card #392: AOT-vs-comptime builtin parity matrix.
//!
//! Builds one deterministic inventory from AOT's fixed Core signatures,
//! bespoke `infer_core_call` arms, direct/static builtins, and value-method
//! registry. Comptime coverage comes from the real split interpreter source
//! tree. Every discovered entry receives exactly one classification.
//!
//! A pair present in AOT but absent from comptime means `use core.x as a;
//! a.f(...)` type-checks and compiles for `jet build`, but hits E0956 in
//! `jet dev`/the REPL/a `comptime` binding — an R12 parity bug (silent
//! AOT-only builtin). New builtins must add themselves to comptime's
//! dispatch (or, for a genuinely-impossible-at-comptime effect, to
//! `KNOWN_OPEN_GAPS` below with a one-line reason) or this test fails.
//!
//! Effectful modules (`core.files`/`core.env`/`core.io`/`core.exec`/
//! `core.net`/`core.tls`/`core.process`) are explicit effect boundaries:
//! comptime handles them as a whole via the `is_tier2` wildcard (E3410
//! `@Impure` gate), not per-method.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// Modules comptime gates wholesale behind `@Impure` (Methods.rs `is_tier2`)
/// rather than dispatching per-method — a per-name diff against these would
/// only prove the gate exists, which other tests (e.g. `repl.rs`'s E3410
/// cases) already cover.
const EFFECT_GATED_MODULES: &[&str] = &[
    "core.files",
    "core.env",
    "core.io",
    "core.exec",
    "core.net",
    "core.tls",
    "core.process",
    // Not gated by the generic `is_tier2` list — denied unconditionally,
    // earlier in `eval_method`, with its own diagnostic (E1265): a build
    // artifact must never bake in a decrypted secret (I1), so there is no
    // `@Impure` escape hatch at all here, unlike the effects above. Correctly
    // handled, just not through the per-method `apply_core_call` table this
    // audit reads — excluded so it isn't flagged as a silent gap.
    "core.vault",
];

// The tuple extractor intentionally over-approximates alternation arms. JSON's
// `to_string_pretty` must not cross-product onto adapters whose sema surface
// exposes only `to_string`.
const EXTRACTOR_ARTIFACTS: &[(&str, &str)] = &[
    ("core.encoding.csv", "to_string_pretty"),
    ("core.encoding.toml", "to_string_pretty"),
    ("core.encoding.yaml", "to_string_pretty"),
];

/// Known, currently-open comptime gaps: real AOT `(module, method)` pairs
/// with no comptime dispatch yet, each with why it isn't in this card's
/// slice. Every entry here is a to-do, not a shrug — remove a line the same
/// PR that closes the gap. Anything AOT supports that ISN'T listed here and
/// isn't dispatched by comptime fails this test.
const KNOWN_OPEN_GAPS: &[(&str, &str)] = &[
    // core.auth verifies signatures and reads the system clock. The dev interpreter
    // names that native boundary and default dev transparently uses the AOT path.
    ("core.auth", "verify_jwt"),
    ("core.auth", "verify_paseto"),
    // core.text: full Unicode Standard normalization/segmentation (NFC/NFD/
    // NFKC/NFKD compose+decompose tables, grapheme/word/sentence boundary
    // algorithms) is a large, separate undertaking even though AOT's own
    // versions (`jet_text_*` in Text.rs) are themselves hand-rolled
    // approximations, not full UAX-compliant tables — porting the
    // approximation was in scope (done, see `TextLite.rs`) but the
    // underlying algorithm gap versus true Unicode isn't this card's job to
    // fix on either tier. (Card #392 ported everything else in core.text.)
    // core.time ambient calls read wall or monotonic clocks. Packet B ports
    // the deterministic constructors/parsers; these remain named boundaries.
    ("core.time", "now"),
    ("core.time", "now_utc"),
    ("core.time", "today"),
    ("core.time", "local_time"),
    ("core.time", "instant"),
    ("core.time", "sleep"),
    ("core.time", "start"),
    // Everything below is PRE-EXISTING debt this card's audit surfaced, not
    // new: entire modules that comptime has never dispatched at all (no
    // `apply_core_call` arm for the module exists, so every call in the
    // module already hit E0956 before this card). Card #392's slice was
    // "BigInt, then audit" — the audit's job is making this backlog visible
    // and CI-enforced (this test), not closing a dozen unrelated stdlib
    // surfaces in one pass. Each group below is its own future card.
    //
    // core.data: fixed-signature stats (sum/mean/min/max/median/variance/
    // stddev/quantile/rolling_mean/describe/status) and plot rendering
    // (bar_text/bar_svg) are PORTED (card #392 pass 3, `DataLite.rs`). The
    // generic call-site-typed table/lazy-pipeline half (D-DATA-SURFACE1) —
    // table/rows/series/values/missing_count/csv/count/lazy/lazy_filter/
    // lazy_sort_by/collect/plan/filter/sort_by/group_count/group_sum/
    // group_mean — is PORTED too (card #392 pass 5, `DataPipeline.rs`):
    // `Table<T>`/`Series<T>`/`LazyFrame<T>` are plain `CtValue::Struct`
    // wrappers over already-dynamically-typed rows, with closures applied
    // through the same `call_closure` path `list.map`/`.filter` use.
    // `inner_join`/`left_join` and Packet B's `pivot_sum` route through the
    // interpreter because their closure arguments need live `Interp` access.
    //
    // core.archive / core.compress.*: zip/tar and gzip/zstd — needs a hand-rolled
    // (I6) compression implementation ported into the interpreter, not a
    // one-line Rust std call.
    ("core.archive", "tar_add"),
    ("core.archive", "tar_get"),
    ("core.archive", "tar_names_json"),
    ("core.archive", "zip_compress"),
    ("core.archive", "zip_decompress"),
    ("core.compress.gzip", "compress"),
    ("core.compress.gzip", "decompress"),
    ("core.compress.zstd", "compress"),
    ("core.compress.zstd", "decompress"),
    // core.crypto.expert / core.crypto.random: security-sensitive — needs a
    // careful, independently-reviewed port (AEAD ciphers, CSPRNG), not a
    // quick approximation that could silently diverge from the audited AOT
    // implementation.
    ("core.crypto.expert", "aes256gcm_open"),
    ("core.crypto.expert", "aes256gcm_seal"),
    ("core.crypto.expert", "argon2id"),
    ("core.crypto.expert", "ed25519_sign"),
    ("core.crypto.expert", "ed25519_verify_strict"),
    ("core.crypto.expert", "hkdf_sha256"),
    ("core.crypto.expert", "secret_bytes"),
    ("core.crypto.expert", "shared_secret_bytes"),
    ("core.crypto.expert", "signing_key_bytes"),
    ("core.crypto.expert", "x25519"),
    ("core.crypto.expert", "x25519_secret_bytes"),
    ("core.crypto.expert", "xchacha20poly1305_open"),
    ("core.crypto.expert", "xchacha20poly1305_seal"),
    ("core.crypto.random", "bytes"),
    // core.encoding.*: card #392 pass 4 ported csv/toml/yaml/xml/cbor/jsonl
    // parse+to_string (or encode/decode) plus json.canonical/events verbatim
    // into comptime (`EncodingLite.rs`, dispatched from `Methods.rs`,
    // rustc-verified in `tests/comptime_diff.rs`). base32 and base64's
    // URL-safe variant were already done (byte-for-byte ports —
    // `base32_encode`/`base32_decode` and the `encode_url`/`decode_url` arms
    // in Methods.rs).
    //
    // `decode_traced<T>` is PORTED too (card #392 pass 5, `TypedDecode.rs`):
    // typed `Decode` dispatch at comptime — resolves the target user type via
    // `self.structs`, walks its fields (honoring `@[Rename]`/`RenameAll`/
    // `Default`/`Flatten`/`DenyUnknownFields`), and — for a
    // `@PublishedSchema` type with `migration { }` blocks — walks the runtime
    // migration chain the same way `Codegen/Items.rs::emit_migration_chain_walker`
    // does (shape detection by wire-key set, newest match first, calling the
    // sema-lowered `__migrate_conv_*`/`__migrate_add_*` synthetic functions
    // through the ordinary `call_func` path). `json`/`csv`/`toml`/`yaml` all
    // share the one `typed_decode_top` walker. (The `fixed_sigs.rs`
    // alternation's paren-balance-parser artifact — `("core.encoding.json",
    // "core.encoding.csv")` and its toml/yaml siblings — used to need an
    // explicit entry here too, but `Methods.rs`'s new dispatch guard uses the
    // identical `("core.encoding.json" | … , "decode" | "decode_traced")`
    // alternation shape, so the same artifact pairs now appear on both sides
    // of the diff and aren't `newly_missing` — no entry needed.)
    //
    // core.os: host/process facts (hostname, arch, pid, cpu_count, …) — real
    // ambient reads of the host, arguably belongs behind the same `@Impure`
    // gate as `core.env`/`core.process` rather than E0956; needs the same
    // effect-boundary design work as `core.time`'s split above.
    ("core.os", "arch"),
    ("core.os", "cpu_count"),
    ("core.os", "executable"),
    ("core.os", "family"),
    ("core.os", "hostname"),
    ("core.os", "name"),
    ("core.os", "on_interrupt"),
    ("core.os", "pid"),
    ("core.os", "set_current_dir"),
    ("core.os", "temp_dir"),
    ("core.os", "username"),
    // core.raylib / core.ui / core.term: native windowing/rendering/terminal
    // backends — genuine ambient effects tied to a real display/terminal,
    // likely belongs in the E3410 Tier-2 gate family rather than "port the
    // pure logic", but that's a call for whoever designs the gate split.
    ("core.raylib", "begin_drawing"),
    ("core.raylib", "clear_background"),
    ("core.raylib", "close_window"),
    ("core.raylib", "color"),
    ("core.raylib", "draw_rectangle"),
    ("core.raylib", "draw_text"),
    ("core.raylib", "end_drawing"),
    ("core.raylib", "key_down"),
    ("core.raylib", "set_target_fps"),
    ("core.raylib", "window_open"),
    ("core.raylib", "window_ready"),
    ("core.raylib", "window_should_close"),
    ("core.ui", "aria_role_button"),
    ("core.ui", "aria_role_container"),
    ("core.ui", "aria_role_label"),
    ("core.ui", "aria_role_text_input"),
    ("core.ui", "constraint"),
    ("core.ui", "gtk_backend"),
    ("core.ui", "key_event"),
    ("core.ui", "node"),
    ("core.ui", "node_color"),
    ("core.ui", "node_role"),
    ("core.ui", "null_backend"),
    ("core.ui", "point"),
    ("core.ui", "rect"),
    ("core.ui", "resize_event"),
    ("core.ui", "size"),
    ("core.ui", "tui_backend"),
    ("core.term", "read_key"),
    ("core.web", "on"),
    ("core.web", "value"),
    ("core.web.storage.local", "clear"),
    ("core.web.storage.local", "get"),
    ("core.web.storage.local", "remove"),
    ("core.web.storage.local", "set"),
    // core.tasks / core.watcher / core.web.devserver: async runtime primitives
    // (channels, timers, file/port watchers, a live dev-server handle) — all
    // inherently tied to the running process's event loop; may never make
    // sense as pure comptime values rather than a genuine effect.
    ("core.tasks", "after"),
    ("core.tasks", "channel"),
    ("core.tasks", "interval"),
    ("core.watcher", "files"),
    ("core.watcher", "port"),
    ("core.watcher", "process_pid"),
    ("core.watcher", "set"),
    ("core.web.devserver", "app"),
    ("core.web.devserver", "for_app"),
    // Misc small surfaces, each its own small future card.
    ("core.args", "spec"),
    ("core.game", "run"),
    ("core.math", "decimal"),
    ("core.testing", "corpus"),
    ("core.testing", "fixture"),
    ("core.testing", "golden"),
    ("core.testing", "snap"),
    ("core.testing", "temp_dir"),
    // Opaque mutable handles still pending.
    ("core.sketch.cms", "new"),
    ("core.sketch.hll", "new"),
    ("core.sketch.reservoir", "new"),
    ("core.sketch.tdigest", "new"),
    // core.url: PORTED (card #392 pass 3) — see `UrlLite.rs` + `Methods.rs`'s
    // `("core.url", ...)` arms, ported verbatim from `JetUrl`/`jet_url_*`
    // (`UrlMime.rs` + `MathRandomTime.rs`).
    // core.uuid: EFFECT BOUNDARY, not a porting gap. `v4`/`v7` need genuine
    // ambient entropy (`jet_uuid_fill_random` reads `/dev/urandom`, POSIX,
    // falling back to a wall-clock-nanosecond seed — `EncodingCodecs.rs`) and
    // `v7` additionally needs the ambient wall clock for its timestamp bits —
    // both non-deterministic, unlike `core.random`'s explicitly-seeded stream.
    // A "pure" comptime UUID would either fake randomness (silently diverging
    // from AOT, R12 risk) or read the real host (a build-time effect with no
    // `@Impure` gate defined for it yet) — genuinely impossible to do purely,
    // not merely unported.
    ("core.uuid", "v4"),
    ("core.uuid", "v7"),
];

fn raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = match (bytes.get(start), bytes.get(start + 1)) {
        (Some(b'r'), _) => start + 1,
        (Some(b'b'), Some(b'r')) => start + 2,
        _ => return None,
    };
    let hashes_start = i;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let hashes = i - hashes_start;
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'"'
            && bytes
                .get(i + 1..i + 1 + hashes)
                .is_some_and(|closing| closing.iter().all(|byte| *byte == b'#'))
        {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(bytes.len())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RustSegment {
    Code,
    String,
}

fn rust_source_segments(text: &str) -> Vec<(RustSegment, usize, usize)> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut i = 0usize;
    let mut start = 0usize;
    while i < bytes.len() {
        let skipped_from = i;
        let mut string = false;
        let skipped;
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            skipped = true;
        } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            let mut comments = 1usize;
            while i < bytes.len() && comments > 0 {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    comments += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    comments -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            skipped = true;
        } else if let Some(end) = raw_string_end(bytes, i) {
            i = end;
            skipped = true;
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i = (i + 2).min(bytes.len()); }
                else if bytes[i] == b'"' { i += 1; break; }
                else { i += 1; }
            }
            string = true;
            skipped = true;
        } else if bytes[i] == b'\'' {
            let end = if bytes.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
                skipped = true;
            } else {
                i += 1;
                skipped = false;
            }
        } else {
            i += 1;
            skipped = false;
        }
        if skipped {
            if start < skipped_from {
                segments.push((RustSegment::Code, start, skipped_from));
            }
            if string {
                segments.push((RustSegment::String, skipped_from, i));
            }
            start = i;
        }
    }
    if start < bytes.len() {
        segments.push((RustSegment::Code, start, bytes.len()));
    }
    segments
}

fn rust_code_ranges(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    rust_source_segments(text)
        .into_iter()
        .filter_map(|(kind, start, end)| (kind == RustSegment::Code).then_some((start, end)))
}

fn rust_code_occurrences(text: &str, needle: &str) -> Vec<usize> {
    rust_code_ranges(text)
        .into_iter()
        .flat_map(|(start, end)| {
            text[start..end]
                .match_indices(needle)
                .map(move |(at, _)| start + at)
        })
        .collect()
}

fn rust_header_before_block(text: &str) -> Option<String> {
    let segments = rust_source_segments(text);
    let brace = segments.iter().find_map(|(kind, start, end)| {
        (*kind == RustSegment::Code)
            .then(|| text.as_bytes()[*start..*end].iter().position(|byte| *byte == b'{'))
            .flatten()
            .map(|at| *start + at)
    })?;
    let mut header = vec![b' '; brace];
    for (_, start, end) in segments {
        if start >= brace {
            break;
        }
        let end = end.min(brace);
        header[start..end].copy_from_slice(&text.as_bytes()[start..end]);
    }
    String::from_utf8(header).ok()
}

fn tuple_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut starts = Vec::new();
    for (start, end) in rust_code_ranges(text) {
        for (i, byte) in bytes.iter().enumerate().take(end).skip(start) {
            match byte {
                b'(' => starts.push(i),
                b')' => {
                    if let Some(start) = starts.pop() { ranges.push((start, i + 1)); }
                }
                _ => {}
            }
        }
    }
    ranges.sort_unstable();
    ranges
}

/// Pull Core module/method tuples from Rust code, ignoring tuples that occur
/// only in comments or string contents.
fn extract_pairs(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (start, end) in tuple_ranges(text) {
        let fields = split_top_level(&text[start + 1..end - 1], ',');
        if fields.len() >= 2 {
            let modules = extract_quoted(fields[0]);
            let names = extract_quoted(fields[1]);
            for module in modules.iter().filter(|module| module.starts_with("core.")) {
                for name in names.iter().filter(|name| *name != "_") {
                    out.insert((module.clone(), name.clone()));
                }
            }
        }
    }
    out
}

fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut fields = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == separator && depth == 0 => {
                fields.push(&text[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    fields.push(&text[start..]);
    fields
}

fn extract_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if let Some(j) = s[i + 1..].find('"') {
                out.push(s[i + 1..i + 1 + j].to_string());
                i = i + 1 + j + 1;
                continue;
            } else {
                break;
            }
        }
        i += 1;
    }
    out
}

fn read(rel: &str) -> String {
    let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {}", path, e))
}

fn read_rust_tree(rel: &str) -> String {
    fn visit(path: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(&entry.unwrap().path(), files);
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path.to_path_buf());
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut files = Vec::new();
    visit(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn source_between<'a>(text: &'a str, start: &str, end: Option<&str>) -> &'a str {
    let start = text.find(start).unwrap_or_else(|| panic!("missing source marker {start}"));
    let rest = &text[start..];
    match end {
        Some(end) => &rest[..rest.find(end).unwrap_or_else(|| panic!("missing source marker {end}"))],
        None => rest,
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Surface {
    Fixed,
    DirectStatic,
    Value,
    Bespoke,
}

impl Surface {
    fn name(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::DirectStatic => "direct_static",
            Self::Value => "value",
            Self::Bespoke => "bespoke",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Class {
    Covered,
    PurePending,
    Boundary,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::PurePending => "pure_pending",
            Self::Boundary => "boundary",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Entry {
    surface: Surface,
    owner: String,
    method: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Classified {
    entry: Entry,
    class: Class,
    reason: &'static str,
}

fn string_constant_values(text: &str, prefix: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": &str = ") else {
            continue;
        };
        if !name.starts_with(prefix) {
            continue;
        }
        let Some(start) = value.find('"') else {
            continue;
        };
        let Some(end) = value[start + 1..].find('"') else {
            continue;
        };
        let value = &value[start + 1..start + 1 + end];
        values.insert(name.to_string(), value.to_string());
    }
    values
}

fn constant_refs(text: &str, prefix: &str) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    let needle = format!("Syntax::{prefix}");
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + "Syntax::".len()..];
        let len = after
            .bytes()
            .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
            .count();
        refs.insert(after[..len].to_string());
        rest = &after[len..];
    }
    refs
}

fn builtin_constant_refs(text: &str) -> BTreeSet<String> {
    constant_refs(text, "BUILTIN_")
}

fn equality_constant_refs(text: &str, subject: &str) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    let needle = format!("{subject} == ");
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let after = rest[at + needle.len()..].trim_start();
        let after = after.strip_prefix("crate::").unwrap_or(after);
        if let Some(after) = after.strip_prefix("Syntax::") {
            let len = after
                .bytes()
                .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
                .count();
            refs.insert(after[..len].to_string());
        }
        rest = &rest[at + needle.len()..];
    }
    refs
}

fn method_candidates(text: &str) -> BTreeSet<String> {
    extract_quoted(text)
        .into_iter()
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        })
        .collect()
}

fn named(name: &str) -> jet::AST::Type {
    jet::AST::Type::Named(name.to_string())
}

fn apply(name: &str, args: Vec<jet::AST::Type>) -> jet::AST::Type {
    jet::AST::Type::Apply {
        name: name.to_string(),
        args,
    }
}

fn builtin_receivers(collections: &str, syntax: &str) -> Vec<(String, jet::AST::Type)> {
    use jet::AST::Type;
    let mut out = vec![
        ("List".into(), Type::List(Box::new(Type::Int))),
        ("FixedList".into(), Type::FixedList { elem: Box::new(Type::Int), len: 4, len_symbol: None }),
        ("Map".into(), Type::Map { key: Box::new(Type::String), key_span: None, value: Box::new(Type::Int) }),
        ("String".into(), Type::String),
        ("Int".into(), Type::Int),
        ("Float".into(), Type::Float),
        ("F32".into(), Type::Float32),
        ("Bool".into(), Type::Bool),
        ("Char".into(), Type::Char),
        ("Option".into(), Type::Option(Box::new(Type::Int))),
        ("Shared".into(), Type::Shared(Box::new(Type::Int))),
    ];
    let values = string_constant_values(syntax, "");
    let mut named_receivers = extract_quoted(collections).into_iter().collect::<BTreeSet<_>>();
    named_receivers.extend(
        constant_refs(collections, "")
            .into_iter()
            .filter_map(|constant| values.get(&constant).cloned()),
    );
    for owner in named_receivers {
        out.push((owner.clone(), named(&owner)));
        out.push((owner.clone(), apply(&owner, vec![Type::Int])));
        out.push((owner.clone(), apply(&owner, vec![Type::String, Type::Int])));
    }
    for (method, source) in jet::Syntax::NUMERIC_CONVERSION_SOURCES {
        let _ = method;
        if let Some(ty) = jet::AST::numeric_type_from_name(source) {
            let owner = ty.name();
            if !out.iter().any(|(existing, _)| existing == &owner) {
                out.push((owner, ty));
            }
        }
    }
    out
}

fn aot_collection_methods() -> BTreeSet<Entry> {
    let source = read("crates/jet-foundation/src/Collections.rs");
    let syntax = format!("{}\n{}", read("crates/jet-foundation/src/Syntax.rs"), read_rust_tree("crates/jet-foundation/src/Syntax"));
    let mut candidates = method_candidates(&source);
    let values = string_constant_values(&syntax, "");
    candidates.extend(
        constant_refs(&source, "")
            .into_iter()
            .filter_map(|constant| values.get(&constant).cloned()),
    );
    candidates.extend(
        jet::Syntax::NUMERIC_CONVERSION_SOURCES
            .iter()
            .map(|(method, _)| (*method).to_string()),
    );
    let mut entries = BTreeSet::new();
    for (owner, ty) in builtin_receivers(&source, &syntax) {
        for method in &candidates {
            for arity in 0..=8 {
                if jet::Collections::builtin_method_return(&ty, method, arity, false).is_some() {
                    entries.insert(Entry { surface: Surface::Value, owner: owner.clone(), method: method.clone() });
                }
                if jet::Collections::builtin_method_return(&ty, method, arity, true).is_some() {
                    entries.insert(Entry { surface: Surface::DirectStatic, owner: owner.clone(), method: method.clone() });
                }
            }
        }
    }
    entries
}

fn aot_special_collection_constructors() -> BTreeSet<Entry> {
    let source = read("crates/jet-codegen/src/Codegen/TIR/subset/methods.rs");
    extract_any_string_pairs(source_between(
        &source,
        "// Shape (d-coll-ctor)",
        Some("// D-MEM1 S6"),
    ))
    .into_iter()
    .map(|(owner, method)| Entry {
        surface: Surface::DirectStatic,
        owner,
        method,
    })
    .collect()
}

fn aot_specialized_value_methods() -> BTreeSet<Entry> {
    fn add_arm(entries: &mut BTreeSet<Entry>, source: &str, owner: &str, marker: &str) {
        for method in method_candidates(source_between(source, marker, Some("_ => None,"))) {
            entries.insert(Entry {
                surface: Surface::Value,
                owner: owner.to_string(),
                method,
            });
        }
    }

    let time = read("crates/jet-sema/src/Sema/CheckerCoreLib/net_text_time.rs");
    let mut entries = BTreeSet::new();
    add_arm(
        &mut entries,
        &time,
        "Mime",
        "Type::Named(n) if n == \"Mime\" => match method {",
    );
    let date_marker =
        "Type::Named(n) if n == \"Date\" || n == \"LocalDate\" => match method {";
    add_arm(&mut entries, &time, "Date", date_marker);
    add_arm(&mut entries, &time, "LocalDate", date_marker);
    for owner in ["LocalTime", "DateTime", "Instant", "Period", "Zone", "ZonedDateTime"] {
        add_arm(
            &mut entries,
            &time,
            owner,
            &format!("Type::Named(n) if n == \"{owner}\" => match method {{"),
        );
    }

    let inference = read("crates/jet-sema/src/Sema/CheckerInfer/calls/method_calls.rs");
    for method in method_candidates(source_between(
        &inference,
        "// D-HONESTNUM1=A: methods on `Measurement<Float>`",
        Some("// D-MEM1 S6"),
    )) {
        entries.insert(Entry {
            surface: Surface::Value,
            owner: "Measurement".to_string(),
            method,
        });
    }
    entries
}

fn ct_value_methods(text: &str) -> BTreeSet<(String, String)> {
    let variants = [
        ("List", "List"),
        ("Bytes", "List"),
        ("Map", "Map"),
        ("Str", "String"),
        ("Int", "Int"),
        ("Float", "Float"),
        ("Bool", "Bool"),
        ("Char", "Char"),
        ("BigInt", "BigInt"),
        ("Some", "Option"),
        ("None", "Option"),
    ];
    let syntax = format!("{}\n{}", read("crates/jet-foundation/src/Syntax.rs"), read_rust_tree("crates/jet-foundation/src/Syntax"));
    let values = string_constant_values(&syntax, "");
    let mut out = BTreeSet::new();
    for (start, end) in tuple_ranges(text) {
        let fields = split_top_level(&text[start + 1..end - 1], ',');
        if fields.len() >= 2 {
            let methods = extract_quoted(fields[1]);
            for (variant, owner) in variants {
                if fields[0].contains(&format!("CtValue::{variant}")) {
                    for method in &methods {
                        out.insert((owner.to_string(), method.clone()));
                    }
                }
            }
            if fields[0].trim() == "v" {
                for (_, owner) in variants {
                    for method in &methods {
                        out.insert((owner.to_string(), method.clone()));
                    }
                }
            }
            if fields[0].contains("CtValue::Struct") {
                let rest = &text[end..];
                let guard = &rest[..rest.find("=>").unwrap_or(0)];
                let mut owners = string_equalities(guard, "type_name");
                owners.extend(
                    equality_constant_refs(guard, "type_name")
                        .into_iter()
                        .filter_map(|constant| values.get(&constant).cloned()),
                );
                if guard.contains("type_name.as_str()") {
                    owners.extend(
                        extract_quoted(guard)
                            .into_iter()
                            .filter(|owner| owner.chars().next().is_some_and(|ch| ch.is_ascii_uppercase())),
                    );
                }
                let mut methods = extract_quoted(fields[1]);
                if fields[1].trim() == "method" {
                    methods.extend(
                        extract_quoted(guard)
                            .into_iter()
                            .filter(|method| method.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')),
                    );
                }
                for owner in owners {
                    for method in &methods {
                        out.insert((owner.clone(), method.clone()));
                    }
                }
            }
        }
    }
    out
}

fn guarded_static_methods(text: &str) -> BTreeSet<(String, String)> {
    let syntax = format!(
        "{}\n{}",
        read("crates/jet-foundation/src/Syntax.rs"),
        read_rust_tree("crates/jet-foundation/src/Syntax")
    );
    let values = string_constant_values(&syntax, "");
    let mut out = BTreeSet::new();
    let needle = "if type_name == ";
    for at in rust_code_occurrences(text, needle) {
        let after = &text[at + needle.len()..];
        let Some(header) = rust_header_before_block(after) else {
            continue;
        };
        let owner = if let Some(quoted) = header.trim_start().strip_prefix('"') {
            quoted.find('"').map(|end| quoted[..end].to_string())
        } else {
            let token = header
                .trim_start()
                .split_whitespace()
                .next()
                .unwrap_or_default();
            token
                .rsplit("::")
                .next()
                .and_then(|constant| values.get(constant).cloned())
        };
        if let Some(owner) = owner {
            let mut methods = string_equalities(&header, "method");
            if let Some(matches) = header.find("matches!(method,") {
                methods.extend(extract_quoted(&header[matches..]));
            }
            for method in methods {
                out.insert((owner.clone(), method));
            }
        }
    }
    out
}

fn ct_static_methods(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (owner, method) in extract_any_string_pairs(text) {
        if owner.chars().next().is_some_and(|ch| ch.is_ascii_uppercase()) {
            out.insert((owner, method));
        }
    }
    let collections = read("crates/jet-foundation/src/Collections.rs");
    let syntax = format!("{}\n{}", read("crates/jet-foundation/src/Syntax.rs"), read_rust_tree("crates/jet-foundation/src/Syntax"));
    let receivers = builtin_receivers(&collections, &syntax);
    for (method, _) in jet::Syntax::NUMERIC_CONVERSION_SOURCES {
        for (owner, ty) in &receivers {
            if ty.is_numeric() {
                out.insert((owner.clone(), (*method).to_string()));
            }
        }
    }
    out.extend(guarded_static_methods(text));
    out
}

fn ct_build_context_methods() -> BTreeSet<String> {
    let bridge = read("crates/jet-comptime/src/Comptime/Build/runtime_bridge.rs");
    let mut methods = method_candidates(source_between(
        &bridge,
        "let result = match method {",
        Some("_ => return None.ok_or_else"),
    ));
    let dispatch = read("crates/jet-comptime/src/Comptime/Methods/dispatch.rs");
    for method in ["find", "fetch", "embed"] {
        if dispatch.contains(&format!("method == \"{method}\""))
            || dispatch.contains(&format!("\"{method}\" =>"))
        {
            methods.insert(method.to_string());
        }
    }
    methods
}

fn string_equalities(text: &str, subject: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let needle = format!("{subject} == \"");
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let value = &rest[at + needle.len()..];
        if let Some(end) = value.find('"') {
            names.insert(value[..end].to_string());
            rest = &value[end + 1..];
        } else {
            break;
        }
    }
    names
}

fn direct_dispatch_names(text: &str) -> BTreeSet<String> {
    string_equalities(text, "name")
}

fn extract_any_string_pairs(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (start, end) in tuple_ranges(text) {
        let fields = split_top_level(&text[start + 1..end - 1], ',');
        if fields.len() >= 2 {
            for owner in extract_quoted(fields[0]) {
                for method in extract_quoted(fields[1]) {
                    out.insert((owner.clone(), method));
                }
            }
        }
    }
    out
}

fn core_boundary(module: &str, method: &str) -> Option<&'static str> {
    if EFFECT_GATED_MODULES.contains(&module) {
        return Some("named comptime effect gate");
    }
    if matches!(module, "core.os" | "core.raylib" | "core.ui" | "core.term" | "core.web" | "core.web.storage.local" | "core.web.storage.session" | "core.tasks" | "core.watcher" | "core.web.devserver" | "core.uuid") {
        return Some("named ambient/native boundary");
    }
    if matches!(module, "core.event" | "core.http.client" | "core.http.server" | "core.mem" | "core.scope") {
        return Some("named runtime/native boundary");
    }
    if (module.starts_with("core.encoding.") && matches!(method, "reader" | "writer"))
        || (module == "core.email" && matches!(method, "smtp" | "smtp_from_env"))
    {
        return Some("named I/O handle boundary");
    }
    if module == "core.time" && matches!(method, "now" | "now_utc" | "today" | "local_time" | "instant" | "sleep" | "start") {
        return Some("named clock boundary");
    }
    if matches!(
        module,
        "core.auth" | "core.crypto.expert" | "core.crypto.random" | "core.vault" | "core.vault.expert"
    ) {
        return Some("named native/security boundary");
    }
    if (module == "core.time.date" && method == "today")
        || (module == "core.time.datetime" && method == "now")
        || (module == "core.time.expiring" && method == "new")
    {
        return Some("named clock boundary");
    }
    None
}

fn value_boundary(owner: &str) -> Option<&'static str> {
    if owner == "Instant" {
        return Some("named clock boundary");
    }
    matches!(owner, "Task" | "Receiver" | "Sender" | "Event" | "AsyncEvent" | "DispatchReport" | "Hook" | "Subscription" | "EventScope" | "EventTrace" | "WatchHandle" | "WatchSet" | "SigningKey" | "X25519SecretKey" | "VerifyKey" | "X25519PublicKey" | "Signature" | "Sealed" | "WrappedKey" | "Digest256" | "Digest512" | "PasswordHash")
        .then_some("named runtime/native handle boundary")
}

fn value_method_boundary(owner: &str, method: &str) -> Option<&'static str> {
    match (owner, method) {
        ("List" | "FixedList", "view") => Some("named E0214 retired spelling boundary"),
        ("FixedList", "insert") => Some("named E0964 fixed-length boundary"),
        _ => None,
    }
}

fn comptime_sequence_methods() -> BTreeSet<String> {
    let source = read("crates/jet-comptime/src/Comptime/SequenceParity.rs");
    method_candidates(source_between(
        &source,
        "if !matches!(",
        Some("return None;"),
    ))
}

fn discover_inventory() -> BTreeSet<Entry> {
    let fixed_src = read("crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs");
    let bespoke_src = read("crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs");
    let fixed = extract_pairs(&fixed_src);
    let core_all = extract_pairs(&bespoke_src);
    let mut entries = BTreeSet::new();
    for (module, method) in &fixed {
        if EXTRACTOR_ARTIFACTS.contains(&(module.as_str(), method.as_str())) {
            continue;
        }
        entries.insert(Entry { surface: Surface::Fixed, owner: module.clone(), method: method.clone() });
    }
    for (module, method) in core_all.difference(&fixed) {
        entries.insert(Entry { surface: Surface::Bespoke, owner: module.clone(), method: method.clone() });
    }

    let syntax_src = format!("{}\n{}", read("crates/jet-foundation/src/Syntax.rs"), read_rust_tree("crates/jet-foundation/src/Syntax"));
    let values = string_constant_values(&syntax_src, "");
    let direct_src = read("crates/jet-sema/src/Sema/CheckerInfer/calls/direct_calls.rs");
    let mut direct_aot = builtin_constant_refs(&direct_src);
    direct_aot.extend(
        equality_constant_refs(&direct_src, "call.name")
            .into_iter()
            .filter(|constant| constant.starts_with("TYPE_") || constant == "RESOURCE_CLOSE"),
    );
    for constant in direct_aot {
        let spelling = values.get(&constant).cloned().unwrap_or_else(|| constant.clone());
        entries.insert(Entry { surface: Surface::DirectStatic, owner: "direct".into(), method: spelling });
    }
    let math_src = read("crates/jet-sema/src/Sema/CheckerCoreLib/math_layout.rs");
    let math_types = math_src
        .split("pub fn math_scalar_ty")
        .next()
        .map(extract_quoted)
        .unwrap_or_default();
    for name in math_types
        .into_iter()
        .filter(|name| name.chars().next().is_some_and(|ch| ch.is_ascii_uppercase()))
    {
        entries.insert(Entry { surface: Surface::DirectStatic, owner: "direct".into(), method: name });
    }
    entries.extend(aot_collection_methods());
    entries.extend(aot_special_collection_constructors());
    entries.extend(aot_specialized_value_methods());
    entries
}

fn classify_inventory(discovered: &BTreeSet<Entry>) -> Result<Vec<Classified>, Vec<String>> {
    let core_dispatch_src = read_rust_tree("crates/jet-comptime/src/Comptime");
    let builtin_dispatch_src = read("crates/jet-comptime/src/Comptime/Builtins.rs");
    let dispatch_src = read("crates/jet-comptime/src/Comptime/Methods/dispatch.rs");
    let ct_core = extract_pairs(&core_dispatch_src);
    let gaps = KNOWN_OPEN_GAPS.iter().copied().collect::<BTreeSet<_>>();
    let syntax_src = format!("{}\n{}", read("crates/jet-foundation/src/Syntax.rs"), read_rust_tree("crates/jet-foundation/src/Syntax"));
    let values = string_constant_values(&syntax_src, "");
    let mut direct_ct = builtin_constant_refs(&dispatch_src);
    direct_ct.extend(equality_constant_refs(&dispatch_src, "name"));
    let mut direct_names = direct_dispatch_names(&dispatch_src);
    direct_names.extend(
        direct_ct
            .iter()
            .filter_map(|constant| values.get(constant).cloned()),
    );
    // Value methods live in both the leaf builtin table and the interpreter
    // spine: higher-order calls need `&mut self` for closure dispatch, while
    // mutators need write-back. Audit the complete canonical comptime tree so
    // those real paths are not mislabeled as open gaps.
    let mut ct_values = ct_value_methods(&core_dispatch_src);
    let core_pure_source = read("crates/jet-comptime/src/Comptime/CorePureParity.rs");
    let core_pure_methods = source_between(
        &core_pure_source,
        "pub(super) fn evaluate_method(",
        Some("fn one<'a>("),
    );
    ct_values.extend(
        extract_any_string_pairs(core_pure_methods)
            .into_iter()
            .filter(|(owner, _)| {
                owner
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
            }),
    );
    let mut ct_statics = ct_static_methods(source_between(
        &builtin_dispatch_src,
        "pub(super) fn apply_static_type_method(",
        Some("pub(super) fn apply_mutating("),
    ));
    ct_statics.extend(guarded_static_methods(&dispatch_src));
    let ct_build_context = ct_build_context_methods();
    let sequence_methods = comptime_sequence_methods();
    let view_methods = [
        "contains", "first", "fold", "get", "index_of", "is_empty", "last", "len", "map",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let mut errors = Vec::new();
    for &(module, method) in KNOWN_OPEN_GAPS {
        let pair_is_discovered = discovered.iter().any(|entry| {
            matches!(entry.surface, Surface::Fixed | Surface::Bespoke)
                && entry.owner == module
                && entry.method == method
        });
        if !pair_is_discovered {
            errors.push(format!("stale pure_pending gap: {module}.{method} is no longer discovered"));
        } else if ct_core.contains(&(module.to_string(), method.to_string())) {
            errors.push(format!("stale pure_pending gap: {module}.{method} is now comptime-covered"));
        }
    }
    let mut out = Vec::new();
    for entry in discovered.iter().cloned() {
        let classified = match entry.surface {
            Surface::Fixed | Surface::Bespoke => {
                let pair = (entry.owner.clone(), entry.method.clone());
                if ct_core.contains(&pair) {
                    Some((Class::Covered, "comptime core dispatch"))
                } else if let Some(reason) = core_boundary(&entry.owner, &entry.method) {
                    Some((Class::Boundary, reason))
                } else if gaps.contains(&(entry.owner.as_str(), entry.method.as_str())) {
                    Some((Class::PurePending, "explicit pure port backlog"))
                } else {
                    None
                }
            }
            Surface::DirectStatic if entry.owner == "direct" => {
                if matches!(entry.method.as_str(), "print" | "input") {
                    Some((Class::Boundary, "named interactive I/O boundary"))
                } else if entry.method == "close" {
                    Some((Class::Boundary, "named runtime resource boundary"))
                } else if direct_names.contains(&entry.method) {
                    Some((Class::Covered, "comptime direct dispatch"))
                } else if matches!(
                    entry.method.as_str(),
                    "consume" | "expect" | "checked" | "saturating" | "wrapping" | "Decimal"
                        | "F32x4" | "F64x2" | "Vec2" | "Vec3" | "Vec4" | "Mat3" | "Mat4"
                ) {
                    Some((Class::PurePending, "direct builtin port pending"))
                } else {
                    None
                }
            }
            Surface::DirectStatic => {
                if ct_statics.contains(&(entry.owner.clone(), entry.method.clone())) {
                    Some((Class::Covered, "comptime static dispatch"))
                } else if let Some(reason) = value_boundary(&entry.owner) {
                    Some((Class::Boundary, reason))
                } else {
                    Some((Class::PurePending, "static method port pending"))
                }
            }
            Surface::Value => {
                let owner = if entry.owner == "FixedList" { "List" } else { &entry.owner };
                let erased_scalar_dispatch = match entry.owner.as_str() {
                    "I8" | "I16" | "I32" | "U8" | "U16" | "U32" | "U64" => {
                        matches!(
                            entry.method.as_str(),
                            "to_string"
                                | "count_ones"
                                | "count_zeros"
                                | "leading_zeros"
                                | "trailing_zeros"
                        )
                            && ct_values.contains(&("Int".to_string(), entry.method.clone()))
                    }
                    _ => false,
                };
                if let Some(reason) = value_method_boundary(&entry.owner, &entry.method) {
                    Some((Class::Boundary, reason))
                } else if owner == "BuildContext" && ct_build_context.contains(&entry.method) {
                    Some((Class::Covered, "interpreter-owned build context dispatch"))
                } else if matches!(entry.owner.as_str(), "List" | "FixedList")
                    && sequence_methods.contains(&entry.method)
                {
                    Some((Class::Covered, "comptime sequence dispatch"))
                } else if matches!(entry.owner.as_str(), "View" | "ViewMut")
                    && view_methods.contains(entry.method.as_str())
                {
                    Some((Class::Covered, "comptime slice-view dispatch"))
                } else if erased_scalar_dispatch {
                    Some((Class::Covered, "comptime scalar representation dispatch"))
                } else if ct_values.contains(&(owner.to_string(), entry.method.clone())) {
                    Some((Class::Covered, "comptime value dispatch"))
                } else if let Some(reason) = value_boundary(&entry.owner) {
                    Some((Class::Boundary, reason))
                } else {
                    Some((Class::PurePending, "value method port pending"))
                }
            }
        };
        match classified {
            Some((class, reason)) => out.push(Classified { entry, class, reason }),
            None => errors.push(format!("unclassified {} {}.{}", entry.surface.name(), entry.owner, entry.method)),
        }
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

fn render_inventory(records: &[Classified]) -> String {
    let mut lines = records
        .iter()
        .map(|record| format!("{} | {} | {}.{} | {}", record.class.name(), record.entry.surface.name(), record.entry.owner, record.entry.method, record.reason))
        .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n") + "\n"
}

fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn validate_records(discovered: &BTreeSet<Entry>, records: &[Classified]) -> Result<(), Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for record in records {
        if !seen.insert(record.entry.clone()) {
            errors.push(format!(
                "duplicate classification: {} {}.{}",
                record.entry.surface.name(), record.entry.owner, record.entry.method
            ));
        }
        if !discovered.contains(&record.entry) {
            errors.push(format!(
                "stale {} classification: {} {}.{}",
                record.class.name(), record.entry.surface.name(), record.entry.owner, record.entry.method
            ));
        }
    }
    for entry in discovered.difference(&seen) {
        errors.push(format!(
            "unclassified: {} {}.{}",
            entry.surface.name(), entry.owner, entry.method
        ));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn record<'a>(records: &'a [Classified], surface: Surface, owner: &str, method: &str) -> &'a Classified {
    records
        .iter()
        .find(|record| {
            record.entry.surface == surface
                && record.entry.owner == owner
                && record.entry.method == method
        })
        .unwrap_or_else(|| panic!("missing {} {owner}.{method}", surface.name()))
}

#[test]
fn canonical_builtin_inventory_is_complete_and_stable() {
    let discovered = discover_inventory();
    let records = classify_inventory(&discovered).unwrap_or_else(|errors| panic!("{}", errors.join("\n")));
    validate_records(&discovered, &records).unwrap();

    let fixed = records.iter().filter(|record| record.entry.surface == Surface::Fixed).count();
    let direct_static = records.iter().filter(|record| record.entry.surface == Surface::DirectStatic).count();
    let value = records.iter().filter(|record| record.entry.surface == Surface::Value).count();
    let bespoke = records.iter().filter(|record| record.entry.surface == Surface::Bespoke).count();
    assert_eq!((fixed, direct_static, value, bespoke), (495, 148, 486, 49));

    assert_eq!(record(&records, Surface::Fixed, "core.math", "round").class, Class::Covered);
    assert_eq!(record(&records, Surface::Fixed, "core.encoding.json", "to_string_pretty").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.reactive.loadable", "idle").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.reactive.loadable", "loading").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.reactive.loadable", "loaded").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.reactive.loadable", "failed").class, Class::Covered);
    assert_eq!(record(&records, Surface::DirectStatic, "Int", "parse").class, Class::Covered);
    assert_eq!(record(&records, Surface::DirectStatic, "Set", "from").class, Class::Covered);
    for (owner, method) in [
        ("SortedSet", "from"),
        ("SortedSet", "new"),
        ("PriorityQueue", "from"),
        ("PriorityQueue", "new"),
        ("Lru", "new"),
        ("Deque", "new"),
        ("BitSet", "new"),
        ("ByteBuffer", "from"),
        ("ByteBuffer", "new"),
    ] {
        assert_eq!(
            record(&records, Surface::DirectStatic, owner, method).class,
            Class::Covered
        );
    }
    assert_eq!(record(&records, Surface::DirectStatic, "Bag", "new").class, Class::Covered);
    assert_eq!(record(&records, Surface::DirectStatic, "Secret", "from_text").class, Class::PurePending);
    assert_eq!(record(&records, Surface::DirectStatic, "direct", "BigInt").class, Class::Covered);
    assert_eq!(record(&records, Surface::DirectStatic, "direct", "Decimal").class, Class::PurePending);
    assert_eq!(record(&records, Surface::DirectStatic, "direct", "Vec3").class, Class::PurePending);
    assert_eq!(record(&records, Surface::Value, "String", "trim").class, Class::Covered);
    for method in ["after", "before", "bytes", "slice"] {
        assert_eq!(record(&records, Surface::Value, "String", method).class, Class::Covered);
    }
    for method in ["is_nan", "is_infinite", "is_finite"] {
        assert_eq!(record(&records, Surface::Value, "Float", method).class, Class::Covered);
        assert_eq!(record(&records, Surface::Value, "F32", method).class, Class::PurePending);
    }
    // CtValue::Float currently erases F32's width. Do not infer coverage from
    // Float dispatch until comptime preserves f32 rounding across stored values.
    assert_eq!(record(&records, Surface::Value, "F32", "to_string").class, Class::PurePending);
    for owner in ["I8", "I16", "I32", "U8", "U16", "U32", "U64"] {
        assert_eq!(record(&records, Surface::Value, owner, "to_string").class, Class::Covered);
    }
    assert_eq!(record(&records, Surface::Value, "Float", "origin").class, Class::PurePending);
    for owner in ["Int", "I8", "I16", "I32", "U8", "U16", "U32", "U64"] {
        for method in [
            "count_ones",
            "count_zeros",
            "leading_zeros",
            "trailing_zeros",
        ] {
            assert_eq!(record(&records, Surface::Value, owner, method).class, Class::Covered);
        }
    }
    assert_eq!(record(&records, Surface::Value, "BigInt", "to_string").class, Class::Covered);
    for method in ["add", "add_new", "clear", "each", "has_key"] {
        assert_eq!(record(&records, Surface::Value, "Map", method).class, Class::Covered);
    }
    assert_eq!(record(&records, Surface::Value, "List", "filter").class, Class::Covered);
    assert_eq!(record(&records, Surface::Value, "List", "clear").class, Class::Covered);
    let sequence_methods = comptime_sequence_methods();
    assert_eq!(sequence_methods.len(), 39, "SequenceParity method-set drift");
    for owner in ["List", "FixedList"] {
        for method in &sequence_methods {
            let expected = if owner == "FixedList" && method == "insert" {
                Class::Boundary
            } else {
                Class::Covered
            };
            assert_eq!(record(&records, Surface::Value, owner, method).class, expected);
        }
        assert_eq!(record(&records, Surface::Value, owner, "view").class, Class::Boundary);
    }
    for owner in ["View", "ViewMut"] {
        for method in [
            "contains", "first", "fold", "get", "index_of", "is_empty", "last", "len", "map",
        ] {
            assert_eq!(record(&records, Surface::Value, owner, method).class, Class::Covered);
        }
    }
    assert_eq!(record(&records, Surface::Value, "BuildContext", "generate").class, Class::Covered);
    assert_eq!(record(&records, Surface::Value, "Duration", "in").class, Class::Covered);
    for method in [
        "int",
        "float",
        "float_range",
        "bool",
        "normal",
        "exponential",
        "bytes",
        "split",
        "pick",
        "weighted_pick",
        "sample",
        "shuffle",
    ] {
        assert_eq!(record(&records, Surface::Value, "Rng", method).class, Class::Covered);
    }
    for method in [
        "len",
        "is_empty",
        "clear",
        "to_bytes",
        "write_u8",
        "write_u16_le",
        "write_u16_be",
        "write_u32_le",
        "write_u32_be",
        "write_u64_le",
        "write_u64_be",
        "write_bytes",
    ] {
        assert_eq!(record(&records, Surface::Value, "ByteBuffer", method).class, Class::Covered);
    }
    for method in [
        "add",
        "add_new",
        "get",
        "remove",
        "has_key",
        "keys",
        "capacity",
        "len",
        "is_empty",
        "clear",
    ] {
        assert_eq!(record(&records, Surface::Value, "Lru", method).class, Class::Covered);
    }
    for method in [
        "push_front",
        "push_back",
        "pop_front",
        "pop_back",
        "peek_front",
        "peek_back",
        "len",
        "is_empty",
        "clear",
    ] {
        assert_eq!(record(&records, Surface::Value, "Deque", method).class, Class::Covered);
    }
    for method in [
        "add",
        "remove",
        "has",
        "first",
        "last",
        "union",
        "to_list",
        "len",
        "is_empty",
        "clear",
    ] {
        assert_eq!(record(&records, Surface::Value, "SortedSet", method).class, Class::Covered);
    }
    for method in [
        "add", "remove", "has", "count", "len", "is_empty", "clear", "to_list",
    ] {
        assert_eq!(record(&records, Surface::Value, "BitSet", method).class, Class::Covered);
    }
    for method in [
        "push",
        "pop",
        "peek",
        "to_sorted_list",
        "len",
        "is_empty",
        "clear",
    ] {
        assert_eq!(
            record(&records, Surface::Value, "PriorityQueue", method).class,
            Class::Covered
        );
    }
    for method in [
        "add", "remove", "has", "union", "to_list", "len", "is_empty", "clear",
    ] {
        assert_eq!(record(&records, Surface::Value, "Set", method).class, Class::Covered);
    }
    for method in ["add", "remove", "has", "count", "len", "is_empty", "any"] {
        assert_eq!(record(&records, Surface::Value, "Bag", method).class, Class::Covered);
    }
    for method in ["media_type", "subtype", "essence", "to_string", "param", "params"] {
        assert_eq!(record(&records, Surface::Value, "Mime", method).class, Class::Covered);
    }
    for owner in ["Date", "LocalDate"] {
        for method in [
            "year",
            "month",
            "day",
            "to_string",
            "add_days",
            "add_months",
            "add_period",
            "day_of_year",
            "diff_days",
            "format",
            "iso_week",
            "iso_weekday",
            "truncate",
            "weekday",
        ] {
            assert_eq!(record(&records, Surface::Value, owner, method).class, Class::Covered);
        }
    }
    for method in ["hour", "minute", "second", "to_string"] {
        assert_eq!(record(&records, Surface::Value, "LocalTime", method).class, Class::Covered);
    }
    for method in ["to_timestamp", "to_unix_ms", "to_string"] {
        assert_eq!(record(&records, Surface::Value, "DateTime", method).class, Class::Covered);
    }
    assert_eq!(record(&records, Surface::Value, "DateTime", "in_zone").class, Class::Covered);
    assert_eq!(record(&records, Surface::Value, "Zone", "name").class, Class::Covered);
    for method in [
        "add_duration",
        "add_period",
        "date",
        "format",
        "offset_seconds",
        "time",
        "to_datetime",
        "to_string",
        "zone",
    ] {
        assert_eq!(record(&records, Surface::Value, "ZonedDateTime", method).class, Class::Covered);
    }
    for method in ["utc", "zone", "zoned", "zoned_local"] {
        assert_eq!(record(&records, Surface::Fixed, "core.time", method).class, Class::Covered);
    }
    for method in [
        "date",
        "format",
        "format_rfc3339",
        "hour",
        "minute",
        "plus_duration",
        "round",
        "second",
        "time",
        "truncate",
    ] {
        assert_eq!(record(&records, Surface::Value, "DateTime", method).class, Class::Covered);
    }
    assert_eq!(record(&records, Surface::Value, "Period", "to_string").class, Class::Covered);
    for method in ["value", "uncertainty"] {
        assert_eq!(record(&records, Surface::Value, "Measurement", method).class, Class::Covered);
    }
    for method in ["add", "sub", "mul", "div"] {
        assert_eq!(record(&records, Surface::Value, "Measurement", method).class, Class::Covered);
    }
    assert_eq!(record(&records, Surface::Value, "Instant", "elapsed_millis").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Value, "Task", "detach").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Bespoke, "core.data", "inner_join").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.data", "left_join").class, Class::Covered);
    assert_eq!(record(&records, Surface::Bespoke, "core.data", "pivot_sum").class, Class::Covered);
    assert_eq!(
        record(&records, Surface::Fixed, "core.testing", "fake_rng").class,
        Class::Covered
    );
    assert_eq!(
        record(&records, Surface::Fixed, "core.testing", "fake_clock").class,
        Class::Covered
    );
    assert_eq!(record(&records, Surface::Fixed, "core.time", "now").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Fixed, "core.crypto.expert", "open_v1").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Fixed, "core.crypto.expert", "migrate_v1").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Fixed, "core.vault.expert", "prepare_import_signing").class, Class::Boundary);
    assert_eq!(record(&records, Surface::Fixed, "core.vault.expert", "commit_import_x25519").class, Class::Boundary);

    let rendered = render_inventory(&records);
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(render_inventory(&reversed), rendered, "inventory rendering must be order-stable");
    let covered = records.iter().filter(|record| record.class == Class::Covered).count();
    let pending = records.iter().filter(|record| record.class == Class::PurePending).count();
    let boundaries = records.iter().filter(|record| record.class == Class::Boundary).count();
    assert_eq!((records.len(), covered, pending, boundaries), (1_178, 748, 67, 363));
    eprintln!(
        "builtin parity inventory: {} total, {covered} covered, {pending} pure pending, {boundaries} boundaries",
        records.len()
    );
    assert_eq!(
        stable_hash(&rendered),
        3127679427168958324,
        "intentional inventory movement must update the reviewed stable hash; counts fixed={fixed} direct_static={direct_static} value={value} bespoke={bespoke}"
    );
}

#[test]
fn inventory_validator_rejects_duplicate_stale_and_unclassified_entries() {
    let live = Entry { surface: Surface::Value, owner: "String".into(), method: "trim".into() };
    let stale_covered = Entry { surface: Surface::Fixed, owner: "core.fake".into(), method: "covered".into() };
    let stale_pending = Entry { surface: Surface::Fixed, owner: "core.fake".into(), method: "pending".into() };
    let stale_boundary = Entry { surface: Surface::Fixed, owner: "core.fake".into(), method: "boundary".into() };
    let discovered = [live.clone()].into_iter().collect::<BTreeSet<_>>();
    let duplicate = Classified { entry: live.clone(), class: Class::Covered, reason: "proof" };
    let records = vec![
        duplicate.clone(),
        duplicate,
        Classified { entry: stale_covered, class: Class::Covered, reason: "old" },
        Classified { entry: stale_pending, class: Class::PurePending, reason: "old" },
        Classified { entry: stale_boundary, class: Class::Boundary, reason: "old" },
    ];
    let errors = validate_records(&discovered, &records).unwrap_err().join("\n");
    assert!(errors.contains("duplicate classification: value String.trim"), "{errors}");
    assert!(errors.contains("stale covered classification: fixed core.fake.covered"), "{errors}");
    assert!(errors.contains("stale pure_pending classification: fixed core.fake.pending"), "{errors}");
    assert!(errors.contains("stale boundary classification: fixed core.fake.boundary"), "{errors}");

    let errors = validate_records(&discovered, &[]).unwrap_err().join("\n");
    assert!(errors.contains("unclassified: value String.trim"), "{errors}");

    let direct = direct_dispatch_names(
        "if name == \"require\" { dispatch(); } fn helper() { let name = \"require_eq\"; }",
    );
    assert!(direct.contains("require"));
    assert!(!direct.contains("require_eq"), "helper-only string must not prove dispatch coverage");

    let fake_core = "// (\"core.fake\", \"comment\")\nlet text = \"(\\\"core.fake\\\", \\\"string\\\")\";";
    assert!(extract_pairs(fake_core).is_empty(), "comment/string tuples must not prove dispatch coverage");

    let fake_statics = r###"
// if type_name == crate::Syntax::TYPE_SET && method == "from" {
/* if type_name == crate::Syntax::TYPE_BIT_SET && method == "new" { */
let example = r#"if type_name == crate::Syntax::TYPE_DEQUE && method == "new" {"#;
"###;
    assert!(
        guarded_static_methods(fake_statics).is_empty(),
        "comment/raw-string guards must not prove static dispatch coverage"
    );

    let commented_method = r#"
if type_name == crate::Syntax::TYPE_SET // method == "from"
{
    disabled_example()
}
"#;
    assert!(
        guarded_static_methods(commented_method).is_empty(),
        "a commented method must not complete an active type guard"
    );
}
