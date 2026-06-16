# E2-M14 — C FFI

**Status:** ready to implement — owner decisions **D-CFFI1…3** and **D-CFFI2-SYN-1…4**
ratified (2026-06-16). Canonical syntax: **S59** in
[`docs/spec/syntax-decisions.md`](../../spec/syntax-decisions.md).

**Depends on:** E2-M13 (pointer rules + `@unsafe` gates at the boundary).

**Unblocks:** sqlite in the E2-M9 ring (D-LR2), TLS in E2-M10 (D-NET1), E2-M15
link configuration.

**Error codes:** E32xx — must be registered in [`docs/spec/diagnostics.md`](../../spec/diagnostics.md)
with ui snapshots before shipping (I4).

---

## Goal

Connect Jet to the C ecosystem **without importing C's unsafety into ordinary Jet**.
Default path: **auto-generated bindings** from a header; power users **overlay**
with `@extern module`. Link discovery is automatic (hangar when Jetpack pins a dep,
else pkg-config). One command for scripts: `jet run pong.jet`.

---

## Ratified decisions

| ID | Decision |
|---|---|
| **D-CFFI1** | Import-only first — no Jet-export to C in M14 |
| **D-CFFI2** | Link: hangar dep if `payload.jet` / `pack.jet` matches **`<lib>`** key → else **`pkg-config <lib>`** → **E3201** |
| **D-CFFI3** | Ship **raylib pong** showcase (`examples/features/49_cffi.jet`) |
| **D-S16-USE** | **`use … as alias`** at call sites (S16; `import` is E0015) |
| **D-CFFI2-SYN** | **`@bindgen module c.<lib>.__bindgen__`** (autogen) + **`@extern module c.<lib>`** (overlay) |
| **D-CFFI2-SYN-1** | One C **`use` form per lib per file** — `use "raylib.h" as rl` *or* `use c.raylib as rl`, not both (**E3204**) |
| **D-CFFI2-SYN-2** | Empty `@extern module c.<lib> { }` = no overrides; full bindgen surface visible |
| **D-CFFI2-SYN-3** | Compile-time bind on cache miss/stale; cache **`.jet/bindings/c/<lib>.jet`**; optional **`jet bind`** CLI refresh |
| **D-CFFI2-SYN-4** | **Merge** effective module = bindgen ∪ overlay; **overlay wins** on name clash |

Rejected (do not implement): bare `extern c raylib { }` globals (old S59 A), shadow-only
override, two C `use` forms in one file.

---

## Surface syntax

### Layers

| Layer | Who writes it | Shape |
|---|---|---|
| **Autogen** | `jet bind` / compiler on cache miss | `@bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| **Overlay** | User (optional) | `@extern module c.<lib> { … }` anywhere in project tree |
| **Call site** | User | **`use … as alias`** — see below |

**Reserved:** segment **`__bindgen__`** — users cannot declare modules under it; compiler
rejects `@extern module c.<lib>.__bindgen__` and `@bindgen` outside generated cache files.

### Call sites (pick one per C lib per file)

```jet
use "raylib.h" as rl;       // header path → bind if needed → merged c.raylib
use c.raylib as rl;         // logical module → merged bindgen + overlay
```

Inside `@extern module` / `@bindgen module` bodies, declarations mirror Rust FFI shape:

```jet
fn init_window(w: Int, h: Int, title: String) = "InitWindow";
fn close_window() = "CloseWindow";
```

- **`= "Symbol"`** — C linker name (default unmangled).
- By-value scalars and `String` at the edge; structs need C-ABI layout (see sema rules).
- Pointers only with **`use core.mem`** + **`@unsafe { … }`** (E2-M13 / S58).

### Merge semantics (D-CFFI2-SYN-4)

When resolving **`use c.<lib> as alias`** (or header `use` that lowers to the same module):

1. Load **bindgen** symbols from `c.<lib>.__bindgen__` (generated cache or test fixture).
2. Load **overlay** symbols from user `@extern module c.<lib> { … }` if present.
3. **Effective module** = bindgen ∪ overlay.
4. On duplicate name: **overlay replaces bindgen** (same signature → ok; different signature → **E3205**).

Empty overlay `{ }` contributes zero symbols — effective module equals bindgen only.

### Link key

**`<lib>`** = last segment of `c.<lib>` (e.g. `raylib`). Used for:

- Hangar / `[dependencies:c]` lookup in `payload.jet` / `pack.jet`
- **`pkg-config <lib>`** when no hangar dep
- Cache filename `.jet/bindings/c/<lib>.jet`

**Header → lib mapping** (for `use "raylib.h" as rl`):

1. Strip directory and extension → basename (`raylib`).
2. Basename is the link key unless **`[dependencies:c]`** maps an alias (future: `raylib-h = { pkg = "raylib", header = "raylib.h" }` — **not in M14 v1**; basename rule only).
3. Trigger bind against that header path + link key on cache miss.

---

## User stories

### Sam — script, autogen only

```jet
// pong.jet
use "raylib.h" as rl;

