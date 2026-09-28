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
`crates/jet-jit/src/Collections.rs:1694-1703,3613-3664`.

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
`crates/jet-jit/src/Collections.rs:3629` is not executable code. The explicit
carrier conversion and mutation callers are the source proof; the hostile
cross-tier mutation fixture is `tests/jit_run.rs:1953-2014`.

## `wasm-list-i64-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript passes a packed `(ptr, len)` token through the `list-i64`
rail in `crates/jet-codegen/src/Prelude/DomRuntime.js:773-787`.

Control: Generated WASM records the allocation kind, pointer, and byte length
in `crates/jet-codegen/src/Codegen/Web.rs:4407-4453`. The argument path checks
`len * size_of::<i64>()` and calls `jet_abi_require` at `4522-4530`.

Sink: `Box::from_raw` runs only after the exact registry check at
`crates/jet-codegen/src/Codegen/Web.rs:4529-4533`. The free export repeats the
check at `4546-4552`.

Boundary and impact: The JS-to-WASM export boundary is reachable by a host
that can call the exported functions. Without the registry, a forged pointer,
wrong count, or replayed token could cause an invalid read or free. The current
boundary traps before reconstruction or free.

Precondition: The caller must invoke a generated export or an ABI free export
with a packed token. The allocator is the only source of a valid token.

Validation: `tests/web_build.rs:4509-4579` checks the generated exports and
uses a real WASM module and Node harness. The harness traps on a forged pointer
and a wrong element count, then checks an exact registered round trip.

## `wasm-list-string-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript creates the count/length/UTF-8 blob in
`crates/jet-codegen/src/Prelude/DomRuntime.js:788-811`.

Control: Generated WASM requires the exact list-string allocation token and
parses the bounded blob at
`crates/jet-codegen/src/Codegen/Web.rs:4647-4674`. It checks the header,
embedded count, each length, UTF-8, and trailing bytes at `4659-4672`.

Sink: The byte `Box` is reconstructed only after `jet_abi_require` at
`crates/jet-codegen/src/Codegen/Web.rs:4654-4657`. The free export checks the
same ownership record at `4685-4690`.

Boundary and impact: The JS-to-WASM list-string boundary accepts host-written
bytes. Without the ownership and bounds checks, the host could cause an
invalid read or free. The current path traps before an invalid reconstruction.

Precondition: A caller must invoke the generated list-string export or free
export. Valid tokens come from `jet_abi_list_string_alloc` or a return value.

Validation: `tests/web_build.rs:4582-4696` builds a real WASM module and Node
harness. The hostile harness traps on a forged pointer and a wrong blob length,
then checks an exact registered list round trip.

## `wasm-map-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript creates the map blob in
`crates/jet-codegen/src/Prelude/DomRuntime.js:813-867`.

Control: Generated WASM requires the map kind, pointer, and byte length at
`crates/jet-codegen/src/Codegen/Web.rs:4722-4758`. It bounds each key and value,
checks UTF-8 and decimal integers, and rejects trailing bytes at `4734-4757`.

Sink: The byte `Box` is reconstructed only after the registry check at
`crates/jet-codegen/src/Codegen/Web.rs:4729-4732`. The free export checks the
same token at `4769-4774`.

Boundary and impact: The JS-to-WASM map boundary accepts host-written bytes.
Without exact ownership and bounds checks, a forged token or serialized count
could cause an invalid read or free. The current path traps before that sink.

Precondition: A caller must invoke a generated map export or free export.
Valid tokens come from `jet_abi_map_string_int_alloc` or a return value.

Validation: `tests/web_build.rs:4699-4941` builds a real WASM module and Node
harness. The hostile harness traps on a forged pointer and a wrong blob length,
then checks an exact registered map round trip.

## `wasm-string-untrusted-ownership`

Disposition: `already-fixed`.

Source: JavaScript passes packed string tokens through
`crates/jet-codegen/src/Prelude/DomRuntime.js:717-735` and generated ABI
helpers receive them in `crates/jet-codegen/src/Codegen/Web.rs:4404-4483`.

Control: Generated WASM records exact string ownership at
`crates/jet-codegen/src/Codegen/Web.rs:4407-4453`. String arguments require
the matching token at `crates/jet-codegen/src/Codegen/Web.rs:4469-4480`, and
the free export requires it at `4491-4496`.

Sink: `Box::from_raw` is reachable only after `jet_abi_require`. UTF-8 decoding
also rejects invalid string bytes.

Boundary and impact: The JS-to-WASM string boundary is host-accessible. The
former unchecked pointer path could read or free memory that the WASM module
did not allocate. The current path traps on forged and mismatched tokens.

Precondition: The caller must invoke the generated string export or free
export. A valid token comes from `jet_abi_string_alloc` or a return value.

Validation: `tests/web_build.rs:4333-4425` runs a real WASM module and Node
harness. It traps on a forged free and a length mismatch, then checks an exact
registered string round trip.

## `d0017-s1-aot-termios-layout`

Disposition: `already-fixed`.

Source: The AOT input-secret path calls the shared terminal function from
`crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:323-343`.

