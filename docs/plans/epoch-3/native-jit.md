# Own Native JIT

Card: #11 / c141. Status: far-horizon research plan. Depends on the bytecode VM
or equivalent dev-tier parity foundation.

## Goal

Build a Jet-owned native JIT backend that emits machine code from executable TIR
without Cranelift. It preserves the same semantic contract as AOT and the bytecode
VM while giving Jet full control over code layout, optimization tiers, register
allocation, executable memory policy, hot-swap, and audit output.

## Beginner/Expert/Hybrid Pass

- Beginner: `jet dev` remains one behavior. No target CPU, register allocator,
  or executable-memory language appears in default workflows.
- Expert: dossier/fact views show target ISA, lowered blocks, register pressure,
  relocation table, deopt/fallback reasons, executable-memory policy, and
  reproducibility data.
- Hybrid: native JIT is another `JitBackend` consumer of TIR. It never owns
  language semantics and never becomes release behavior unless separately
  ratified.

## Required Foundation

- R12 parity batteries compare stdout, stderr, exit code, diagnostics, panics,
  and side effects.
- `jet-rt` provides shared runtime semantics and safe trap boundaries.
- `JetVal` represents compound values for all dev tiers.
- Bytecode VM or equivalent typed interpreter gives a trusted fallback for every
  TIR operation the native JIT declines.

## Architecture Slices

1. IR bridge: lower TIR to a small machine-independent block IR with explicit
   types, effects, call boundaries, traps, and safepoints.
2. Baseline x64 backend: instruction selection for scalar ops, branches, calls,
   and runtime shims; no optimization beyond correctness-preserving local forms.
3. Register allocation: linear-scan allocator with spill slots, verified calling
   convention, and span-preserving trap metadata.
4. Executable memory: W^X page lifecycle, relocation, symbol table, teardown,
   and process-isolation policy.
5. AArch64 backend: same IR contract and tests, target selected from host.
6. Hot-swap linker: replace functions atomically when type-stable; retire old
   code after no frames point at it.
7. Optimizing tier: optional hot block recompile under the same observable
   contract; never changes diagnostic or panic text.
8. Fallback ladder: unimplemented or unsafe native-JIT cases run bytecode VM or
   transparent AOT subprocess.

## Safety Rules

- Native JIT crashes or miscompiles are P0 parity failures.
- Rust or OS faults never surface raw to users; dev-tier reports stay Jet-owned.
- Generated executable memory is dev-runtime internal. Safe Jet cannot observe
  pointers to JIT code.
- `#Unsafe` user code does not gain extra authority because it is JIT-hosted.
- Foreign code and platform APIs default to transparent AOT subprocess until an
  audited in-process host is proven.

## Test Strategy

- Assembler/disassembler golden tests for each instruction family.
- IR verifier tests for type mismatch, bad control flow, relocation errors, and
  calling convention mismatch.
- Differential execution against bytecode VM and AOT for every covered example.
- Stress tests for hot-swap during active frames, trap-then-continue, and code
  page teardown.
- Target matrix for x64 and aarch64 where available.
- Security tests for W^X transitions, no writable+executable pages, and no raw
  host fault leakage.

## Ballots Needed

No immediate ballot. Before implementation starts, queue owner ballots for:

- native JIT entering the default `jet dev` tier after bytecode VM parity;
- any new CLI/fact command beyond existing dossier/expand surfaces;
- any invariant amendment around executable memory hosting.

Recommended default: native JIT remains an internal dev tier with expert
inspection through existing dossier/facts.
