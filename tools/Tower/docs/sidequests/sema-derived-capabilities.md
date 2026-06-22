# Plan: Derive capabilities from sema/effects

**Status:** planned. Full effect integration waits on D-EFF2/D-EFF3, but replacing Rust
text scans can start with explicit sema facts.

## Goal

Stop inferring package/runtime capabilities by searching generated Rust strings.

## Implementation Steps

1. Define a `CapabilitySet` data structure in sema or a shared analysis module.
2. Record capabilities at semantic resolution sites: fs, net, unsafe, subprocess, FFI,
   package operations.
3. Thread the set through bundle checking.
4. Replace `Capabilities::from_rust` substring scans with sema-derived data.
5. Keep a compatibility fallback only during transition, with tests proving both agree.

## Verification

- Unit tests for each capability source.
- CLI `--capabilities-json` snapshot if available.
- Regression test that generated Rust text changes do not alter capability output.
