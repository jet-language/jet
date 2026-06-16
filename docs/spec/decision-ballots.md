# Decision ballots — open queue (owner input needed)

Ratified decisions live in `docs/spec/syntax-decisions.md` and
`docs/plans/epoch-2/` / `docs/plans/epoch-3/` and are **removed from this queue**.

Write owner picks in `docs/spec/decision-ballots-owner.md`.

---

## Open queue

| ID | Question | Needed by |
|---|---|---|
| **D-CFFI2-SYN** | C FFI **surface syntax** — how you declare and call foreign C libs (link resolution already ratified) | E2-M14 |

**Interactive copy:** open [`decision-ballots.html`](decision-ballots.html) in a browser — card **D-CFFI2-SYN** under *C FFI syntax*.

**Worked examples:** [`docs/plans/epoch-2/c-ffi-syntax-examples.md`](../plans/epoch-2/c-ffi-syntax-examples.md) (raylib user stories per option).

---

## D-CFFI2-SYN — C FFI surface syntax

**Question:** How should Jet source declare C imports and expose them at call sites?

**Already ratified (do not re-litigate here):**

- **D-CFFI1** — import-only first; Jet-export to C later.
- **D-CFFI2 resolution** — Jetpack hangar dep (content-hash pinned) if `payload.jet` /
  `pack.jet` declares a matching key; else **`pkg-config <link-name>`**; missing → **E3201**
  naming both fixes.
- **D-CFFI3** — ship a **raylib** showcase example.
- **Boundary** — by-value scalars/`String` at the edge; pointers only via E2-M13 +
  `import core.mem` + `@unsafe`.

**User story (Sam):** build a small raylib pong game — one `.jet` on a fresh laptop *or* a
Jetpack team project with a pinned hangar dep. Same source should work in both contexts;
only link discovery changes (automatic per D-CFFI2).

---

### How other languages do it

| Language | Declaration shape | Link / deps | Call site |
|---|---|---|---|
| **D** | `extern (C) int init_window(...);` per declaration | `dub.json` / `-L-lraylib` separate from syntax | Global names in translation unit |
| **Rust** | `#[link(name = "raylib")] extern "C" { fn InitWindow(...); }` | `build.rs` + pkg-config, or `-l` flags | Block-local or crate-root names; often `unsafe` |
| **Zig** | `@extern(c, .{ .name = "InitWindow" }) fn init_window(...)` | `@import("raylib")` or `-lraylib` in build | Module/file scoped |
| **C / C++** | `#include <raylib.h>` + header prototypes | `-lraylib` on linker command line | Global C names |
| **Swift** | `import raylib` (Clang module / generated interface) | SPM / Xcode link settings | Module-qualified |
| **Nim** | `proc init_window(...){.importc.}` + `{.passL:"-lraylib".}` | Pragmas on declarations or cfg | Usually qualified via import |
| **Go (cgo)** | `import "C"` + `#cgo LDFLAGS: -lraylib` comment block | Embedded in Go source above `import "C"` | `C.init_window(...)` — **`C.` namespace** |
| **Odin** | `foreign import raylib "system:raylib.lib"` + `foreign raylib { ... }` | Separate `foreign import` line | `raylib.init_window(...)` |

Jet already mirrors **Rust** for `extern rust` (S50). The open question is whether **`extern c`**
should follow Rust's block shape, Go's **`C.`** namespace, Odin's split import, Swift's
module import, or something Jet-specific like **`c.raylib`** beside **`core.fs`**.

---

### Options

Pick **one primary** (overrides for `system` / `hangar` can ride along — see each option).

#### A — D-style block + bare link id *(recorded in S59 today; owner re-review)*

```jet
extern c raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}

fn main() {
    init_window(800, 600, "pong");   // block-local global names
    close_window();
}
```

Overrides: `extern c system raylib { … }` · `extern c hangar raylib { … }`

| | |
|---|---|
| **Like** | D `extern (C)` block + separate link (link auto in Jet) |
| **Pros** | Shortest calls; one block; D-like |
| **Cons** | Pollutes file namespace; two C libs → name clashes; `raylib` must match dep key / pkg-config name; feels unlike `import core.fs` |

---

#### B — Rust-style `@link` on block

```jet
@link(name = "raylib")
extern c {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}
```

Resolution still uses `@link(name = …)` as the hangar/pkg-config key.

| | |
|---|---|
| **Like** | Rust `#[link(name = "raylib")] extern "C" { … }` |
| **Pros** | Familiar to Rust refugees; link metadata explicit; pairs with Jet `@` attributes (S82) |
| **Cons** | Still block-local global fn names; attribute + keyword noise; link name divorced from path |

---

#### C — Go cgo-style `C` namespace module

```jet
extern c raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}

fn main() {
    c.init_window(800, 600, "pong");   // fixed `c.` prefix, not `raylib.`
    c.close_window();
}
```

Link key remains `raylib` inside the block header; calls always go through **`c.`**.

| | |
|---|---|
| **Like** | Go `C.function()` after cgo `import "C"` |
| **Pros** | One obvious foreign prefix; no clash with Jet `fn init_window` |
| **Cons** | **Two** C libraries both live under `c.` — still clashes; prefix doesn't name the lib |

---

#### D — Odin-style split: `foreign import` + bare `extern c`

```jet
foreign import raylib;              // hangar if dep, else pkg-config

extern c {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}
```

