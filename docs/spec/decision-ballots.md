# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file:** a full decision card carries a worked,
user-story example for each option (what a real person types, sees, and hits as an
error) — not abstract option tables. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card
(with examples) when it's time to decide it.

---

## Open decisions

Every open decision, listed and available now — nothing parked or hidden. Decide
any directly when it has a full card; for the one-liners, ask the dashboard to
**expand into a full card** (options + worked examples) when you want to decide it.
Submitting a decision records it in `syntax-decisions.md` and removes it from here.

### Constructors & values

### D-CTOR2 — Constructor marker (rec A)

D-CTOR1 ratified named constructors: a static function on a type that takes no `self` and returns the type *is* a constructor. The question now is whether such a function also needs a keyword or attribute to mark it as one, or whether its shape alone is enough. This is the first thing a beginner writes after `struct`, so the answer sets the tone for the whole language.

- **Option A — No marker.** The shape is the signal. A `self`-free static returning `Self` is a constructor; nothing else to learn.

    ```jet
    struct Point {
        x: Int
        y: Int

        fn origin() -> Point {
            Point { x: 0, y: 0 }
        }
    }

    let p = Point.origin()
    ```

- **Option B — `new` keyword.** A constructor must be introduced with `new`, so intent is explicit and the call site reads like allocation.

    ```jet
    struct Point {
        x: Int
        y: Int

        new origin() -> Point {
            Point { x: 0, y: 0 }
        }
    }
    ```

- **Option C — `@constructor` attribute.** Mark it with an attribute, leaving room for future constructor-only checks the compiler could enforce.

    ```jet
    struct Point {
        @constructor
        fn origin() -> Point {
            Point { x: 0, y: 0 }
        }
    }
    ```

**Recommendation:** A. A no-`self` static returning the type already reads as a constructor; a marker is ceremony a beginner has to learn for nothing. Keep the surface quiet.

### Allocators & memory

### D-ALLOC-C — Which allocators ship + namespace (rec A)

A beginner never touches this, but an expert writing a game loop or parser reaches for an allocator on day one. `Arena` is settled. The open questions: do we also ship `Bump`, `Pool`, and `Fixed` in v1, and does the expert allocator API sit flat in `core.mem` or grouped under `core.mem.alloc`?

- **Option A — Arena now, others staged, flat namespace.** Ship `Arena` only; add `Bump`/`Pool`/`Fixed` when a real use case forces them. Allocators live flat in `core.mem` alongside the rest of memory.

    ```jet
    let a = core.mem.Arena.new()
    let p = a.alloc(Point { x: 0, y: 0 })
    ```

- **Option B — All four now, flat.** Ship the full set up front so experts never wait on us, all flat in `core.mem`.

    ```jet
    let a = core.mem.Arena.new()
    let b = core.mem.Bump.new(cap: 4096)
    let pool = core.mem.Pool.new(of: Point)
    ```

- **Option C — All four now, grouped under core.mem.alloc.** Ship all four but namespace them under `core.mem.alloc` to keep `core.mem` uncluttered.

    ```jet
    let a = core.mem.alloc.Arena.new()
    let b = core.mem.alloc.Bump.new(cap: 4096)
    ```

**Recommendation:** A. One allocator carries v1; shipping four before a use case demands them is feature weight against the simplicity ratchet (I8). Flat keeps the import short; we can regroup if `core.mem` ever crowds.

### D-ALLOC-D — Reset/free verb + use-after-reset wording (rec A)

An arena is reused by clearing it between frames, not by freeing each object. The verb for that, and what the compiler says when you keep a pointer past the clear, is the difference between a teaching moment and a cryptic crash.

- **Option A — `reset` verb + diagnostic naming the reset site.** `reset` says "reuse this storage." Touch reset memory and the error points at the exact `reset` call that invalidated it.

    ```jet
    let a = core.mem.Arena.new()
    let p = a.alloc(Point { x: 1, y: 2 })
    a.reset()
    print(p.x)
    // > error[E0142]: use of arena memory after reset
    // >   the arena was reset here, which frees everything it allocated
    // >   `p` was allocated before that reset, so it no longer points at live memory
    // >   fix: move the `reset` after the last use, or re-allocate `p` afterward
    ```

- **Option B — `free` verb.** Use `free`, matching C muscle memory; the same use-after error fires.

    ```jet
    let a = core.mem.Arena.new()
    let p = a.alloc(Point { x: 1, y: 2 })
    a.free()
    ```

- **Option C — Both verbs, distinct meaning.** `reset` keeps the backing buffer for reuse; `free` returns it to the OS. Two verbs, two lifetimes.

    ```jet
    let a = core.mem.Arena.new()
    a.reset()  // keep buffer, reuse next frame
    a.free()   // give the memory back entirely
    ```

**Recommendation:** A. `free` reads like per-object deallocation, which an arena does not do; `reset` says reuse. A single verb keeps the model small, and the diagnostic pointing at the reset site is where the teaching happens.

