# Native Jet CoreLib contract

This specification is for Core maintainers, provider authors, and reviewers. It
defines where Core owns semantics, where a host may provide a typed leaf, how
capabilities and effects are exposed, and what evidence makes an export
complete. The declaration registry is
[`Core.jet`](../../crates/jet-codegen/src/Prelude/Core.jet); Jet source bodies
are under [`Core/`](../../Core/). Generated call and module views are derived
from those inputs by the [Core table generator](../../Tools/agent/gen-core-tables.mjs).
The [surface ledger](../../tests/core_surface_ledger.rs) checks selected
identity rows; runnable provider examples live under
[`Examples/features/`](../../Examples/features/).

The Rust-hosted compiler under `Source/` and `crates/` remains the production
and reference compiler. The default CLI continues to use that reference path.
The staged Jet-authored compiler under `Compiler/` may implement the same
contracts in bootstrap scopes, but this document does not claim that Jet is
self-hosted. Rust emission and rustc/LLVM are transitional bootstrap seams that
leave with the rest of Rust once the Jet back end covers every program (D-EXEC2,
D-BACKEND-OWN1, owner directive 19). No Core contract is defined as
permanently owned by a particular compiler implementation.

Loaded declaration graphs close over source-owned Core nominal field and
payload types, including optional/container members, aliases, and distinct
bases. An implicit bare name such as `Stat.modified: DateTime` selects the
registered Core owner (`core.time`), unless a local declaration or generic
binder owns that name. The owner source and its own declaration dependencies
join the ordinary checked module queue before MIR nominal facts are frozen;
emission must not substitute a native carrier, invent an alias declaration, or
resolve an unrelated module merely because its type shares the leaf name.

Service state framing and signature codecs explicitly import their source
owners (`core.sys` and `core.encoding`). The selected-target `usize_max` and
hex codecs remain the same public functions; these imports make the checked
source dependency visible rather than treating it as a root-Core dispatcher
leaf. Production calls and service signature-vector tests use those same owners.

The private native Dyn codec providers retain exact Int and Text owners when
encoding, including the unsigned high half of a U64 input word. Char encoding
uses the existing invalid-scalar replacement contract. `FieldError.under` clones
the input errors and frames all paths in order: empty paths take the segment,
index paths concatenate without a dot, and other child paths add a dot.
The codec JSON entry reuses the strict compiler JSON parser (exact Number
lexemes and its existing depth/error contract), wrapping syntax failures as one
empty-path FieldError with `invalid JSON (line N): reason`. It does not substitute
the public Core JSON policy parser.

The native file line leaf owns no UTF-8, terminator, or fault policy. It reads
through LF inclusively, preserves every CR byte, and returns an empty byte list
at EOF. Its refill state is the same FD72 buffer used by subsequent reads and
relative seeks; read-ahead must not skip bytes or advance the logical cursor.
`DynCodecTests.jet` and `CompilerFileLeavesTests.jet` are direct provider
witnesses. Hosted checks or execution do not constitute native-pack C ABI proof;
that proof requires the freshly composed Jet-only runtime object.

Native task-stop publication requires both a current task control and the
compiler's scoped publication capability: only a checked operation immediately
followed by MIR `Poll(Propagate)` may arm it. Plain calls and root bodies remain
process-fatal. Runtime-internal traps (overflow, division by zero, and list
bounds) in task bodies are not publishable unless compiler-armed.
The native raw-pointer Deref sentry read uses that same immediate-Propagate
predicate; it cannot lend publication merely because another block polls.
Sentry and ordinary traps share `trap_can_publish`; a task body alone is
not publication authority for an unarmed raw memory fault.
`jet_rt_poll_call_leave` restores the saved TCB64 word on a returning call.
If a compiled callback leaves through its `Stopped` exit instead, the task
runner restores the body word through `scheduler_body_leave(body)` after
`coll_call2` (`Compiler/JetRuntime/Source/Task.jet`). A process-fatal call does
not resume the scheduler and therefore needs no surviving capability restore.

Program-side require, panic, and index-miss producers run registered cleanup
before the call only when they cannot emit a real Propagate (plain mode,
cleanup-masked code, transactions, or a closed block). Otherwise the Poll stop
edge owns that cleanup exactly once, and an unconditional stop's impossible
resume seals `Stopped`. This minimal runtime-port seam is mirrored in
`crates/jet-codegen/src/Codegen/TIR/{mir,tir_to_mir_expr}.rs`; that seed and
verification mirror dies at the Rust cutover.
The Rust seed's plain `TOrFallback::Panic` path in `tir_to_mir_expr.rs`
emits RequireStop then Unreachable without pre-call cleanup, unlike the Jet
`jet_codegen_lower_fallback_panic` path in `Statements.jet`. This seam preserves
that pre-existing plain-mode divergence rather than changing seed semantics.

The factory selects `jet_driver_compile_mir` for both native programs and
runtime-library objects. `JetDriverCompileRequest.native_stop_points` selects
returning-stop MIR before the shared `lower_functions` lowers functions and
closures; runtime-library objects leave it false. It does not enable comptime
budgets. This matches the Rust task
boundary's panic-to-TaskFailure catch rather than letting an ordinary native
task panic abort its whole process. Plain/no-task MIR production uses the
same false-mode implementation; its output is expected to remain byte-identical.