fn main() {
    rl.init_window(800, 600, "pong");
    while !rl.window_should_close() {
        rl.begin_drawing();
        rl.clear_background(rl.dark_gray());
        rl.draw_text("pong", 360, 280, 32, rl.white());
        rl.end_drawing();
    }
    rl.close_window();
}
```

```bash
$ jet run pong.jet
# cache miss → jet bind (or internal equivalent) → .jet/bindings/c/raylib.jet
# link       → pkg-config raylib (or hangar if payload.jet pins raylib)
```

### Alex — team project, overlay trim

```jet
// payload.jet
[dependencies:c]
raylib = "nixpkgs:raylib#5.5.0"

// src/c/raylib.jet
@extern module c.raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
    fn window_should_close() -> Bool = "WindowShouldClose";
    // trimmed surface — bindgen still supplies the rest via merge unless
    // we later add "overlay replaces entirely" mode (not ratified; merge only)
}

// src/pong.jet
use c.raylib as rl;
```

**Note:** With merge semantics, overlay **adds/overrides** symbols; it does not hide
unmentioned bindgen symbols unless the project ships **no** bindgen cache and a fully
hand-written overlay (valid: delete/regen cache, maintain overlay only).

### Generated cache file shape

```jet
// .jet/bindings/c/raylib.jet — generated; do not hand-edit
@bindgen module c.raylib.__bindgen__ {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
    // …
}
```

```bash
jet bind raylib.h --pkg raylib -o .jet/bindings/c/raylib.jet   # manual refresh
```

---

## Link resolution (D-CFFI2)

| Context | Linker finds `<lib>` via… |
|---|---|
| `payload.jet` / `pack.jet` has `[dependencies:c]` or `deps:` entry for `<lib>` | **Hangar** (content-hash pinned) |
| Single file / no matching dep | **`pkg-config <lib>`** |
| Neither | **E3201** — install system lib *or* add hangar dep |

Codegen emits Rust `extern "C"` + link flags from resolved include/lib paths.

**Deferred (not M14):** explicit `c.system.<lib>` / `c.hangar.<lib>` override paths from
the old ballot — not ratified; reopen if needed.

---

## Implementation phases

Agents should land in order; each phase has tests before the next.

### Phase 1 — Overlay + merge (no bind yet)

**Unblocks:** hand-written raylib showcase, sema/codegen path proof.

- [x] Register in `src/syntax.rs`: `@bindgen`, `@extern`, `module` attribute forms,
      reserved `__bindgen__`, `c.` module root (I7).
- [x] Parser: `@extern module c.<lib> { … }`, `@bindgen module c.<lib>.__bindgen__ { … }`
      (generated files only for `@bindgen` — E3207 enforced in `cffi::assemble`).
- [x] Sema: merge bindgen + overlay; `use c.<lib> as alias`; C fn types at boundary.
- [x] Codegen: `extern "C"` + symbol strings + compiler-vetted wrapper shims; link
      flags threaded into the build (`-L native=…`, `-l <lib>`).
- [x] Hand-written `@bindgen module` cache fixtures drive `tests/cffi.rs` (simulates cache);
      a real linked+run example is the e2e test (small C lib via `cc`).
- [x] Register **E3203**, **E3205** (+ **E3206**, **E3207**) + ui snapshots under `tests/ui/cffi_*`.

### Phase 2 — Link discovery

- [x] Hangar dep → include/lib paths (`[dependencies:c]` reader in `payload.jet`;
      Jetpack realization of pinned refs is still fixture/manifest-text-backed).
- [x] Fallback `pkg-config <lib>` (`parse_pkg_config`).
- [x] **E3201** + snapshot (pinned in `tests/cffi.rs`; link-time, not in the ui harness).
- [x] Integration test: pkg-config flag parsing + E3201 path; hangar dep reader unit-tested.
      (`pkg-config` itself is absent from the dev shell, so the live pkg-config path is mocked.)

### Phase 3 — Header `use` + compile-time cache

- [x] `use "raylib.h" as rl` lowers to `c.raylib` merge path (header basename → link key).
- [x] Cache dir `.jet/bindings/c/<lib>.jet` is discovered and loaded on any C `use`.
- [ ] Invalidation by header hash + cflags hash, and bind on miss/stale: **WAIVED** — the
      bind backend (D-CBIND3) is not built in; caches are read as-is. See Exit criteria.
- [x] **E3204** (duplicate C `use` forms) + ui snapshot.

### Phase 4 — `jet bind` CLI

- [x] **`jet bind <header.h> --pkg <lib> [-o …]`** subcommand (**D-CBIND2** ✅) — shares the
      (not-yet-built) bind backend; reports **E3208** honestly on the missing translator.
- [x] Document in `jet bind --help` and top-level `jet help`; intended for cache refresh.

---

## Diagnostics (register before exit)

| Code | When |
|---|---|
| **E3201** | C library `<lib>` not found (hangar + pkg-config both failed) |
| **E3202** | Pointer or gated type crosses C boundary outside `@unsafe` / `core.mem` |
| **E3203** | Non-C-ABI type used by value in `@extern` / `@bindgen` fn signature |
| **E3204** | Both `use "…h"` and `use c.<lib>` for the same lib in one file |
| **E3205** | Overlay symbol clashes with bindgen with incompatible signature |
| **E3206** | User declared reserved `__bindgen__` segment |
| **E3207** | `@bindgen` outside `.jet/bindings/c/` generated file |
| **E3208** | `jet bind` failed — header parse / translation error (wrap backend message) |

Each needs what/why/fix in [`docs/spec/diagnostics.md`](../../spec/diagnostics.md) and a
`tests/ui/` snapshot (I4).

---

## Examples & tests

| Artifact | Purpose |
|---|---|
| `examples/features/49_cffi.jet` | Raylib pong (D-CFFI3); golden stdout |
| `tests/ui/cffi_*.jet` | E3201–E3208 |
| `tests/cffi.rs` | Link resolution (hangar mock + pkg-config skip/mock) |
| Hand-written `.jet/bindings/c/raylib.jet` in testdata | Phase 1 before bind lands |

---

## Scope

**In M14:**

- `@extern module` / `@bindgen module` declarations and merge
- `use c.<lib>` and `use "<header>"` (Phase 3)
- By-value boundary; pointers via E2-M13 gates
- Hangar + pkg-config link discovery
- Compile-time bind **integration hook** (calls bind backend on cache miss)
- Rust FFI (S50) unchanged

**Out of M14 (do not block exit on these):**

- Jet-export to C (D-CFFI1 rejected for v1)
- C++ ABI, name mangling beyond `= "symbol"`, varargs
- Callbacks from C into Jet
- Full cpp macro expansion (CBIND6 — skip + stubs when bind lands)
- `c.system.<lib>` / `c.hangar.<lib>` explicit override paths
- `[dependencies:c]` alias maps (header basename → different pkg name) — v2 ergonomics

---

## Prerequisites

### Owner (ballot)

| ID | Status |
|---|---|
| **D-CBIND2** | ✅ **A** — auto on compile + `jet bind` subcommand |
| **D-CBIND3** | ✅ **B** — bindgen helper (I6) |
| **D-CBIND5** | ✅ **A** — `String` at string boundary |
| **D-LL2** | ✅ **B** — `@audit("…")` on `@unsafe` |
| **D-CBIND6** | ✅ **B** — `#define` constants only; skip function-like macros |

