# #3114: ratified two-adapter embedding delivery (SCRIPT-F24)

Closer10, 2026-09-29. Binary: `~/.cache/jet-dev/safe-jet.sh` →
`jet-debug-snapshot14`. The host-language hosts were compiled with cc/c++
inside `Tools/agent/jet-env`. Verdict: **PARTIAL**: criterion 1 (route inventory) met; criteria 2 and 3 unmet because of the defects below.

## Question

Under D-EMBED1=E and D-EMBED2=C:

- Do the native libjet adapter and the Component adapter deliver the same
  typed exports, values, errors and capability denials?
- Is native trust versus sandbox enforcement stated?
- Do source identity, no-execution preparation, first/repeated calls and
  release have receipts?

## Delivered routes (criterion 1), traced from the CLI and examples

| Step | Native Library (D-EMBED2=C, trusted in-process) | Component (sandbox) |
|---|---|---|
| Build | `jet build --lib library.jet`, driven by the package.jet output `.Library{native: true, loadable: true, bindings: [c, python, swift]}` | `jet build --target=sandbox library.jet` → `.jet/build/library.wasm` (per `tests/library_outputs.rs:646-657`) |
| Artifacts | `.jet/build/{libloadable.a, libloadable.so, loadable.h, loadable.jetlib, bindings/loadable.{h,py,swift}}` plus the completion marker `.loadable.jet-library.complete` (`jet-library-set-v1`, sha256 per file) | `library.wasm` plus a frozen interface snapshot `.jet/cache/api/plugin__<name>.api` (E1257 without it) |
| Source prepare / state create | None. No `jet_init`/`jet_shutdown` symbols; `foreign.cpp:82-85` asserts they are absent. State is per call. | The host is checked against the frozen `.api` snapshot before execution. `plugin.load(literal, Authority)` preflights imports against the narrowed Authority. |
| Load from a foreign host | C: link `libloadable.a` + `loadable.h`. C++: `dlopen(libloadable.so)` + `dlsym`, repeated for 3 cycles. | n/a (the Jet host only) |
| Load from a Jet host | `core.mod.load(".jet/build/loadable.jetlib", grant: {read: […]})` (`host.jet`). Identity, effect and target checks are E1338/E1339/E1341 (`tests/library_outputs.rs:463-535`). | `core.plugin.load("…wasm", Authority.from_rights([…]))` (`component_host.jet`, `sandbox_mathkit/run.jet`) |
| Release | `dlclose` per cycle; `jet_text_free` for returned text | not observable from source; no explicit release verb |

## Evidence (observed)

### Native, built from the current tree

The build ran in a copy of `library_loadable` with the test's authority line
added (`~/.cache/jet-dev/scratch/Closer10/embed`):

```
$ safe-jet.sh build --lib library.jet          # real 1m3.9s
built: …/.jet/build/loadable.jetlib, bindings/loadable.{h,py,swift}
.loadable.jet-library.complete:
  libloadable.so  sha256-97b96def…0364   libloadable.a  sha256-3651b8df…9155
  loadable.h      sha256-d600e888…5b1b   loadable.jetlib sha256-6c5a7e50…4a1f
  (each matches `sha256sum` of the file)
$ cc -std=c11 -I .jet/build foreign.c .jet/build/libloadable.a -o foreign -ldl -lpthread -lm && ./foreign
42
$ c++ -std=c++17 -I .jet/build foreign.cpp -o foreign-cpp -pthread -ldl && ./foreign-cpp $PWD/.jet/build/libloadable.so
cpp-ok      # 3 dlopen/dlclose cycles, 4 threads x 8 calls each, no init/shutdown symbol, host SIGUSR1 handler preserved
```

Both match the goldens `library_loadable.out` (`42`) and
`library_loadable_cpp.out` (`cpp-ok`). Source identity:

| File | sha256 |
|---|---|
| `library.jet` | `1bc82639…0cc9` |
| `host.jet` | `8ec07ef2…d716` |
| `component_host.jet` | `32b1c37b…653b` |

### Native, loaded by a Jet host: fails on every tier

```
$ safe-jet.sh run host.jet            (and run --interpret, run --release)
internal compiler error: checked TIR cannot lower to MIR at 40..46: run: missing checked MIR owner type MirTypeId(12534809514496156272) (source bytes 40..46)
  at Source/lib.rs:263:33                        (default and --interpret)
  at crates/jet-driver/src/Driver/mod.rs:6546:33 (--release)
```

Bytes 40..46 are `loaded` in `loaded :: library.load(".jet/build/loadable.jetlib", grant: {read: [".jet/build"]})`.
The same ICE happens with `target/debug/jet` (2026-09-29 01:52).

