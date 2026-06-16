# Jet decision ballot — owner responses

Ratified answers are in `docs/spec/syntax-decisions.md` (decision log) and
milestone plans.

## Open — needs owner pick

| § | ID | Question |
|---|---|---|
| — | **D-CFFI2-SYN** | C FFI **surface syntax** — see full ballot in [`decision-ballots.md`](decision-ballots.md) (options A–I, other-language comparisons). Link resolution (hangar → pkg-config) stays ratified. |

**Review:** [`decision-ballots.html`](decision-ballots.html) → card *D-CFFI2-SYN* · examples → [`c-ffi-syntax-examples.md`](../plans/epoch-2/c-ffi-syntax-examples.md)

## Ratified this cycle

| § | ID | Decision |
|---|---|---|
| — | S82 | `@` attribute syntax |
| — | D-ERR2 | `Fallible` trait + `Error` type |
| — | D-DEV2 | JIT runtime type server → **Epoch 3** (`docs/plans/epoch-3/jit-runtime-type-server.md`) |
| — | D-FP2 | **C:** defer `fn … = expr` |
| — | D-REF3 | **A:** borrowed-return + cleanup inlay hints |
| — | D-DX5 | **A now:** PATH `jet-*` discovery · **B Epoch 3:** formal plugin API |
| — | D-PAT5 / S83 | **B:** accept multi-head functions |
| — | D-PURE1 | **A:** pure eval + sandboxed package build blocks (not JetOS) |
| — | D-PURE2 | **A:** no ambient I/O/network; `embed_file` only |
| — | E2-V12 | **Retired** — use D-PURE + Epoch 3 pillar docs |
| — | D-TOOL4 | **A:** snapshot testing · flags **`-u` / `--update-snapshots`** |
| — | D-CFFI2 | Layered hangar/pkg-config resolution *(syntax → **D-CFFI2-SYN**, re-open)* |
| — | S54 | Amended: PascalCase default for types/traits/enums/constants; snake_case fn/modules; no user lint |
| — | D-NET2 | Go-scale concurrency → **Epoch 3** (`docs/plans/epoch-3/async-networking.md`) |
| — | S56 | User-defined derives / typed reflection → **Epoch 3** (`docs/plans/epoch-3/user-derives-reflection.md`) |

## When you decide D-CFFI2-SYN

Add a row under *Ratified this cycle* with your letter (A–I) and any override tweak; agents amend S59 and unblock E2-M14.