### Agent checklist (not owner decisions)

| Item | Status | Notes |
|---|---|---|
| **E3201–E3208** in `diagnostics.md` | ✅ registered | ui snapshots ship with each diagnostic (I4) |
| **S59 keywords** in `syntax.rs` | ✅ registered | `c`, `__bindgen__`, `@bindgen`, `@extern module`, `bindings/c` |
| **E2-M13** pointer gates | E2-M13 | E3202 enforcement; Phase 1 can omit pointers in showcase |
| **`[dependencies:c]`** jetpack | jetpack | Phase 2 hangar tests may use fixtures until jetpack parses C deps |
| **D-CBIND1** primary surface | ✅ tool-generated `.jet` | aligns with S59 autogen default |
| **D-CBIND7** cache dir | ✅ `.jet/bindings/c/` | ratified via D-CFFI2-SYN-3 |

---

## Exit criteria

- [x] A C-FFI program links against a real C library and runs with deterministic
      output — `tests/cffi.rs::cffi_end_to_end_links_and_runs` builds `libjetc.a`
      via `cc` and prints `42` / `hi from C`. **Note:** the raylib pong showcase
      (`examples/features/49_cffi.jet`) is **NOT** shipped — it is graphical (no
      golden stdout) and the `examples/` golden harness has no C-link path; see
      waiver below.
