# Epoch-2 implementation stream — progress & resume log

**This is the resume doc for the EPOCH-2 implementation stream.**

## Current branch: master

All prior epoch-2 work (`epoch-2-impl`) and the jetpack/jetos track
(`jetos-ratified-arc`) have been merged into `master` (commit `5eec357`).
All future epoch-2 work is directly on `master`. No separate worktrees.

## Completed milestones

E2-M1 ✅ M2 ✅ M3 ✅ M4 ✅ M5 ✅ M6 ✅ M8 ✅ M13 ✅ M14 ✅

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
