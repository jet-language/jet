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

The generator must verify that:

- every declared export has one canonical Jet source or an explicit registered
  provider/leaf role;
- generated module and call views agree with the declaration source;
- duplicate mechanisms, unapproved aliases, shims, and obsolete projections do
  not create a second public identity;
- effect, authority, capability, fallback, signature, and tier metadata remain
  consistent.

The generated Rust tables are projections, not a second API catalog. Never
hand-edit them. The main generated views are
[`CoreModuleExports.rs`](../../crates/jet-foundation/src/CoreModuleExports.rs),
[`core_calls.rs`](../../crates/jet-foundation/src/Syntax/core_calls.rs), and the
corresponding generated projections in the Prelude. Rust implementation files
such as [`Core.rs`](../../crates/jet-codegen/src/Prelude/Core.rs),
[`PortableCore.rs`](../../crates/jet-codegen/src/Prelude/PortableCore.rs), and
[`CoreLib/`](../../crates/jet-codegen/src/Prelude/CoreLib/) provide registered
leaves and adapters; they do not become an independent public API registry.

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

The internal entropy fill boundary borrows an exclusive writable span and
returns zero only when every requested byte is filled. On Linux x86-64 it
uses `getrandom` with flags zero: short reads continue on the remaining suffix,
and interruption clears that suffix before retrying. Other provider failures
clear the entire attempted span; there is no file, time, or deterministic
fallback. Zero-byte requests do not invoke a provider. Core retains ownership
of count validation and public error projection.

The test/fault harness installs its Rand provider at this same adapter site,
scoped to the current OS thread. Its callback and context stay live until
reset; reset restores the real provider and releases the adapter state. Thread
exit also wipes and releases that state. Private error rendering accepts only
public operation metadata and never reads the password-policy reason or any
secret material. Scripted provider and redaction results are compared with
a once-frozen Rust seed receipt, separately from live-provider checks.

The runtime SHA-256 byte boundary borrows its input and writes to a caller-owned
32-byte digest. Its 576-byte, word-aligned scratch region is exclusive to the
call and wiped before return. Domain-separated callers can hash up to four spans
without allocating a concatenated message; the digest must not alias any
input span. The algorithm is shared by vault and other runtime crypto callers.

Vault provider errors are private to the persistence boundary and follow the
ratified 13-case source `core.crypto.vault.VaultError` order. Tag10 stores the real
Core `CryptoError` inline I64 discriminant (OpenFailed0, VerifyFailed1,
KeyRejected2, Length3, Unavailable4), never owned diagnostic strings or a box.
IO owns operation/redacted-path text; Internal owns incident text.
The richer seed crypto diagnostics are superseded by CoreFix8's source enum:
OpenFailed maps by name to OpenFailed; encoding/version/algorithm/key/password
policy rejection to KeyRejected; input/output length to Length; entropy/resource/
internal failure to Unavailable. No richer payload is forwarded or retained.
The private Vault Display retains its prefix and formats its Crypto payload
using standard unit-enum variant names; Debug is `Crypto(<variant>)`.
The generator rule is `Compiler/JetBackend/Source/Lower/Display.jet:423-424`;
`JetEval/Source/Formatting/Values.jet:299,356` uses the same bare names.
The historical pair codec preserves
UTF-8 values byte-for-byte, including embedded NUL; it rejects truncation,
invalid UTF-8, and trailing bytes. Owned value buffers are wiped before release.

