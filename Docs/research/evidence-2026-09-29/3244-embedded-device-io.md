# #3244: bounded embedded and device IO contracts (CORE-F017)

Closer11, 2026-09-29.

- Test binary: the prebuilt
  `target-integ/debug/deps/target_machines-26bd6929cfe143b9`, built 2026-09-29
  01:56 from HEAD a8d8417c2, with the working-tree edits present at that time.
- CLI binary: `jet-debug-snapshot14`, run through `~/.cache/jet-dev/safe-jet.sh`.

## Question

Classify the selected fixed-buffer, serial/device, volatile and
target-availability jobs against the existing embedded owners, using executed
evidence:

- MMIO volatility versus synchronization;
- bounded buffers and cancellation;
- target rejection and unused-dependency exclusion;
- no host-only fallback presented as embedded support.

## Method

Command 1, the exact criterion-5 filter set (plus the plan's two witnesses):

```
systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax=6G \
  env CARGO_MANIFEST_DIR=$PWD JET_NIX_TMP_CLEANED=1 Tools/agent/jet-env \
  target-integ/debug/deps/target_machines-26bd6929cfe143b9 --test-threads=2 \
  selected_target_machine no_os_machine_rejects_dev_and_jit_explicitly \
  mcu_firmware_build_writes_elf_map_audit_and_size_budget \
  target_provider_fact_matrix_covers_present_missing_and_malformed \
  allocator_firmware_target_witness heap_free_core_target_witness
```

Result: `test result: FAILED. 3 passed; 5 failed`. The log is
`~/.cache/jet-dev/scratch/Closer11/target_machines2.log`.

Command 2 repeats the MCU test with the nix cc-wrapper hardening disabled:
`... Tools/agent/jet-env env NIX_HARDENING_ENABLE= target-integ/.../target_machines-... mcu_firmware_build_writes_elf_map_audit_and_size_budget`.
Result: `1 passed`. This run includes the QEMU MPS2 serial check
(`program-linked\n`).

The CLI probes are in `~/.cache/jet-dev/scratch/Closer11/emb/`.

## Cell table

| Job | Owner | Executed evidence | Verdict |
|---|---|---|---|
| Direct MMIO volatile write/read inside the declared MMIO region | `core.mem.volatile_read/write` (#Unsafe) + TargetMachine MMIO policy | `selected_target_machine_validates_direct_mmio_accesses` **FAILED**: `E3303 This no-OS program allocates memory but has no allocator fact`, with its span at `Core/mem/mem.jet` bytes 281..291 (`AllocError`). The sensor machine *does* declare `AllocatorPolicy::Fixed{ram, 8 KiB}`. | FAIL (defect D1) |
| Volatile write outside the MMIO region is rejected | TargetMachine MMIO policy, E3313 | `selected_target_machine_rejects_direct_mmio_outside_region` **FAILED**: E3313 is absent; the same E3303 from `core.mem` fires first. | FAIL (D1 masks it) |
| Volatile is ordering-only, distinct from synchronization | `core.mem` (volatile) vs `Atomic<T>` | Host only; **not embedded evidence**. `Examples/features/lowlevel/mmio_board_write.jet` fails on every tier: `jet run` → `R0802 use of storage after its lifetime ended ... valid_ptr = false`; `--interpret` → `E0956 MIR place is unavailable`; `jet build` → rustc rejects the generated Rust (E0392 `Pin<__jet_T>` unused parameter, E0282, 2× E0308 `JetInt` vs `i64` in `jet_sentry_volatile_write` / `jet_sentry_address_of`), a hidden-backend (I2) failure. `Examples/features/concurrency/bounded_spsc_atomic.jet`: `jet run` and `--interpret` agree, but print `ring=7:4611686018427387905` where the golden says `...907` (also the `seen` / `final` values); `jet build` → ICE `checked C record field head has no native Rust representation` (`crates/jet-pkg-model/src/FFI.rs:447`). Core itself has no atomics API in `core.mem`, although its package description says "atomics". No test or example shows a volatile access being *used* as synchronization and rejected. | FAIL; volatile/atomic separation is not proved |
| Fixed buffers (Fixed allocator) on a no-OS target | `core.mem.Fixed` + `AllocatorPolicy::Fixed` | `allocator_firmware_target_witness` **FAILED** with the same E3303 at `core.mem` `AllocError`. | FAIL (D1) |
| Byte-sink serial I/O through the provider | `ByteSinkPolicy` / `__jet_target_write` | `mcu_firmware_build_writes_elf_map_audit_and_size_budget` passes only with `NIX_HARDENING_ENABLE=` set. It produces ELF, map, linker script and audit, stays within the size budget, and QEMU MPS2 serial prints `program-linked`. Under plain `Tools/agent/jet-env` it **FAILS**: `clang: error: unsupported option '-fzero-call-used-regs=used-gpr' for target 'thumbv7em-unknown-none-eabihf'` (D3). The program object is a C fixture, not Jet source. | PARTIAL: the provider path is proved; no Jet-source serial/UART API exists |
| Provider facts present/missing/malformed (Scheduler, IO.Read, IO.Write, Allocator, Startup, clocks, entropy, panic) | TargetMachine provider contracts | `target_provider_fact_matrix_covers_present_missing_and_malformed` **ok** | PASS |
| Cancellation bounded under the cooperative scheduler | Scheduler provider | There is no witness. The provider matrix only proves that the Scheduler fact is present, missing or malformed. `grep cancel Core/` finds only hosted cancellation (`core.tasks.cancel`, `core.watcher.cancel`, data/web loaders), with no no-OS path. | GAP → ballot B2 |
| Target rejection: `core.files` on board.sensor_v1 | Sema `CoreApiUnavailable` → E3310 | `selected_target_machine_rejects_unavailable_core_api_before_codegen` **ok** through the driver API. From the CLI: `jet check --target=board.sensor_v1 files_on_board.jet` exits **0** (`ok ... has no errors (115 warnings)`), and `jet check --help` lists no `--target` flag, so the CLI check does not apply the target. `jet build --target=board.sensor_v1` stops earlier with `E3302 Target thumbv7em-none-eabihf is not available` (no Jet toolchain for that triple here). | PASS via the driver; the CLI does not reach it (D4) |
| Dev/JIT rejected on no-OS | `supports_execution_tier` | `no_os_machine_rejects_dev_and_jit_explicitly` **ok** | PASS |
| Heap-free Core program on `wasm_no_os` | Core layer | `heap_free_core_target_witness` **FAILED**: generated Rust lacks `// jet:target-dossier layer=core provider=target-providers-v1:` (tests/target_machines.rs:1027). | FAIL (D2) |
| Unused dependency exclusion | import closure + target audit | Contradicted. An *unused* `use core.mem as mem` (the MMIO tests import it; the allocator witness uses only `mem.Fixed`) is enough to raise E3303 from `core.mem`'s own `AllocError` declaration. `jet check --target=board.sensor_v1 emb/unused_mem.jet` (unused import only; the target flag is ignored, see D4) reports `L0505 heap_growth_in_loop` at `Core/mem/mem.jet:15:12` (`pub struct AllocError`), so the imported module body is analysed as user code. No test asserts that unused Core modules are absent from the ELF/map. | FAIL (D1) + GAP |
| No host-only fallback presented as embedded | — | This table labels every host run as host. The MCU witness uses a C program object, not Jet-compiled firmware. | Upheld by this report |

## Defects

- **D1: E3303 from `core.mem` on a no-OS machine that has an allocator fact.**
  - Repro: tests `selected_target_machine_validates_direct_mmio_accesses`,
    `selected_target_machine_rejects_direct_mmio_outside_region` and
    `allocator_firmware_target_witness`.
  - The source is just `use core.mem as mem` plus volatile or `Fixed` calls;
    `sensor_machine()` declares `AllocatorPolicy::Fixed`.
  - Observed: `E3303` at `<corelib>/Core/mem/mem.jet` 281..291 (`#Error pub struct AllocError { ... allocator: String }`).
  - Expected: compiles, and for the out-of-region case reports only E3313.
  - The sema no-OS allocation checks (`crates/jet-sema/src/Sema/CheckerInfer/expr.rs:5994,6297`
    push `e3303` on `self.no_os` alone) do not consult the machine's allocator
    fact, and they also run over imported Core declarations. [INFERENCE from
    source reading; the exact emitting site for the struct span was not traced.]
- **D2: missing target dossier.**
  - Repro: `heap_free_core_target_witness` (`fn run() { value :: 6 * 7 ... }`
    on `TargetMachine::wasm_no_os()`).
  - Observed: `output.rust` lacks
    `// jet:target-dossier layer=core provider=target-providers-v1:`.
  - Expected: the marker is present.
- **D3: nix hardening breaks the thumbv7em fixture.**
  - Repro: `mcu_firmware_build_writes_elf_map_audit_and_size_budget` under
    `Tools/agent/jet-env`.
  - Observed: the nix cc-wrapper injects `-fzero-call-used-regs=used-gpr`, which
    clang rejects for thumbv7em.
  - Expected: `jet-env` or the test disables hardening for cross targets. With
    `NIX_HARDENING_ENABLE=` the test passes.
- **D4: `jet check --target` is ignored.**
  - Repro: `jet check --target=board.sensor_v1 ~/.cache/jet-dev/scratch/Closer11/emb/files_on_board.jet`
    (`use core.files as fs` + `fs.read`).
  - Observed: exit 0, 115 Core-internal lint warnings, and no E3310.
    `jet check --help` has no `--target`.
  - Expected: the target is applied and E3310 is reported, or the unknown flag
    is rejected.
- **D5: `mmio_board_write.jet` fails on every tier** (R0802 / E0956 / rustc
  errors), contradicting its golden `mmio wrote 42`.
- **D6: `bounded_spsc_atomic.jet` mismatch and AOT ICE.**
  - `jet run` and `--interpret` print `ring=7:4611686018427387905` and
    `seen=...905 ... final=...905`; the golden has `...907`.
  - AOT ICE: `checked C record field head has no native Rust representation`.

## Ballots for uncovered jobs (compact drafts for Pip)

- **B1: D-EMBED-SERIAL1, a typed serial/UART device API.**
  - Options:
    - A: a Core `core.device.serial` byte-sink/byte-source over the existing
      `ByteSinkPolicy` provider, with a `write(bytes) -> Int IOError!` bounded
      by a caller-supplied `[U8#N]` buffer.
    - B: no Core module; board packages expose providers only.
    - C: reuse `core.io` streams on no-OS targets through the provider.
  - Recommendation: A. It stays in the Core layer, has no heap, and rejects
    hosted-only calls through E3310.
  - Beginner path: `serial.write(port, "hi")`. Expert path: the raw
    provider-level `ByteSinkPolicy`.
- **B2: D-EMBED-CANCEL1, bounded cancellation on no-OS.**
  - Options:
    - A: the cooperative scheduler provider exposes a cancel token checked at
      yield points, with a bounded wake budget.
    - B: cancellation stays hosted-only, and no-OS programs poll flags by hand.
  - Recommendation: A, only after a no-OS task witness exists. Until then, no
    cancellation support is claimed.
- **B3: D-EMBED-UNUSEDDEP1, unused-dependency exclusion witness.**
  - Not an owner choice: the missing piece is a test. It should assert that an
    unused `use core.<x>` adds no symbols to the firmware map and raises no
    target diagnostic. D1 is the current counter-example, so this goes on a
    card, not a ballot.

## Verdict

**FAIL.**

- Criterion 5's named run does not pass: 3 of 8 tests pass as run, and 4 of 8
  with hardening disabled.
- MMIO validation, fixed-buffer allocation and unused-dependency exclusion are
  all broken by D1.
- Cancellation and serial/UART are uncovered; ballot drafts B1 and B2 are above.
