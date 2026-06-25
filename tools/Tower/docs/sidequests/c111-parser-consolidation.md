# c111 — Replace hand-rolled parsers (audit + scoped plan)

Status: **DONE (2026-06-25)** — D-PARSE-1=C built end-to-end. The audit below is
retained for context; the consolidation question it raised was resolved by the
owner ratifying full native parsers (no lossy subsets, I6 stays hard).

## D-PARSE-1 build (2026-06-25)

Full native std-only parsers replacing every silently-lossy subset reader:

- **SemVer** — `Source/Publish/SemVer.rs` rewritten to complete SemVer 2.0.0:
  `+build` metadata (parsed, ignored in precedence), strict numeric identifiers,
  full pre-release precedence, and the node-semver range grammar (`=` `>` `>=`
  `<` `<=`, `^`, `~`, x-ranges, hyphen ranges, `||`). `VersionReq` normalizes to
  an OR of comparator-sets; `intersects()` replaces the old Caret/Exact
  disjointness heuristic in `Resolve.rs`. Tests in `Source/Publish/mod.rs`.
- **JSON** — `Source/Prelude/CoreLib.rs` user-facing parser is now full RFC 8259
  (exponents + strict number grammar, every escape incl. `\uXXXX` with
  surrogate-pair combining, rejects invalid escapes / lone surrogates / raw
  control chars with line + message); the renderer escapes all control chars.
  `Source/LSP/JSON.rs` and `Source/Jetpack/JSON.rs` gained surrogate-pair
  handling. End-to-end test `json_parser_is_rfc8259_complete` in `tests/corelib.rs`.
- **TOML** — new `Source/Jetpack/TOML.rs` is a complete TOML 1.0 parser (all
  value types, bare/quoted/dotted keys, arrays, inline tables, multi-line
  strings, arrays-of-tables, comments, line-numbered statements with recovery).
  `Source/Jetpack/ManifestTOML.rs` is now a thin schema validator over it,
  preserving the `JetpackToml`/`load`/`render_errors` API and the E1214/E1215
  contract; non-string schema values get a clear E1214 instead of silent
  stringification.

Full suite green (1107 passed, 0 failed). Three commits on `master`.

---

## Original audit (retained for context)

