# Jetpack / jetos — implementation status

**Updated:** 2026-06-18. Durable record of what the `jet` + `jetpack` + `jetos`
ecosystem actually IMPLEMENTS, versus the ratified design. The design-of-record
is [`unified-ecosystem.md`](unified-ecosystem.md) (target surface, U1–U18);
sequencing/roadmap is [`README.md`](README.md); the jetos tier is
[`jetos-design.md`](jetos-design.md). This file answers "is feature X built?".

Ratified ≠ implemented — check the tables below before assuming a feature exists.

---

## Shipping today (built, tested, green)

The typed `module { … }` surface builds and realizes end-to-end offline. Full
suite green: **`nix develop -c cargo test` → 386 passed, 0 failed**;
`tests/decisions.rs` (ratification enforcement) green; `jet run
examples/features/01_hello.jet` prints `hello, world`.

### Bootstrap + typed env surface (pre-existing)
- Two binaries `jet` + `jetpack` (Cargo.toml `[[bin]]`).
- `pkg.jet` manifest (U10, D-JPK-FILES): `payload:`/`deps:`/`packages: { name:
  library|executable }`/`edition` — `Source/Jetpack/PackageManifest.rs`. Diagnostics
  E1210–E1213.
- Typed env eval (`Source/Jetpack/ModuleEval.rs`): `module dev { sources imports
  env.dev: Env {…} }` → `EnvPlan`; `find("./modules")` discovery; source merge.
  E0960–E0971.
- Sources U8/U9 (`github@`/`path@`/`nixpkgs@`, provider kind inferred);
  providers `core` + `nix` realizing into the hangar `/etc/jet/hangar`
  (`Source/Jetpack/Provider.rs`); unified lockfile `.jet/lock` (`store.rs`).
- Commands `jetpack run/enter/build/list/clean/add/remove`; `jet dev` →
  `jetpack enter`. `Pkg` sugar (`default.[a, b]`). `use` keyword (S16/D-S16-USE);
  `import` → teaching error E0015.

### Landed in the `jetos-ratified-arc` pass (2026-06-16)
Branch `jetos-ratified-arc` off `master` @ `3e6be24`. Not yet merged to `master`.

| Feature | Decisions | Commit | Entry points | Diagnostics |
|---|---|---|---|---|
| **gap #5** — `System`/`Image`/`Service` semantically live: parse → elaborate (bare `{…}` inferred ctors) → field-check → captured into `EnvPlan.systems`/`.images` | U11–U14, U13, U18 | `4e44146` | `Source/Jetpack/ModuleEval.rs` (`SystemPlan`/`ServicePlan`/`OptionPlan`/`ImagePlan`), `Source/Parser.rs`, `Source/Sema.rs`, `examples/jetpack-typed/` | E0972–E0978 |
| **gap #4** — `jetpack os build/switch [<config>]@<host>`: config.jet loader (reuses modeval), `@host` select, default `~/.jet/config.jet`, generation dir + `manifest.json` + `current`/boot `default` symlinks | U15, U16 | `be82d01` | `Source/Jetpack/JetOS.rs`, `Source/Jetpack/CLI.rs`, `examples/jetpack-config/` | E0979–E0981 |
| **U17** — `library` package consumed with ordinary `use <pkg>` (extra resolver search root); executables stay on PATH | U17 (D-LIB-USE A) | `d885585` | `Source/Loader.rs`, `tests/lib_use.rs` | E0982, E0983 |
| **B1–B4** — four codegen/parse ICEs fixed (I2/I3): JSON view-param move (clone + L0201), std-struct field mangling, `Map.get` via `Object(root)` pattern, `for … in recv.field {}` struct-lit misparse | — | `c4fff23` | `src/{sema,codegen,parser}.rs`, `tests/ice_regressions.rs` | (lint L0201) |
| **E2-M14 C FFI** — Phases 1–2 + compile-time hook: `@extern`/`@bindgen module c.<lib>`, bindgen∪overlay merge (overlay wins), `use c.<lib>`/`use "hdr.h"`, hangar/pkg-config link discovery, `jet bind` CLI | S59, D-CFFI1–3, D-CFFI2-SYN-1..4, D-CBIND2/3/5/6, D-LL2 | `cec262a` | `Source/CFFI.rs`, `tests/cffi.rs`, `tests/ui/cffi_*` | E3201–E3208 |
| **S84** — hyphens allowed in package/module/system/image/env *names* (kebab-case, finalist 2): dashed-name `ident (-ident)*` in name positions only (`expect_dashed_name`), span-adjacent hyphens so `a - b` stays subtraction — no lexer change. `image.halcyon-iso` + `from: system.my-host` parse→elaborate→field-check→realize | S84 | (this commit) | `Source/Parser.rs` (`expect_dashed_name`), `Source/Syntax.rs` (`NAME_SEGMENT_SEP`), `Source/Jetpack/ModuleEval.rs` (tests), `examples/jetpack-typed/system.jet` | (reuses E0003 for malformed names) |

---

## Deferred / waived (with the prerequisite that gates each)

