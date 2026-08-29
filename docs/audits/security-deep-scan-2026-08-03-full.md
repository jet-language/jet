# Security deep scan: memory and ABI safety

This file records the source traces for the nine candidates owned by Tower card
#1378. The trace uses the current checkout. Line numbers point to the source
files named in the trace.

The boundary labels are:

- source: the value or control flow that starts the path;
- control: the check or lifetime rule that limits the path;
- sink: the operation that could read, free, or call an invalid value;
- impact: the failure if the control is missing;
- precondition: the access needed to reach the boundary;
- validation: the source or test proof in this checkout.

## `jit-jetarena-vec-layout-casts`

Disposition: `already-fixed`.

Source: JIT list mutation hosts call `JetArena::list_values_mut` from
`crates/jet-jit/src/Collections.rs:1668-1677,3670-3719`.

Control: `crates/jet-rt/src/lib.rs:466-480` checks the carrier, moves an
`IntList` into a `List`, converts each value to `JetVal::Int`, and returns the
real `Vec<JetVal>`.

Sink: Shared `collection_semantics` receives the returned `Vec`. The current
path has no `JetArena`-to-`Vec` layout cast and no `from_raw_parts` operation.

Boundary and impact: This is an in-process JIT collection host. The reported
layout cast would have caused undefined behavior if it were live. The current
source has no such sink.

Precondition: A JIT list mutation must reach the host binding. No external
pointer or ownership token reaches this path.

Validation: The stale comment at
`crates/jet-jit/src/Collections.rs:3685` is not executable code. The explicit
carrier conversion and mutation callers are the source proof.

## `wasm-list-i64-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript passes a packed `(ptr, len)` token through the `list-i64`
rail in `crates/jet-codegen/src/Prelude/DomRuntime.js:773-787`.

Control: Generated WASM records the allocation kind, pointer, and byte length
in `crates/jet-codegen/src/Codegen/Web.rs:3812-3847`. The argument path checks
`len * size_of::<i64>()` and calls `jet_abi_require` at `3916-3927`.

Sink: `Box::from_raw` runs only after the exact registry check at
`crates/jet-codegen/src/Codegen/Web.rs:3924-3927`. The free export repeats the
check at `3940-3945`.

Boundary and impact: The JS-to-WASM export boundary is reachable by a host
that can call the exported functions. Without the registry, a forged pointer,
wrong count, or replayed token could cause an invalid read or free. The current
boundary traps before reconstruction or free.

Precondition: The caller must invoke a generated export or an ABI free export
with a packed token. The allocator is the only source of a valid token.

Validation: `tests/web_build.rs:4263-4332` checks the generated exports and
uses a real WASM module and Node harness. The harness traps on a forged pointer
and a wrong element count, then checks an exact registered round trip.

## `wasm-list-string-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript creates the count/length/UTF-8 blob in
`crates/jet-codegen/src/Prelude/DomRuntime.js:788-811`.

Control: Generated WASM requires the exact list-string allocation token at
`crates/jet-codegen/src/Codegen/Web.rs:4041-4050`. It checks the header,
embedded count, each length, UTF-8, and trailing bytes at `4053-4067`.

Sink: The byte `Box` is reconstructed only after `jet_abi_require` at
`crates/jet-codegen/src/Codegen/Web.rs:4048-4051`. The free export checks the
same ownership record at `4079-4084`.

Boundary and impact: The JS-to-WASM list-string boundary accepts host-written
bytes. Without the ownership and bounds checks, the host could cause an
invalid read or free. The current path traps before an invalid reconstruction.

Precondition: A caller must invoke the generated list-string export or free
export. Valid tokens come from `jet_abi_list_string_alloc` or a return value.

Validation: `tests/web_build.rs:4336-4450` builds a real WASM module and Node
harness. The hostile harness traps on a forged pointer and a wrong blob length,
then checks an exact registered list round trip.