| | |
|---|---|
| **Like** | Odin `foreign import` + `foreign lib { … }` |
| **Pros** | Link boundary explicit; multiple `extern c` blocks can share one `foreign import` |
| **Cons** | New keyword **`foreign import`**; two top-level forms; still global fn names inside block |

*Same idea, Jet spelling:* **`link c raylib;`** + `extern c { … }` — see option **F**.

---

#### E — Jet module path `c.<lib>` *(mirrors `core.fs`)*

```jet
extern c.raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}

fn main() {
    import c.raylib as rl;
    rl.init_window(800, 600, "pong");
    rl.close_window();
}
```

Link key = last segment **`raylib`**. Overrides: `extern c.system.raylib { … }` ·
`extern c.hangar.raylib { … }`

| | |
|---|---|
| **Like** | `import core.fs as fs` / `jet.core.fs` — first segment marks the ring |
| **Pros** | Same import story as core std; multiple C libs compose; `c.` signals foreign ABI |
| **Cons** | Longer unless aliased; amends today's bare `extern c raylib` ratification |

---

#### F — Separate `link c` + shared `extern c` block

```jet
link c raylib;                      // resolution only

extern c {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}
```

| | |
|---|---|
| **Like** | C translation unit `#include` + `-l` split; Odin without `foreign` keyword |
| **Pros** | Link once, many declaration blocks; clear separation |
| **Cons** | **`link c`** is new surface; active link is implicit state — harder for beginners and LSP |

---

#### G — Manifest-only link + `import c.<lib>` module file

```jet
// pack.jet / payload.jet
[dependencies:c]
raylib = "nixpkgs:raylib#5.5.0"     // hangar pin; or system via pkg-config when absent

// src/raylib_bindings.jet  (hand-written or future jet-bind output)
extern c.raylib { fn init_window(...) = "InitWindow"; }

// src/pong.jet
import c.raylib as rl;
```

| | |
|---|---|
| **Like** | Swift `import Raylib` + SPM; separates **deps** from **call** syntax |
| **Pros** | Single-file scripts stay thin; team projects mirror package boundaries; natural home for Epoch 3 `jet bind` output |
| **Cons** | Two files for nontrivial C use; single-file pong needs inline block anyway |

---

#### H — Nim-style header string + per-fn import

```jet
import c "raylib.h" as rl;          // conceptual — may map to hangar include root

@extern(c, name = "InitWindow")
fn rl.init_window(w: Int, h: Int, title: String);
```

| | |
|---|---|
| **Like** | Nim `importc` / header paths |
| **Pros** | Matches C header mental model; header path visible |
| **Cons** | Heavy attribute surface; hangar deps are often **libs**, not single headers; noisy for beginners |

---

#### I — Quoted link string block *(S59 draft spelling)*

```jet
extern c "raylib" {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
}
```

Same resolution as **A**; quotes mark the name as a **foreign link id** (like `extern rust "crate@1"`).

| | |
|---|---|
| **Like** | Rust string link names; C string literal habit |
| **Pros** | Visually foreign; odd pkg-config names easy to spell |
| **Cons** | Extra punctuation; identifier form cleaner when dep key == link name |

---

### Leans

| | |
|---|---|
| **Expert lean** | **E** or **G** — module paths scale; **A** is fine for one-off scripts but doesn't compose. **C** breaks with multiple C libs. **F**/`link c` is explicit but implicit active link is a footgun. |
| **Beginner lean** | **E** — same rule as `import core.fs as fs`; one prefix tells you "foreign C". **A** reads shortest but hides where symbols live. **G** is clearest once projects grow past one file. |
| **Prior rec (pre–owner review)** | **A** — recorded in S59. Owner flagged dissatisfaction → **re-open**. |

**Agent recommendation (2026-06-16):** **E** for call-site clarity + consistency with **`core.*`**; pair with **G** long-term (bindings live in their own module, main file only `import c.raylib as rl`). If you want maximum script brevity, **A** or **I** with mandatory `import c.raylib as rl` re-export is a hybrid worth spelling out in comments when you pick.

---

### Pick format

Reply in `decision-ballots-owner.md`:

```markdown
| D-CFFI2-SYN | **E** (+ optional: overrides `c.system` / `c.hangar`) |
```

Or mix: **G + E**, **A but require import alias**, etc.

---

## Recently ratified (this cycle)

| ID | Decision |
|---|---|
| S82 | `@` attributes (amends S43, S55, S58) |
| D-ERR2 / S80 | `Fallible` trait + `Error` type |
| D-DEV2 | JIT runtime type server → **Epoch 3** |
| D-FP2 | defer `fn … = expr` |
| D-REF3 | borrowed-return + cleanup inlay hints (A) |
| D-DX5 | PATH `jet-*` now; plugin API → Epoch 3 |
| D-PAT5 / S83 | multi-head functions (accept B) |
| D-PURE1 | pure eval + sandboxed package build blocks |
| D-PURE2 | no ambient I/O/network; `embed_file` only |
| E2-V12 | retired |
| D-TOOL4 | snapshot testing; `-u` / `--update-snapshots` |
| D-CFFI2 | hangar if dep else pkg-config *(resolution only)* |
| D-NET2 | Go-scale servers → Epoch 3 |
| S56 | user derives → Epoch 3 |
| S54 | PascalCase types; snake_case fn/modules; no user lint |

**Note:** D-CFFI2 **link resolution** is ratified. **D-CFFI2-SYN** (surface syntax) supersedes the provisional **`extern c raylib { }`** spelling in S59 until the owner picks again.