The Linux vault directory boundary owns raw private descriptor words only.
It follows the seed's root open and uses `openat2` for `.jet` with CLOEXEC,
NOFOLLOW and RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS; there is no fallback.
The held directory must belong to the effective user, be a directory, and
disallow group/other writes. Negative errno maps at the seed's exact boundary:
EXDEV, ENOTDIR, EINVAL, ENOSYS and ELOOP become UnsupportedProvider; other
open/stat failures retain operation and redacted-path IO payloads.
Scratch follows the native x86-64 stat ABI already used by SentryLedger.
Every temporary/root descriptor closes on success and failure; only the
successful held `.jet` descriptor transfers to the caller. Supplemental real
filesystem fixtures cover accepted, writable and symlinked directories but
do not claim the original full `prepare_generate_at` fixture before composition.
The descriptor-relative inode gate retains the seed renameat2, proc-fd linkat,
quarantine/reopen/compare and exact-owner unlink sequence
(`SecretsCrypto.rs:2178-2342,2818-2834`). It checks both the visible staging
inode and the installed inode, attempting exchange rollback on a mismatched
installed inode. The seed's lock-substitution hook replaces the name after
its first inode check; quarantine rechecks and restores that replacement rather
than deleting it. `O_TMPFILE` staging and proc-fd linking have no named-temp
fallback. Supplemental native fixtures use actual descriptors/files and the
shared hook, not a C implementation of this policy. The full writer's crypto,
state rechecks, lock ownership and durability protocol are separate callers.
Held-file reads duplicate via F_DUPFD_CLOEXEC, seek the shared open-file position
to zero, and read into an owned packed byte sequence, retrying interrupted reads.
An exclusive caller byte probes EOF before growing a full sequence, so an
exact-capacity EOF neither reallocates nor copies. There is no new read bound.
Descriptor-relative reads preserve successful absence as None, with the seed's
distinct open failure mapping (ENOSYS/EINVAL/ELOOP to UnsupportedProvider, other
non-ENOENT failures to Locked). Supplemental actual-file fixtures preserve NUL
and all byte values, exercise growth and exact-capacity EOF, and observe clone's
shared seek position plus error-output preservation.
Vault persistence encryption/decryption orchestrates the shared accepted age v1
primitives; it does not implement a second age engine. Keygen encodes directly
into final native Text buffers from one default-provider seed. Its private leaf
returns Crypto(Unavailable); the public infallible seed API must retain its
canonical entropy panic when public producer glue is admitted.
The shared entropy adapter supplies file key, recipient ephemerals, nonce and
grease. Grease follows age-core 0.10.0 format.rs:97-129,340-376: graphic ASCII
words, `-grease` tag suffix, 0..4 arguments, 0..99 random body bytes, unpadded
base64 at 64 columns with a final short/empty line. Rejection sampling is
uniform; the scratch byte pool is not another Rand adapter. All key material
and exclusive workspaces wipe on every exit. Store open/seal failures map to
Locked at the seed persistence boundary; no rich crypto prose is invented.
Supplemental default-provider fixtures cover empty/full/next STREAM chunks,
wrong identities, tampered last tags, output preservation and Rand failure.
Recipient policy splits newline-delimited UTF-8 input, Unicode-trims each line,
canonicalizes lowercase age keys, rejects duplicates/empty/over-u16 counts,
and hashes the sorted u16-count/u16-length canonical encoding. Public key slots
are permuted in place; no keys or Text owners are cloned during sorting.
Historical starting-store identity uses duplicate-first name validation and
sorted u16-name/u32-value bytes, distinct from the legacy wire pair codec.
Typed plaintext must exactly match its canonical re-encoding. Absence uses a
zero-UUID empty store and its own starting-hash domain without reading identity
or recipients. Owned decrypted plaintext and identity buffers wipe on release.
Identity selection reuses the shared startup environment capture (`rt_env_get`):
nonempty JET_KEYS_DIR, then HOME (including empty), USERPROFILE, then ".".
Only component-wise CurDir roots take this environment route. Other roots
append keys/secrets.identity directly, including the relative empty-root case.
Identity opens deliberately follow symlinks; vault-directory opens do not.
Plain identity open retries EINTR, then reads its descriptor to EOF without
duplication or rewind, preserving FIFO/pipe/character-device behavior. The held
file reader reuses this same EINTR-aware EOF loop after its required duplicate
and seek. Failed reads wipe the entire partial allocation before release.
Supplemental transport fixtures supply actual 8192/8193-byte pipes, /dev/null,
a FIFO identity after non-SA_RESTART signals, and observe the real error-buffer
free for full-capacity zeroization; execution remains owned by composition.
One plain-open EINTR helper serves the seed's std-file root, identity and proc
read sites; raw openat2 and pidfd syscall sites gain no extra retry policy.
Proc start time uses the last ')' and the twentieth Unicode-whitespace token,
returning absence on read/UTF-8/parse failure. Checked unsigned parsing accepts
one leading '+' and preserves zero/all 64 bits, with a distinct u32 PID bound.
Decimal path/lock-field rendering writes final bytes through the shared Display
digit helpers without allocating temporary Text owners. Supplemental fixtures
cover unsigned boundaries/overflow, Unicode separators, nested proc names,
unchanged failed outputs and actual repeated self-proc reads.
Unrecovered reads stay explicit, allowing the recovered caller to perform its
lock-aware recovery first. Supplemental transport fixtures carry independent
starting/recipient hash vectors and exercise trim/case/duplicate/UTF-8 policy,
historical owner ordering, typed trailer rejection, and component-path joins.
These additions do not claim native execution or the full recovered writer.