## `wasm-map-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript creates the map blob in
`crates/jet-codegen/src/Prelude/DomRuntime.js:813-867`.

Control: Generated WASM requires the map kind, pointer, and byte length at
`crates/jet-codegen/src/Codegen/Web.rs:4116-4126`. It bounds each key and value,
checks UTF-8 and decimal integers, and rejects trailing bytes at `4128-4151`.

Sink: The byte `Box` is reconstructed only after the registry check at
`crates/jet-codegen/src/Codegen/Web.rs:4123-4126`. The free export checks the
same token at `4163-4168`.

Boundary and impact: The JS-to-WASM map boundary accepts host-written bytes.
Without exact ownership and bounds checks, a forged token or serialized count
could cause an invalid read or free. The current path traps before that sink.

Precondition: A caller must invoke a generated map export or free export.
Valid tokens come from `jet_abi_map_string_int_alloc` or a return value.

Validation: `tests/web_build.rs:4453-4548` builds a real WASM module and Node
harness. The hostile harness traps on a forged pointer and a wrong blob length,
then checks an exact registered map round trip.

## `wasm-string-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript passes packed string tokens through
`crates/jet-codegen/src/Prelude/DomRuntime.js:717-735` and generated export
wrappers receive them in `crates/jet-codegen/src/Codegen/Web.rs:4235-4361`.

Control: Generated WASM records exact string ownership at
`crates/jet-codegen/src/Codegen/Web.rs:3818-3847`. String arguments require
the matching token at `3863-3874`, and the free export requires it at
`3884-3890`.

Sink: `Box::from_raw` is reachable only after `jet_abi_require`. UTF-8 decoding
also rejects invalid string bytes.

Boundary and impact: The JS-to-WASM string boundary is host-accessible. The
former unchecked pointer path could read or free memory that the WASM module
did not allocate. The current path traps on forged and mismatched tokens.

Precondition: The caller must invoke the generated string export or free
export. A valid token comes from `jet_abi_string_alloc` or a return value.

Validation: `tests/web_build.rs:4087-4178` runs a real WASM module and Node
harness. It traps on a forged free and a length mismatch, then checks an exact
registered string round trip.

## `d0017-s1-aot-termios-layout`

Disposition: `already-fixed`.

Source: The AOT input-secret path calls the shared terminal function from
`crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:413-430`.

Control: AOT embeds `Term.rs` at
`crates/jet-codegen/src/Codegen/mod.rs:256-260`. The shared Prelude defines
target-specific flags, control-byte offsets, field widths, and compile-time
sizes at `crates/jet-codegen/src/Prelude/Term.rs:450-567`. Unknown Unix targets
return `false` instead of using a guessed layout at `657-675`.

Sink: The native terminal ABI calls `tcgetattr` and `tcsetattr` through the
target-specific `Termios` at
`crates/jet-codegen/src/Prelude/Term.rs:569-603`.

Boundary and impact: This is a local process-to-OS terminal ABI. A wrong
layout could corrupt terminal state or memory. The current implementation
uses one shared layout and fails closed on unsupported Unix targets.

Precondition: The target must use the terminal input path. The target-specific
compile-time size assertion must also hold.

Validation: `tests/os_native.rs:68-97` checks the Darwin/BSD offsets, supported
sizes, absence of guessed padding, and shared AOT/JIT/interpreter inclusion.

## `d0017-s1-jit-termios-layout`

Disposition: `duplicate-of-d0017-s1-aot-termios-layout`.

Source: JIT includes the same terminal Prelude at
`crates/jet-jit/src/IO.rs:21-23` and calls `jet_term_input_secret` at
`267-282`.

Control and sink: There is no second JIT `Termios` definition. The shared
target-specific control and native sink are the ones traced for
`d0017-s1-aot-termios-layout` at `Term.rs:450-603`.

Boundary and impact: This is the same local process-to-OS terminal ABI
boundary. It is not a separate JIT vulnerability.

