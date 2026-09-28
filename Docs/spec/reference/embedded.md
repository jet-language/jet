# Embedded targets

Jet can compile one source model for hosted, browser, WASI, and embedded target
machines. This page is for firmware and systems engineers choosing a target,
inspecting its machine contract, and embedding a generated library. The
executable definitions are in
[`jet-driver/src/Driver/mod.rs`](../../../crates/jet-driver/src/Driver/mod.rs),
[`jet-foundation/src/TargetMachine.rs`](../../../crates/jet-foundation/src/TargetMachine.rs),
and [`tests/target_machines.rs`](../../../tests/target_machines.rs).

## Choose a target machine

A target machine is a named platform contract, not just a CPU triple. The
built-in names are:

| Machine | Intended boundary |
| --- | --- |
| `hosted` | Native process with the host runtime and operating-system services. |
| `board.sensor_v1` | Cortex-M4-style firmware using the `thumbv7em-none-eabihf` boundary. |
| `board.virt_aarch64` | AArch64 bare-metal firmware using the `aarch64-unknown-none` boundary. |
| `wasm.browser` | Browser WebAssembly plus the generated JavaScript boundary. |
| `wasm.wasi` | WebAssembly with WASI imports. |
| `wasm.no-os` | WebAssembly without an operating-system import surface. |

Inspect the machine dossier before selecting a board profile:

```sh
jet inspect dossier target board.sensor_v1
jet inspect dossier target board.virt_aarch64
```

The dossier describes the target triple, runtime providers, linker and runner
expectations, and available capabilities. A raw triple is not a substitute for
a target-machine descriptor: it can select code generation while leaving the
board's provider and execution contract unspecified.

## Firmware artifacts and boundaries

A board build is an AOT artifact. The firmware boundary must provide the
runtime services named by the package and must use the linker/sysroot selected
for the target machine. The sensor profile maps to an MPS2 AN386/Cortex-M4
QEMU model; the virtual AArch64 profile maps to the `virt`/Cortex-A57 model.
Those mappings are target-machine facts, not a promise that every board can run
on every host.

Use the target machine at build time rather than smuggling board behavior into
source conditionals:

```sh
jet build --target board.sensor_v1 path/to/firmware.jet
jet build --target board.virt_aarch64 path/to/firmware.jet
```

A no-OS profile has no host filesystem, process, or network fallback. If a
module requests an unavailable provider, the build should report the missing
capability at the boundary. The test matrix in
[`tests/target_machines.rs`](../../../tests/target_machines.rs) exercises
machine selection, artifacts, and the QEMU runner contract.

## QEMU and inspection

QEMU is a runner for the machine profiles that define one; it is not the
source of the target definition. The sensor profile uses `mps2-an386` with a
Cortex-M4 CPU, while the AArch64 virtual profile uses `virt` with a
Cortex-A57 CPU. Keep the machine name in the build and dossier command so the
same target contract is visible in logs and receipts.

When diagnosing a target, inspect in this order:

1. the target-machine dossier;
2. the generated artifact and linker inputs;
3. the provider/capability diagnostics;
4. the QEMU model, if the machine defines one.

This order distinguishes a source or capability error from a runner that is
simply unavailable on the development host.

## Native Library embedding

The `Library` backend is the boundary for embedding Jet code in a host
application. It emits the selected native library artifacts and, when the
backend requests them, a C-compatible header and bindings. Export only values
supported by the native ABI: scalar integers/floats, booleans, and the
explicitly supported string/handle forms. Rich Jet values must cross through a
serialized or host-owned representation rather than an accidental Rust or
Jet layout.

A loadable package can expose a C entry point with the current function syntax:

```jet
#Export(c)
pub fn on_tick(dt: Int) -> Int {
    dt + 1
}
```

The host owns initialization, calls, and shutdown. It must not call an exported
function after unloading its library or while an owned buffer/handle is still
in use. The generated header and library set are the ABI record; the source
signature alone is not a license to reinterpret an opaque value.

For a package-level loadable artifact, the canonical Jet-side loader is:

```jet
use core.mod as library

loaded :: library.load(
    ".jet/build/loadable.jetlib",
    grant: { read: [".jet/build"] },
)
```

The grant is explicit. A loadable package does not acquire arbitrary filesystem
or process access merely because the host has loaded it.

## Browser and sandbox targets

`wasm.browser`, `wasm.wasi`, and `wasm.no-os` are separate target-machine
choices. Browser partitioning and JavaScript/Wasm ABI rules are documented in
[`web-backend-wasm.md`](web-backend-wasm.md). A browser artifact is not an
embedded board image, and a no-OS artifact must not assume WASI or browser
imports.

## Reproducible target selection

Record the target machine, package lock, compiler identity, and linker/sysroot
inputs with the artifact. Do not infer a board from the host operating system,
and do not use a host `PATH` compiler as an undeclared replacement for a
machine's configured toolchain. The target-machine source and tests are the
executable truth when this page and a local wrapper disagree.
