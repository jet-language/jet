//! Card #392: AOT-vs-comptime builtin parity matrix.
//!
//! Enumerates every `("core.<module>", "<method>")` pair AOT's sema accepts
//! (`crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs::core_fixed_sig` +
//! its `is_polymorphic_core_special` sibling table) and diffs it against
//! every pair the comptime/REPL tier-0 interpreter dispatches
//! (`crates/jet-comptime/src/Comptime/Methods.rs::apply_core_call`).
//!
//! A pair present in AOT but absent from comptime means `use core.x as a;
//! a.f(...)` type-checks and compiles for `jet build`, but hits E0956 in
//! `jet dev`/the REPL/a `comptime` binding — an R12 parity bug (silent
//! AOT-only builtin). New builtins must add themselves to comptime's
//! dispatch (or, for a genuinely-impossible-at-comptime effect, to
//! `KNOWN_OPEN_GAPS` below with a one-line reason) or this test fails.
//!
//! Effectful modules (`core.files`/`core.env`/`core.io`/`core.exec`/
//! `core.net`/`core.tls`/`core.process`) are exempt: comptime handles them as a whole
//! via the `is_tier2` wildcard (E3410 `#Impure` gate), not per-method, so a
//! per-name diff would just be noise — those modules are filtered out below.

use std::collections::BTreeSet;
use std::fs;

/// Modules comptime gates wholesale behind `#Impure` (Methods.rs `is_tier2`)
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
    // `#Impure` escape hatch at all here, unlike the effects above. Correctly
    // handled, just not through the per-method `apply_core_call` table this
    // audit reads — excluded so it isn't flagged as a silent gap.
    "core.vault",
];

/// Known, currently-open comptime gaps: real AOT `(module, method)` pairs
/// with no comptime dispatch yet, each with why it isn't in this card's
/// slice. Every entry here is a to-do, not a shrug — remove a line the same
/// PR that closes the gap. Anything AOT supports that ISN'T listed here and
/// isn't dispatched by comptime fails this test.
const KNOWN_OPEN_GAPS: &[(&str, &str)] = &[
    // core.text: full Unicode Standard normalization/segmentation (NFC/NFD/
    // NFKC/NFKD compose+decompose tables, grapheme/word/sentence boundary
    // algorithms) is a large, separate undertaking even though AOT's own
    // versions (`jet_text_*` in Text.rs) are themselves hand-rolled
    // approximations, not full UAX-compliant tables — porting the
    // approximation was in scope (done, see `TextLite.rs`) but the
    // underlying algorithm gap versus true Unicode isn't this card's job to
    // fix on either tier. (Card #392 ported everything else in core.text.)
    // core.time: mixes pure value construction (`period`/`hours`/`minutes`/
    // `seconds`, `Duration`-style) with genuine ambient effects (`now`/
    // `today`/`utc`/`local_time`/`sleep`/`instant` — wall-clock/monotonic
    // clock reads, non-deterministic). Splitting the pure half out and
    // gating the effectful half behind `#Impure` (like `core.files` etc)
    // needs its own pass to avoid rushing the effect boundary.
    ("core.time", "now"),
    ("core.time", "now_utc"),
    ("core.time", "today"),
    ("core.time", "utc"),
    ("core.time", "local_time"),
    ("core.time", "zoned"),
    ("core.time", "zoned_local"),
    ("core.time", "zone"),
    ("core.time", "instant"),
    ("core.time", "sleep"),
    ("core.time", "start"),
    ("core.time", "period"),
    ("core.time", "period_days"),
    ("core.time", "period_months"),
    ("core.time", "period_years"),
    ("core.time", "hours"),
    ("core.time", "minutes"),
    ("core.time", "seconds"),
    ("core.time", "from_unix_ms"),
    ("core.time", "parse_rfc3339"),
    ("core.time", "parse_time"),
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
    // `inner_join`/`left_join`/`pivot_sum` are a pre-existing, separate blind
    // spot in this test's own extractor (they live in `core_call.rs`'s
    // `infer_core_call`, not `fixed_sigs.rs`, so this scan never sees them) —
    // still an open gap at comptime, but out of this test's coverage either
    // way; left for a future card rather than folded into this one's scope.
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
    ("core.crypto.expert", "aes256_gcm_open"),
    ("core.crypto.expert", "aes256_gcm_seal"),
    ("core.crypto.expert", "chacha20_open"),
    ("core.crypto.expert", "chacha20_seal"),
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
    // `self.structs`, walks its fields (honoring `#[Rename]`/`RenameAll`/
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
    // ambient reads of the host, arguably belongs behind the same `#Impure`
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
    // paren-balance-parser artifact (see the `core.encoding.json` note
    // above): `("core.web.storage.local" | "core.web.storage.session",
    // "get")`-style module alternation.
    ("core.web.storage.local", "core.web.storage.session"),
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
    ("core.gc", "collect"),
    ("core.game", "run"),
    ("core.mime", "extension"),
    ("core.mime", "from_extension"),
    ("core.mime", "parse"),
    ("core.numeric", "decimal"),
    ("core.testing", "bench_budget"),
    ("core.testing", "corpus"),
    ("core.testing", "fake_clock"),
    ("core.testing", "fake_rng"),
    ("core.testing", "fixture"),
    ("core.testing", "golden"),
    ("core.testing", "snap"),
    ("core.testing", "temp_dir"),
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
    // `#Impure` gate defined for it yet) — genuinely impossible to do purely,
    // not merely unported.
    ("core.uuid", "v4"),
    ("core.uuid", "v7"),
];

