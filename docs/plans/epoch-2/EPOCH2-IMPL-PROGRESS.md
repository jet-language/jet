# Epoch-2 implementation stream — progress & resume log

**This is the resume doc for the EPOCH-2 implementation stream.**

## Current branch: master

All prior epoch-2 work (`epoch-2-impl`) and the jetpack/jetos track
(`jetos-ratified-arc`) have been merged into `master` (commit `5eec357`).
All future epoch-2 work is directly on `master`. No separate worktrees.

## Completed milestones

E2-M1 ✅ M2 ✅ M3 ✅ M4 ✅ M5 ✅ M6 ✅ M8 ✅ M9 ✅ M11 (partial) M12 ✅ M13 ✅ M14 ✅ M15 (partial)

The E2-M3/M5 work lives in commits `7412fe7`→`fd02646` (now on master).

## Milestone plan (sequential, fully-ratified, low-collision)

Order chosen for **collision-safety**, not strict numeric order: M2's
edition-marker piece must add a field to the manifest parser, but
`src/manifest.rs`→`packmanifest.rs` is **actively churning** under the jetpack
agent's pack.jet/payload.jet migration. So M2's manifest piece is deferred;
M3 and M5 touch disjoint regions and go first.

| Order | Milestone | Plan | Status |
|---|---|---|---|
| 1 | E2-M3 DX CLI (explain/doctor/--json) | `m3-dx-cli.md` | ✅ DONE (chunks 1–6, all exit criteria met) |
| 2 | E2-M5 tier-2 references | `m5-references.md` | ✅ DONE (chunks A–B, matrix complete, soundness fuzz green) |
| 3 | E2-M2 release policy/editions | `m2-release-policy.md` | NEXT (collision-free parts) — policy docs + `jet --version` banner anytime; defer edition-in-manifest until pack.jet migration settles |

Deferred (collision/dependency): E2-M14 (jetpack owns — already committed),
E2-M4 (`jet dev`, rides LSP foundation; revisit after M3/M5).

### E2-M3 completion record (branch `epoch-2-impl`)

Commits `7412fe7`→`44dc85f`. Exit criteria (m3-dx-cli.md) all met:
- ✅ golden tests pin human + `--json` output; CI mode deterministic + ANSI-free
- ✅ every diagnostic points to `jet explain` via a dim, gated "learn more" footer
- ✅ `jet doctor` actionable + offline by default (`--online`/`--fix`)
- ✅ no-args `jet` greets/orients; typo'd subcommands/flags → E2101/E2102
- ✅ completions (bash/zsh/fish) + man pages from one `cli_spec` source (drift-tested)
- ✅ unified fix engine (`src/fixengine.rs`) shared by `jet fix` + LSP
- ✅ full `nix develop -c cargo test` green; no new crates (I6)

