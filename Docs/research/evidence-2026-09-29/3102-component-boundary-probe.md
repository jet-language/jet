# #3102 — Component plugin trust and resource boundary (runtime probe)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`). Guest: the committed `Examples/features/packages/sandbox_mathkit/mathkit.wasm`, copied to `~/.cache/jet-test-scratch/Closer07/plugin/`.

The API snapshot needed a patch. The committed `fixture-state/cache/api/plugin__mathkit.api` lists only `gcd`, `hypot` and `scale`, although the golden `run.jet` calls `greet` and `is_enabled`: E1257 "Plugin interface `plugin__mathkit` has no export `greet`" when staged unchanged. The scratch `.jet/cache/api/plugin__mathkit.api` adds `fn greet(name: String) String` and `fn is_enabled(flag: Bool) Bool`. The package allows FS, IO, Exec, Log, Mem.Alloc, Time (without Exec: E1220 "`run::run` uses the `Exec` effect").

## Probes
`run.jet` loads mathkit under `Authority.from_rights(["FS.Read:repo"])`. It then calls `greet` with names of 2^10, 2^16, 2^20, 2^22, 2^23 and 2^24 bytes. Each call handles failure with `?? { print(err) … }`, and each is followed by `gcd(48, 18)` on the same handle. Finally the program does ordinary host work (sum 0..1000).

## Evidence
### `safe-jet.sh run --interpret run.jet`
Exit 0:
```
gcd before = 6
greet 2^10 result len=1032
gcd after 2^10 on same handle = 6
greet 2^16 result len=65544
gcd after 2^16 on same handle = 6
greet 2^20 failed: budget-exhausted:plugin call `greet`: fuel budget exhausted
gcd after 2^20 failed: user-error:plugin call `gcd`: guest trapped: wasm trap: cannot enter component instance
greet 2^22 failed: user-error:plugin call `greet`: guest trapped: wasm trap: cannot enter component instance
greet 2^23 failed: user-error:… cannot enter component instance
greet 2^24 failed: budget-exhausted:plugin call `greet`: argument wire exceeds the 16 MiB resource budget
gcd after 2^24 failed: user-error:… cannot enter component instance
host work continues: 500500
```

### Other tiers
- **Default `jet run` (JIT):** even the minimal `plugin.load` + `gcd` program (`minimal.jet`) fails: `internal compiler error: Cranelift cannot execute MIR function `.::minimal.jet::run`: MIR call ABI expects 3 arguments, got 2`.
- **AOT (`safe-jet.sh build minimal.jet`):** `E0704 Couldn't fetch or build jet-foundation@…/crates/jet-pkg-model/../jet-foundation` — `error[E0053]: method `table_growing` has an incompatible type for trait`. The AOT plugin runtime is compiled from the live tree, and `crates/jet-pkg-model/src/Prelude/Plugin.rs:151-154` (`fn table_growing(&mut self, _current: u32, desired: u32, …)`) no longer matches the wasmtime `ResourceLimiter` trait.

## Criteria
- **Crit 1 — denied import before operation:** NOT EXERCISED. mathkit declares no host imports. Jet cannot author a guest with an effectful import (`--target=sandbox` rejects effects, E1258), and no `wasm-tools` is installed. The source preflight exists (`Plugin.rs:893-899`, `:936-955`), but source inspection is not execution.
- **Crit 2 — limits report real failure:**
  - MET on the interpreter for fuel (`budget-exhausted … fuel budget exhausted`) and for the argument wire (`argument wire exceeds the 16 MiB resource budget`). No fabricated success occurred.
  - The result-wire limit cannot be reached with this guest, because the 16 MiB memory cap equals the 16 MiB wire cap.
  - Call time (epoch deadline) was not triggered.
- **Crit 3 — failed guest vs host work:**
  - Host work continues (`500500`, exit 0).
  - After one fuel trap, however, the **same handle is permanently unusable**: every later call fails with `cannot enter component instance`, classified as `user-error: guest trapped`. The failure is typed, but the kind is wrong (a poisoned instance, not a user error).
  - The plan's `plugin_failed_call_leaves_host_and_handle_usable` would fail today.
- **Crit 4 — blocking import bounded:** NOT EXERCISED at runtime. Source shows an unbounded `to_socket_addrs` before `connect_timeout` (`Plugin.rs:743-751`).
- **Crit 5 — identity, no native generalization:** this doc records binary, guest and source identity. Nothing here is claimed about native Library panic containment.
- **Crit 6 — named cargo tests:** none of them exist in `tests/authority.rs` or `crates/jet-pkg-model` (grep).

## Defects
1. JIT: any `core.plugin` call ICEs ("MIR call ABI expects 3 arguments, got 2"). Default `jet run` cannot host a Component.
2. AOT: the plugin runtime build fails E0053 on `ResourceLimiter::table_growing` (`Plugin.rs:151`).
3. After a budget trap, the component instance is poisoned for its whole lifetime, and later calls report `user-error: guest trapped … cannot enter component instance`. Expected: either re-instantiate/recover, or a typed "instance unusable after trap" failure.
4. The committed mathkit API snapshot omits `greet`/`is_enabled`, which the golden `sandbox_mathkit/run.jet` calls.
