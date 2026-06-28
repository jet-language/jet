# Epoch 3 — product pillars (planning)

**Status:** owner-directed backlog — **not** Epoch 2 exit criteria. Items here
may start as design notes during Epoch 2 but do not ship until Epoch 3 unless
promoted.

Epoch 2 GA (E2-M17) is complete; development highlights are in `docs/spec/roadmap.md`.

---

## Pillars

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
