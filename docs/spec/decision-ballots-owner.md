# Owner ballot responses

## Open

*(none)*

## Ratified 2026-06-16 — jetpack/jetos surface (gaps #4 & #5)

Recorded as **U11–U18** in [`syntax-decisions.md`](syntax-decisions.md).

| ID | Pick | Decision |
|---|---|---|
| D-SYS-FIELDS | **A** | `System` = `target` (typed) / `packages` / `services` / `options` → **U11** |
| D-SVC | **A** | `Service` open record (`enable: Bool` + typed fields) → **U12** |
| D-OPTS | **B, list-typed** | `options: [ net.hostName: laptop, … ]` — bare `key: value`, no `set()`; quotes only on free-form strings → **U13** |
| D-IMG-FIELDS | **B** | `Image { from: system.X, format: iso\|qcow\|raw }`; target/packages inherited → **U14** |
| D-JETOS-BIN | **C-mod** | `jetpack os <verb>` subcommand (no separate `jetos` binary) → **U15** |
| D-CFG-LOAD | **A-mod** | `jetpack os <verb> [<path>]@<host>`; path defaults `~/.jet/config.jet` → **U16** |
| D-LIB-USE | **A** | realized `library` consumed with `use <pkg>` → **U17** |
| D-INFER-CTOR | **ratify** | bare `{…}` elaborates to the expected type; explicit `Type {…}` optional → **U18** |

## Ratified 2026-06-16

| ID | Decision |
|---|---|
| D-CBIND2 | **A** — auto bind on compile/build + **`jet bind`** subcommand |
| D-CBIND3 | **B** — bindgen helper (I6 waiver) |
| D-CBIND5 | **A** — **`String`** at C string boundary |
| D-CBIND6 | **B** — `#define` **constants only**; skip function-like macros |
| D-LL2 | **B** — **`@audit("…")`** on `@unsafe` blocks |

C FFI surface, `use`, link resolution — see [`syntax-decisions.md`](syntax-decisions.md).