### Component build: rustc rejects the generated guest

```
$ safe-jet.sh build --target=sandbox library.jet
internal compiler error: rustc rejected generated code … rustc rejected the generated plugin guest module
error[E0308]: mismatched types … __jet__d_c_clibrary_djet_c_con_utick(p0) … expected `&JetInt`, found `i64`
error[E0308]: mismatched types … expected `i64`, found `JetInt`
```

Separately, `component_host.jet` loads `"build/library.wasm"`, while the
build and `tests/library_outputs.rs:652` use `.jet/build/library.wasm`. Run
without a snapshot, it stops at E1257 ("plugin.load needs a registered
Component interface").

### Component golden `sandbox_mathkit`: stale interface snapshot

Run from a golden-harness-shaped staging copy (`fixture-state/` → `.jet/`,
`~/.cache/jet-dev/scratch/Closer10/mk`), on all three tiers:

```
Error [E1257]: Plugin interface `plugin__mathkit` has no export `is_enabled`   (run.jet:20)
Error [E1257]: Plugin interface `plugin__mathkit` has no export `greet`        (run.jet:22)
```

- `fixture-state/cache/api/plugin__mathkit.api` lists only `gcd`, `hypot`
  and `scale`.
- `mathkit.wasm` (sha256 `dc7fdc88…9256`) does contain `greet` and
  `is-enabled`.
- `run.jet` (sha256 `2ea3f772…07c`) calls all five; it was changed in
  `c0c5493a9` (2026-09-29 00:47).
- Run from the repository root with no staged state, all five calls are
  E1257.

What happens with the snapshot corrected in scratch (the two missing lines
added: `fn greet(name: String) String`, `fn is_enabled(flag: Bool) Bool`):

- The check passes on every tier.
- `--interpret`, with an inline package authority block because
  `--interpret` rejects `--allow` (E2102), prints:
  - `scale(6, 7) = 42.0`
  - `hypot(3, 4) = 5.0`
  - `gcd(48, 18) = 6`
  - then `mathkit.is_enabled(true)` fails; with the error printed (a scratch
    edit replacing `panic`), it reads
    `is_enabled error: user-error:plugin call is_enabled: no exported function has this name`;
  - `greet(Ada) = hello, Ada!`.
- So the Float, Int and Text exports work through the Component host. The
  Bool export fails at runtime because the snake_case name `is_enabled` is
  not mapped to the guest's kebab-case `is-enabled` export (the wasm export
  strings contain `is-enabled`). `greet` has no underscore, so it is
  unaffected.
- Default and `--release` fail before running with E0704. The plugin host
  build compiles `crates/jet-foundation` from the live checkout, and its
  build script panics:
  `generated table …/CoreModuleExports.rs is stale for …/Prelude/Core.jet; run node Tools/agent/gen-core-tables.mjs --write`.
  That is live-tree state, not a verdict on the example, but it shows the
  host build depends on the checkout.

## Parity table (criterion 2)

| Surface | Native Library | Component |
|---|---|---|
| Exports | `on_tick(Int)->Int`, `is_enabled(Bool)->Bool`, `greet(String)->String` + `jet_text_free` (header lines checked by the test) | same three names; the guest build fails (above) |
| Values | C: `42`; C++: `on_tick(on_tick(41))==43`, `greet("Ada")=="hello, Ada!"` | not observable: build fails; the mathkit host fails check |
| Errors | panic crosses as `Stop [E3001]` with a non-zero exit (per the test; not re-run) | not observable |
| Capability denials | Jet host: E1339 for over-granted effects (per the test; not re-run, the host ICEs) | Authority preflight before linking (per source comments; not observed) |
| Trust statement | Native is trusted in-process (`component_host.jet:1-4` comment) | sandboxed, zero host imports for mathkit (`sandbox_src/run.jet:1-7` comment) |

## Criteria

1. **Met as an inventory.** The routes above were traced from the
   CLI/package surface and the examples, not from `PluginArtifacts`.
2. **Fail.**
   - Only the foreign C/C++ native path runs.
   - The Jet-host native load ICEs on every tier.
   - The Component build is rejected by rustc.
   - The committed Component golden fails E1257 on every tier.
3. **Partial.**
   - Source identity: recorded (sha256s above).
   - The completion marker binds artifact digests.
   - No-execution preparation for native: there are no init symbols
     (asserted by `foreign.cpp`), and building executes no user code.
   - First/repeated calls and release: observed only for the C++ host
     (3 dlopen/dlclose cycles plus a threaded fan-out).
   - Receipts for the Component path and the Jet-host path are impossible
     until the defects are fixed.
   - No new public contract is needed.
