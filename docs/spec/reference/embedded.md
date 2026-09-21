# Embedded and Typed Target Profiles (E2-M15)

The current implementation exposes cross-compilation and typed no-OS and
hosted WASM paths using rustc's target matrix. This document records the
source-backed target ownership, local QEMU harness, and known proof boundary
for the preparation slice (D-CROSS3 option A).

## Cross-compilation and target ownership

`--target` is the existing machine axis. It is not a standalone
`TargetProfile` API: `Source/main.rs` parses the request, the driver resolves
named `TargetMachine` values, and the compile command owns target admission and
artifact publication. A request is either a Jet backend alias, a declared
machine name, or a raw rustc target triple.

The current source-backed matrix is intentionally bounded:

| Request | Effective target and owner | Availability gate | ABI, artifact, and provenance |
|---------|----------------------------|-------------------|-------------------------------|
| no `--target` | Hosted default. The driver creates the hosted machine for the host triple; no named machine is selected. | The ordinary host rustc path. No cross-target probe runs. | Hosted providers and linker defaults. The normal native artifact path is used; the dossier records the hosted/default boundary. |
| raw triple, for example `aarch64-unknown-linux-gnu` | `Source/main.rs` keeps the exact triple. The driver creates a hosted `TargetMachine` for that triple; it does not invent a named board profile. | `validate_target` calls `Doctor::probe_target_component`: target-list membership, `rustc --print sysroot`, and non-empty `$sysroot/lib/rustlib/<triple>/lib` are all required. Failure is E3302 before rustc. | rustc owns the target ABI. Jet does not pass a user sysroot or select a Jet-owned toolchain descriptor here. The target triple, hosted dossier, compiler identity, provider identity, linker identity, Prelude closure, profile, and layout feed artifact identity; cross-target builds bypass the host native binary cache. |
| `web` | Jet web backend alias. Doctor maps the rustc component check to `wasm32-unknown-unknown`; the driver uses the browser target machine for the dossier. | The mapped wasm target component must be present. No host-native execution path is used. | Browser providers are typed target facts. Web codegen publishes the manifest, DOM/HTML, JavaScript, Wasm, and applicable map artifacts under the build output authority; the manifest carries the selected Wasm/runtime target facts. |
| `sandbox` | Jet sandbox backend alias. Its rustc guest target is `wasm32-unknown-unknown`. | The mapped wasm component must be present; Component Model assembly additionally requires `wasm-tools`. Missing target support remains E3302, while assembly failure remains the plugin diagnostic. | The plugin owns the Component Model ABI and publishes the WIT, guest Rust, core Wasm, and component Wasm artifacts. This is not a native host artifact or a source fallback. |
| `wasm.browser` | Named `TargetMachine::wasm_browser()`; triple `wasm32-unknown-unknown`, hosted browser environment. | The machine triple still passes the target-component probe. | Explicit browser providers and their digestable target dossier own the runtime boundary. Direct driver fixtures emit browser artifacts; CLI materialization is listed below as a proof gap until exercised end to end. |
| `wasm.wasi` | Named `TargetMachine::wasm_wasi()`; triple `wasm32-wasip2`, hosted WASI environment. | The WASI target component must be present. | Explicit WASI providers and the target dossier own the runtime boundary. The direct target-machine fixture proves a hosted WASI compile; a CLI artifact and runtime proof still need the production-path check below. |
| `wasm.no-os` | Named `TargetMachine::wasm_no_os()`; triple `wasm32-unknown-unknown`, Core/no-OS environment. | The wasm component must be present and typed no-OS facts must validate. | The target is AOT-only and heap-free. The no-OS Wasm output is a reactor (`cdylib`, `--no-entry`, explicit `__jet_program_entry` export); its provider and dossier identities are target inputs, not host defaults. |
| `board.sensor_v1` | Named no-OS machine; triple `thumbv7em-none-eabihf`. The machine supplies memory, generated linker/startup, providers, and board facts. | The rustc target component is checked before codegen; typed machine validation and target C/LLD tools must then succeed. | Startup uses the explicit C/1 provider ABI. Firmware work is keyed by `jet-firmware-build-v1` plus source, profile, native toolchain, and target dossier. The machine emits `memory.ld`, startup/provider sources and objects, `firmware.elf`, `firmware.map`, and `<machine>.target.json`; the CLI copies the ELF to its requested build output. |
| `board.virt_aarch64` | Named no-OS machine; triple `aarch64-unknown-none`. The machine supplies AArch64 startup, memory, linker, UART, and QEMU facts. | The rustc target component is checked before codegen; typed machine validation and target C/LLD tools must then succeed. | Startup uses the explicit AArch64/1 provider ABI. Firmware artifacts and the target dossier follow the same content-bound path as `board.sensor_v1`; QEMU proof selects `virt`/`cortex-a57` from the machine facts. |

