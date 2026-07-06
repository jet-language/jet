# Epoch 3 — product pillars (planning)

**Status:** owner-directed backlog — **not** Epoch 2 exit criteria. Items here
may start as design notes during Epoch 2 but do not ship until Epoch 3 unless
promoted.

Epoch 2 GA (E2-M17) is complete; development highlights are in `docs/spec/roadmap.md`.

---

## Pillars

## Canonical Active Pushes

As of 2026-06-30, Tower groups related Epoch 3 work into these canonical parent
cards. Do not reopen the merged child cards unless the owner explicitly asks for
separate tracking again.

| Tower | Push | Merged child scope |
|---|---|---|
| #125 | JIT tier-1a parity | Cranelift dev-loop JIT must converge toward `tir_covers` parity |
| #126 | Concurrency runtime | M:N scheduler, native parkers, taskgroup combinators, select, deadlines, cancellation, observability |
| #134 | Reactive UI stack | signals, renderer, typed styles, component kit, motion, a11y, web/TUI/native backends |
| #1 | Memory/allocation controls | arenas, labeled `#Ref`, explicit buffers/allocators, `:= uninit`, opt-in GC |
| #117 | Core library breadth | DB driver interface, YAML, Unicode text, compression, linalg |
| #64 | Crypto/secrets | misuse-resistant envelopes, algorithm agility, TTL/rotting secrets |
| #65 | Precise numerics | BigInt, Decimal, float-money lint |
| #25 | Static guarantees | refinements, contracts, IFC, budgets, bounds proof, replay soundness |
| #41 | Protocol/representation hooks | iterator/index/suffix hooks, Display/Debug, rollback hooks |

Cards left separate are intentionally separate: syntax reopens (#149/#150),
module/comptime/plugin surfaces (#91/#92/#94/#5), developer tooling (#84/#97),
and JIT/debugger successors outside Epoch 3.

| Doc | ID(s) | Summary |
|---|---|---|
| [`../sidequests/jit-cranelift.md`](../sidequests/jit-cranelift.md) | D-JITDEP1, D-JIT2 | Cranelift tier-1 JIT over the `JitBackend` seam; hot-swap dev loop |
| [`async-networking.md`](async-networking.md) | D-NET2, E2-V5 | `@async` runtime; Go-class concurrency; 100k+ connections |
| [`plugin-api.md`](plugin-api.md) / [`../sidequests/plugin-target.md`](../sidequests/plugin-target.md) | D-PLUGIN1, D-DEP-WASM1 | Sandboxed WASM plugin target + formal plugin ABI |
| [`../sidequests/epoch-3-handoff.md`](../sidequests/epoch-3-handoff.md) | 2026-06-27 sweep | Current unblocked/gated card handoff |
| Tower cards c129–c131 | S56, D-METAREFLECT1, D-METADERIVE1 | User-defined derives and typed reflection |
| [`c-header-bindings.md`](c-header-bindings.md) | D-CBIND2…6 ✅ ratified | `jet bind` engine — surface in **E2-M14** / S59 |
| [`testing-docs-ergonomics.md`](testing-docs-ergonomics.md) | D-TEST1, D-TEST4 | property testing (w/ shrinking), doctests, coverage — syntax-gated M11 niceties (owner, 2026-06-18: → Epoch 3) |
| [`../sidequests/compression-codecs.md`](../sidequests/compression-codecs.md) | D-CODECS1 | `core.compress.gzip` + `core.compress.zstd` |
| [`../sidequests/unicode-text.md`](../sidequests/unicode-text.md) | D-GRAPHEME1 | Opt-in Unicode grapheme + normalization package |
| [`../sidequests/raylib-graphics.md`](../sidequests/raylib-graphics.md) | D-RAYLIB1 | Official `core.raylib` graphics bridge |
| [`typed-data-core.md`](typed-data-core.md) | D-WD9 | typed data Core plus accelerator bridges |
| [`core-game-substrate.md`](core-game-substrate.md) | D-WD10 | stable `core.game` substrate |
| [`typed-target-profiles.md`](typed-target-profiles.md) | D-WD11 | typed embedded and freestanding target profiles |
| [`adaptive-runtime.md`](adaptive-runtime.md) | D-ADAPTFID1 | adaptive runtime policy research |
| [`logic-programming-subset.md`](logic-programming-subset.md) | research | explicit solver and logic subset research |
| [`structural-merge.md`](structural-merge.md) | research | semantic-identity merge planning |

---

## Also deferred here (cross-links)

| Topic | Epoch 2 today | Epoch 3 doc |
|---|---|---|
| Expression-body `fn … = expr` | deferred (D-FP2) | revisit when one-liner `fn`s pile up |
| Cranelift JIT in `jet dev` | interpreter only | `../sidequests/jit-cranelift.md` |
| Go-scale HTTP/WebSocket servers | S53 tasks/channels for internal scale | `async-networking.md` |

---

## Promoting a pillar

1. Owner ratifies syntax in `docs/spec/syntax-decisions.md`.
2. Add `epoch-3/mNN-….md` milestone file with exit criteria.
3. Move rows out of this README into that milestone when work starts.