The framing below ("decide where I6 should bend, or narrow the feature
contracts") was the policy question; the owner answered it with D-PARSE-1=C.

## What was audited

Every parser in `Source/` that does NOT go through the main Jet lexer
(`Source/Lexer/`) + parser (`Source/Parser/`). None of the candidates reuse the
main lexer/parser — each is a self-contained scanner. Confirmed by `use` lines:
no candidate imports `Source/Lexer` or `Source/Parser`.

| Parser | Parses | Technique | Type / error surface | Consumers |
|---|---|---|---|---|
| `Source/LSP/JSON.rs` (231) | JSON (LSP wire) | char scanner | `JsonValue` (int/float split), `Result<_,()>`, `HashMap`, `pub(crate)` | LSP only |
| `Source/Jetpack/JSON.rs` (280) | JSON (`nix --json`, state files) | `Vec<char>` scanner | `Json` (single `Num(f64)`), `Result<_,String>` w/ messages, `BTreeMap`, `pub` | Jetpack Store/Provider/JetOS |
| `Source/Prelude/CoreLib.rs` (~488,676) | JSON (user-facing stdlib) | char scanner | `jet_std::Json` + `JsonError`; **generated into user programs**, golden-pinned | `30_json.jet`, `73_json_coerce.jet`, runtime |
| `Source/Publish/SemVer.rs` (167) | SemVer + `^`/exact/`*` ranges | `split('.')` + `parse::<u64>` | `SemVer`/`VersionReq`, `Option<T>` | Publish/Advisory |
| `Source/CBind.rs` (440) | C prototypes (FFI bindgen) | byte scanner + brace depth | `BindResult`, `Result<_,String>` | CmdDevTools, CFFI |
| `Source/CFFI.rs` (865) | pkg-config flags (delegates C to CBind) | `split_whitespace` + `strip_prefix` | `CFfi`/`LinkFlags`, `Vec<Diagnostic>` (E32xx) | Loader |
| `Source/Lock.rs` (448) | `.jet/lock` (TOML-ish, line-oriented) | `lines()`+`split_once('=')`+`trim_matches('"')` | `LockFile`, `Result<_,String>` (E120x) | Loader, CmdSupply/Pkg |
| `Source/Manifest.rs` (480) | wrapper over PackageManifest | delegates; own version cmp `version_ge` | `Manifest`, `Diagnostic` (E120x/E121x) | Loader, Cmd* |
| `Source/Jetpack/PackageManifest/*` (~1.6k) | `pack.jet` (Jet-syntax blocks) | comment-strip + balanced-brace block extract + comma/colon split | `PackManifest`, `ManifestError` enum | Manifest.rs, Loader |
| `Source/Jetpack/ManifestTOML.rs` (444) | `jetpack.toml` (3-table subset) | line split + state machine | `JetpackToml`, `TomlError` (E1214/E1215) | Jetpack/CLI |
| `Source/Jetpack/EnvFile.rs` (500) | `env.jet` directive calls | tolerant string-search + char depth | `EnvFile` (never fails) | Jetpack/CLI |
| `Source/Jetpack/RefSpec.rs` (431) | `src:pkg` / `provider@target` refs | `split_once(':'/'@')` | `RefSpec`/`ProviderRef`, `RefError` | EnvFile, JetOS, ParseBlocks, CLI |

## Verdict: nothing is strictly behavior-preserving to auto-consolidate

The apparent duplications do not survive inspection:

- **Three JSON parsers** look like one job but differ observably: number model
  (`Number(i64)`+`Flt(f64)` vs single `Num(f64)` vs stdlib `Number`), error type
  (`()` vs `String`-with-message vs `JsonError`), container (`HashMap` vs
  `BTreeMap`), visibility/API (free `pub(crate)` fns vs `pub` methods), and one
  (`CoreLib`) is **emitted into user code and pinned by golden examples**.
  Merging changes types, error text, and map ordering at call sites → not
  behavior-preserving.
- **Four `json_escape`/`json_str`/`quote` writers** escape *different character
  sets* (e.g. SBOM omits `\t` and control chars; Diagnostics adds `\b`/`\f`/`\u`
  and wraps in quotes; LSP escapes all control chars but doesn't wrap). Unifying
  changes the exact bytes written to LSP wire / SBOM / diagnostic JSON / lockfile.
- **Two `unquote()`** (ManifestTOML vs PackageManifest/Helpers) differ: TOML's
  also unescapes `\"` and `\\`; Helpers' does not. Sharing changes accepted input.
- **SemVer vs Lock vs RefSpec**: no real overlap — Lock stores versions as raw
  `String` (never parses them), RefSpec never touches versions. The only echo is
  `Manifest.rs::version_ge` reimplementing numeric `.`-split compare for the
  toolchain constraint; it is cohesive, tested via its callers, and rewriting it
  to call `SemVer` would change tolerance (SemVer wants 3 components) → leave it.

Per I8 (simplicity ratchet) and the task standard, a forced rewrite that perturbs
any accepted input / error text / output is worse than leaving cohesive parsers
alone. So **no code was changed**. Baseline left untouched: arena.rs 4, tir.rs 3,
closures.rs 1, grammar.rs 1, pkg.rs 2.

## What *could* be unified later, and the risk

Two tiers, both deliberately deferred:

1. **Internal structural helpers (low value, low risk).** `balanced` /
   `top_level_commas` / depth-scan logic recurs in PackageManifest/Helpers.rs,
   PackageManifest/Edit.rs, and EnvFile.rs. A shared `depth_scan(open, close)`
   primitive could back all three. Risk: low (internal, no error text), but the
   payoff is small and the three call styles (indexed `Vec<char>`, peekable iter,
   line+`matches()` count) differ enough that a shared helper adds an abstraction
   layer without removing real complexity. Only worth doing alongside other work
   in those files. **Recommendation: skip until one of those files is touched
   for a feature; do it then, behavior-preserving, with a full suite check.**

2. **The real question the card is about (owner decision): I6 vs correctness.**
   The card's actual finding is that the zero-dependency rule (I6) pushed
   *correctness-sensitive* formats — JSON, TOML, SemVer, C prototypes — into
   hand-written *subsets*. The risk isn't duplication; it's that a hand-rolled
   subset silently accepts or rejects inputs a real parser wouldn't (e.g. the
   JSON parsers accept lone surrogates / don't reject duplicate keys; the TOML
   subset only understands 3 tables; SemVer ignores build metadata `+...` and
   most range operators). This is a feature-contract / dependency-policy call,
   not a refactor. It must go to the owner. See ballot `D-PARSE-1`.

## Recommended approach (pending D-PARSE-1)

- If owner keeps I6 hard: **narrow and document the contracts** — make each subset
  parser reject (with a clear diagnostic) inputs outside its documented subset
  rather than mis-accepting them, and state the supported subset in docs. This is
  the I8-aligned path: a great error + documented limit beats a silent wrong parse.
  Then **collapse the three JSON value models onto one shared `jet_std::Json`**
  used by LSP and Jetpack too (one parser, one escaper), accepting the call-site
  churn as a *deliberate* behavior change reviewed against new snapshots — this is
  the only consolidation worth doing and it is explicitly out of scope for the
  behavior-preserving pass.
- If owner allows a vetted dependency for a specific format: scope it to that one
  format (e.g. a real JSON or SemVer crate) behind owner approval per I6, and
  delete the corresponding subset.

No code change should land for this card until D-PARSE-1 is decided.