### Named arguments

### D-NARG-D2 — Default referencing earlier params (rec B)

Someone writing `fn box(w: Int, h: Int = w)` expects height to default to width — a square. The question is whether a default expression may read an earlier parameter, or must stand on its own.

- **Option A — Allow it.** A default may reference parameters declared before it, evaluated left to right.

    ```jet
    fn box(w: Int, h: Int = w) -> Box {
        Box { w: w, h: h }
    }

    let sq = box(w: 10)        // h defaults to 10
    let r  = box(w: 10, h: 4)
    ```

- **Option B — Self-contained only.** Defaults must be literals or constants in v1. Reference an earlier param and you get a teaching error pointing you at passing the value explicitly.

    ```jet
    fn box(w: Int, h: Int = w) -> Box { ... }
    // > error[E0138]: default value refers to another parameter
    // >   `h`'s default reads `w`, but defaults must stand on their own in v1
    // >   fix: drop the default and pass `h` at the call site, e.g. `box(w: 10, h: 10)`
    ```

**Recommendation:** B. Param-referencing defaults pull evaluation order, and questions like "what if `w` itself is defaulted?" into v1 for a narrow win. Keep defaults self-contained; the call site is one short line.

### D-NARG-D4 — Dedicated label-mismatch diagnostic (rec A)

Swapping two argument labels, or typing one that does not exist, currently lands in the general E0104 type/arg error. A person who transposed `w` and `h` deserves to be told exactly that, with the fix.

- **Option A — Dedicated code (E0131).** Detect a swapped or unknown label and say so directly, suggesting the correct label.

    ```jet
    fn box(w: Int, h: Int) -> Box { ... }
    let b = box(h: 10, w: 20)   // labels present, order looks transposed
    let c = box(width: 10, h: 20)
    // > error[E0131]: unknown argument label `width`
    // >   `box` takes labels `w` and `h`; there is no `width`
    // >   fix: did you mean `w: 10`?
    ```

- **Option B — Keep folding into E0104.** Treat label mistakes as ordinary argument errors; no new code.

    ```jet
    let c = box(width: 10, h: 20)
    // > error[E0104]: arguments to `box` do not match its parameters
    ```

**Recommendation:** A. A label mistake has a precise, common cause and a one-word fix; a dedicated code (E0131) names it and hands over the correct label instead of leaving the user to diff the signature.

### Language surface

### S83 — External definitions for structs/modules (no rec)

Defining a method outside its struct body is common in other languages and handy for organizing large types. The blocker is purely the separator: every obvious token is already spent — `::` by D-BIND1, `.` by D-MOD1 — so out-of-body definitions have nothing to attach to.

- **Option A — Withdraw the feature.** Keep all members in the body. No new separator, no new mental model; large types stay in one place.

    ```jet
    struct Point {
        x: Int
        y: Int

        fn dist(self) -> Int { self.x + self.y }
    }
    ```

- **Option B — `->` separator.** Reuse the arrow to attach an external definition to a type.

    ```jet
    struct Point { x: Int, y: Int }

    Point->dist(self) -> Int {
        self.x + self.y
    }
    ```

- **Option C — `extend` keyword.** A keyword block names the type and holds the out-of-body members.

    ```jet
    struct Point { x: Int, y: Int }

    extend Point {
        fn dist(self) -> Int { self.x + self.y }
    }
    ```

**Recommendation:** none. The owner should pick a separator (or kill the feature); the simplicity ratchet leans toward A, but the call is open.

### D-JSON3 — Surface lenient JSON coercions (no rec)

D-JSON1 lets decode quietly turn `"8080"` into `8080` so a stringly config still loads. Quiet is friendly until something coerces that you did not expect. The question is how, if at all, a coercion is shown.

- **Option A — Per-decode coercion report.** Decode returns a value you can inspect for what got coerced.

    ```jet
    let r = json.decode(Config, text)
    for c in r.coercions {
        print("coerced " + c.field + ": " + c.from + " -> " + c.to)
    }
    // coerced port: String -> Int
    let cfg = r.value
    ```

- **Option B — Build/decode log line.** Emit one log line per coercion during decode; the value comes back plain.

    ```jet
    let cfg = json.decode(Config, text)
    // > note: json decode coerced `port` from String "8080" to Int 8080
    ```

- **Option C — Silent.** Coerce and move on; no surfacing at all.

    ```jet
    let cfg = json.decode(Config, text)
    print(cfg.port)   // 8080, no trace of the coercion
    ```

**Recommendation:** none. Trade-off between an inspectable value (A), ambient logging (B), and quiet (C) is the owner's call.

### D-TOOL-SPLIT — Split lsp/fmt/lint out of the `jet` binary (no rec)

Editor tooling — format, lint, language server — can live inside the one `jet` binary or ship separately. This shapes install size, release cadence, and how an editor finds the LSP.

- **Option A — One bundled binary.** Everything is a `jet` subcommand; one install, one version.

    ```jet
    // shell
    jet fmt src/
    jet lint src/
    ```

