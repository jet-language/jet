# Jetpack / jetos — implementation status

**Updated:** 2026-06-17. Durable record of what the `jet` + `jetpack` + `jetos`
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
- `payload.jet` manifest (U10): `payload:`/`deps:`/`packages: { name:
  library|executable }`/`edition` — `src/jetpack/packmanifest.rs`. Diagnostics
  E1210–E1213.
- Typed env eval (`src/jetpack/modeval.rs`): `module dev { sources imports
  env.dev: Env {…} }` → `EnvPlan`; `find("./modules")` discovery; source merge.
  E0960–E0971.
- Sources U8/U9 (`github@`/`path@`/`nixpkgs@`, provider kind inferred);
  providers `core` + `nix` realizing into the hangar `/etc/jet/hangar`
  (`src/jetpack/provider.rs`); unified lockfile `.jet/lock` (`store.rs`).
- Commands `jetpack run/enter/build/list/clean/add/remove`; `jet dev` →
  `jetpack enter`. `Pkg` sugar (`default.[a, b]`). `use` keyword (S16/D-S16-USE);
  `import` → teaching error E0015.

### Landed in the `jetos-ratified-arc` pass (2026-06-16)
Branch `jetos-ratified-arc` off `master` @ `3e6be24`. Not yet merged to `master`.

| Feature | Decisions | Commit | Entry points | Diagnostics |
|---|---|---|---|---|
| **gap #5** — `System`/`Image`/`Service` semantically live: parse → elaborate (bare `{…}` inferred ctors) → field-check → captured into `EnvPlan.systems`/`.images` | U11–U14, U13, U18 | `4e44146` | `src/jetpack/modeval.rs` (`SystemPlan`/`ServicePlan`/`OptionPlan`/`ImagePlan`), `src/parser.rs`, `src/sema.rs`, `examples/jetpack-typed/` | E0972–E0978 |
| **gap #4** — `jetpack os build/switch [<config>]@<host>`: config.jet loader (reuses modeval), `@host` select, default `~/.jet/config.jet`, generation dir + `manifest.json` + `current`/boot `default` symlinks | U15, U16 | `be82d01` | `src/jetpack/jetos.rs`, `src/jetpack/cli.rs`, `examples/jetpack-config/` | E0979–E0981 |
| **U17** — `library` package consumed with ordinary `use <pkg>` (extra resolver search root); executables stay on PATH | U17 (D-LIB-USE A) | `d885585` | `src/loader.rs`, `tests/lib_use.rs` | E0982, E0983 |
| **B1–B4** — four codegen/parse ICEs fixed (I2/I3): JSON view-param move (clone + L0201), std-struct field mangling, `Map.get` via `Object(root)` pattern, `for … in recv.field {}` struct-lit misparse | — | `c4fff23` | `src/{sema,codegen,parser}.rs`, `tests/ice_regressions.rs` | (lint L0201) |
| **E2-M14 C FFI** — Phases 1–2 + compile-time hook: `@extern`/`@bindgen module c.<lib>`, bindgen∪overlay merge (overlay wins), `use c.<lib>`/`use "hdr.h"`, hangar/pkg-config link discovery, `jet bind` CLI | S59, D-CFFI1–3, D-CFFI2-SYN-1..4, D-CBIND2/3/5/6, D-LL2 | `cec262a` | `src/cffi.rs`, `tests/cffi.rs`, `tests/ui/cffi_*` | E3201–E3208 |
| **S84** — hyphens allowed in package/module/system/image/env *names* (kebab-case, finalist 2): dashed-name `ident (-ident)*` in name positions only (`expect_dashed_name`), span-adjacent hyphens so `a - b` stays subtraction — no lexer change. `image.halcyon-iso` + `from: system.my-host` parse→elaborate→field-check→realize | S84 | (this commit) | `src/parser.rs` (`expect_dashed_name`), `src/syntax.rs` (`NAME_SEGMENT_SEP`), `src/jetpack/modeval.rs` (tests), `examples/jetpack-typed/system.jet` | (reuses E0003 for malformed names) |

