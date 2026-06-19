# Epoch 3 — product pillars (planning)

**Status:** owner-directed backlog — **not** Epoch 2 exit criteria. Items here
may start as design notes during Epoch 2 but do not ship until Epoch 3 unless
promoted.

Epoch 2 GA (E2-M17) is complete; development highlights are in `docs/spec/roadmap.md`.

---

## Pillars

| Doc | ID(s) | Summary |
|---|---|---|
| [`jit-runtime-type-server.md`](jit-runtime-type-server.md) | D-DEV2 | Long-lived JIT process; hot-swap typed handlers; TS/JS-class app server |
| [`async-networking.md`](async-networking.md) | D-NET2, E2-V5 | `@async` runtime; Go-class concurrency; 100k+ connections |
| [`plugin-api.md`](plugin-api.md) | D-DX5-B | Formal `jet` plugin ABI (PATH discovery stays Epoch 2) |
| [`user-derives-reflection.md`](user-derives-reflection.md) | S56, layer 3 | User-defined `@derive` / typed reflection |
| [`c-header-bindings.md`](c-header-bindings.md) | D-CBIND2…6 ✅ ratified | `jet bind` engine — surface in **E2-M14** / S59 |
| [`package-build-from-source.md`](package-build-from-source.md) | M9 wave-2 | jetpack compiles a dependency from its Jet source in `realize()`; then `jet.regex`/`jet.archive`/`jet.db` ship as real packages (owner, 2026-06-18: → Epoch 3) |
| [`testing-docs-ergonomics.md`](testing-docs-ergonomics.md) | D-TEST1, D-TEST4 | property testing (w/ shrinking), doctests, coverage — syntax-gated M11 niceties (owner, 2026-06-18: → Epoch 3) |

---

## Also deferred here (cross-links)

| Topic | Epoch 2 today | Epoch 3 doc |
|---|---|---|
| Expression-body `fn … = expr` | deferred (D-FP2) | revisit when one-liner `fn`s pile up |
| Cranelift JIT in `jet dev` | interpreter only | `jit-runtime-type-server.md` |
| Go-scale HTTP/WebSocket servers | S53 tasks/channels for internal scale | `async-networking.md` |

---

## Promoting a pillar

1. Owner ratifies syntax in `docs/spec/syntax-decisions.md`.
2. Add `epoch-3/mNN-….md` milestone file with exit criteria.
3. Move rows out of this README into that milestone when work starts.