- **Option B — Separate binaries.** Ship `jet-fmt`, `jet-lint`, `jet-lsp` independently so each can release and update on its own.

    ```jet
    // shell
    jet-fmt src/
    jet-lsp --stdio
    ```

- **Option C — Plugin model.** `jet` loads tools as plugins discovered at runtime.

    ```jet
    // shell
    jet fmt src/      // dispatched to the loaded fmt plugin
    ```

**Recommendation:** none. Owner's call on packaging philosophy.

### Bigger directions (previously deferred — available to decide now, not parked)

### S53 — Concurrency: tasks & channels (rec C)

This is a direction card about timing, not final syntax. The planned surface spawns tasks, joins them, and passes data over channels; ownership rejects shared mutable state, so concurrency is data-passing by construction. The choice is how much of it lands in v1.

- **Option A — Pull the whole surface into v1.** Spawn, join, and channels all ship now.

    ```jet
    let t = tasks.spawn(|| heavy_work(input))
    let result = t.join()

    let ch = tasks.channel<Int>()
    tasks.spawn(|| ch.send(42))
    let got = ch.recv()
    ```

- **Option B — Minimal slice in v1.** Ship `spawn`/`join` only; defer channels.

    ```jet
    let t = tasks.spawn(|| heavy_work(input))
    let result = t.join()
    // channels: later
    ```

- **Option C — Hold for v2 as planned.** Keep the surface sketched but out of v1.

    ```jet
    // proposed for v2:
    let t  = tasks.spawn(|| heavy_work(input))
    let r  = t.join()
    let ch = tasks.channel<Int>()
    ```

**Recommendation:** C. v1 scope is already full; the surface is sketched so nothing is lost by waiting, and concurrency deserves to land complete rather than in slices.

### S56 — Typed reflection / user-defined derives (rec C)

Direction card. Built-in derives (S55) already cover the common cases. This adds user-written derive macros plus typed reflection — the S26 Layer 3 work slated for Epoch 3. Decide whether to pull any of it forward.

- **Option A — Start now.** Ship user derives and typed reflection in v1.

    ```jet
    derive Serialize for Point {
        // user-written derive body, runs at compile time
    }

    let fields = reflect(Point).fields   // [{ name: "x", ty: Int }, ...]
    ```

- **Option B — Reflection only, defer derives.** Ship typed reflection queries; hold user-written derive macros.

    ```jet
    let fields = reflect(Point).fields
    for f in fields { print(f.name + ": " + f.ty.name) }
    ```

- **Option C — Hold for E3 as planned.** Keep both with the S26 Layer 3 schedule.

    ```jet
    // proposed for Epoch 3:
    derive Serialize for Point { ... }
    let fields = reflect(Point).fields
    ```

**Recommendation:** C. Built-in derives carry v1; user derives + reflection are a large, coupled surface that belongs with the rest of S26 Layer 3 in E3.

### S60 — Compile-time pure evaluation + data embedding (rec C)

Direction card. `comptime` Layer 2 runs pure evaluation at build time and can embed a data file into the binary. The design is complete; the question is whether to promote it from post-1.0 into v1.

- **Option A — Promote both into v1.** Pure `comptime` eval and file embedding ship now.

    ```jet
    let table = comptime { build_lookup_table(256) }
    let schema = comptime { embed("schema.json") }
    ```

- **Option B — Pure eval only.** Ship `comptime` evaluation; defer embedding.

    ```jet
    let table = comptime { build_lookup_table(256) }
    ```

- **Option C — Hold post-1.0 as planned.** Keep the whole feature where it is.

    ```jet
    // proposed post-1.0:
    let table  = comptime { build_lookup_table(256) }
    let schema = comptime { embed("schema.json") }
    ```

**Recommendation:** C. Design-complete is not the same as in-scope; promoting `comptime` widens v1 surface and its testing burden. Hold as planned unless a v1 feature actually needs it.

### D-OS1 — jetos config & platform surface (no rec)

Direction card. The jetos track is a declarative-OS config and platform DSL — a large surface scheduled as post-Epoch-3 research (context in `docs/plans/jetpack-jetos/`). The question is whether to start shaping it, prototype a sliver to learn, or hold.

- **Option A — Start shaping the config DSL now.** Begin designing the full declarative-OS surface.

    ```jet
    module jetos {
        service web {
            port: 8080
            restart: OnFailure
            depends: [db]
        }
    }
    ```

- **Option B — Prototype one slice.** Build a single service declaration end to end to learn the shape before committing.

    ```jet
    service web {
        port: 8080
        exec: "jet run ./serve.jet"
    }
    ```

- **Option C — Hold until after Epoch 3 as planned.** Keep it on the research track; no surface work yet.

    ```jet
    // proposed post-Epoch-3:
    module jetos {
        service web { port: 8080 }
    }
    ```

**Recommendation:** none. Whether to open this research track early is the owner's direction call.