Precondition: The JIT host must reach its input-secret adapter. The shared
Prelude then applies the target layout.

Validation: `tests/os_native.rs:91-97` checks that AOT, JIT, and interpreter
include the same `Term.rs` source. This candidate has no independent layout
or independent fix.

## `jit-http-worker-runtime-uaf`

Disposition: `already-fixed`.

Source: HTTP workers invoke JIT handler pointers in
`crates/jet-jit/src/net_http_hosts.rs:544-575,2264-2289`. The handlers reach
the resident runtime through `Concurrency::with_http_jet_runtime`.

Control: `crates/jet-jit/src/Concurrency.rs:320-388` acquires the runtime
access guard before loading the shared pointer. The shared HTTP shutdown path
stops each server and clears its handles under that guard at
`crates/jet-jit/src/net_http_hosts.rs:57-75`. The listener joins every worker
in the shared Prelude at
`crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:1054-1078`.
The same Prelude now makes duplicate shutdown callers wait on the first
shutdown report at `HTTPServer.rs:862-889`; they cannot leave the clear boundary
while the original worker joins are still pending.

Sink: The raw runtime pointer is dereferenced at
`crates/jet-jit/src/Concurrency.rs:327-333`. Resident replacement previously
dropped the module/runtime without covering the hot-swap path.

Fix and impact: `crates/jet-jit/src/jit/resident.rs:628-649` now clears and
joins HTTP workers before it takes the live runtime or drops the old resident
module. The shared duplicate-shutdown wait closes the concurrent teardown
window as well. These controls prevent a worker from retaining a raw runtime
or code pointer across replacement.

Precondition: A live resident HTTP server and a concurrent resident hot swap
are required. Ordinary one-shot teardown already used the same shutdown path.

Validation: `tests/jit_run.rs:266-328` checks normal teardown, duplicate
shutdown waiting, and hot-swap ordering. The test requires the HTTP clear to
occur before the hot-swap module drop.

## `jit-event-four-capture-abi`

Disposition: `already-fixed`.

Source: Event lowering adds the payload parameter in
`crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3516-3566`.

Control: JIT callback lowering counts captures plus parameters and rejects a
total above four at `crates/jet-jit/src/jit/lower_ctx.rs:24444-24481`. The
callback signature includes both captures and parameters at
`crates/jet-jit/src/jit/functions_compile.rs:448-461`.

Sink: `crates/jet-jit/src/Reactive.rs:30-92` calls typed function pointers
with four native capture slots. Event and async payload insertion is capped at
four at `360-404,722-772`.

Boundary and impact: This is an in-process generated JIT callback ABI. Four
captures plus a payload would produce five native arguments and could call a
function with the wrong ABI. The lowering guard rejects that shape before a
native call.

Precondition: A Jet event callback must contain four captures and a payload
parameter. Valid callbacks with at most four total arguments remain supported.

Validation: `tests/jit_run.rs:199-263` proves AOT and forced-interpreter output
keep all captures and the payload, then checks that JIT compilation rejects
five callback ABI arguments before native code runs.

## Source ledger

The trace uses these owned paths:

- `crates/jet-rt/src/lib.rs`
- `crates/jet-jit/src/Collections.rs`
- `crates/jet-jit/src/IO.rs`
- `crates/jet-jit/src/Concurrency.rs`
- `crates/jet-jit/src/net_http_hosts.rs`
- `crates/jet-jit/src/jit/lower_ctx.rs`
- `crates/jet-jit/src/jit/resident.rs`
- `crates/jet-codegen/src/Codegen/Web.rs`
- `crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs`
- `crates/jet-codegen/src/Prelude/Term.rs`
- `crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs`
- `crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs`
- `crates/jet-codegen/src/Prelude/DomRuntime.js`
- `tests/jit_run.rs`
- `tests/os_native.rs`
- `tests/web_build.rs`