Native cleanup frames bracket their actions with `jet_rt_cleanup_enter/leave`:
one runtime cleanup scope (`jet_rt_session_cleanup_enter/leave`, whose owned
token restores the control's scope flags for nesting) plus Shield. The first
owned Stop stays in the control slot, while Check/Propagate delivery of it is
masked even inside deferred thunks and resource closes. Cancellation and
deadlines do not interrupt those
closes. A new panic during cleanup with a pending Stop is process-fatal (70),
matching the Rust seed's panic-during-unwinding abort semantics
(`crates/jet-codegen/src/Prelude/Scheduler.rs:234–244`). No additional TLS state
or specialized plain cleanup call graph is introduced.
The Source acceptance runner
`Compiler/JetBackend/Tests/run-trap-source-fixtures.mjs` reuses the registered
`defer close(^resource)` examples, checking native task publication, plain
task/root fatal report parity, LIFO closes exactly once, cancellation held
through closing and delivered at the next wait, fatal panic inside closing,
and the plain no-task output against its existing byte golden. Its
`assemble/lower/link/run` modes require the fresh composed runtime for execution;
source parsing alone is not a behavioral receipt.

Native expectation probes consume only a pending panic with a recorded raw
`E3001`, `E3010`, `E3011` or `R0801`–`R0803` stop in the innermost matching
scope. The panic rail records that code and raw message on every active
expectation frame before publishing, as `Core.rs` `jet_test_record_stop` does,
so a frame's first recorded stop is the code its expectation is matched against.
A successful
`jet_rt_test_catch_for_scope` completes that expectation but retains the original
scheduler stop while deeper frames unwind. `jet_rt_test_catch_boundary` clears it
at the canonical expectation exit, before that frame's own cleanup: the Rust
reference `MIRRust.rs:14998–15061` catches the block closure, then dispatches this
exit after `catch_unwind` returns. Thus a deeper cleanup panic still sees pending
and aborts, while a panic in the expectation's own cleanup is a fresh stop.
A mismatch replaces the original with `E3001` and sets unmet so no
parent can catch the mismatch. Cancellation, deadlines, skips, unrecognized
codes, and probes for another scope do not mutate the pending stop. Sentry
records its raw fault detail before dropping it, never its rendered report.
The native checked-MIR transport calls the CLI's authorized compiler request
and serializes the resulting MIR through the canonical Jet MIR writer. The
transport records the real driver outcome and requires a fresh output stream.
`Tests/Fragments.fixture/Fixtures/ExpectFail.jet` supplies compiled native match, mismatch,
unmet, nested, checked/unarmed R080 and deeper/own-close witnesses through the
real Task transport. `run-expect-producer.mjs` builds a focused proof as the
compiler is built (`jet build Compiler` over a copy whose CLI entry is the
producer): it reuses the CLI's authorized request, selects Test mode, checks
real MIR branch/boundary/exit-poll edges, then emits and runs that checked
test's object. `check-expect-mir-identity.mjs` compares a fixed non-test corpus
through before/after checked-MIR drivers built from their respective pinned
compiler sources. Both drivers use the same native transport and stop-point
mode; compiling the same current frontend twice does not establish a historical
frontend comparison. These execution and byte-identity checks belong to the
composed compiler-unit proof, not the light source-parser lane.

## Purpose and scope

Core modules own public behavior. A Core API must be useful for real programs,
safe by default, explicit about effects and authority, and complete enough to
serve its intended domain. The contract applies to every module row, exported
function, exported type, root type, format row, namespace-only row, and
explicit dispatcher row declared in `Core.jet`.

Each declaration has a meaningful role. A metadata-only declaration identifies a
compiler or runtime fact and has a generated consistency check; it cannot hide
missing callable behavior. An exported operation must not silently return an
empty value, report false success, discard data, manufacture deterministic
entropy in place of required entropy, or use an unimplemented fallback.

Core expands through a dependency graph and a usage mission, not a blind union
of peer libraries. High-value capabilities follow the graph and the real jobs
that need them. A scope may state explicit non-goals, but a declared export
still needs a contract, an error meaning, and evidence appropriate to its role.

## Source authority and generated projections

`Core.jet` is the authority for Core's bootstrap dependencies, module rows,
exports, root types, namespace-only declarations, format rows, and explicit
native-dispatch metadata. A `source_module` row names the canonical Jet source
path and the members that it owns. The corresponding `Core/**/*.jet` file owns
public algorithms, validation, defaults, policy, effects, authority, typed
errors, and composition.
Native method-table inventories track only tables the compiler still ships;
source-owned types must not keep obsolete native inventory entries after cutover.

Parsed Core declarations retain their original module and owner identity, ordered
generic binders (including authoritative empty binders), logical fields,
visibility, bounds, and computed-getter provenance. Qualified imported
declarations retain their exact owner when a local declaration shadows the leaf
name. Substitution requires an exact binder count. A known declaration's field
miss or arity mismatch must not switch to a differently owned builtin or native
field schema; unknown binder metadata is not an empty nongeneric declaration.

Readable logical fields do not grant struct-literal construction or make computed
getters stored fields. This declaration-fact contract does not supply missing
native-only declarations or reconcile Source/native public-contract differences,
including Web, store, and virtual-record shapes. Source `VJPRun` remains
nongeneric with `value` and `gradient: Tensor`; native generic `grads` or `pull`
semantics do not follow from these parsed facts.

The generator must verify that:

- every declared export has one canonical Jet source or an explicit registered
  provider/leaf role;
- generated module and call views agree with the declaration source;
- duplicate mechanisms, unapproved aliases, shims, and obsolete projections do
  not create a second public identity;
- effect, authority, capability, fallback, signature, and tier metadata remain
  consistent.

The generated Rust-bootstrap `CORE_CALLS` item alone permits long-running
constant evaluation: the complete declaration registry exceeds rustc's
const-evaluator step guard. This does not change row order, lookup facts,
runtime initialization, or the Jet-native contract; the generator owns the
item-level attribute alongside the table.

Shared bootstrap kernels embedded by comptime bind representation helpers
locally, without copying their policy. Borrowed list indexing lives with the
shared collection kernel so both embeddings use the same bounds check and
E3010 stop. Recursive mkdir dispatch names the single no-fault-probe raw leaf;
the source caller, not a second evaluator arm, owns the FS.Write preflight.

Email's secret-bearing native carriers retain the checked `core.crypto.Secret`
declaration as their hidden Rust type argument; they never substitute a host
secret with the same leaf spelling. The SMTP provider adapter supplies the
runtime's TLS, wiping, signing, cancellation, and deadline callbacks. Optional
native fields cross the checked `JetOutcome` boundary on both value and place
reads, including borrowed payload projections.
The generated Rust tables are projections, not a second API catalog. Never
hand-edit them. The main generated views are
[`CoreModuleExports.rs`](../../crates/jet-foundation/src/CoreModuleExports.rs),
[`core_calls.rs`](../../crates/jet-foundation/src/Syntax/core_calls.rs), and the
corresponding generated projections in the Prelude. Rust implementation files
such as [`Core.rs`](../../crates/jet-codegen/src/Prelude/Core.rs),
[`PortableCore.rs`](../../crates/jet-codegen/src/Prelude/PortableCore.rs), and
[`CoreLib/`](../../crates/jet-codegen/src/Prelude/CoreLib/) provide registered
leaves and adapters; they do not become an independent public API registry.

### Borrowed UI-host callback scopes

The seed `HostServices.rs` adapter exposes borrowed hosts only through the
lexical `jet_ui_with_host` callback. Its lifetime-bound restoration guard and
unsafe installer are private: safe callers cannot retain, forget, or reorder a
guard while leaving a borrowed host in thread-local storage. The stored thin
pointer addresses the wrapper's live stack-held host reference; no trait-object
lifetime is transmuted. Unwinding restores the predecessor before that reference
or the host borrow ends.

Current-host access holds its mutable thread-local borrow throughout the host
callback. Re-entry is refused with a diagnostic panic before a second mutable
host reference can be created; nested host installation during that callback
is likewise refused. Ordinary nested lexical host scopes remain valid.
`crates/jet-codegen/tests/ui_host_scope.rs` covers the rejected dangling guard
and recursive-callback rejection with subsequent access and unwind recovery.

## Jet semantics and native leaves

Jet owns the parts that give an operation its meaning:

- algorithms and composition;
- parsing, validation, bounds, and normalization;
- safe defaults and user-facing policy;
- effects and authority requirements;
- typed error meaning and diagnostics;
- data transformation and public API behavior.

A native provider may supply a typed, policy-free leaf. Such a leaf has a narrow
ABI, no user-facing default selection, no multi-step application semantics, and
no backend-specific interpretation of a Jet error. It may expose an operating
system service, an unsafe platform operation, a vetted cryptographic primitive,
a database or network transport, a font shaper, or a hardware interface.

The surrounding Jet function remains the semantic boundary. For example,
`core.files` keeps path composition, walking, limits, and error mapping in Jet
while its typed provider performs the OS file operation. `core.http` keeps
method, header, framing, limits, and deterministic convenience behavior in Jet
while the host supplies a typed transport leaf. `core.crypto` keeps typed
values and fail-closed policy in Jet; the OS CSPRNG is a host cell. The raw
algorithm controls in `core.crypto.expert` remain an explicit audited door, not
a hidden provider policy.

Private service delivery/mailbox reader results, CSV bounded-read results,
crypto physical records (`__JetcRaw` and `__JetcStat`), and testing transport
records take their checked shapes from canonical Core source declarations.
They must not also appear in compiler-owned record inventories. Root-Prelude
carrier mappings remain ABI spellings, not owners of those checked fields.

Core runtime source contracts use the ratified ownership and failure spellings:

- SQL projection `field()` returns an owned `String?`; field traversal borrows
  projection payloads directly rather than allocating names for its callback.
  There is no live-reference `@String` return contract.
- The Path `walk()` convenience wrapper supplies `None` for the canonical
  filesystem walk's optional ignore-file argument, preserving an unfiltered
  traversal rather than inventing a second walk policy.
- Base32 and Base64 error formatters implement the plain `String` result
  required by `Display` and `Debug`; a `Never!` result is not the trait hook's
  declared carrier. Their reason-only output and receiver ownership are unchanged.
- Expiring-secret storage acquires its edit guard in the removal helper, moves
  the payload from that owned local, restores `None`, then drops the payload.
  Close consumes its own receiver's storage field directly, preserving the
  destructor's field-take authority, and live callbacks retain a read guard.
  Callbacks are non-failing (`Never!`); expiry uses the canonical root `Expired`
  failure carrier, not a second `core.time` nominal.
  Shared handle operations never copy credential material.
- XML CR normalization encodes U+000D through the canonical Unicode scalar
  encoder, since plain Jet text has no carriage-return escape. Code-point
  encoding at the byte-token boundary handles failure as `InvalidEncoding`.
  Public `Float` and `Bytes` patterns reject non-XML values. Private `Number`
  carriers fall through the invalid-carrier arm without borrowing their payload
  outside the stream owner.
- `FileCryptoError.SealFailed` is a unit variant, like the other file-error
  cases, under D-CRYPTO-ERRFAM1. Core maps primitive seal failures to that case
  rather than attaching a retired crypto-error payload.
- `Clock.equal` is pure: manual clocks compare stored millisecond values;
  system clocks compare stored start and offset values. Comparing clocks never
  samples the host clock or invokes a time wait.

The bootstrap Rust assembler embeds the SQL and bounded CSV-read kernels into
the generated source; it does not retain paths relative to the Prelude tree.
Its private String/byte carrier helpers come solely from `Core/ForeignBytes.rs`,
including the borrowed-slice and iterator packing paths.
Nested bootstrap host kernels explicitly import the generated Foundation facade;
their physical C-ABI headers are projected from the same generated carrier
declarations as the native-backend adapters, without importing optional wrappers.
File-handle kernels have one owner in the CommonTypes dependency closure;
selecting mapped-file operations must not emit a second copy.

Bootstrap Dyn operations use the helpers embedded in `jet_std`; generated
calls retain that namespace and the helpers' checked text-borrow contracts.
A callback's declared parameter ABI must agree with both its generated closure
and the callable body, including borrowed String parameters.
Concrete read String formals lend `&str`, so literal text and views need no
owner allocation. Abstract `&T` formals retain their String owner; both compiler
emitters distinguish these boundaries before selecting a literal borrow.
Default `Err` construction retains every source-declared field and its checked
default, including the private memoized error metadata. The native projection
must not reconstruct a shortened error record from only message, code, and cause.

Native record-sequence provider symbols use `jet_rt_record_seq_` followed by
the checked sequence kind with dots replaced by underscores. The lowering
computes that suffix before interpolating the symbol, preserving the same
typed carrier name without escaped string literals inside interpolation.

`core.email` decodes DKIM key material into a fixed, zero-filled Secret buffer.
It validates every encoded quartet before applying the final 32-byte length
rule; a late alphabet or padding error still takes precedence over length.
Borrowed Secret bytes may be read or written by index inside audited Core
code, but their storage must never grow, shrink, or be freed through ordinary
List operations without the Secret owner's zeroization.

The `core.http.client` transport entries `get`, `post`, and `request` remain
native rows. String-only source forwarders narrow the existing typed-URL
contract and change the transport result's checked failure carrier; they are
not a source-owned transport implementation. Session configuration and policy
continue to use the Jet source around those same native rows.
Typed URLs are rendered through their registered source `to_string` method
before entering the transport's String ABI; both semantic checkers preserve
the source nominal identity while checking that crossing.

`core.math` keeps IEEE classification, adjacent-value and exponent composition,
rounding/conversion policy, remainder validation, distance accumulation, and
special-function approximations in Jet. Its numeric host boundary is limited
to primitive bit casts and libm operations that Jet cannot yet express through
an intrinsic or an external call. A classification or composite operation is
not a native leaf merely because Rust exposes it as a scalar method.

The caller-owned RNG stream uses SplitMix64, distinct from the ambient
generator. Its Jet value kernels retain explicit wrapping U64 transitions and
the complete U128 product for Lemire bounded rejection; modulo-only selection
is not equivalent. A zero word-width bound denotes the full signed-64 domain.
Equal endpoints and rejected backwards intervals do not advance the state.

`core.net.url.URL` is a source-owned record. Typed `URL{"…"}` heads validate
their literal skeleton in sema, then call `core.net.url.literal` as an ordinary
Jet function. That function applies the validator's Unicode-whitespace boundary
through `core.text.trim`, parses the skeleton once, and substitutes holes inside
components without trimming hole values; interpolated slashes, authority
delimiters, and dot segments never become URL grammar. All backends compile the
same source constructor, accessors, normalization, and rendering rather than
projecting a native URL carrier or concatenating a backend-local URL string.

`core.perf` owns unit-interval validation, rejected-value errors, the `1.0`
default, reset semantics, and f32 rounding in Jet. The provider only loads and
stores one zero-initialized atomic word. Jet encodes a stored IEEE value as its
signed f64 bit pattern plus one, reserving zero for the default; this preserves
both signed zeros without assigning policy to the native cell.
Both cell leaves carry `Env` and use the ambient runtime route, so a source
read cannot be folded into the default while compiling the program. Prelude
`Perf.*` calls load the same source module as an explicit `core.perf` import.

`core.game.raylib` sound handles are Jet records retaining the supplied path.
`play_sound` preserves the existing non-empty-path acceptance contract, not a
claim of native audio submission; this pair requires no host operation.
The `color` constructor also runs entirely in Jet: each channel is clamped to
`0..255` before the existing drawing carrier is constructed. No native or
compile-time color constructor duplicates that normalization.

`core.web.virtual` windows are Jet records. Window construction, measured-size
estimation, bounded slicing and indices, and `WebVirtualWindow.facts_json` use
the source implementation rather than a native window carrier. The facts
method emits all nine stored fields in declaration order without serializing
row payloads or recomputing layout.

A high-level native implementation must not be retained as an invisible runtime
fallback for a Jet implementation. If an API changes identity or semantics,
make the change at the contract boundary and migrate its users in one cutover.

Prepared foreign bridges that depend on Foundation expose its complete owned
source closure to the native build sandbox. This includes the `jet-unicode`
path dependency at its canonical Cargo location. External inputs are discovered
transitively from Rust `include!`, `include_str!`, and `include_bytes!` expressions,
including literal `concat!` paths rooted at `CARGO_MANIFEST_DIR`; comments and
string contents are not dependencies. Cargo-generated `OUT_DIR` inputs remain
inside the build target. Missing includes or includes escaping the repository
fail the preparation. The bridge identity fingerprints both crate trees and
the sorted include closure, so changing or adding an included registry or
Unicode table invalidates a previously prepared bridge.
The Jet bridge and bootstrap Rust bridge derive the same closure rather than
maintaining separate input lists. Linux projects only these external files at
their original source root, then mounts the complete Foundation and Unicode
crates; this does not authorize unrelated directories.
External build-script hashing inputs are declared with `include_bytes!`, so
the same closure includes their bytes before Cargo compiles or runs the script.
Owned-crate source walks and generated-view reads remain inside their complete
crate mounts; registry build inputs remain inside the admitted registry tree.
Relocated runtime module text splices its canonical `include!` inputs into the
generated source instead of resolving them beside the new bridge crate. The
bridge identity fingerprints those spliced bytes as well as the root module.
Embedded carrier references resolve to their actual bridge-owned module. Entry
error formatting retains the shared Foundation Registry lookup, bound to that
dependency rather than assuming a bridge-owned compiler Registry.
Inline assembly keeps early clobbers on every host profile. A word-input push
rail that exceeds the allocatable register budget marshals its inputs through
one read-only stack carrier, consumed before the body can overwrite its
address; in-budget rails retain register operands.
Returned immutable views name the lifetime of their underlying data, not the
temporary reference slot used to pass the view. Reusing a loop-local view slot
must not invalidate a returned section that still borrows the original bytes.
Shared references to owning values and exclusive view carriers retain their
existing provenance rules. Runtime-owned CORS origin variants implement the
same structural Show/Debug hooks as source-derived enums, including when an
origin variant is the payload of a generated union.
The Prelude and resident HTTP hosts marshal string-list payloads through the
same Foundation list-formatting kernel; a host adapter does not choose a
different container shape. The single-file Prelude flattens that kernel into
its own module; only separate resident collection modules import it.
Lowering helpers that mutate their builder read retained plain metadata from
an explicit owned `.copy()` snapshot, not a row still borrowed through that
builder. This does not copy trait adapters or widen a value's Copy capability.
The snapshot is bound with `:=`; `::` remains a view even when its initializer
spells `.copy()`. Enum definitions and selected implementation metadata follow
the same rule while their emitter context is being updated.
Hosted bootstrap callback carriers preserve their capture lifetime rather than
implicitly requiring `'static`. A single-use borrowed callback is constructed
at its consumer; other borrow-only callbacks, iteration cursors, and reference
bindings live in their defining lexical scope. Owned handles and shared-capture
cells retain their existing release rules. Disjoint fields behind an exclusive
guard project from one reborrow of its referent, not repeated guard dereferences.
Map-store callbacks capture their owned payload, not the reusable MIR slot.
An owned callback return does not introduce a caller-supplied capture lifetime
when no callback-bearing input supplies one. Stored callback handles retain
their owning lifetime; forwarding a borrowed input retains its capture lifetime.
Function and trait signatures apply the same rule without wrapping or copying
the callback environment.
Nested callable results apply that rule independently of their outer callable's
capture lifetime; argument elision must not turn an owned callback into a view
of its reader argument. Reference storage initialized by an iteration's `None`
reset has that iteration's scope, with the same indentation as its instructions.
Before a checked write changes an owner, hosted emission retires its dead
in-place payload, cursor, and nested-field reference storage, deepest loan
first. This includes mutable parameter places, preserving their projections
so a disjoint-field write does not retire a still-live view.

Native lowering must fail closed when a construct cannot preserve the shared
runtime's ABI, error behavior, or observable output. Report an unsupported
construct before producing an executable; do not marshal an unproved carrier
or run a backend-local approximation. A focused same-program regression must
exercise either the shared meaning or that explicit refusal.

Pattern adapters preserve typed captures, binary bit widths and byte order, and
advance a cursor or reader only according to the canonical scan's success and
failure contract. A sequence of scalar reader calls is not a replacement for a
pattern scan. When that scan/capture boundary is unavailable, lowering refuses
the semantic operation rather than emitting a partial parser.

Module scalar storage has one local data cell per checked declaration. Global
reads and writes through static-rooted places address that same cell, including
when the shared object writer packages a runtime library. Atomic values still
require their typed allocation, ordering and ownership contract; a raw machine
compare-and-swap primitive does not establish that contract.

The runtime allocator distinguishes an invalid layout from exhausted storage.
Invalid layouts retain the canonical Jet panic diagnostic. Failure to allocate
a valid nonzero layout writes exactly `memory allocation of N bytes failed\n`
to stderr, with `N` rendered as unsigned decimal, then aborts the process
(SIGABRT on the hosted native target). This fatal path uses stack storage and
the shared raw-byte terminal writer: it neither allocates a diagnostic carrier
nor invokes a Rust panic hook, task-stop publication, or backtrace rendering.
The Rust-tier seed adapter preserves the same bytes and termination as the
Jet-owned allocator; it does not inherit host-toolchain allocation-error policy.

Runtime C routes use canonical `Float` for a direct 64-bit floating argument or
result, matching the Rust `f64` register carrier. The validator does not admit
convenience spellings or a 32-bit floating carrier. Pointer and indirect output
slots remain `I64` or `U64` words even when their payload is a float; byte and
boolean carriers retain their separate checks. This is carrier validation, not
a numeric conversion.

`Collections.jet` owns the single runtime-wide `RT_HOST_U8_STRIDE` fact and
`host_u8_get`/`host_u8_put` accessors. Later host layers reuse them; changing
the compiler's U8 storage epoch flips this one fact with its byte-list fixtures.

Native root Shared guards retain the existing cell and hold its canonical
reader/writer permit for the entire consumer scope. Guard payload addresses
borrow that cell's descriptor-owned storage; acquisition does not copy it or
allocate a guard token. Concurrent readers coexist, writers exclude readers,
and waiting writers precede new readers. Scope cleanup releases the permit
before dropping its retained cell owner, so final payload cleanup runs outside
the lock. An edit guard commits one revision on drop even without a write or
on an already-delivered stop rail. Generation exhaustion also releases both
permit and owner before delivering its stop.

Native `Shared<T>` construction supplies `(payload slot, NativeMeta descriptor,
monomorphized payload-drop function)` to `jet_rt_native_shared_new`. Its 96-byte
cell stores the function pointer at byte 80; the thunk borrows the owned slot
address and returns `Unit`. The packed reference word at byte 0 holds logical
owners in its low 32 bits and physical pins in its high 32 bits; each half is
bounded by `2^31 - 1`, a deliberately tighter native limit than Rust Arc's
`isize::MAX` refcount-abort bound. Exhausting either packed half stops through
the existing overflow door rather than spilling into the other half. Payload
retirement requires zero logical owners and zero lifetime physical borrows at
byte 72, including suspended physical guards.
Retirement clears the payload and descriptor under the cell mutex, then invokes
the thunk exactly once outside all Shared locks. The glue performs complete
typed destruction, including `T.Close` and owned fields. The provider must not
also invoke the descriptor's drop callback. A null function is reserved for
trivial payloads and means plain free, not a descriptor-drop fallback.

Weak references use the appended count at byte 88; the drop-code pointer stays
at byte 80 and every other existing field offset is unchanged. All logical
owners and physical pins collectively hold one implicit weak reservation.
Downgrade reserves a weak without retaining the payload; upgrade atomically
reserves a logical owner only while the low reference half is nonzero, writing
its out slot only on success. Physical pins alone cannot resurrect an expired
logical owner. The implicit weak retires when the entire packed reference word
reaches zero; final weak release alone frees the cell. Payload destruction may
precede that transition when only passive pins remain. Temporary weak ownership
keeps the cell live across out-of-lock drop-glue reentry and concurrent final
pin release. The existing Linux locked `sync_cas`/`sync_add` operations supply
acquire/release fences.

The Source physical-owner API reuses native weak identity without retaining a
factory context. Passive pins keep the cell allocation alive, while physical
Guard40 leases retain a pin and a lifetime borrow across suspension. The
thread-confined header records cell, editability, held permit, dirty state and
shared projection references. Clean physical release does not bump revision;
dirty publication bumps once under the original edit permit. Abort releases
without committing. Reacquisition checks cancellation/deadlines and refreshes
the borrowed canonical slot only after obtaining that same permit. Final guard
drop releases the protocol before payload cleanup. Ordinary native guards
remain logical owners and eager-dirty, preserving their existing ABI.
Alias24 receipts reserve one logical owner and a monotonic token from the
existing publication header; consuming their owned slot clears it before the
one release. The publication header is 16 bytes (node head at 0, alias counter
at 8), with no new global or sidecar cell registry.

The existing publication fixture assembly also supports `link-weak`/`run-weak`.
Its C client checks failed upgrade leaves the out slot unchanged, exactly-once
payload destruction, the counting allocator's remaining 96-byte cell until the
last weak drops, and a two-thread upgrade/final-strong-drop race.
The same client distinguishes logical owners from physical pins, verifies
clean/dirty/abort revision behavior and alias-slot exact-once release, and
checks failed upgrade while a physical guard still retains the live payload.
The existing sync fixture exercises physical wait cancellation: registration
occurs under the original edit permit, dirty publication releases it, cleanup
removes the same condition node before reacquisition, and interrupted waiting
returns an unheld guard with no lingering registration.

Like Rust's `Arc<T>` releasing its final strong owner, the callback runs in the
releasing caller's context. The provider adds neither a cleanup mask nor a
session shield; releases inside a MIR cleanup frame inherit that frame's
bracket. A callback that returns with pending stop still permits the provider
to free the payload allocation and descriptor and release the strong owners'
implicit weak reservation before the caller propagates it. Reclamation on a
pending stop is explicit, not unwind-dependent.
The seed-only C fixture retains its separate
two-argument Rust-provider declaration for comparison; it is not a native alias
and retires at cutover. Native runtime fixtures pass explicit drop functions,
including two threads each racing 100,000 clone/drop pairs and a null-glue
case that must not fall back to the descriptor callback.

The Shared site publication witness uses the actual native providers, not a
Rust seed implementation. `run-channel-fixtures.mjs OUT assemble-publication`
enrolls the registry observer after the production runtime; the existing
`lower` mode emits the fixture object, and `link-publication`/`run-publication`
link and exercise it with the freshly composed native library. Two initializers
race to publish one winner; the losing payload drop runs once. World reset
clears the cell and releases only its owner while a retained clone survives;
republishing the same site must retain exactly one registry node. Process
sites must never enter the World registry or be released by its reset.
The observer only counts actual registry nodes after publishers have joined;
the C client supplies callbacks and barriers, never publication or reset logic.

### Exact integer carrier

An exact `Int` crosses the scalar ABI in one 64-bit word. Values whose two
highest bits agree are their signed inline payload (`-2^62..2^62-1`).
A boxed word has bit 62 set, bit 63 clear, and its remaining bits hold an
8-byte-aligned pointer. The immutable C-layout node is 48 bytes: atomic owner
count at byte 0, retired-list link at 8, sign word (`0` or `1`) at 16, limb
pointer at 24, limb count at 32, and limb capacity at 40. Limbs are 32-bit
base-1,000,000,000 digits, least-significant first. Zero is nonnegative;
arithmetic normalizes leading zero limbs and returns to the inline form when
possible. No Rust `Vec` layout is part of this ABI.

Native task workers run a callback to completion on an OS thread. Cooperative
waits park that thread; bounded compensation supplies progress for blocked
workers. Cancellation requests deliver at wait points, while the native Check
rail delivers explicit session interrupts. Post-call propagation observes only
an already-delivered stop, so a new request cannot invalidate an initialized
result. Shielded cleanup defers delivery until its outermost boundary exits.
Context deadlines save and restore the absolute millisecond budget in a
separate caller-owned 16-byte slot on every normal and stopped cleanup edge.
Scheduler state, task controls, timers, completion frames, lexical groups and
all/any/race selection are Jet-owned runtime carriers. A host entering a
control preserves the native task-body marker: root/session panics remain
process-fatal, while task-body panics become owned `TaskFailure.Panicked`
outcomes and do not print stderr. Flattened body errors participate in
completion-order selection, including heterogeneous tuple children.
Task panic producers publish through the existing stop rail and return; private
callers observe the delivered status before reading results or performing further
effects, and discard only their owned partial results. The outcome carries the
raw program reason, not the rendered diagnostic or location. Root reports retain
their registered bytes and exit status. A panic during an already-delivered
stop's cleanup aborts instead of replacing that outcome; a shielded pending
cancel or deadline is not yet delivered and cannot suppress a task panic.
Exact-Int arithmetic and native narrowing use that same publication path.
Runtime marshaling checks each narrowing before subsequent syscalls, allocations,
or output-slot initialization; moved constructor children and select inputs are
released on the stopped path, while borrowed inputs remain caller-owned.
Compiler runtime-call stop facts are generated from the direct named-call closure
of the runtime's panic publication entry, including private helpers. The
projection rejects generic declarations/calls and computed or bound-value calls;
foreign/member calls and address-call transports are not semantic callback edges.
Callback stop analysis remains the compiler's responsibility. Both compiler
halves consume `Registry/RuntimeStopExports.jet`; the runtime generator freshness
gate checks this projection instead of maintaining a parallel symbol list.
Core `Task` handles are move-only: one consuming joiner uses the completion
park slot, while lexical cleanup drains the retained state with a wake-all
completion word. Rejected bounded-group spawns return their admission permit.
An exiting thread drops unrun `atexit` closures without invoking their bodies,
matching the reference thread-local queue destructor.
`Compiler/JetRuntime/Tests/run-scheduler-fixtures.mjs` compiles the actual
runtime with Jet callback assertions and uses C only to transport callback
addresses. Its run phase covers one-, two- and eight-worker pools; root panic
is compared byte-for-byte to a supplied freshly rendered Rust-tier reference.
The shared native affinity probe `scheduler_available_parallelism(fallback)`
does not read the pool override: the worker pool retains its override and
fallback 4, while joined parallel walking supplies fallback 1.

Native resident-session reservations use the same heap as ordinary native
storage, but return a null place for an invalid layout or allocation failure.
A failed slab mapping releases the heap lock without publishing slab metadata;
a failed direct mapping never publishes a payload or its allocation header.
The ordinary allocation door still delivers its existing fatal layout/OOM
diagnostic. Successful reservations retain their original size and alignment
for the shared exact-layout free operation, including aligned zero-size places.

`jet_rt_alloc_try(size: I64, align: I64) -> I64` is the shared native fallible
door: zero means invalid layout, injected allocation failure, selected-program
allocator refusal, or heap failure, without a Stop or stderr. The allocation
fault channel observes before reservation; a refused reservation changes no
accounting; a heap failure cancels its live reservation before returning.
Ordinary `jet_rt_alloc` uses the same policy reserve and exact-layout free, but
retains its fatal layout/OOM edge and does not observe the fallible fault rail.
System-mode blocks keep the two-word heap header. Only counted payloads own
private bypass-policy side-table nodes; freeing a tracked payload releases its
reservation even after switching back to system mode. Policy facts retain
historical allocation/request/high-water counters on cancelled heap requests,
while live bytes and tracker nodes return to zero.

The explicit native host policy edges are `jet_rt_program_allocator_configure`,
`jet_rt_program_allocator_restore`, and `jet_rt_program_allocator_fact`
(allocations, requested, live, high-water, cap, mode at indices 0..5). Hosts
serialize run configuration, preserve mode and cap, and use the distinct
restore edge to retain historical counters even after live bytes reach zero.
Configure resets those counters only when no counted bytes remain live.
These words carry unsigned source `usize` bits. The
allocation schedule uses dynamic TLS, not a new task-control slot; its arming,
fact and clearing edges implement the allocation channel only, not an effect
path or the full fault-plan loop. `AllocationPolicyTests.jet` is included in
the composed World/Sentry fixture and requires empty output, no pending Stop,
fault/counting-cap refusal, cancellation after a real refused mmap, policy-
switch-safe free, and zero live bytes/tracker nodes.

The native runtime allocates nodes with `(size=48, align=8)` and limb buffers
with `(size=capacity*4, align=4)` through the shared Jet allocator. A zero
capacity uses the dangling limb pointer `4`, which is never freed. Borrowing
arithmetic and conversion exports leave their input owners intact; arithmetic
results and clones return owners. `jet_rt_int_drop` atomically consumes one
owner. At zero it calls the explicitly registered memory-only
`jet_rt_int_retire_zero` leaf: the existing shared hazard domain frees
unhazarded nodes immediately, or links hazarded nodes through byte 8 and
reclaims them when the last read guard clears. Atomic slots remove their owner
before dropping it, and stale readers must revalidate the slot under a hazard.
A second retirement list would be unsafe while Rust atomic consumers remain.
The hazard-domain leaf dies when that domain ports to Jet. Native lowering
must pair every owned boxed carrier with drop and copying with retain;
the runtime export alone does not establish that compiler ownership glue.
Private unpublished arithmetic workspaces use their unique-owner discard.
Rust-tier seed/verification transport uses the identical C layout, with its
allocator paired at construction and release; that registered transport dies
at the Rust cutover, rather than defining a second numeric representation.
`Atomic<Int>` (`AtomicInt.jet`) owns one exact publication per cell; `add`
returns the independently owned previous exact value. Compare-exchange uses
numeric equality while retaining the observed node through publication, not
pointer equality. Fallible add leaves the cell unchanged on refusal and reports
the physical failed native request in `AllocError.requested_bytes` with
allocator `"exact-int"`; those byte counts reflect native node/limb layout, not
Rust temporary-vector layout. An inline fast path allocates nothing and takes
no allocation fault probe. A spilling operation probes once even if its CAS
retries, then separately allocates a node and limbs; a limb-capacity refusal
frees the already allocated node.


Native iterator operations preserve the delivered stop rail. A stopped callback
does not initialize its result slot. Borrowed pulls and cursor advances leave
their input owner for caller cleanup; owning eager operations and cursor
initialization discard every partial result before returning a neutral, unread
value. Collection outputs in caller-provided storage are dropped and zeroed,
not deallocated. Join checks the rail before using a show callback's text, and
numeric terminals abandon partial reductions after a stopped drain.

Channel receiver iterators likewise own their receiver and receive its element
descriptor at construction, before any output slot is allocated. A blocking
pull writes directly into that slot; closed and stopped pulls leave it
unwritten. Dropping the iterator releases the receiver exactly once.

Channel's Observe imports retain their explicit C symbol bindings, and Shared's
process-pinned publication registry remains a mutable root initialized at
runtime. Until formatter issue #4770 is fixed, diff-scoped formatting receipts
preserve those declarations rather than accepting empty import bodies or
rewriting the registry as an immutable binding.

At native C byte boundaries, `List<U8>` retains its word-stride layout.
`jet_rt_bytes_pack` borrows that header and returns an independently owned
packed byte buffer; `jet_rt_bytes_free` releases it with its original length
and alignment one. Empty packed bytes use the non-null dangling pointer `1`
and require no deallocation. `jet_rt_bytes_unpack` copies C bytes into a fresh
owned native list header and packed byte buffer allocated at alignment eight;
the empty header has all three words zero. Native list destruction releases
`capacity` bytes at that same alignment. Neither conversion transfers or frees
its borrowed input.
`Collections.jet` owns the single runtime-wide `RT_HOST_U8_STRIDE` fact and
`host_u8_get`/`host_u8_put` accessors. Later host layers reuse them; changing
the compiler's U8 storage epoch flips this one fact with its byte-list fixtures.

Native sentry scopes and unsafe function gates save the canonical per-thread
gate/fence state in caller-owned frame storage. Normal and stopped cleanup
edges restore that state exactly once, including nested policy scopes; file
and reason strings remain borrowed module literals at the ABI boundary. The
temporary Rust sentry marshal seam retires with the Jet sentry/TLS layer.

The seed `IOContext`, `IOError`, and `TaskFailure` record adapters use the same
reviewed C headers as their native records, not Rust `Box` or enum layouts:
`IOContext` is 40 bytes and `TaskFailure` is 16 bytes (`tag`, owned nullable
reason). Their generator and Rust projection retain the paired allocation,
borrow, and release helpers.

A `T? Never!` call has no outer failure alternative. Outcome matching consumes
the checked empty-error carrier once and matches the optional success value;
it must not treat that carrier as a fallible subject for `Val`/`None` patterns.

## One meaning across execution tiers

AOT, interpreter/comptime evaluation, and web execution are
points on one semantic continuum. Preparation and runtime cost may differ, but
the following observable contract does not:

- validation and bounds;
- defaults and policy;
- effects and authority;
- typed errors and diagnostics;
- resource limits and cancellation;
- deterministic behavior and observable values.

Every applicable tier consumes one semantic Core call record and one provider
contract. The tier adapter supplies representation and execution mechanics; it
does not reimplement Core policy. The shared call record names the signature,
fallibility, effect, capability, provider route, pure/interpreter route, and
AOT adapter facts so that a tier cannot quietly invent a second meaning.

The compiler host's internal Source resource slot substrate keeps its owned
payload in the same slot as its checked identity and semantic loan state.
Destructive slot transfer validates scope, generation, kind, checked type and
active loans before mutation, moves the whole owned slot once, and preserves the
consumed capability receipt. Requesting retirement rejects new registrations and
new retained roots, while existing retained owners remain usable until canonical
finalization. Logical slot, loan and retained-root tickets are data, not managed
external leases or executing-image authority. Native payloads require the actual
registered managed owner, source/image/code holds, sole initialization cell, Meta
lifecycle and final-alias/weak-backedge providers; the logical substrate alone
does not establish native cleanup or executor availability.

A portable fallback is valid only when it preserves the same contract. A web or
test provider is explicit when it changes the host resource, and its typed
availability or policy error remains visible to the caller. Cross-tier evidence
compares semantic values and typed errors, not merely whether each adapter
returned.

## Provider classification and policy

The canonical call record classifies each host-sensitive operation with one
capability kind:

| Kind | Contract |
| --- | --- |
| `Pure` | The operation needs no ambient provider, fallback, or capability failure. |
| `NativeRequired` | A named provider is required; no semantic fallback is valid, and absence has a typed failure. |
| `PortableFallback` | A named provider has a canonical portable implementation and an optional native acceleration; the failure identity is shared. |
| `ExplicitSimulation` | A named virtual or test provider is selected explicitly; it is not an ambient production substitution. |

The `CoreCapabilityContract` records the provider, fallback identity when one
exists, and typed failure identity. `Effect` and its precise effect leaf remain
the authority for effect checking; capability metadata does not create a second
effect vocabulary or permission system.

Safe defaults and an `auto` selector may choose a semantics-preserving portable
fallback. An explicit provider request never silently downgrades. When a
required capability is missing, every tier returns the same typed,
deterministic capability error.

Examples establish the intended distinction:

- `core.compute` uses its checked CPU oracle for `auto`; an explicit CUDA,
  Metal, Vulkan, or WebGPU request reports a typed device or unsupported error
  rather than silently using the CPU.
- `core.font.shape` requests the font-shaping provider, while
  `core.font.shape_with` is an explicit deterministic approximate fallback.
- `core.crypto.random` uses the operating-system CSPRNG. A missing or rejected
  provider returns `CryptoError.Unavailable`; it never falls back to a weak or
  deterministic generator.
- Virtual filesystems, fake clocks, in-memory databases, and test networks are
  named providers selected by the caller or test tool, not hidden production
  substitutions.

Effects and authority are the user-visible access boundary. Provider objects
and capability metadata exist for explicit expert control and provider
machinery, not as a second permission system.

## API identity and migration

A first comparison pass preserves valid semantics and a coherent identity closely
enough for direct semantic comparison. It does not preserve a misleading API
shape merely because another language has one.

When an export is incoherent, replace it at the contract boundary. Migrate every
caller, example, test, registry row, generated consumer, document, and affected
environment variable in one clean cutover. Delete runtime compatibility
wrappers, aliases, parallel parsers, duplicate implementations, and hidden
fallback paths.

A later design pass may improve names, types, defaults, diagnostics, algorithms,
allocation, or performance when the new Jet contract has proof. Material public
choices are owner-gated before implementation. Peer languages supply semantic
and edge-case evidence; Jet still uses its own types, effects, authority,
diagnostics, and ergonomic surface. Common operations have low-friction safe
defaults, while expert control is explicit rather than hidden.

## Contract and evidence

Before code, give every module or coherent module cluster a contract record that
freezes:

- public API and types;
- semantics, bounds, and edge cases;
- effects and authority;
- capability kind and fallback policy;
- error and diagnostic codes;
- peer references and valid baseline receipts;
- examples and goldens;
- hostile and failure cases;
- the applicable tier matrix;
- frequency and hot-path cells;
- explicit non-goals.

Every export receives evidence scaled to its role but complete for its declared
surface. Cover normal behavior, boundaries, hostile inputs, typed errors,
diagnostics, authority and effects, resource limits, determinism, concurrency or
reentrancy where relevant, and cross-tier parity.

Use semantic fixtures for first-pass comparison. A coherent surface may compare
directly with valid Rust behavior. A redesigned surface maps valid Rust inputs
to the new Jet contract and compares semantic values and errors. Do not keep a
runtime compatibility layer merely to make that comparison.

TLS root-certificate and trust data use native carriers independently of the
TLS transport engine. `TLSCarriers.jet` transfers consumed list/root headers
without copying their buffers; cloning creates independent byte storage using
the canonical host U8 stride. The root PEM field is a borrowed list header,
whereas a trust-variant projection returns an owned root clone. System trust
has no active root buffer. These ownership operations do not perform certificate
validation or a TLS handshake. `TLSRootsWitness.c` pins their ownership and
wrong-variant contracts against the composed Jet object.
Certificate and peer snapshots also use independent native data storage: cloning
recurses through byte lists, DNS text lists, and the certificate chain without
duplicating an inline record's header. The chain stores inline certificates;
growth moves their owned children, and a sequence projection borrows its slot.
Certificate subject/issuer and peer name/cipher projections lend caller-owned
E4 descriptors, which must not be freed, grown, or consumed. Signed validity
timestamps remain raw i64 facts. `TLSCertificateWitness.c` exercises deep clones,
parent lifetimes, list growth, and these borrowed projections.

Native client identity owns its inline PEM byte-list header and a native
Secret handle. Cloning copies only initialized U8 elements into independent
buffers; dropping delegates private-key erasure and release to the canonical
Secret provider, including its entire capacity and host U8 stride. Client
configuration owns inline trust, optional identity, raw protocol-version tags,
and an ALPN text list. Its trust projection borrows the parent's inline field.
`TLSClientWitness.c` checks independence, optional identity, all trust variants,
ALPN ownership, null drops, and private-key capacity erasure before deallocation.
This data-provider contract does not claim TLS engine or certificate validation.

`DataSequences.jet` owns the shared native List header constants
`DATA_SEQUENCE_BYTES`, `DATA_SEQUENCE_LENGTH`, `DATA_SEQUENCE_CAPACITY`, and
`DATA_SEQUENCE_DATA`, plus `data_sequence_new`, `data_sequence_slot`, and
`data_sequence_reserve`. Dyn and TLS certificate sequences use those
unchanged helpers. TLS byte-list children use `data_u8_list_copy_into` and
`data_u8_list_drop_fields` from the same provider; their old private TLS
definitions are removed. The shared sequence index and capacity failure
providers still require composition before native caller admission.

Native mail attachment data uses `MailAttachment.jet`'s 40-byte carrier:
two owned E4 text pointers followed by an inline native U8 list header.
Construction clones borrowed filename/MIME text and transfers the consumed
byte-list buffer without copying it, releasing only the donor list shell.
Text projections return independent owned clones; the bytes projection borrows
the parent's inline header. Cloning uses the shared `DataSequences.jet` U8
ownership helpers and the packed `RT_HOST_U8_STRIDE`; equality compares text
and initialized bytes, ignoring capacity. Null drops are harmless. The
`AttachmentWitness.c` composed-object client pins binary text/bytes, ownership,
capacity-sized frees, mutation independence, and projection lifetimes. These
data operations make no claim about SMTP transport or MIME serialization.
`attachment_copy_into` and `attachment_drop_fields` in that same provider
single-source field ownership for both heap handles and inline sequence slots;
callers own their respective shells, so inline children never allocate a
temporary attachment shell or free their enclosing sequence slot.

Native network readiness snapshots use two normalized Bool words in
`NetReadiness.jet`. Getters, clone and equality interpret the low byte of each
slot, not unrelated upper-byte padding; constructors normalize C U8 inputs.
Ready-interest and shutdown handles use the canonical scalar-enum ownership
helpers from `EncodingErrors.jet` (declaration order Read/Write/ReadWrite and
Read/Write/Both respectively). They do not own socket resources. Null drops
release nothing. `NetReadinessWitness.c` covers all boolean/ordinal combinations,
low-byte normalization, independent clones and exact-sized shell deallocation.

The native DB connection handle is a move-only 8-byte backend ID wrapper.
As in the seed's `HandlesRaylib.rs` definition, it has no implicit close action;
connection close remains explicit. `DbConnectionHandle.jet` releases only the
wrapper allocation and ignores null handles. `DbConnectionHandleWitness.c`
pins that shell-only deallocation for zero, ordinary and maximal u64 IDs;
it does not claim admission of a database transport engine.

The held native TLS stream handle is 112 bytes: ID, owned canonical FD72,
optional timeout facts, seven borrowed callback pointers and one nullable
owned peer snapshot. The seventh callback is the seed's `peer_snapshot`,
distinct from its optional `peer_identity` owner. Drop releases the descriptor
through `HostFiles.jet`'s canonical `jet_fd_drop_raw` (dependency `047fb460b`),
then the peer snapshot in field declaration order, then the stream shell.
It never invokes a TLS-close or other borrowed callback, matching the absence
of a custom seed destructor. Show is exactly `TLSStream(<signed ID>)`.
`TLSStreamHandleWitness.c` checks real descriptor closure before snapshot
release, optional ownership and untouched callbacks. This does not claim a
native TLS handshake engine or transport callback admission.

The held native DB scope is 48 bytes: a raw backend ID, inline canonical
24-byte row policy, owned user text and a nullable request-ID text owner.
`RowPolicyHandles.jet` single-sources `row_policy_copy_into` and
`row_policy_drop_fields`; both standalone row-policy handles and inline
scope fields use them without temporary owner shells. Clone preserves the
compiled policy and raw ID while independently cloning every text child.
Drop releases the inline policy fields, user and optional request text, then
the scope shell; it does not close the backend ID. Policy projection borrows
the inline field, and user projection fills a caller-owned read/clone-only
E4 descriptor. `DbScopeHandleWitness.c` checks those lifetimes, both compiled
policies, absent versus present-empty requests and exact-once destruction.

The native delivery handle owns three independent text children in a 40-byte
carrier, plus normalized duplicate Bool and raw signed generation facts.
Construction borrows/clones text and range-converts borrowed exact Int before
allocation; text getters return owned clones and generation returns a fresh
exact Int. Clone and equality interpret the duplicate slot's low byte.
Show preserves exactly `Delivery(` plus the raw ID bytes plus `)`, with no
debug escaping or additional fields. `DeliveryHandleWitness.c` covers all
equality fields, binary and empty text, independent projections, i64 edges
and exact-sized deallocation. Its `native-overflow` mode requires the shared
native-Int range stop (exit 70, empty stdout); process execution is separate
from parser and C syntax receipts.

The generated musl error-text provider exposes `host_errno_append` and
`host_errno_cause_append` for callers that already own a text builder.
`gen-runtime-host-errors.mjs` single-sources the same pinned errno ladder;
the existing owned `host_errno_text`/`host_errno_cause` entry points allocate
one builder and delegate. Composite renderers can therefore append exact
error text without allocating and copying an intermediate Text owner.

TCP stream field projections are borrowed Bool-slot addresses, not snapshots
or independent owners. `TCPStreamFields.jet` uses the held Host carrier:
closed projects the retained FD72's canonical closed slot, while read/write
shutdown project the parent's Stream56 slots. Their lifetime is the stream's
lifetime; logical close and shutdown remain visible through those aliases.
`TCPStreamFieldsWitness.c` uses real canonical owners and tests pointer
identity, low-byte reads, live shutdown/close updates and parent-only release.

Native mail messages use `MailMessage.jet`'s 160-byte owner: inline Address16,
two inline Address lists, three owned text pointers, an inline Attachment40
list, inline Envelope40 and a raw wire bound. `MailAddress.jet` and
`MailEnvelope.jet` own the canonical inline copying, destruction and equality
helpers; attachment lists reuse `MailAttachment.jet`'s existing field owners.
Clones allocate only initialized list elements, without temporary child or
list shells; destruction releases buffers by their actual capacities.
Equality includes every field and the wire bound, but not capacities.

The generator-derived `message_parts` ABI takes twelve raw arguments:
borrowed Address/Envelope handles, borrowed text descriptors, three borrowed
pointer-to-handle slot arrays with lengths, and a borrowed exact Int wire bound.
It clones every input owner and only performs the source-checked native range
conversion. Like the canonical C ABI list input adapter, null slot arrays or
nonpositive lengths mean an empty list. Source-owned validation, private views
and MIME serialization remain outside this carrier provider. Field projections
borrow inline Address/Envelope storage or fill read/clone-only text descriptors.
`MailMessageWitness.c` links the real composed object and covers bcc and
attachment stack arguments, binary and empty data, optional display states,
deep ownership, every equality field, exact wire edges, projection lifetimes
and capacity-sized frees. Runtime execution and route enrollment are separate
admission steps; the witness source alone does not establish execution parity.

Service workflow start, step, activity, retry and completion decisions live in
`Core/Service/Service.jet`. They retain the exclusive tree borrow across durable
synchronization, validation, replay checks and cursor updates. The existing
durable append and handle creation operations remain registered bootstrap leaves;
field leaves only project or assign the existing carrier and die at cutover.
Handle-only `activity_wait`, `all` and `sleep` remain native and uncredited:
their decisions inspect cursor/history/status under the handle's existing mutex,
so independently locked getters/setters would change the atomic region.

Delivery status and wait sample the existing durable read once. Receipt and
event history framing, state spelling, error ordering and HMAC domains live in
Jet; the read/cleanup provider keeps its original file-operation and authority
mutex critical sections. Retry validates first, constructs the native
zero-retention runtime in source, then invokes the existing runtime retry
operation without changing its locks. Endpoint receive retains the original
captured channel Arc and two registry acquisitions, rather than re-looking up
the mailbox after the gate check. Its conditional queue-depth read and receive
retain the native queue acquisitions. Private plain transport records are
bootstrap marshalling only, not new authority or delivery state.
Endpoint send and delivery cancel remain native and uncredited: their locks
cover source-dependent policy and the enqueue or durable mutation, respectively.

## Dependency order

Core work follows the dependency graph. These categories describe dependency
order, not a progress ledger:

1. **Foundations:** root carriers, text, collections, math, units, time,
   encoding, regex, args, files and path contracts, memory and I/O foundations,
   and portable cryptographic primitives.
2. **System:** archives, networking, process, tasks, events, watchers, terminal,
   logging, email, and database.
3. **Data and web:** data loaders and transformations, HTTP, web/router/forms,
   reactive, synchronization, services/jobs, storage, and browser.
4. **Specialized:** compute/models, UI/TUI/font, games/raylib, plugin/mod,
   compiler/reflect, testing, and remaining host domains.

Within a category, high-use foundations lead according to the dependency graph.
For each coherent area:

1. owner-gate material contract choices;
2. freeze valid baseline behavior;
3. implement Jet semantics in the approved portable Jet subset;
4. implement the shared provider and approved typed leaf seams;
5. prove each applicable tier and failure surface;
6. migrate all callers and generated consumers;
7. remove obsolete high-level implementations and placeholders;
8. close correctness and record a performance baseline;
9. optimize only after correctness closure;
10. rerun parity and strict performance cells.

## Performance and learnability

The performance gate is strict per matched workload and metric. No aggregate
score hides a loss. Hot paths and representative workloads carry peer cells for
latency, throughput, allocation, copying, startup, compilation, and artifact
size where applicable. No performance claim extends beyond measured coverage.

A performance-motivated surface has paired plain and optimized programs. Optimize
safe Jet code first. Audited native or unsafe leaves are explicit expert paths
and preserve the same contract.

Use a layered, frequency-weighted corpus of examples, conformance cases,
repository usage, representative beginner tasks, and domain workloads.
High-frequency APIs receive paired beginner and expert evidence for source size,
diagnostics and recovery, preparation cost, and runtime cost.

## Native Observe startup

The native `jet_rt_main` starts Observe exactly once after host argument capture,
logical Env seeding, and native TLS/registry initialization, before invoking the
entry. Startup reads the logical Env cell; it does not reread the process's
startup environment or run early in the backend object-main thunk.

The `assemble-observe`, `lower`, `link-observe-startup`, and
`run-observe-startup` fixture sequence requires the Jet main and writer exports
in the checked native object. Separate enabled and unset child processes prove
that enabled startup runs one real periodic writer even after repeated starts,
whereas unset startup creates no writer, extra thread, or publication file.
The writer is a detached process-lifetime daemon; child exit, not a fabricated
join or writer free, is its teardown.

## Completion criteria

A CoreLib area is complete only when:

- Jet owns its public behavior;
- every declaration has a meaningful contract;
- every applicable tier shares one observable meaning;
- capability policy is explicit;
- errors, effects, authority, limits, and hostile cases are proven;
- examples, goldens, and generated views are current;
- callers and obsolete implementations are migrated or removed;
- correctness evidence is integrated;
- a performance baseline exists;
- no known gap remains in the declared scope.

A stronger comparative claim additionally requires the strict performance and
frequency-weighted learnability evidence above. Execution order, active work
records, blockers, and evidence logs belong in Tower rather than in this
specification.

### Native service TaskOutcome carrier

The registry-owned service TaskOutcome handle uses one private NativeTaskOutcome16
in `Task.jet`: tag at byte0, owned Text at byte8 only for Panicked. The existing
C ABI ordinals remain Finished0, Panicked1, Cancelled2 and DeadlineBlown3. The
four record constructors are the producers; Panicked borrows and deep-copies its
input. Clone and its getter return independent Text ownership. Unit variants
never inspect, compare or release inactive payload storage. Drop releases an
active reason and then the16-byte holder; equality compares tags and active Text
bytes, not allocation identity.

Show and Display preserve the raw `Panicked(reason)` bytes. Debug delegates the
reason to the canonical `rt_text_push_rust_debug_str` renderer. This matches the
read-only contract in `Core/Service/Service.jet:91-125` at497daa7a0, including
the `a"b` source test vector; that source file is not imported by this carrier
batch. `Tests/TaskOutcomeWitness.c` constructs through the real producers and
checks every variant, deep clone/getter ownership, embedded NUL, Unicode,
the shared Debug goldens, inactive payload poison and exact-once frees. Its
stdout file is expected output, not native execution evidence. Its `wrong`
mode uses the shared canonical record-variant panic, not a new trap.

Workflow take/return APIs remain held without direct AOT; this carrier does not
establish service or workflow behavior or native parity. The existing shared
record-variant panic's task publication repair and the canonical Prelude stop-fact
classification fix are separate composition gates, recorded in
`Tests/TaskOutcomeRoutes.tsv`. The getter explicitly returns its stopped zero
word after shared panic publication; it never clones an inactive reason after
publication returns. TaskFailure and TaskOutcome use one shared terminal render
tail, with TaskOutcome adding only Finished; the fixture checks the unchanged
TaskFailure bytes against the same raw and Debug vectors.
The same table records the never-both retirement rule: if TaskOutcome enters
`core.service types{}`, its native carrier glue, ABI constants, exports and
fixtures retire in that same composition. No frozen registry, route or C ABI
row is hand-edited, and no generated carrier alias is added.