Typed vault records own native Secret buffers. Equality compares the key bytes,
not just immutable hashes, while Debug always prints a redacted key marker.
Cloning creates independent key and provenance owners. Unsigned repository,
generation, and timestamp words preserve all 64 bits. Immutable record identity
binds the name, key type/material/public key, generation, opaque ID, creation
time, provenance, and optional origin; lifecycle status/reason updates do not
change that identity. The encoder checks that immutable hash before validating
an optional origin name. Hashing therefore streams even over-limit origin names
through borrowed spans and fixed scratch; a bad hash remains the earlier
`InvalidEncoding` error, whereas a matching hash reaches `InvalidName`.

The typed JVLT2 codec keeps the historical canonical wire: sorted UTF-8 string
names, sorted `(key type, name)` groups, contiguous generations, bounded counts
(4096 strings, 1024 groups, 8192 versions), and a domain-separated payload hash
within the 16-MiB total limit. Sorting uses stable borrowed-pointer index merges,
not key-owner copies. Private bounded AVL scratch detects duplicate opaque IDs
at the seed's exact insertion point; it is never persisted or a public carrier.
Decoding rejects reserved fields, noncanonical order, inconsistent group/current
metadata, malformed origins, lifecycle violations, trailing data, and incorrect
public-key/payload hashes. It derives immutable record hashes absent from the
wire. Raw unsigned words are preserved. Decoder lifecycle checks intentionally
remain stricter than the historical encoder's checks, rather than silently
strengthening encoding. Every partial owner is released on failure; native
Secret capacity is wiped on release. The private binary output's storage owner
must likewise be wiped by the persistence caller before release.

The imported-origin native fixture draws its signing seed once, directly into
the adopted packed NativeSecret buffer through the shared default entropy
provider. It exercises Ed25519 public hashing, origin/hash round-trip and the
encoder's rejection of Generated provenance with a retained imported origin.
Native execution remains a composition obligation, not a parser receipt.

Vault test hooks are explicitly initialized test transport, never an authority
mechanism for production callers. Authorizer, trust-path, and fault overrides
are thread-local and released at thread exit. Backup barriers are shared,
root-filtered immutable snapshots: clearing a hook does not invalidate a writer
already blocked on its entered/release phases. Reentrant mutable authorizer
access preserves the reference foreign-panic body and exit status 101.

The internal X25519 boundary borrows 32-byte scalar and point spans and writes
32 bytes to a disjoint output. Its 1536-byte, word-aligned scratch is exclusive
and wiped before return (1568 bytes for public-key derivation). Field arithmetic
uses bounded radix-65536 limbs, fixed-index loops, and mask-based swaps, not
secret-dependent branches or arbitrary-precision reduction. A zero shared
result is rejected by recipient protocols, not by the RFC 7748 primitive.

Vault signing-key public derivation uses the internal Ed25519 fixed-limb
boundary with a borrowed 32-byte seed, 32-byte output, and exclusive 4096-byte
scratch wiped before return. Its SHA-512 engine separately accepts a borrowed
byte span, disjoint 64-byte digest, and 704-byte aligned scratch. Signed machine
words carry hash bit patterns with wrapping additions and explicitly logical
right shifts; scalar multiplication uses mask swaps and complete additions.

The internal ChaCha20/XChaCha20 engines use borrowed key/nonce/input/output
spans and caller-owned word-aligned scratch (320/368 bytes). Exact in-place XOR
is supported; partial overlap is not. Counter exhaustion is rejected before
output writes. HChaCha20 derives the XChaCha key without feed-forward, then
uses four zero nonce bytes followed by the final eight extended-nonce bytes.

Poly1305 uses bounded 26-bit limbs and mask-based canonical reduction. AEAD
borrows AAD and ciphertext independently instead of allocating a MAC message;
the Poly1305 length block commits to both byte lengths. Open compares all 16
tag bytes before plaintext writes, leaving output untouched on failure.
Exclusive aligned scratch is 560 bytes (608 for XChaCha) and is wiped on
success and failure.

HMAC-SHA256 and HKDF-SHA256 share the runtime SHA-256 engine, which supports
four borrowed spans without concatenation. HKDF uses extract then expand,
accepts an empty salt as RFC 5869's zero salt, and limits output to 255 hash
blocks (8160 bytes). Aligned scratch is 672 bytes for HMAC and 744 for HKDF;
key pads, intermediate digests, PRK, and expansion blocks are wiped on return.

The vault recovery KDF uses Argon2id version 19 with the existing JVKW policy:
65536 KiB, three passes, one lane, 32-byte result and 16-byte salt. Its internal
RFC vector entry also carries optional secret/associated-data fields. Memory
and 4800-byte scratch are caller-owned and wiped on return. The first half of
the first pass uses data-independent addresses; later references are
data-dependent as required by RFC 9106, not a constant-time substitution.
Its BLAKE2b engine streams borrowed spans through one fixed 128-byte block
buffer, avoiding a concatenated copy of the password and parameter fields.

