# #3247 — Pinned-to-current companion for the CORE-F020 peers

Date: 2026-09-29. Card #3247 (CORE-F020). Research evidence only. This claims no latest-parity verdict and approves no API, default or dependency.

## Question

For each selected baseline and library peer: what are the archived pin and the current identity, which material additions, removals and default changes lie between them (labelled stable, preview or optional), and where does each useful delta route?

## Inputs

- **Pins and the 2026-09-11 current identities.** `Docs/research/mine-for-jet-2026-09-12.md` lines 35866–35880 ("Core currentness records", 15 rows).
- **Retained companions.** The west `current-release-notes.md` (lines 61226–61324, SHA-256 `bebb86c0…670e26b`) and the east `currentness-2026-09-11.md` (lines 62349–62403, SHA-256 `adf4a940…c241cbac3`). The card plan says these are not in the repo, but both are retained verbatim in the report and were read there.
- **Re-derivation today.** `python3 ~/.cache/jet-dev/scratch/Closer06/keep/currentness.py`, run 2026-09-29, output below. It reads each peer's official machine-readable feed: GCC timeline, `channel-rust-stable.toml`, endoflife.date Python, `go.dev/dl?mode=json`, Adoptium `available_releases`, the .NET `releases-index.json`, the GitHub releases API, crates.io, PyPI, Maven Central `maven-metadata.xml`, and NuGet flat-container.
- **Glaze tags**, from `git ls-remote` and the GitHub releases API bodies for v8.4.0 and v9.0.0. **JDK 27**, from `https://openjdk.org/projects/jdk/27/`.

```
C++/libstdc++ (GCC) | 13.5 September 11, 2026; 16.2 August 7, 2026; 14.4 June 26, 2026
Rust | 1.98.1 (48a229cea 2026-09-01)
Python | 3.14: 3.14.7 (2026-08-05); 3.13: 3.13.15 (2026-08-05)
Go | go1.27.1; go1.26.8
Java (OpenJDK/Adoptium) | most recent feature 27; most recent LTS 25; available LTS [17, 21, 25]
.NET | 11.0: runtime 11.0.0-rc.1.26425.128 (go-live); 10.0: runtime 10.0.12 sdk 10.0.401 (active); 9.0: runtime 9.0.20 (maintenance)
Glaze | v9.0.0 (2026-09-24)
Asio (standalone tags) | asio-1-38-2, asio-1-38-1, asio-1-38-0
Boost | 1.92.0
Serde | 1.0.229 | serde_json | 1.0.151 | Tokio | 1.53.1
HTTPX | 0.28.1 (2024-12-06)
Cobra | v1.10.2 (2025-12-04)
Jackson databind | release 2.22.3      (com.fasterxml; tools.jackson databind release 3.2.3)
Jackson annotations | release 3.0-rc5  (maven <release>; newest non-RC is 2.22)
MVVM Toolkit | 8.4.2
```

## Companion (criteria 1–4)

Labels: **S** means stable GA, **P** preview or release candidate, **I** incubator, **O** an optional or separate line outside the selected scope. "Unknown" means a complete symbol-level export diff was not produced; this is criterion 5's explicit unknown.