---

## Deferred / waived (with the prerequisite that gates each)

| Item | Status | Gated on |
|---|---|---|
| **C FFI real bind backend** — `jet bind` translates a C header into a `@bindgen` cache | ✅ done 2026-06-18 | owner superseded D-CBIND3=B (bindgen) with a **native std-only parser** (`src/cbind.rs`); no external crate, no libclang. `jet bind <hdr.h>` works; E3208 is now the honest parse-failure path. Compile-time auto-invoke on cache miss → E3. |
| **C FFI Phase-3 cache hash-regen** — caches are read as-is; no header/cflags-hash invalidation | deferred | follows the real bind backend |
| **E3202** (pointer crosses C boundary outside `@unsafe`/`core.mem`) — registered + snapshotted but unreachable from real source | unreachable | **E2-M13** (pointer / `@unsafe` / `core.mem` tier, S58) not yet built |
| **Raylib pong showcase** (`examples/features/49_cffi.jet`, D-CFFI3) — replaced by a deterministic small-C-lib e2e in `tests/cffi.rs` (graphical app has no golden stdout / no headless run) | substituted | a non-graphical golden, or a CI display harness |
| **gap #6 remainder** — richer interactive dev-shell beyond `jet dev`/`jetpack enter` | low priority | rides existing `shell::enter`; no new syntax |
| **The real Jet→binary compiler** — both package kinds stage source/prebuilt bytes; no compile step | not started | its own architecture design pass |
| **Cross-file language walls** (v1) | ✅ resolved 2026-06-18 | the Jet module system landed (D-MOD1–4): `module name;` file/dir modules, inline modules, `use alias.Item` / group / `pub use` re-export, private-by-default visibility. See `docs/plans/modules.md` + examples `42`–`49`. |
| **jetos D-OS2–D-OS6** — service/guard/option *declaration* syntax | open ballots | owner ratification (activation internals already shipped in gap #4) |

---

## Shipped since the arc

**S84 — hyphens in package / module / system / image / env *names*** (ratified
2026-06-16, finalist 2; see `docs/spec/syntax-decisions.md`). Name positions are
never expression positions, so hyphen handling lives entirely in the parser — no
lexer change, no `a - b` ambiguity:
1. `expect_dashed_name(where_)` next to `expect_ident` (`src/parser.rs`): reads an
   ident, then while the next token is `-` *span-adjacent* to the prior segment
   and immediately followed by an adjacent ident, appends `-<segment>`. Rule
   `ident (-ident)*` (no leading/trailing/double hyphen). Span adjacency keeps a
   spaced `a - b` as subtraction.
2. Swapped at the name call sites: contribution name, the `from: system.<name>`
   reference in `image_field`, and the `module` declaration name. Code
   identifiers (variables/fields/types/functions) stay plain `expect_ident`. The
   `payload.jet` manifest parser (`packmanifest.rs`) was already
   hyphen-transparent (it splits keys on `:` and keeps the name verbatim).
3. `src/syntax.rs` (I7): `NAME_SEGMENT_SEP` records the dashed-name rule under S84
   (no new sigil — reuses the `-`/Minus token).
4. modeval: no logic change; the System name definition and the image `from:`
   reference both flow through `expect_dashed_name`, so the E0978 cross-check
   still string-matches hyphenated names.
5. Malformed names (`image.-iso` leading hyphen, `image.a--b` double hyphen) fall
   through to the existing E0003 teaching diagnostic — never an ICE (tested).
6. `examples/jetpack-typed/system.jet` restored to the ratified spelling
   (`system.my-host`, `image.halcyon-iso`).

---

## How to verify
```
nix develop -c cargo test                         # 386 passed, 0 failed
nix develop -c cargo test --test decisions        # ratification gate
nix develop -c jet run examples/features/01_hello.jet
git log --oneline 3e6be24..jetos-ratified-arc     # the arc's commits
```