Age identities and recipients retain age v1's Bech32 checksum (constant 1),
not the typed Core recipient format's Bech32m checksum. Identity output is
uppercase `AGE-SECRET-KEY-1…`; recipient output is lowercase `age1…`. Parsing
rejects mixed case and nonzero unused key bits. Header base64 is unpadded and
canonical; padded, URL-safe, and nonzero-trailing-bit encodings are rejected.

The internal age v1 recipient framing uses a 16-byte file key, X25519 stanza
wrapping, HKDF-SHA256 header/payload keys, HMAC-SHA256 over the header through
the three `---` bytes, and a 16-byte file nonce. Payload STREAM uses 65536-byte
plaintext chunks, a big-endian eleven-byte counter and one-byte final flag;
a full final chunk is marked final without an extra empty chunk. An empty
final chunk is accepted only for an empty whole payload. Every chunk uses
ChaCha20-Poly1305 without AAD. Open wipes previously emitted plaintext on any
later authentication failure and wipes its exclusive aligned 2048-byte
workspace on every return.

Framing accepts binary recipient files, as the existing SecretsCrypto path
does, and rejects scrypt passphrase files. The reader retains the seed's two
beta-era stanza spellings: missing body and missing short final body line
(`age0.10.1/tests/testkit.rs:606-620`). Key/header decoding is not claimed to be
constant-time. Seal borrows recipient keys, CSPRNG-provided per-recipient
ephemeral scalars, file key, nonce and a valid caller-provided grease stanza;
all input/output/workspace spans are disjoint. There is no entropy fallback.
`AgeV1Fixtures.json` freezes the official binary age testkit, Rust 0.10.1 chunk
boundary outputs and an independent fixed-material Node/OpenSSL seal recipe.
`run-crypto-fixtures.mjs` composes the Jet KAT exports, audits strong symbols,
and emits Jet seal captures for a one-time Rust-opener comparison; the old raw
crypto leaves stay until native execution and consumer admission.
A high-level native implementation must not be retained as an invisible runtime
fallback for a Jet implementation. If an API changes identity or semantics,
make the change at the contract boundary and migrate its users in one cutover.

Prepared foreign bridges that depend on Foundation expose its complete owned
source closure to the native build sandbox. This includes the `jet-unicode`
path dependency at its canonical Cargo location, as well as the existing
external include inputs. The bridge identity fingerprints both crate trees
and those includes: changing Unicode data invalidates an old bridge instead of
reusing a build prepared against a different Foundation dependency closure.

Native lowering must fail closed when a construct cannot preserve the shared
runtime's ABI, error behavior, or observable output. Report an unsupported
construct before producing an executable; do not marshal an unproved carrier
or run a backend-local approximation. A focused same-program regression must
exercise either the shared meaning or that explicit refusal.
Module scalar storage has one local data cell per checked declaration. Global
reads and writes through static-rooted places address that same cell, including
when the shared object writer packages a runtime library. Atomic values still
require their typed allocation, ordering and ownership contract; a raw machine
compare-and-swap primitive does not establish that contract.

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
monomorphized payload-drop function)` to `jet_rt_native_shared_new`. Its 88-byte
cell stores the function pointer at byte 80; the thunk borrows the owned slot
address and returns `Unit`. Only the atomic `1 -> 0` strong-release transition
invokes it, exactly once, outside all Shared locks. The glue performs the complete
typed destruction, including `T.Close` and owned fields. The provider must not
also invoke the descriptor's drop callback. A null function is reserved for
trivial payloads and means plain free, not a descriptor-drop fallback.

Like Rust's `Arc<T>` releasing its final strong owner, the callback runs in the
releasing caller's context. The provider adds neither a cleanup mask nor a
session shield; releases inside a MIR cleanup frame inherit that frame's
bracket. A callback that returns with pending stop still permits the provider
to free the payload allocation, descriptor and cell before the caller propagates
it. Reclamation on a pending stop is explicit, not unwind-dependent.
The seed-only C fixture retains its separate
two-argument Rust-provider declaration for comparison; it is not a native alias
and retires at cutover. Native runtime fixtures pass explicit drop functions,
including two threads each racing 100,000 clone/drop pairs and a null-glue
case that must not fall back to the descriptor callback.

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
owned native list header and word-stride buffer; the empty header has all three
words zero. Neither conversion transfers or frees its borrowed input.

Native sentry scopes and unsafe function gates save the canonical per-thread
gate/fence state in caller-owned frame storage. Normal and stopped cleanup
edges restore that state exactly once, including nested policy scopes; file
and reason strings remain borrowed module literals at the ABI boundary. The
temporary Rust sentry marshal seam retires with the Jet sentry/TLS layer.
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