| peer | archived pin | current 2026-09-11 (report) | current 2026-09-29 (observed) | material deltas pin→current (label) | route |
|---|---|---|---|---|---|
| C++ / libstdc++ | C++23 / GCC 14.2 | GCC 16.2; 14.4 maintenance | unchanged newest (16.2). GCC 13.5 (2026-09-11) is an older-branch maintenance release, not newer | 16.2 is a bug-fix release on 16.1 (S). Export diff 14.2→16.2: **unknown** | none (compiler baseline; no Jet API delta) |
| Rust | 1.85.0 | 1.98.1 | 1.98.1 (no change) | 1.98.0 stabilized `str::substr_range`, `[T]::subslice_range`, `core::fmt::NumBuffer`, integer `format_into`, `String::from_utf16le/be`, `Atomic<T>::from_mut*`, `std::range::legacy` (S). 1.85→1.97 per-version: **unknown** | text ranges → #3262; iterator census stays with IteratorAstra |
| Python | 3.13.2 | 3.14.7 | 3.14.7 (no change; 3.13 line now 3.13.15) | 3.14: free-threaded build supported (S, PEP 779), deferred annotations (S), t-strings (S, PEP 750), `concurrent.interpreters` (S, PEP 734), `compression.zstd` (S, PEP 784), UUID v6–8 (S), remote pdb (S); experimental JIT binaries (P). Module diff: **unknown** | zstd → #3263; t-strings → #3252 |
| Go | go1.24.1 | go1.27.1 | go1.27.1 (1.26 line at 1.26.8) | 1.26: `new(expr)`, recursive generic constraints, `go fix` modernizers, Green Tea GC default (S). 1.27: generic methods, generalized inference (S). **1.25 deltas unaccounted** in both captures; export diff **unknown** | none (language baseline) |
| Java | 21.0.6 | OpenJDK 26.0.2.1+1 production; 25 LTS | **moniker correction:** newest feature release is now **JDK 27** (GA 2026-09-15, Adoptium `jdk-27+35`); 25 remains the newest LTS; 26 line at 26.0.2.1+1 | JDK 27: JEP 527 PQ hybrid TLS key exchange (S), JEP 534 compact object headers by default (S), JEP 536 JFR redaction (S), JEP 523 G1-related (S; title truncated in the fetch, exact title **unknown**); JEP 531 lazy constants, 532 primitive patterns, 533 structured concurrency, 538 PEM (P); JEP 537 Vector API (I). Java21 Gatherer presence stays **disputed/unknown** as recorded | PQ TLS → #3256; structured concurrency (P) not routed |
| .NET | 9.0.3 / SDK 9.0.201 / net9.0 | 10.0.12 / SDK 10.0.401 / net10.0 | 10.0.12 (no change). **11.0 RC1** `11.0.0-rc.1.26425.128` is go-live (P) and is not stable | .NET 10 LTS JIT/NativeAOT/crypto/serialization additions (S); net9→net10 export diff **unknown** | none until 11 GA |
| Glaze | 5.0.0 (baseline); v8.3.0 is the selected-scope tag (CORE-C022) | v8.3.0 | **v9.0.0** (2026-09-24), after **v8.4.0** (2026-09-15) | v8.4.0 (S): HTTP streaming progress callback, contiguous buffers need only data()/size(), opt-in redirect following; CSV/TOML/YAML/ostream_buffer fixes. v9.0.0 (S, **breaking**): MessagePack/CBOR variant wire shape changed, MessagePack structs keyed maps; removed `glz::invoke_update`, `shared_async_map`/`vector`, `glz::hostname_include`, `disable_padding_on/off`; `meta<T>::skip` applies to every keyed format; `is_padded` semantics; stricter `1e` and JSON Pointer leading zeros; TLS 1.0/1.1 refused; empty `raw_json` writes `null`. Header delta v8.3.0→v9.0.0: 101 files changed, 3 added (`bson/wrappers.hpp`, `msgpack/wrappers.hpp`, `simd/structural.hpp`), 4 removed (`file/hostname_include.hpp`, `thread/shared_async_{map,vector}.hpp`, `thread/value_proxy.hpp`) per `diff -rq include/` | MsgPack → #3312; CBOR → #3271; skip/wrappers → #3236; pointer strictness → #3237; JSON include removal → #3251; BSON wrappers → #3267 |
| Asio | Boost 1.88.0 / Asio 1.34.2 | Boost 1.92.0 / Asio 1.38.2 | unchanged | 1.35–1.38.2: allocator ctors, resolver threads, UTF-8 Windows paths, inline-namespace versioning, io_uring/SSL buffer options, cancellation fixes (S); experimental channels and parallel_group changes (P). Export diff **unknown** | none (reference only) |
| Serde | 1.0.219 | 1.0.229 | unchanged | "Update to syn 3" (S); derive/data-model delta **unknown** | #3236 |
| serde_json | 1.0.140 | 1.0.151 | unchanged | `RawValue::from_string_unchecked` (S; unchecked, not a safe default) | #3255 |
| Tokio | 1.44.2 | 1.53.1 | unchanged | maintenance fixes captured (S); module delta **unknown** | none |
| HTTPX | 0.28.1 | 0.28.1 | unchanged (pin = current) | none since pin | none |
| Cobra | v1.9.1 | v1.10.2 | unchanged | yaml dependency move to `go.yaml.in/yaml/v3` (S); public symbol diff **unknown** | #3291 context only |
| Jackson (core/annotations/databind) | 2.18.3 | core/databind 2.22.2; annotations 2.22 | databind **2.22.3** (Maven `lastUpdated` 2026-09-21). **Moniker correction:** annotations `maven-metadata <release>` reads `3.0-rc5`, a release candidate (P), so the newest stable annotations remain `2.22`. Jackson 3 ships separately under groupId `tools.jackson.core` (databind 3.2.3) (O) | 2.19–2.22 semantic delta **unknown**; Jackson 3 is a separate major line, not a 2.x delta | unknown/duplicate policy → #3077 criterion 3 and #3255 |
| MVVM Toolkit | 8.4.0 | 8.4.2 | unchanged | 8.4.2 build-target analyzer fix only (S); generated-member diff **unknown** | #3243 |

## Verdict

- **Criterion 1** (pins and current identities, moniker ambiguity corrected): met. The table carries each pin, both dated current identities and three explicit corrections: Java 27 against 26/25, the Jackson annotations RC `<release>` together with the separate `tools.jackson` line, and Glaze's 5.0.0 baseline against the v8.3.0 selected tag, now v9.0.0.
- **Criterion 2** (every material addition, removal or default change): **partial**. Release-level deltas are accounted for every peer, and Glaze v8.3.0→v9.0.0 gets a file-level header diff. Symbol-level export diffs remain **unknown** for C++, Rust, Python, Go, Java, .NET, Asio, Serde, Tokio, Cobra, Jackson and MVVM. Go 1.25 is unaccounted in every capture.
- **Criterion 3** (stable, preview and optional): met through the S/P/I/O labels.
- **Criterion 4** (route, don't claim parity): met by the route column.
- **Criterion 5** (explicit unknowns): met.

Card verdict: PARTIAL, on criterion 2 only. Closing it needs a per-peer export differ, the Glaze analogue of `diff -rq include/` extended to symbols, for 13 ecosystems. That is a bounded but large follow-up. It is not re-derivable from release notes.