Control: AOT embeds `Term.rs` at
`crates/jet-codegen/src/Codegen/mod.rs:258-263`. The shared Prelude defines
target-specific flags, control-byte offsets, field widths, and compile-time
sizes at `crates/jet-codegen/src/Prelude/Term.rs:450-567`. Unknown Unix targets
return `false` instead of using a guessed layout at `676-695`; the shared PTY
configurator is exported at `714-746`.

Sink: The native terminal ABI calls `tcgetattr` and `tcsetattr` through the
target-specific `Termios` at
`crates/jet-codegen/src/Prelude/Term.rs:569-607`.

Boundary and impact: This is a local process-to-OS terminal ABI. A wrong
layout could corrupt terminal state or memory. The current implementation
uses one shared layout and fails closed on unsupported Unix targets.

Precondition: The target must use the terminal input path. The target-specific
compile-time size assertion must also hold.

Validation: `tests/os_native.rs:68-120` checks the Darwin/BSD offsets, supported
sizes, absence of guessed padding, the shared AOT/JIT/interpreter inclusion,
and PTY reuse of the canonical ABI.

## `d0017-s1-jit-termios-layout`

Disposition: `duplicate-of-d0017-s1-aot-termios-layout`.

Source: JIT includes the same terminal Prelude at
`crates/jet-jit/src/IO.rs:21-23` and calls `jet_term_input_secret` at
`273-282`.

Control and sink: There is no second JIT `Termios` definition. The shared
target-specific control and native sink are the ones traced for
`d0017-s1-aot-termios-layout` at
`crates/jet-codegen/src/Prelude/Term.rs:450-607`.

Boundary and impact: This is the same local process-to-OS terminal ABI
boundary. It is not a separate JIT vulnerability.

Precondition: The JIT host must reach its input-secret adapter. The shared
Prelude then applies the target layout.

Validation: `tests/os_native.rs:91-120` checks that AOT, JIT, and interpreter
include the same `Term.rs` source. This candidate has no independent layout
or independent fix.

## `jit-http-worker-runtime-uaf`

Disposition: `already-fixed`.

Source: HTTP workers invoke JIT handler pointers in
`crates/jet-jit/src/net_http_hosts.rs:592-640,2383-2425,2450-2501`. The
handlers reach the resident runtime through the epoch-validating
`Concurrency::try_with_http_jet_runtime_at` wrapper.

Control: `crates/jet-jit/src/Concurrency.rs:324-395,400-469` acquires the runtime
access guard before loading the shared pointer and rejects stale epochs. The
shared HTTP shutdown path stops each server, then clears its handles under that
guard, at
`crates/jet-jit/src/net_http_hosts.rs:58-76`. The listener joins every worker
in the shared Prelude at
`crates/jet-codegen/src/Prelude/CoreLib/Top/HTTPServer.rs:1050-1089`.
The same Prelude now makes duplicate shutdown callers wait on the first
shutdown report at `HTTPServer.rs:878-897`; they cannot leave the clear boundary
while the original worker joins are still pending.

Sink: The raw runtime pointer is dereferenced at
`crates/jet-jit/src/Concurrency.rs:330-338`, and the raw JIT callbacks are
invoked at `crates/jet-jit/src/net_http_hosts.rs:597-607,623-633,2389-2393,2460-2470`.
Resident replacement previously dropped the module/runtime without covering
the hot-swap path.

Fix and impact: `crates/jet-jit/src/jit/resident.rs:632-652` now clears and
joins HTTP workers before it takes the live runtime or drops the old resident
module. The shared duplicate-shutdown wait closes the concurrent teardown
window as well. These controls prevent a worker from retaining a raw runtime
or code pointer across replacement.

Precondition: A live resident HTTP server and a concurrent resident hot swap
are required. Ordinary one-shot teardown already used the same shutdown path.

Validation: `tests/jit_run.rs:283-393` checks normal teardown, duplicate
shutdown waiting, HTTP/2 dispatch draining, epoch guards, and hot-swap ordering.
The test requires the HTTP clear to occur before the hot-swap module drop.

## `jit-event-four-capture-abi`

Disposition: `already-fixed`.

Source: Event lowering adds the payload parameter in
`crates/jet-codegen/src/Codegen/TIR/lower/method_calls.rs:3611-3733`.

Control: JIT callback lowering counts captures plus parameters and rejects a
total above four at `crates/jet-jit/src/jit/lower_ctx.rs:25006-25043`. The
callback signature includes both captures and parameters at
`crates/jet-jit/src/jit/functions_compile.rs:455-468`.
The normal backend turns this failed lowering into a whole-program interpreter
fallback at `crates/jet-jit/src/jit/backend.rs:104-135`; the direct compile
helper exposes the rejection without entering native code.

Sink: `crates/jet-jit/src/Reactive.rs:30-95` calls typed function pointers
with four native capture slots. Event and async payload insertion is capped at
four at `360-408,722-780`.

Boundary and impact: This is an in-process generated JIT callback ABI. Four
captures plus a payload would produce five native arguments and could call a
function with the wrong ABI. The lowering guard rejects that shape before a
native call.

Precondition: A Jet event callback must contain four captures and a payload
parameter. Valid callbacks with at most four total arguments remain supported.

Validation: `tests/jit_run.rs:199-280` proves AOT, forced-interpreter, and
default-run output keep all captures and the payload; default run deopts
without entering native code, and direct JIT compilation rejects five callback
ABI arguments before native code runs.

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