- [x] pkg-config link path tested via flag-parser mock (`parse_pkg_config`); hangar
      `[dependencies:c]` reader unit-tested (`parse_c_dep`). Live `pkg-config` is
      absent from the dev shell, so its invocation is mocked, not run.
- [x] Merge: overlay overrides bindgen symbol; empty overlay = bindgen-only
      (`cffi_overlay_overrides_bindgen`, `cffi_empty_overlay_is_bindgen_only`).
- [x] `use "header.h"` and `use c.<lib>` both work; **E3204** on duplicate
      (`cffi_header_use_form_lowers_to_lib`, `tests/ui/cffi_e3204_two_use_forms`).
- [~] Pointer misuse → **E3202**: the diagnostic is registered + snapshot-pinned
      (`e3202_pointer_boundary_snapshot`), but **unreachable from real source**
      because the E2-M13 pointer tier (`Ptr<T>`/`core.mem`) is not implemented.
- [x] **E3201–E3208** registered with snapshots (front-end E3203/E3204/E3205/E3206/
      E3207 under `tests/ui/cffi_*`; link-time E3201, gated E3202, and CLI E3208
      pinned in `tests/cffi.rs` / produced by `jet bind`).
- [x] Rust `extern rust` unchanged; `nix develop -c cargo test` green.
- [WAIVED] Phase 3 cache regen on stale hash: the bind backend (D-CBIND3 bindgen
      helper) is not built into this binary, so there is no header→Jet translator
      to invalidate against. Caches at `.jet/bindings/c/<lib>.jet` are read as-is;
      `jet bind` and compile-time auto-bind both report **E3208** with the
      hand-written-overlay workaround. This ships Phase 1–2 + the header `use`
      lowering and cache *loading* (Phase 3 less regen) + the Phase 4 CLI shell.

### Status summary (REAL vs MOCKED/WAIVED)

- **REAL:** parser/sema/codegen for `@extern`/`@bindgen`/`use c.<lib>`/`use "h.h"`;
  merge semantics; synthetic-module wiring; `extern "C"` codegen with string/char
  edge conversions; build-time link-flag threading into `rustc`; end-to-end
  link+run against a `cc`-built static lib; E3203/E3204/E3205/E3206/E3207 with
  fixtures; E3201/E3202/E3208 with pinned/produced snapshots.
- **MOCKED:** live `pkg-config` (parser tested, binary absent); hangar realization
  of pinned `[dependencies:c]` refs (the table is read; Jetpack fetch is future).
- **WAIVED/STUBBED:** the header→Jet bind backend (E3208 stub), Phase 3 hash-based
  cache invalidation/regen, the raylib pong golden example, and E3202 reachability
  (blocked on E2-M13).

---

## References

- **S59** — [`docs/spec/syntax-decisions.md`](../../spec/syntax-decisions.md)
- **S16 / D-S16-USE** — `use` keyword
- **Ratified owner picks** — [`docs/spec/decision-ballots-owner.md`](../../spec/decision-ballots-owner.md)
- **Bind engine pillar** — [`docs/plans/epoch-3/c-header-bindings.md`](../epoch-3/c-header-bindings.md) (CBIND open picks)
