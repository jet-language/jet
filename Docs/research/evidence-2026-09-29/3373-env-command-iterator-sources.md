# #3373 — Environment and command iterator source contracts (ITER-F062)

Question: do Jet's argv, environment and command-spec surfaces keep argument
order, environment snapshot/mutation and removal rules exact, and follow
platform law for non-Unicode OS data rather than lossy conversion?

Binary: `jet-debug-snapshot14` through `~/.cache/jet-luna/safe-jet.sh`, 2026-09-29.
Scratch: `~/.cache/jet-test-scratch/Closer04/` (`os_sources.jet`, `nonutf.jet`,
`nonutf2.jet`). Only variables prefixed `JET_OS_SOURCES_` are printed, so no
private environment data appears.

## Surface map (source read)

| Rust row | Jet surface | Status |
| --- | --- | --- |
| `Args` | `core.process.args()` / `argv()` → `[String]` (Core/process/process.jet:16-18) | partial: String only |
| `ArgsOs` | none | unsupported |
| `Vars` | `core.sys.vars()` → sorted **names** `[String] EnvError!` (Core/sys/sys.jet:20; core-library.md:1461) | partial: names, not pairs |
| `VarsOs` | none | unsupported |
| `CommandArgs` | `ProcessSpec.arg/args_extend` (write-only) | partial: no readback |
| `CommandEnvs` | `ProcessSpec.env/env_remove/env_clear` (HandleMethods.jet:938-940) | partial: no readback |

## c2 — order, snapshot/mutation, removal (`os_sources.jet`)

`jet run os_sources.jet -- b a c`:

```
argv tail: [b, a, c]
vars start: []
get BAD: <none>
snapshot: [JET_OS_SOURCES_A, JET_OS_SOURCES_B]
unset: true again: false
live: [JET_OS_SOURCES_B]
get A after unset: <none>
child: z|a|m|
JET_OS_SOURCES_A=1 JET_OS_SOURCES_C=3
parent after spawn: [JET_OS_SOURCES_A, JET_OS_SOURCES_B]
```

What this shows on `jet run`:
- argv keeps order.
- `vars()` returns a value snapshot: the list taken before `unset` still holds
  A, and a fresh `vars()` does not.
- `unset` returns true and then false.
- The child's argv keeps order (`z|a|m|`).
- `spec.env(C)` adds C, `spec.env_remove(B)` removes the inherited B, and the
  child sees the parent's logical `sys.set` value A.
- The parent's environment is unchanged by the spec edits.

Other tiers:
- `jet run --interpret`: E0956 ``core.sys.vars()`` isn't supported by the
  current evaluator yet. The span points at os_sources.jet:20:4 (`sys.set`),
  not at the `vars()` call.
- AOT (`jet build os_sources.jet`): rustc E0308 at generated
  `os_sources.rs:153349`: `Some(jet_std_env_set(..))`, expected `()`, found
  `Result<(), EnvError>`. Any AOT program that calls `core.sys.set` fails to
  build. The existing golden `io/env_overlay` shows the same: run MATCH,
  interpret DIFF (rc 1), AOT BUILD FAILED.

Verdict c2: observed exact on `jet run` only. It is not proven on the
interpreter or AOT, so the I9 tiers disagree.

## c1 — non-Unicode OS data

Unix, byte `0xFF` injected with bash `$'…'`:

| cell | `jet run` | `--interpret` | AOT binary |
| --- | --- | --- | --- |
| env `JET_OS_SOURCES_BAD=a\xffb`, `sys.get` | `a�b` (U+FFFD) | `a�b` | `a�b` |
| same env, `sys.vars()` | Ok (no error) | E0956 unsupported | Ok (no error) |
| argv `x\xffy`, `process.args()` bytes | `[120, 239, 191, 189, 121]` | same | same |

Every tier silently replaces the byte with U+FFFD (`EF BF BD`). This
contradicts:
- core-library.md:1440 and 1485-1486: "`vars` does not silently skip an
  unrepresentable entry: `EnvError.NonUnicode` reports it";
- the AOT prelude source, which returns `None` from `get`
  (`into_string().ok()`, FSIoEnvOsTesting.rs:569-571), `Err(NonUnicode)` from
  `vars` (:707-713), and panics in `std::env::args()` for argv (:122-124).

The shipped binary behaves like neither. Verdict c1: FAIL. The conversion is
lossy on every tier, with no typed outcome.

## c3 — owned vs borrowed forms

Every surface returns owned `String`/`[String]` copies. There are no borrowed
or OS-string forms and no readback of a spec's args or envs. Verdict:
unsupported beyond owned String snapshots; new forms need a gate.

## Extra divergence seen

`nonutf2.jet` with the variable unset prints `get BAD: <none>` on `jet run`
and AOT, but `get BAD: null` on `--interpret` (`print("get BAD: {bad ?? "<none>"}")`).

## c4

Not met. `Examples/features/io/os_sources.jet` was not added to the repo:
its AOT tier cannot build, and the interpreter lacks `vars`.
`tests/os_sources.rs` does not exist. A cargo test encoding the current
lossy behaviour would bless a defect.

## Follow-ups (defects)

1. Non-UTF-8 argv/env is lossy (U+FFFD) on every tier, against the documented `EnvError.NonUnicode` law.
2. AOT: `core.sys.set` codegen type mismatch (Result vs unit), so no AOT program using `sys.set` builds.
3. Interpreter: `core.sys.vars()` is unsupported (E0956), and the span is misplaced.
4. Interpreter: `{opt ?? "<none>"}` on a `None` from `sys.get` prints `null`.
5. Owner-gated gaps: OS-string argv/env, key/value `vars`, spec args/envs readback.