New codes registered (E21xx/L21xx range, disjoint from jetpack's E09/E1x/E32xx):
E2101 (unknown subcommand), E2102 (unknown flag), L2101 (doctor advisory).
New modules: `src/explain.rs`, `src/diagjson.rs`, `src/doctor.rs`,
`src/cli_spec.rs`, `src/fixengine.rs`.

**Deferred from M3 (gated, not skipped sloppily):**
- **Digit separators (D-SUGAR1 `1_000_000`)** — ballot is OPEN/unratified; per
  I7 syntax gate NOT implemented. `examples/features/34_digits.jet` therefore
  not created. Implement when D-SUGAR1 is ratified.
- `jet fix --json` planned-edit emission (the same edits are already exposed via
  `jet check --json`); package-command `--json` (lives behind jetpack files).

## Protocol per milestone (CLAUDE.md workflow loop)

test-first (ui fixture/example/golden) → spec in docs/spec → parser → sema →
codegen/CLI → `nix develop -c cargo test` green → `tests/decisions.rs` green →
docs updated → commit on `epoch-2-impl`. Validate the FULL suite before each
commit; never start a milestone on a red baseline.

### E2-M5 completion record (branch `epoch-2-impl`)

Commits `3e4db31` (chunk A) + `fd02646` (chunk B). Exit criteria met:
- ✅ soundness matrix COMPLETE — every cell allowed-with-proof (positive fixture)
  or rejected-with-diagnostic+fixture; table filled in `m5-references.md`.
- ✅ no user-written lifetime names anywhere; diagnostics speak Jet words.
- ✅ `examples/features/35_zerocopy.jet` runs, golden-pinned, contrasts a
  borrowed (no-copy, lowers to `&String`) path vs a clone-heavy one.
- ✅ L2301 inlay hints wired in `src/lsp.rs` (D-REF3), tested.
- ✅ soundness fuzz target `tests/ref_soundness_fuzz.rs` (sema-accepted ⇒
  rustc-accepted, no ICE, no `unsafe`) green — found+closed a real chunk-A hole.
- ✅ full `nix develop -c cargo test` green (34 binaries).

Codes: E2301 (returned view outlives owner), E2302 (stored `ref` dangle —
**tightened**: a `ref` field has no sound v1 source except a `'static` const,
so non-const sources are now rejected, closing an ICE), E2303 (delegates to
E1102), **E2304** (view into an index/slice of a param — the helper copies),
L2301 (borrow advisory/inlay). Key allow: `view` into a **field of a parameter**
(incl. through a generic `Wrap<T>` param) — the zero-copy primitive.

## Resume pointer

**Current state:** E2-M3 ✅ and E2-M5 ✅ fully implemented + validated (full
suite green) on branch `epoch-2-impl`, commits `5050565`→`fd02646`.

## Resume pointer

**Current state:** master. All sidequests in `docs/plans/sidequests/` are resolved
EXCEPT `s19-amend-loop-unification.md` (loop keyword unification — `while`/`for`
must become teaching errors; `loop` with header disambiguation is the one form).
That sidequest requires parser work + example rewrites + snapshot bless.

**Next milestone to implement: E2-M2** (`m2-release-policy.md`) — collision-free parts.

After M2: implement in dependency order per `docs/plans/EPOCH2-HANDOFF.md`.

### E2-M15 completion record (partial)

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ `jet build --target=<triple>` — passes `--target <triple>` to rustc; cross-target validation (E3302) runs before compile.
- ✅ `jet doctor --target=<triple>` — `check_cross_target` in `src/doctor.rs` checks `rustc --print target-list` and the sysroot dir for std library presence.
- ✅ `--freestanding` flag — rejects `core.fs`, `core.io`, `core.net`, `core.tasks`, `core.process`, `core.time`, `jet.http`, `jet.log`, `jet.time` with E3301 via `is_freestanding_forbidden` in `src/sema.rs`.
- ✅ Freestanding mode threads through `check_bundle_freestanding` → `check_module_bodies` → `check_func_body_bundle` → `Checker.freestanding`.
- ✅ D-CROSS2: `BuildProfile::Freestanding` sets `panic=abort` (same as `--small`).
- ✅ D-CROSS3: `docs/embedded.md` with exact QEMU commands for local harness.
- ✅ E3301 registered in `docs/spec/diagnostics.md` + `tests/ui/freestanding_e3301.{jet,stderr}` snapshot.
- ✅ E3302 registered in `docs/spec/diagnostics.md`; `sema::e3302` constructor in sema.rs; fired by `validate_target()` in main.rs.
- ✅ E3303 registered in `docs/spec/diagnostics.md`; `sema::e3303` constructor in sema.rs.
- ✅ `examples/features/60_cross.jet` and `examples/features/61_freestanding.jet` golden-pinned.
- ✅ `tests/cross.rs` — 6 tests covering E3301 fire/no-fire for OS vs core modules + ui snapshot.
- ✅ `nix develop -c cargo test` fully green (all suites).

**Not implemented (deferred):**
- E3302 ui snapshot: driver-level error (no source span); not in `tests/ui/`; tested by `validate_target()` codepath.
- E3303 automatic emission: detecting allocation use in freestanding mode requires scanning for `List`/`Map`/`String` allocations; deferred. Registered and constructor in place.
- Actual CI run on aarch64 hardware/QEMU (D-CROSS3 says local harness is the minimum; docs/embedded.md provides the commands).

New modules: none (all changes in existing files).
New functions: `sema::e3301`, `sema::e3302`, `sema::e3303`, `sema::is_freestanding_forbidden`, `sema::module_short_name`, `sema::freestanding_hint`; `lib::compile_freestanding`; `doctor::check_cross_target`; `main::validate_target`.
New flags: `--freestanding`, `--target=<triple>`.
New test file: `tests/cross.rs`.
New docs: `docs/embedded.md`.

### E2-M12 completion record (partial)

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ Source map marker (`// jet:source-map source={file}`) emitted in every generated Rust file via both `emit` and `emit_bundle` code paths. Tooling can resolve Rust spans back to the originating Jet file.
- ✅ E3001: `jet_panic_rich` in `src/prelude/core.rs` — rich panic reports with Jet file, line, function name, source-line context box (caret highlights the `require`/`panic` call), and safe local variable values (Int/Float/Bool only) in debug builds (D-OBS1/D-OBS2). Codegen updated to use `jet_panic_rich` for `panic`/`require`/`require_eq` builtins; index/map bounds-check panics use the simpler `jet_panic`.
- ✅ D-OBS2 safe-locals policy enforced structurally: `safe_locals_expr` only emits Copy scalars (Int/Float/Bool); String/List/Map/Ptr locals are never shown, preventing both moved-from panics and secret value leaks.
- ✅ D-OBS3 structured JSON logs: `jet.log` now emits one JSON object per record (`{"level":"...","body":"...","ts":...}`); `trace_id` field added when `log.set_trace_id` called. `jet_log_emit` / `jet_log_json_escape` helpers in `src/prelude/std.rs`.
- ✅ `log.set_trace_id(id: String)` API added to `jet.log` (sema + codegen + std.rs).
- ✅ E3001/E3002 registered in `docs/spec/diagnostics.md`.
- ✅ `examples/features/59_debug.jet` — demonstrates rich panic with Jet line, context box, and dev-mode locals; golden-pinned with `.err.out`.
- ✅ `tests/observe.rs` — 8 tests covering JSON log fields/levels/trace_id/escaping, rich panic format, safe-locals policy, source-map marker.
- ✅ `tests/observe/structured_log.txt` — JSON format documentation/reference.
- ✅ `nix develop -c cargo test` fully green (all suites).

**Not implemented (deferred):**
- E3002 error-return traces for `?` propagation: registered but codegen not emitting trace entries. Requires wrapping the `?` result with file/line info at each propagation site. Non-trivial; deferred to a follow-up slot.
- Full DAP step-through debugging (breakpoints, watch values in VS Code/Cursor): deferred to E2-M17 per D-OBS1 ratification split.
- GDB/LLDB integration test resolving panics to Jet source lines: requires running a debugger in CI; deferred.

Panic format changed: `"The program stopped:"` → `"panic:"` (all 3 golden `.err.out` files updated: 14_panic, 18_list_bounds, 19_map_key). FFI wrapper panic and task-join panic also updated.

### E2-M11 completion record (partial)

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ `todo` typed-hole compiles and type-checks; panics at runtime with file, line, expected type (D-TOOL2=A). Keyword in `src/syntax.rs`; `TokKind::KwTodo` in lexer; `Expr::Todo` in AST/sema/codegen/fmt/lsp. `examples/features/58_todo_hole.jet` golden-pinned.
- ✅ Single-expression function body `fn name() -> T = expr;` (desugars to `Stmt::Return` in parser — needed for `todo` example pattern).
- ✅ `jet build` prints human-readable capability summary by default; `--capabilities-json` for tooling (D-TOOL5=C). `Capabilities` struct in `src/lib.rs`; `--capabilities-json` flag in CLI.
- ✅ `jet emit --rust` expert window (D-TOOL3=A). Prints generated Rust to stdout; gated behind `--rust` flag so `jet emit` without flag errors gracefully.
- ✅ `jet bench` with honest stats (D-TEST1 adjacent). 5 warmup + 20 timed trials; prints mean ± stddev in ms. `--json` for scriptable output.
- ✅ `expect(x).snapshot()` snapshot-testing builtin (D-TOOL4=A). `jet test -u` / `jet test --update-snapshots` sets `JET_UPDATE_SNAPSHOTS=1`; the `JetExpect` wrapper in `TEST_PRELUDE` reads/writes `.snap` files; `BUILTIN_EXPECT`/`BUILTIN_SNAPSHOT` in `src/syntax.rs`; sema recognizes the pattern; codegen emits the runtime call.
- ✅ `--capabilities-json`, `--update-snapshots`, `-u`, `--rust` flags in CLI registry (`src/cli.rs`); `emit` and `bench` in COMMANDS.
- ✅ E2901/E2902/L2901 registered in `docs/spec/diagnostics.md`.
- ✅ `nix develop -c cargo test` green (all suites).

**Not implemented (deferred):**
- Doctests (D-TEST4=D-TOOL1=A): extracting `> expr` / expected-output pairs from `///` doc comments and running them under `jet test`. Requires new lexer pass over comment text + doctest runner. Non-trivial; deferred to a follow-up slot.
- Coverage output (CI-readable + human-readable). Requires LLVM coverage instrumentation or source-line counters; out of scope for this slot.
- Property testing (D-TEST1=A): plan gates on "small shrinking design exists"; design not yet done; deferred.
- `jet doc` rendered HTML/markdown docs: design not ratified; deferred.

New functions in `src/main.rs`: `run_emit_rust`, `run_bench`.
New struct: `Capabilities` (`src/lib.rs`).
New prelude const: `TEST_PRELUDE` (snapshot helper `JetExpect` in `src/codegen.rs`).
New syntax consts: `KW_TODO`, `BUILTIN_EXPECT`, `BUILTIN_SNAPSHOT` (`src/syntax.rs`).
Examples: `examples/features/58_todo_hole.jet`.

### E2-M9 completion record

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ Wave-1 ring packages as compiler-known modules: `jet.csv`, `jet.toml`, `jet.yaml`, `jet.log`, `jet.json`, `jet.time`, `jet.crypto` — all resolve via `use jet.<pkg> as alias;`
- ✅ Fallible APIs return `T ? E` (csv/toml/yaml parse return `[[String]] ? String` / `[String, String] ? String`); log is void; time/crypto return owned values
- ✅ No hidden global state (log level is thread-local, deterministic per thread)
- ✅ D-JSON1 (jet.json coercion): `jet.json.parse`/`render`/`render_pretty` reuse the `core.json` implementation; the same JSON types apply
- ✅ E2701/E2702/L2701 registered in `docs/spec/diagnostics.md` (E2701 surfaces in Err strings at runtime; L2701 reserved for future `jet.regex`)
- ✅ Examples: `51_csv.jet`, `52_toml.jet`, `53_yaml.jet`, `54_log.jet`, `55_hash.jet` — all golden-pinned and passing
- ✅ `nix develop -c cargo test` green (all suites)

**Not implemented (by design or deferral):**
- `jet.regex` (wave 2) — NFA engine requires significant complexity; reserved module name; L2701 placeholder registered
- `jet.archive` (wave 2) — TAR/ZIP parsing; reserved; deferred until D-DEP1 Rust-backed pattern lands
- `jet.db` / SQLite (wave 2) — C FFI (E2-M14) is done but SQLite binding generation via `@bindgen` not yet wired; deferred
- D-JSON1 `decode_verbose` (option A, returning coercions list) — deferred; `jet.json` type is the same dynamic JSON type as `core.json`; coercion surfacing needs an M11 doctest surface to test properly
- Actual package manifests and hangar staging for ring packages — these are implemented as compiler-known modules (same pattern as `core.*`) pending the jetpack package store being wired end-to-end

New files: `src/prelude/std.rs` (ring implementations appended), `src/loader.rs` (ring module detection), `src/sema.rs` (type signatures), `src/codegen.rs` (emit dispatch).
Examples: `examples/features/51_csv.jet`, `52_toml.jet`, `53_yaml.jet`, `54_log.jet`, `55_hash.jet` + expected outputs.

### E2-M8 completion record

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ Publish refuses breaking changes under non-breaking version bump: `src/publish.rs` → `diff_public_api` + `e2601` (E2601). `jet publish` runs pre-publish gate + API surface extraction.
- ✅ Publish refuses packages that fail `jet build` or `jet test` (D-PKGS4 pre-publish gate): `run_publish` in `src/main.rs` runs sema check; `--force` bypasses with warning.
- ✅ `jet fetch --locked` and vendored builds work offline: `jet vendor` copies resolved dep dirs; existing `--locked` path in `fetch.rs` unchanged + tested in `vendored_offline_locked_build`.
- ✅ Resolver conflict diagnostics are readable: `check_conflicts` + `e2602` with PubGrub-style explanation (named packages and dependents).
- ✅ Private mirror flow works without hard-coding public infrastructure: `parse_registries_from_env` + `RegistryConfig` (configured via `JET_REGISTRY_<NAME>_URL` env vars).
- ✅ SBOM emits in SPDX 2.3 (tag-value) and CycloneDX 1.5 JSON from the lockfile; both golden-tested.
- ✅ Single-file programs still bypass all package machinery (unchanged).
- ✅ Advisory database format + `jet audit` with E2603; `e2604` integrity checks.
- ✅ E2601/E2602/E2603/E2604 registered in `docs/spec/diagnostics.md`.
- ✅ `examples/features/50_publishable_pkg/` — publishable package with public API; golden-pinned.
- ✅ `tests/pkg.rs` — 15 new M8 tests covering all exit criteria (semver_break_e2601, resolver_conflict_e2602, vendored_offline_locked_build, sbom_spdx_golden, sbom_cyclonedx_golden, audit_e2603_on_vulnerable_dep, integrity_e2604_on_tampered_store, private_registry_from_env, pre_publish_gate_*).
- ✅ full `nix develop -c cargo test` green (45 pkg tests, all other suites green).

**Not implemented (by design or deferral):**
- Live registry upload (D-PKGS1: git registry ops are "hosted later"; `jet publish` validates locally and explains the git-push path).
- Signed binary cache with rollback (explicitly out-of-scope per plan; designed only).
- `jet explain E2601` etc. auto-populate from diagnostics.md (explain reads the spec).

New modules: `src/publish.rs`.
New commands: `jet publish`, `jet vendor`, `jet audit`, `jet sbom`.

### E2-M6 completion record

Implemented on `master` (2026-06-17). Exit criteria status:

- ✅ S61 argument labels (checked at call site) + trailing default values (synthesised at call site when args omitted)
- ✅ S62 trait delegation `impl Type: Trait using field` — synthesis pass injects forwarding methods before sema body check; codegen emits correct trait-method calls (no `user_` prefix for trait methods)
- ✅ S77 field punning `Type { name }` ≡ `Type { name: name }` — parser fills pun value with `Expr::Ident`
- ✅ D-LIB2 default method bodies — synthesis pass injects default bodies for omitted trait methods
- ✅ D-LIB3/D-ERR2 Fallible `?` conversion — `impl E: Fallible` lets `?` in `T ? Error` functions convert; codegen emits `.map_err(|e| e.to_error())?`
- ✅ E2401 (delegation field missing or field type not implementing trait) — ui fixture `tests/ui/e2401_delegation_no_impl.{jet,stderr}`
- ✅ E2402 (Fallible path missing for `?`) — ui fixture `tests/ui/e2402_fallible_missing.{jet,stderr}`; the existing `tests/ui/try_wrong_error.jet` (non-Error return type mismatch) still emits E0403
- ✅ E2403 (field pun not in scope) — covered by E0107 ("nothing named X exists"); fixture `tests/ui/e2403_pun_not_in_scope.{jet,stderr}` captures this
- ✅ L2401 (public fn with positional Bool) — lint fires in both `check_with_mode` and `check_bundle` paths
- ✅ `examples/features/47_library.jet` — demonstrates all M6 features; golden-pinned
- ✅ E24xx codes registered in `docs/spec/diagnostics.md`
- ✅ full `nix develop -c cargo test` green

**Blocked (not implemented):**
- D-ERR1 (Error carrier growth — msg + code + source): decision not ratified; left as `String` backing
- E2403 distinct code for field-pun context: E0107 fires instead; "did you mean" for similar field names not implemented (I8: would need significant code, no ratified slot yet)

Codegen fix: `emit_trait_method` params were emitted with `_user_` prefix (hiding them from the body); removed `_` prefix. Trait def `emit_trait_def` now uses matching convention (Read non-scalars → `&T`) so impl and def agree. Trait-method calls on concrete types no longer get `user_` prefix (tracked in `cx.trait_methods`).
Read the sidequest files in `docs/plans/sidequests/mN-*.md` alongside each plan.

(Update this section after each milestone commit so a fresh agent resumes here.)