| Item | Status | Gated on |
|---|---|---|
| **C FFI real bind backend** — `jet bind` translates a C header into a `@bindgen` cache | ✅ done 2026-06-18 | owner superseded D-CBIND3=B (bindgen) with a **native std-only parser** (`Source/CBind.rs`); no external crate, no libclang. `jet bind <hdr.h>` works; E3208 is now the honest parse-failure path. Compile-time auto-invoke on cache miss → E3. |
| **C FFI Phase-3 cache hash-regen** — caches are read as-is; no header/cflags-hash invalidation | deferred | follows the real bind backend |
| **E3202** (pointer crosses C boundary outside `#Unsafe`/`core.mem`) — registered + snapshotted but unreachable from real source | unreachable | **E2-M13** (pointer / `#Unsafe` / `core.mem` tier, S58) not yet built |
| **Raylib pong showcase** (`examples/features/49_cffi.jet`, D-CFFI3) — replaced by a deterministic small-C-lib e2e in `tests/cffi.rs` (graphical app has no golden stdout / no headless run) | substituted | a non-graphical golden, or a CI display harness |
| **gap #6 remainder** — richer interactive dev-shell beyond `jet dev`/`jetpack enter` | low priority | rides existing `shell::enter`; no new syntax |
| **The real Jet→binary compiler** — both package kinds stage source/prebuilt bytes; no compile step | not started | its own architecture design pass |
| **Cross-file language walls** (v1) | ✅ resolved 2026-06-18 | the Jet module system landed (D-MOD1–4): `module name;` file/dir modules, inline modules, `use alias.Item` / group / `pub use` re-export, private-by-default visibility. See `syntax-decisions.md` (D-MOD1–4) + examples `42`–`49`. |
| **jetos D-OS2–D-OS6** — service/guard/option *declaration* syntax | open ballots | owner ratification (activation internals already shipped in gap #4) |

---

## Shipped since the arc

**S84 — hyphens in package / module / system / image / env *names*** (ratified
2026-06-16, finalist 2; see `docs/spec/syntax-decisions.md`). Name positions are
never expression positions, so hyphen handling lives entirely in the parser — no
lexer change, no `a - b` ambiguity:
1. `expect_dashed_name(where_)` next to `expect_ident` (`Source/Parser.rs`): reads an
   ident, then while the next token is `-` *span-adjacent* to the prior segment
   and immediately followed by an adjacent ident, appends `-<segment>`. Rule
   `ident (-ident)*` (no leading/trailing/double hyphen). Span adjacency keeps a
   spaced `a - b` as subtraction.
2. Swapped at the name call sites: contribution name, the `from: system.<name>`
   reference in `image_field`, and the `module` declaration name. Code
   identifiers (variables/fields/types/functions) stay plain `expect_ident`. The
   `pkg.jet` manifest parser (`packmanifest.rs`) was already
   hyphen-transparent (it splits keys on `:` and keeps the name verbatim).
3. `Source/Syntax.rs` (I7): `NAME_SEGMENT_SEP` records the dashed-name rule under S84
   (no new sigil — reuses the `-`/Minus token).
4. modeval: no logic change; the System name definition and the image `from:`
   reference both flow through `expect_dashed_name`, so the E0978 cross-check
   still string-matches hyphenated names.
5. Malformed names (`image.-iso` leading hyphen, `image.a--b` double hyphen) fall
   through to the existing E0003 teaching diagnostic — never an ICE (tested).
6. `examples/jetpack-typed/system.jet` restored to the ratified spelling
   (`system.my-host`, `image.halcyon-iso`).

---

## D-JPK-FILES — file structure rename + `jetpack.toml` (2026-06-18)

Ratified 2026-06-18. Phase 1 committed; Phase 2 parser landed (wiring pending).

**Phase 1 — `payload.jet` → `pkg.jet`** (commit `ca42fd0`): `PAYLOAD_FILE`
constant in `Source/Syntax.rs` changed to `"pkg.jet"`; all hardcoded strings in
`src/`, `tests/`, `examples/` updated; 12 fixture files renamed; 8 stderr
snapshots re-blessed. Tests: all green.

**Phase 2a — `jetpack.toml` parser** (this commit): `Source/Jetpack/ManifestTOML.rs`
— std-only TOML parser (I6); active tables `[repo]`, `[sources]`; retired
`[packages]` now emits E1225 pointing at `workspace.jet` (D-WORKSPACE1).
Diagnostics E1214/E1215/E1225 have rendered-form snapshots pinned inside
`manifest_toml.rs` (I4 note: `tests/ui/` harness only renders front-end `.jet`
diagnostics). New constants in `Source/Syntax.rs` (I7). Example at
`examples/jetpack/jetpack.toml`.

**Phase 2b — CLI wiring + discovery** (commit `TBD`): `manifest_toml::load`
wired into `load_project_plan` and `cwd_table` in `Source/Jetpack/CLI.rs`; `[sources]`
folded into the source table (additive — env.jet inline declarations win);
`SourceTable::merge_defaults` added to `Source/Jetpack/RefSpec.rs`; malformed
`jetpack.toml` exits 2 and prints E1214/E1215 from the real CLI path (tests
`malformed_jetpack_toml_fires_e1214_from_cli`, `malformed_jetpack_toml_fires_e1215_from_cli`
in `tests/jetpack.rs`); multi-package monorepo example at `examples/jetpack-mono/`
with `jetpack.toml` + `packages/greeter/pkg.jet` + `packages/logger/pkg.jet`;
`jet new` `.gitignore` now includes `.jet/lock` and `.jet/cache/`.

---

## How to verify
```
nix develop -c cargo test                         # all passed, 0 failed
nix develop -c cargo test --test decisions        # ratification gate
nix develop -c jet run examples/features/01_hello.jet
```
