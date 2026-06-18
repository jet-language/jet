# D-JPK-FILES — Jetpack file structure rename + `jetpack.toml`

**Status: ratified 2026-06-18** (D-JPK-FILES in `syntax-decisions.md`); ready to
implement. Revises U1/U10.

## What ratified

| File | Format | Location | Role | Checked in? |
|---|---|---|---|---|
| `jetpack.toml` | TOML | repo root | monorepo manifest: `[repo]`, `[sources]`, `[packages]` | yes |
| `env.jet` | Jet | repo root | dev environment (sources + packages + prompt) | yes |
| `pkg.jet` | Jet | package dir (user-chosen) | package identity: `payload: { name, version }` + `packages: {…}` | yes |
| `.jet/lock` | TOML | `.jet/` | generated lockfile | no |
| `.jet/cache/` | — | `.jet/` | generated build cache | no |

Two surface changes from U10:
1. **Rename** the package-manifest filename `payload.jet` → `pkg.jet`. The
   `payload: { … }` identity **block name inside the file is unchanged**.
2. **Add** a new TOML monorepo manifest `jetpack.toml` at repo root.

`config.jet` / jetos tier stays deferred to Epoch 3 — out of scope here.

## Why this is a milestone, not a one-liner

`payload.jet` is hardcoded as `PAYLOAD_FILE` in `src/syntax.rs:604` and drives the
loader, manifest parser, and jetpack provider/env modules, plus tests and example
fixtures. The `jetpack.toml` parser is genuinely new (TOML, distinct from the
Jet-syntax `pkg.jet`). Renaming example files alone would break the build because
the loader searches for `payload.jet`.

## Plan

### Phase 1 — rename `payload.jet` → `pkg.jet` (mechanical, keep green)

1. `src/syntax.rs:604` — `PAYLOAD_FILE = "payload.jet"` → `"pkg.jet"`. Update the
   doc comments at lines ~425/446/601/626 that name the file.
2. `src/loader.rs` — retarget the upward-walk discovery (lines ~5, 49, 108, 118,
   261, 362) and comments to `pkg.jet`.
3. `src/manifest.rs` — module doc + `jet new` template generator (line ~317) emit
   `pkg.jet`; comment-preserving edit helpers unchanged in logic.
4. `src/jetpack/*` — `envfile.rs`, `refspec.rs` comments/probe target → `pkg.jet`
   (U9 provider-kind probe peeks at `pkg.jet`).
5. `src/publish.rs` — registry-section placeholder comment → `pkg.jet`.
6. Examples: rename every `examples/jetpack*/**/payload.jet` → `pkg.jet`
   (`jetpack/jet-pkgs/`, `jetpack-config/jet-pkgs/`).
7. Tests: `tests/ffi.rs` (`payload.jet` path), `tests/ui/manifest_*/payload.jet`
   fixtures, `tests/ui/use_unrealized_library/payload.jet`, any `tests/pkg/`
   fixtures → `pkg.jet`. Re-bless ui snapshots that print the filename.
8. `nix develop -c cargo test` green. Commit: "D-JPK-FILES P1: payload.jet → pkg.jet".

### Phase 2 — `jetpack.toml` monorepo manifest (new parser)

1. `src/jetpack/manifest_toml.rs` (new) — hand-parsed TOML subset (I6, reuse the
   `src/manifest.rs` TOML-subset approach if one exists, else std-only): tables
   `[repo]` (`name`, `version`), `[sources]` (`name = "provider@target#ver"`),
   `[packages]` (`name = "relative/pkg.jet"` index, optional — discovery is
   `find . -name pkg.jet`).
2. Diagnostics: reuse E12xx family; add codes for malformed `jetpack.toml`
   (claim in `docs/spec/diagnostics.md` first, per I4 — no snapshot, no
   diagnostic). At minimum: bad TOML shape, unknown table/key with did-you-mean.
3. Wire into jetpack CLI discovery: `jetpack list/build/enter` read root
   `jetpack.toml` for sources, then `find . -name pkg.jet` for packages.
4. `env.jet` source names resolve against `[sources]` in `jetpack.toml` (or
   inline `pkg.source(...)`).
5. Examples: add a root `jetpack.toml` to `examples/jetpack/` and a multi-package
   fixture showing `[packages]` index + two `pkg.jet` members.
6. Tests: `tests/` fixture for `jetpack.toml` parse, source-name resolution,
   `find`-based package discovery, malformed-manifest ui snapshots.
7. `.gitignore` guidance: `.jet/lock`, `.jet/cache/` ignored; root manifests
   tracked. Update any scaffolding (`jet new` / `jetpack` init) to match.

### Phase 3 — docs + invariants

1. Update `IMPLEMENTATION-STATUS.md` in `jetpack-jetos/` to mark D-JPK-FILES built.
2. Confirm `spec.md` package section and `m12-packages.md` reflect `pkg.jet` +
   `jetpack.toml` (currently say `payload.jet` / `jet.toml`).
3. `nix develop -c cargo test` + golden examples green; `jet run` single-file
   path untouched (R9 holds — no manifest needed for `jet run file.jet`).

## Exit criteria

- No `payload.jet` string remains in `src/`, `tests/`, `examples/` (except a
  teaching alias if we keep one — default: clean break, no alias).
- `jetpack.toml` parses; bad input yields an E12xx diagnostic with a ui snapshot.
- Monorepo example: root `jetpack.toml` + ≥2 `pkg.jet` members build via
  `find`-based discovery.
- `jet run file.jet` still needs zero manifest (R9 / PM-I8).
- All snapshots re-blessed; `cargo test` green.

## Out of scope

- `config.jet` / jetos tier (Epoch 3).
- Registry/publish changes beyond the filename (M12.2 owns those).
- Backward-compat alias for `payload.jet` (clean break unless owner asks).