`TargetProfileIdentity` in `jet-pkg-model` remains the manifest
`targets:` data model. Its closed built-ins are currently
`web.browser`, `web.wasi_server`, and `web.no_os`, with validated named
identities. Those manifest identities must not be silently treated as the
CLI machine names `wasm.browser`, `wasm.wasi`, and `wasm.no-os`: the current
CLI trace resolves the latter through `Driver::target_machine_by_name`.
No source-backed path currently makes a manifest profile a replacement for
that command-owned dispatch.

Unavailable targets remain explicit. `validate_target` runs before the
cross-target compile, and an unknown `board.*` name is rejected before
dispatch. A missing target-list entry, sysroot, or target library produces
E3302; the driver does not relabel a host artifact as the requested target or
silently fall back to the host. The checked-in `tests/cli/unknown_target_e3302.txt`
fixture covers the user-facing unavailable-target wording.

### Current production-path proof gaps

This matrix records source-backed ownership, not completion of the full #758
slice:

- Raw triples only prove local rustc readiness. There is no production
  Jet-owned toolchain/sysroot descriptor, target-specific sysroot binding, or
  clean-machine acquisition/air-gap proof; `rustc`, `clang`, and `ld.lld`
  remain process-resolved tools.
- The raw-triple path has no dedicated CLI artifact dossier containing
  compiler, sysroot, linker, ABI, debug-data, and output digests. Target
  dossier/cache identities exist in the compiler facts, but their end-to-end
  publication and inspection are not proven here.
- `tests/target_machines.rs` exercises target-machine compilation, firmware
  files, dossiers, and QEMU directly through driver functions. It does not
  prove `jet build --target=<named-machine>` through CLI parsing, publication,
  and final artifact inspection for every named machine.
- No focused production fixture currently proves raw foreign-target ABI
  compatibility, linker/sysroot selection, debug-data parity, or emulator
  runtime parity. `jet run` intentionally does not execute a cross-compiled
  artifact on the host; it emits the emulation note instead.
