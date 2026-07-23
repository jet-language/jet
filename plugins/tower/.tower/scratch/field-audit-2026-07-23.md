---
title: Field audit: day-zero peer gaps, actionable only
---
# Field audit — 2026-07-23 (day-zero frame)

Rule for this report: every language, Jet included, ships tomorrow with no history. Age, trust, and package counts are not findings. Every gap names work Jet can do.

Verified shipped Jet base: rustc AOT + Cranelift JIT + interpreter; `jet run/build/test/check/bench/fmt/repl/dev`; LSP + vscode/zed/tree-sitter; web/canvas target; D-MEM1 memory model; `T ? E` + `?`; comptime tiers; structured concurrency; allocator families; C FFI bridge; edition law D-REL1–5; 405 golden examples.

## Peers

**Rust** — systems code, wasm, CLIs. Rust does better: borrows are first-class, so you can return and store them; deep traits; strong IDE rename/refactor; a rich lint set. Jet does better: a memory model beginners can learn; one comptime instead of three macro systems; a JIT dev loop. Verdict: Jet takes the beginner; Rust takes the expert who hits the second-class-borrow ceiling. Do: audit what that ceiling blocks in real code, then ballot the sanctioned way past it; bring LSP rename/refactor to parity; add a lint tier. Flip: an expert ports a borrow-heavy Rust library without giving up.

**Go** — network services, CLIs, ops glue. Go does better: sub-second builds; fast `go run` start; a race detector; a built-in profiler (pprof); http and json in the box. Jet does better: sum types + `?` beat `if err != nil`; no GC pauses; scoped tasks beat leaked goroutines. Do: prove the dev-loop budgets (#666/#669/#676/#677); finish the http/encoding wave (#699–#730); add a profiler story; state and test whether the memory model rules out data races. Flip: edit-to-run under one second on a real service.

**Zig** — C replacement, embedded, cross builds. Zig does better: `zig cc` cross-compiles C to any target with bundled libcs; it imports C headers directly; binaries are tiny. Jet does better: memory safety; comptime is at parity. Do: add C header import to the FFI bridge arc; give cross-compile a first-class UX; set a binary-size budget. Flip: one jet command builds a project with C dependencies for another target.

**Python** — learning, scripts, glue. Python does better: repl-first work; zero-setup single-file scripts; instant start. Jet has a repl and an interpreter tier, so this is the closest fight for the beginner facet. Do: measure and budget script start time; ship notebook #442; polish the repl (completion, inline help). Flip: `jet run script.jet` feels as fast as `python script.py`.

**TypeScript** — web apps, tool scripts. TS does better: the browser is the runtime; feedback is live; structural types absorb messy data. Jet ships a web/canvas target and a dev server. Do: widen DOM/web API bindings; add browser debugging (source maps); ballot the union/any type (idea b4eclxq) before web data APIs bake casts in. Flip: a small real web app in Jet with no JS escape hatch.

**Kotlin** — services, Android, DSLs. Kotlin does better: flow typing (smart casts) removes casts after a check; type-safe builders make config read clean. Jet's typed config is the direct rival to those builders. Do: audit whether Jet narrows an optional after a check; if not, ballot it. Flip: no cast ceremony after `if x != none`.

**C#** — enterprise apps, game scripting. C# does better: a first-class step debugger; lazy LINQ queries; hot reload with state kept. Do: debugger #12; the lazy protocol (surface-research §1) — it is also the LINQ answer; check what state `jet dev` keeps across reloads. Flip: set a breakpoint and step a jet program in vscode.

**Swift** — apps, progressive disclosure. Swift does better: declarative UI (SwiftUI) and playgrounds. Do: nothing this epoch; a declarative layer above canvas is a later candidate, and notebook #442 covers playgrounds. Verdict: park it.

**Java** — large services, tooling depth. Java does better: mature profiler and heap tools; hot-swap debugging. The actionable items repeat Go (profiler) and C# (debugger). No new work item.

**Julia** — numeric and scientific work. Julia does better: linear algebra in the box; broadcasting on every operator; repl-driven science. Jet has fan-out `f.[…]` and unit literals. Do: settle the math/BLAS route (nixpkgs interim is ratified); ballot vector/matrix core types before a scientific stdlib bakes. Flip: a matrix example that reads as clean as Julia.

**Elixir** — long-running fault-tolerant services. Elixir does better: supervision restarts failed parts; you can inspect a live system. Jet chose a narrow restart rule on purpose; keep it. Do: make `jet live`/`services` show a running service's tasks and state. Flip: inspect a stuck task without stopping the service.

**Nim / Crystal** — Python-feel compiled code. Their pitch is Jet's pitch. Jet already avoids their dialect footgun (I8, no macro dialects). No unique gap; the binary-size budget (Zig row) covers the rest.

**Odin** — game code. Odin does better: vector/matrix/SOA types in the box. Do: the same vector/matrix ballot as Julia; one decision serves both. Flip: a small game loop needs no hand-rolled math types.

**Nix** — reproducible builds and system config. Day-zero Nix does better: binary-cache substitution; one lockfile pins a whole system; module merge with priorities. Jet does better: typed config; real diagnostics (I2/I4); one language from program to OS. Do: ship #653/#470 merge law with provenance; prove hangar substitution; land jetpack Phase 1 as the daily driver. Flip: you replace nix-shell for a week and do not go back.

## Ranked backlog (all actionable; existing cards cited)

Core:
1. Error why-chain — a failure through many `?` sites is anonymous today. surface-research §3. Card candidate.
2. Lazy iteration protocol — unlocks adapters, LINQ-class queries, and streams. §1, deferred slot.
3. Type ballots before stdlib bakes: union/any (idea b4eclxq) and vector/matrix (Julia + Odin + games).
4. Expert borrow-ceiling audit — name what second-class borrows block, then ballot the way out if one is missing.

Stdlib:
5. http/encoding wave (#699–#730, in flight); math/BLAS route via the ratified nixpkgs interim.

Tooling:
6. Dev-loop budgets proven (#666/#669/#676/#677) — this one item answers Go, Python, and TS feel.
7. Debugger #12; a profiler story; repl completion/help; browser source maps.
8. jetdoc #86 unfreeze — every peer ships a doc generator.

Packaging:
9. Publish UX + first-party semver gate (#6/#423 + surface-research §5); cross-compile UX (the Zig bar).

Docs:
10. A narrative first-hour tour. Cheapest item here; every peer ships one; spec and examples do not cover the first hour. Card candidate.

## Keep list — footguns Jet already avoids

No async two-color split. No null. No lifetime syntax. No macro dialects (I7/I8). rustc stays hidden (I2). One package tool. Typed config, never text templates. No detached spawn. Golden-tested examples (I5). Editions + `jet fix` ratified before day zero (D-REL1–5).
