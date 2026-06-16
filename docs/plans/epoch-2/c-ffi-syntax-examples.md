# D-CFFI2-SYN — C FFI syntax examples (raylib)

**Status:** owner review — full ballot with other-language comparisons in
[`docs/spec/decision-ballots.md`](../../spec/decision-ballots.md) (options **A–I**).
Link resolution (hangar → pkg-config) is ratified; pick surface syntax before E2-M14.

**Sam's story:** build a small raylib game. Single `.jet` on a fresh laptop, or a
Jetpack team project with a pinned hangar dep.

---

## Resolution (same for every variant)

| Context | Linker finds raylib via… |
|---|---|
| `payload.jet` has `deps: { raylib: nixpkgs:raylib#5.5.0 }` | **Hangar** (exact hash) |
| Single file, no Jetpack | **`pkg-config raylib`** (system install) |
| Neither | **E3201** — install system lib *or* add hangar dep |

---

## Variant map → ballot options

| Example section | Ballot letter | Notes |
|---|---|---|
| A — auto-resolve identifier | **A** | Provisional S59 spelling |
| B — quoted link name | **I** | Quoted link id |
| C — separate `link c` | **F** | `link c` + bare `extern c` |
| D — `deps.` / `system.` prefix | *(override tweak)* | Can pair with A or E |
| E — `c.<lib>` namespace | **E** | Mirrors `core.fs` |

Ballot-only options (no separate section here): **B** `@link` block, **C** Go `c.` prefix,
**D** Odin `foreign import`, **G** manifest + module file, **H** Nim header import — see
[`decision-ballots.md`](../../spec/decision-ballots.md).

---

## Variant A — auto-resolve identifier *(ballot A — provisional S59)*

```jet
extern c raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
    fn window_should_close() -> Bool = "WindowShouldClose";
}

fn main() {
    init_window(800, 600, "pong");
    while !window_should_close() {
        // draw …
    }
    close_window();
}
```

**Single file — no Jetpack:**

```bash
$ jet run pong.jet
# → pkg-config --cflags --libs raylib
# missing → "C library 'raylib' not found. Install it (pacman -S raylib)
#            or add raylib to payload.jet deps."
```

**Team — Jetpack:**

```jet
// payload.jet
deps: { raylib: nixpkgs:raylib#5.5.0 }
```

```bash
$ jetpack build && jet run src/pong.jet
# → hangar paths only; no system raylib required
```

**Explicit overrides (same variant):**

```jet
extern c system raylib { … }     // force pkg-config even in a Jetpack project
extern c hangar raylib { … }     // force hangar dep (error if dep missing)
```

| Pros | Cons |
|---|---|
| D-like: one block, one name | `raylib` must match dep key / pkg-config name |
| Auto picks best source | Magic unless diagnostics are excellent |
| Shortest spelling | |

---

## Variant B — quoted link name *(ballot I)*

```jet
extern c "raylib" {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
}
```

Same resolution as A; quotes echo C/header habit (`"raylib.h"` adjacent in docs).

| Pros | Cons |
|---|---|
| Visually “foreign string” | Extra punctuation |
| Matches `extern rust "crate@1"` feel | Identifier form is cleaner for deps |

---

## Variant C — separate `link c` + bare `extern c` *(ballot F)*

```jet
link c raylib;                    // hangar if dep, else pkg-config

extern c {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}
```

Symbol strings default to unmangled C names inside the active `link` scope.

| Pros | Cons |
|---|---|
| Link once, many blocks | Two top-level forms to learn |
| Good for many blocks same lib | `link c` is new keyword surface |
| Explicit link boundary | Less D-like (D uses `extern(C)` on declarations) |

---

## Variant D — Jetpack-aware path prefix

```jet
// auto (same as A)
extern c raylib { fn init_window(…) = "InitWindow"; }

// explicit hangar dep path — mirrors `import scoring` / payload keys
extern c deps.raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
}

// explicit system
extern c system.raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
}
```

| Pros | Cons |
|---|---|
| Hangar vs system obvious in source | Longest spelling |
| Mirrors payload `deps:` keys | Three forms to teach |
| No ambiguity in mixed projects | Heavier than D's `extern(C) import(...)` |

---

## Variant E — language-namespaced `c.<lib>` *(ballot E)*

Mirror **`core.fs`**: the **`c`** segment marks foreign C ABI; **`<lib>`** is
the link name (pkg-config / hangar dep key). Symbols live under a module path,
not in the file's global namespace.

```jet
extern c.raylib {
    fn init_window(w: Int, h: Int, title: String) = "InitWindow";
    fn close_window() = "CloseWindow";
}

fn main() {
    import c.raylib as rl;
    rl.init_window(800, 600, "pong");
    while !rl.window_should_close() {
        // draw …
    }
    rl.close_window();
}
```

Link resolution is unchanged: **`raylib`** (last path segment) → hangar dep if
present in `payload.jet`, else `pkg-config raylib`.

**Explicit overrides** (same layered rule, dotted spellings):

```jet
extern c.system.raylib { … }     // force pkg-config
extern c.hangar.raylib { … }     // force hangar dep
```

| Pros | Cons |
|---|---|
| Same import story as `core.fs` / `jet.core.fs` | One more segment vs variant A |
| Multiple C libs compose without name clashes | Slightly longer calls unless aliased |
| `c.` prefix signals foreign ABI at a glance | Amends ratified bare-`extern c raylib` shape |
| Resolution still keyed on `<lib>` segment | |

**Rejected pairing:** PascalCase link segments (`c.Raylib`) — C ecosystem and
pkg-config names stay lowercase; Jet PascalCase default (S54) applies to Jet
types inside the block, not to foreign link ids.

---

## Pointers + `@unsafe` (all variants)

By-value scalars/`String` need no gate. Raw pointers — same story as today:

```jet
import core.mem;

extern c raylib {
    fn load_file(path: String) -> Ptr<U8> = "LoadFileData";   // gated type
}

fn main() {
    @unsafe {
        val data = load_file("sprite.png");
        // …
    }
}
```

---

## D language comparison

D typically:

```d
extern (C) int init_window(int w, int h, const char* title);
// + separate -L-lraylib or dub.json dependency
```

Jet goal: **one block** + **automatic link resolution** (hangar pin beats system
when Jetpack is present) + **value types at the boundary** unless `@unsafe`.

---

## Pick

See **[`docs/spec/decision-ballots.md`](../../spec/decision-ballots.md)** — options **A–I** with
D / Rust / Zig / Go / Swift / Nim / Odin comparisons. Reply in
[`decision-ballots-owner.md`](../../spec/decision-ballots-owner.md) when ready.