- C headers, libraries, linker inputs, and runtime objects remain the
  separately owned follow-on slice (#1058); this matrix does not claim those
  production proofs.

The focused source-backed checks currently available are the unavailable
target CLI fixture, `tests/cross.rs` for target-aware front-end behavior, and
`tests/target_machines.rs` for direct typed-machine/artifact/QEMU behavior.



## Native Library embedding

`Library` is the native guest output. Its export surface is the marked top-level
`#Export(c)` function surface. Each exported function must use one homogeneous
`Int`, `Float`, `Bool`, or `Text` scalar for every parameter and its return
value. The matching import form is `#Import(c)`, which declares the foreign
symbol after `=`.

```jet
#Export(c) pub fn on_tick(dt: Int) Int -> dt + 1
#Export(c) pub fn greet(name: String) String -> "hello, {name}!"
```

Build the checked native projection with:

```sh
jet build --lib examples/features/packages/library_loadable/library.jet
```

The build emits a static library, a shared library, a C header, and only the
requested named C, Python, and Swift projections. A loadable output also emits
`.jetlib`. Its versioned header pins the compiler identity, native target, ABI
version, Library name, exact `#Export(c)` names and C symbols,
scalar shapes, parameter counts, declared effects, and payload length before
the payload is mapped. A native `Library` cannot select `--target=<triple>`;
`E1341` is reported before native artifacts are published and also rejects a
loadable artifact whose header does not match the current native loader.

### Host lifecycle contract

Native Library linkage has zero Jet process-lifecycle hooks. There are no
`jet_init` or `jet_shutdown` calls. The host loads the shared library, calls
exports while it remains loaded, and unloads it only after all calls, host
threads, and returned values finish.

Calls are synchronous and may be nested. The host may enter from ordinary
threads; Jet keeps any thread-local runtime state on the calling thread. Do
not call an export from an asynchronous signal handler. The host owns signal
handlers and process shutdown.

`Text` results are library-owned `JetText` values. Release each result with
`jet_text_free` from the same loaded library before unloading it. Never free a
Jet-owned buffer with the host allocator or retain it after `dlclose`.

The Jet `Mod` value owns both the native handle and the staged payload. Dropping
it closes the handle first, then removes the staged file. `jet run` and
interpreter invocations clear their loaded-module table at teardown, so failed
calls do not retain mapped libraries or staged payloads.

Jet panic cannot cross the C ABI as a C++ exception. The export boundary catches
the panic, writes the normal `Stop [E3001]` runtime report to stderr, and
terminates the calling process with status 70. A failed Jet error edge is
reported before termination with status 1. Hosts must treat either outcome as
process failure, not recover it with C++ exception handling. The checked-in
`foreign.cpp` host exercises thread entry, nested calls, allocator ownership,
thread-local entry, signal preservation, and repeated load/unload cycles;
`tests/library_outputs.rs` also invokes a panic-only Library through that host
in a child process.

## Typed target profiles

The package manifest and CLI target axes are intentionally separate. Embedded
and hosted WASM builds use the named `TargetMachine` entries in the matrix
above; a selected machine supplies one typed target dossier to sema, codegen,
and artifact identity. There is no standalone no-OS switch and no alternate
Prelude.


## Typed target machine facts

Card #239 / D-TARGET-* makes embedded builds use typed board machines. Hosted
Jet keeps hidden defaults. A selected target machine carries these facts before
codegen and into build artifacts:

- target triple plus runtime layer (`Core`, `Alloc`, or `Hosted`)
- named memory regions: origin, size in bytes/KiB/MiB, kind (`flash`, `ram`,
  `mmio`, `reserved`), access (`r`, `rw`, `rx`, `rwx`)
- linker provenance: generated from machine facts, or a file path with a
  `sha256:` hash
- allocator policy: none, fixed region/size, supplied provider, or hosted default
- panic policy: abort, report sink, or hosted default
- execution honesty: no-OS machines are AOT-only (`dev` / `jit` rejected)
- provider identities, Prelude closure, and stable artifact/cache identity

Validation is data-first. It reports missing flash/RAM, overlapping or
overflowing memory, RAM budget overflow, heap use with no allocator, hosted
Core APIs on a no-OS machine, MMIO outside declared regions, MMIO without an
unsafe audit gate, missing panic policy, and missing linker provenance.

### Real firmware artifacts

Selecting a typed machine builds deterministic firmware under
`.jet/target/<name>/` (tests use a temp dir):

- `memory.ld` — generated linker script from memory regions
- `startup.c` / `startup.S` — reset or `_start` for the triple
- `firmware.elf` — linked image (`clang` + `ld.lld`)
- `firmware.map` — linker map
- `<name>.target.json` — stable audit JSON plus size/budget fields

Representative boards:

| Machine | Triple | Proof |
|---------|--------|-------|
| `board.sensor_v1` | `thumbv7em-none-eabihf` | MCU ELF + map + audit + flash budget |
| `board.virt_aarch64` | `aarch64-unknown-none` | QEMU `virt` boots and prints `OK` |

```sh
jet inspect dossier target board.sensor_v1
jet inspect dossier target board.virt_aarch64 --json
```

Hostile machines fail closed (overlap, missing panic, heap without allocator,
MMIO outside regions). Unsupported Dev/JIT paths return
`ExecutionTierUnsupported` for no-OS machines.

## Running under QEMU (D-CROSS3 local harness)

After a cross-build, run the binary under QEMU user-mode emulation without
a full system image. Install `qemu-user-static` (or `qemu-arch-extra` on
Arch), then:

```sh
# aarch64 example
qemu-aarch64-static ./build/61_freestanding
```

For a typed no-OS machine, QEMU system-mode is the live proof path. The
`board.virt_aarch64` machine builds `firmware.elf` and boots under:

```sh
qemu-system-aarch64 \
  -machine virt \
  -cpu cortex-a57 \
  -kernel .jet/target/board.virt_aarch64/firmware.elf \
  -nographic
```

The smoke harness expects the UART to print `OK` (see `tests/target_machines.rs`).

## Checking target availability

```
jet self doctor --target=aarch64-unknown-linux-gnu
```

The `cross` doctor row uses the same `Doctor::probe_target_component` gate as
`jet build`: it checks target-list membership, the rustc sysroot, and a
non-empty target library. The report is `ok` only when all three are proven;
otherwise it reports the concrete missing component and its fix. Backend
aliases map to their effective rustc target, and `sandbox` also checks for
`wasm-tools`.

## Caveats (v1)

- No board-support package or HAL library (use the low-level tier directly).
- The package `targets:` field remains manifest data; CLI machine names
  dispatch through the `TargetMachine` registry described in the matrix.
- Direct driver APIs return typed `TargetMachineError` values. The CLI wraps
  selected-machine validation failures as the existing target-support E3302
  path; this does not add a second public diagnostic family.
- WASM profiles use the exact named browser, WASI, and no-OS provider sets
  above.
- E3303 (missing allocator on a no-OS target) remains registered; the typed
  machine model catches the same fact as data.