/// Find every `(` whose next non-whitespace content is `"core.`, balance
/// parens to find its matching `)`, and pull (module, method) pairs from the
/// quoted strings inside — first quote is the module, every other quote in
/// the same tuple is a method name (covers `"a" | "b" | "c"` alternates).
/// Paren-balancing (not scanning to the next `=>`) is what makes this work
/// uniformly over both a `match` arm and a `matches!(..., p1 | p2 | ...)`
/// pattern list, which has no `=>` at all.
fn extract_pairs(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        if bytes[i] == b'(' {
            let mut j = i + 1;
            while j < n && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            if text[j..].starts_with("\"core.") {
                let mut depth = 0i32;
                let mut k = i;
                let mut end = None;
                while k < n {
                    match bytes[k] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(k + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if let Some(end) = end {
                    let header = &text[i..end];
                    let quoted = extract_quoted(header);
                    if let Some((module, names)) = quoted.split_first() {
                        for name in names {
                            if name != "_" {
                                out.insert((module.clone(), name.clone()));
                            }
                        }
                    }
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
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

#[test]
fn comptime_covers_every_aot_core_builtin_or_lists_the_gap() {
    let sigs = read("crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs");
    let methods = read("crates/jet-comptime/src/Comptime/Methods.rs");

    let aot = extract_pairs(&sigs);
    let ct = extract_pairs(&methods);
    let known_gaps: BTreeSet<(String, String)> = KNOWN_OPEN_GAPS
        .iter()
        .map(|(m, n)| (m.to_string(), n.to_string()))
        .collect();

    assert!(
        aot.len() > 50,
        "sanity check: extracted only {} AOT (module, method) pairs from fixed_sigs.rs — \
         the paren-balance parser probably broke against a source reformat",
        aot.len()
    );

    let mut newly_missing: Vec<&(String, String)> = aot
        .iter()
        .filter(|(m, _)| !EFFECT_GATED_MODULES.contains(&m.as_str()))
        .filter(|pair| !ct.contains(*pair))
        .filter(|pair| !known_gaps.contains(*pair))
        .collect();
    newly_missing.sort();

    assert!(
        newly_missing.is_empty(),
        "{} AOT builtin(s) have no comptime dispatch and aren't in KNOWN_OPEN_GAPS \
         (a silent AOT-only builtin — `jet dev`/REPL/`comptime` would hit E0956):\n{}",
        newly_missing.len(),
        newly_missing
            .iter()
            .map(|(m, n)| format!("  ({:?}, {:?})", m, n))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // The other direction: a `KNOWN_OPEN_GAPS` entry that comptime already
    // handles (or that AOT no longer has) is stale bookkeeping — keep the
    // allowlist honest so it stays a real to-do list, not a growing pile of
    // outdated exemptions.
    let mut stale_gaps: Vec<&(&str, &str)> = KNOWN_OPEN_GAPS
        .iter()
        .filter(|(m, n)| {
            let pair = (m.to_string(), n.to_string());
            !aot.contains(&pair) || ct.contains(&pair)
        })
        .collect();
    stale_gaps.sort();
    assert!(
        stale_gaps.is_empty(),
        "KNOWN_OPEN_GAPS has {} stale entr(y/ies) — either comptime now handles it \
         (remove the line) or AOT no longer has it (remove the line):\n{}",
        stale_gaps.len(),
        stale_gaps
            .iter()
            .map(|(m, n)| format!("  ({:?}, {:?})", m, n))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
