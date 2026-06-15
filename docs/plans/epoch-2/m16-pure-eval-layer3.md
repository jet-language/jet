# E2-M16 — Pure evaluation and package layer 3

**Status:** draft — **blocked on D-PURE1…D-PURE3** (Group M16) and bounded by
the strategic JetOS boundary **E2-V12** (Rec C: JetOS stays research-only).
Implements ratified S60 (`pure fn`).
**Depends on:** E2-M8 (store/lockfile), E2-M4 (interpreter for evaluation).
Provides the foundation for the jetpack-jetos track Phase 2.
**Error codes:** E34xx block (claim in docs/spec/diagnostics.md).

## Goal

Make purity a product feature and lay the groundwork for declarative
configuration/package recipes — Nix's best property (declarative, function-shaped
config) without Nix's evaluator mystique (design: docs/plans/jetpack-jetos/pack-abi.md).
Epoch 2 builds the foundation; JetOS itself stays research (E2-V12).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred |
|---|---|---|---|
| D-PURE1 | Recipe scope | **A** — pure eval + sandboxed package recipes | A |
| D-PURE2 | Sandbox guarantees | **A** — no ambient I/O or network during eval | A |
| D-PURE3 | Signed cache / rollback | **A** — design now, ship later; record generations | A |
| E2-V12 | JetOS / layer-3 boundary | **C** — JetOS research-only | C |
| D-FP3 (Group 16) | Core `module name { … }` declaration | **A** — typed, lowers to a pure fragment | A |

## Scope (from S60 + M12 layer 3)

- **`pure fn` checked modifier.** Purity in public signatures; impure calls
  inside a `pure fn` fail with a path explaining why (D-PURE2).
- **`jet eval --pure`.** Deterministic evaluation of a pure program/expression
  with call-trace diagnostics on failure.
- **Sandboxed package recipes** on the existing store/lockfile (D-PURE1): no
  ambient I/O or network access during evaluation.
- **`module … { }` declarations (D-FP3).** A core, typed, top-level declaration
  that lowers to a public pure fragment — the better-than-Nix `pack.jet` shape.
  `Shell`/`Profile`/`System`/`Image` stay ordinary types (jetpack supplies the
  schemas + merge semantics); LSP parses one shape everywhere, no DSL injection.
- **Signed caches + generations/rollback (D-PURE3).** Design now (generations,
  rollback depth), ship the signing later.
- **Jetpack integration path.** This unlocks jetpack-jetos Phase 2 system builds;
  Phase 1 (`jetpack run/build/list/...`) remains an independent track.

## Surface (example)

```jet
pub pure fn config(env: Env) -> Settings {
    return Settings { workers: 4, log_level: env.level ?? "info" };  // no I/O
}
```
```
$ jet eval --pure config.jet
{ "workers": 4, "log_level": "info" }     # deterministic, stable JSON
```
Calling an impure function inside `config` is **E3401** with the impurity path.

## Diagnostics to register

- **E3401** impure call inside a `pure fn` / pure-eval context (call-trace path).
- **E3402** package recipe attempted ambient I/O or network (names the call).
- **E3403** non-deterministic construct in pure evaluation (e.g. time/random).

## Examples & tests

- `examples/features/52_pure.jet` — a `pure fn` that evaluates to stable JSON.
- `examples/jetpack/` module-declaration example evaluating to a typed result.
- ui fixtures for E3401–E3403, each showing the path that broke purity.
- A determinism test: same input → byte-identical output across runs.

## Out of scope

- Shipping JetOS as a product (E2-V12 = research-only).
- Shipping the signed cache (design only this milestone).
- General effect system beyond the `pure`/impure split (S60 is the line).
- JetOS option/module merge semantics beyond what jetpack Phase 2 needs.

## Exit criteria

- Pure evaluation is deterministic and has call-trace diagnostics.
- Impure calls fail with a path explaining why.
- Package recipes cannot perform ambient I/O or network access.
- A small declarative config example evaluates to stable JSON.
- `nix develop -c cargo test` green.
