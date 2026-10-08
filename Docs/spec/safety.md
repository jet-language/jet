# Safety

This specification defines Jet's default safety boundary and its audited
expert escape. It is for Jet users, library authors, and reviewers. The
executable spelling lives in [`Syntax.rs`](../../crates/jet-foundation/src/Syntax.rs);
sema checks the gate and its obligations in
[`UnsafeObligations.rs`](../../crates/jet-sema/src/Sema/UnsafeObligations.rs).
The focused [`unsafe_ratchet` tests](../../tests/unsafe_ratchet.rs), the
[`golden` safety checks](../../tests/golden.rs), and the
[`ratchet checker`](../../Tools/agent/check-unsafe-ratchet.mjs) provide the
repository proof; gated examples live under
[`Examples/features/`](../../Examples/features/). Terms such as stream and
reader follow the [Jet vocabulary](vocabulary.md).

## The audited escape

Safe Jet code cannot perform a raw memory operation, call an unsafe function,
or cross an unsafe foreign boundary without an enclosing audited gate. Write
the gate with Pascal-case `Unsafe` and a string reason:

```jet
cell :: 1
#Unsafe("`cell` is live on this stack frame and the pointer never escapes") {
    ptr :: *cell
    value :: ptr.*
}
```

The same contract applies to a whole function:

```jet
#Unsafe("the foreign ABI preserves the scalar contract") fn add(a: Int, b: Int) -> Int {
    a + b
}
```

The reason belongs to `#Unsafe` itself. Bare `#Unsafe { … }` and
`#Unsafe fn …` are rejected (E3112). Lowercase `unsafe` is recognized only to
teach the foreign-spelling diagnostic; it is not Jet syntax. An unsafe function
body is an audited region, but calling an unsafe function still requires an
enclosing `#Unsafe` region.

`use core.mem` is the import gate for the low-level memory names. It does not
replace the audit gate: raw-pointer formation, dereference, address-based
pointer construction, and volatile access also require `#Unsafe`. The sema
obligation pass can require `assert valid_ptr, aligned, no_alias` immediately
after a tracked operation. Those assertions are meaningful only inside the
gate and are removed before TIR.

Inline `#FFI(c)`, `#FFI(cpp)`, and `#FFI(asm)` bodies require an enclosing
`#Unsafe("reason")` gate. The same boundary covers foreign calls whose
signature transfers ownership or exposes raw memory. A package or
organization policy may forbid unsafe code; a lexical gate cannot raise that
safety floor. Under a per-site policy, the gate chooses
`obligations: .Track` or `.Skip` according to the governing policy.

Generated Rust may contain `unsafe` only for an approved audited Jet gate or
inside vetted runtime, standard-library, and memory-internal implementations.
The generated code is not a new user escape hatch; the gate and sema proof are
the source-level authority.

## Unsafe-region ratchet

The repository records every user-written semantic unsafe region in the
baseline below. Each row records its package, source file, marker position, and
reason. The JSON data and Markdown tables are one generated registry; do not
edit either representation by hand.

[`check-unsafe-ratchet.mjs`](../../Tools/agent/check-unsafe-ratchet.mjs)
recursively scans `.jet` files while excluding its configured generated and
build directories. It recognizes block and whole-function
`#Unsafe("reason")` gates and grouped
`#[Unsafe("reason"), …]` function gates. It skips comments, strings, malformed
or incorrectly placed markers, and generated FFI files below `.jet/bindings/`.

Run the checker without `--update` to compare source with this baseline. A
larger count fails and reports each unmatched semantic region. When a new
region is approved, refresh the baseline in the same change with:

```sh
node Tools/agent/check-unsafe-ratchet.mjs --update
```

If source removes only recorded regions, the checker lowers and rewrites the
baseline automatically. A new semantic region cannot hide behind a net
decrease. A count-preserving edit that changes a recorded file, position, or
reason is stale and fails until the baseline is refreshed. The
`unsafe-ratchet` block is intentionally kept byte-stable between such
refreshes.

The baseline is also consumed by `tests/golden.rs` to identify the audited
example stems whose generated Rust may contain `unsafe`. The positive check
matches every source region against the baseline's count, position, and reason,
then requires that region's lowered audit sentry with its source line and reason.
An unrelated prelude `unsafe` or another region's sentry cannot satisfy it.
A source-level gate does not itself require a Rust unsafe operation: expert
crypto calls may use safe, source-owned Core Jet bodies while sema still requires
`#Unsafe` for nonce, algorithm, and secret-exposure obligations (E0510).
All actual Rust unsafe operations retain the gated-form scan, and every ungated
example retains the negative unsafe-outside-vetted scan.

<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 215,
  "counts": {
    "Examples": 38,
    "jet": 89,
    "jetpack": 1,
    "tests": 87
  },
  "regions": [
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_migration.jet",
      "line": 31,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/crypto_suite.jet",
      "line": 36,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/expert_projection.jet",
      "line": 30,
      "column": 5,
      "reason": "expert cryptography is an explicit opt-in"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 96,
      "column": 5,
      "reason": "known-answer check reads the RFC 7748 shared secret"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/primitive_matrix.jet",
      "line": 158,
      "column": 5,
      "reason": "expert known-answer vectors need raw key bytes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/random_api_split.jet",
      "line": 33,
      "column": 5,
      "reason": "compare the typed and raw HKDF rungs"
    },
    {
      "package": "Examples",
      "file": "Examples/features/crypto/vault_keys.jet",
      "line": 41,
      "column": 5,
      "reason": "restore audited raw signing-key material"
    },
    {
      "package": "Examples",
      "file": "Examples/features/effects/audited_gate_ladder.jet",
      "line": 6,
      "column": 5,
      "reason": "the audited block has no low-level operation"
    },
    {
      "package": "Examples",
      "file": "Examples/features/effects/single_use_discard.jet",
      "line": 35,
      "column": 5,
      "reason": "event cancelled; the ticket admits to nothing, so voiding it is correct"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 34,
      "column": 5,
      "reason": "the stop runs the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/cleanup_matrix.jet",
      "line": 72,
      "column": 5,
      "reason": "the exit callbacks have no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_process_control.jet",
      "line": 30,
      "column": 5,
      "reason": "POSIX process and pipe control for the core.sys surface"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/os_stop_cleanup.jet",
      "line": 24,
      "column": 5,
      "reason": "the stop callback uses the shared cleanup boundary"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/process_exit_cleanup.jet",
      "line": 20,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_privileged_entry.jet",
      "line": 10,
      "column": 5,
      "reason": "run only inside a disposable user namespace with a dedicated test identity"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_projection.jet",
      "line": 14,
      "column": 5,
      "reason": "the clock file was created above in this test-owned directory"
    },
    {
      "package": "Examples",
      "file": "Examples/features/io/system_projection.jet",
      "line": 20,
      "column": 5,
      "reason": "fork and reap both children before returning"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_asm.jet",
      "line": 8,
      "column": 1,
      "reason": "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_asm.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 6,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 13,
      "column": 1,
      "reason": "the scalar ABI contract deliberately returns a mismatched result"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 19,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 26,
      "column": 9,
      "reason": "call the audited inline C contract with bounded Int values"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/inline_c.jet",
      "line": 32,
      "column": 9,
      "reason": "call the audited inline C contract with a deliberately wrong C result"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/lowlevel.jet",
      "line": 8,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/mmio_board_write.jet",
      "line": 9,
      "column": 5,
      "reason": "stand-in MMIO cell stays on this stack frame"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/opaque_handles/run.jet",
      "line": 34,
      "column": 5,
      "reason": "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/lowlevel/unsafe_obligations.jet",
      "line": 9,
      "column": 5,
      "reason": "cell stays live and the pointer remains local"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/expiring_secret.jet",
      "line": 20,
      "column": 5,
      "reason": "vault key import: expiring secret lifetime"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/pin.jet",
      "line": 43,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/rawptr.jet",
      "line": 7,
      "column": 5,
      "reason": "`cell` is a live Int on this stack frame; the pointer never escapes"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/uninit.jet",
      "line": 12,
      "column": 5,
      "reason": "uninitialized plain-data storage is filled before every read"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_package_off/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this package-policy proof"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "Examples",
      "file": "Examples/features/memory/unsafe_sentries_source_off.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "Examples",
      "file": "Examples/features/tooling/checked_text_type.jet",
      "line": 20,
      "column": 5,
      "reason": "checked text example output"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Calls.jet",
      "line": 55,
      "column": 9,
      "reason": "each argument cell is inside this call's writable mapping"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Calls.jet",
      "line": 60,
      "column": 5,
      "reason": "loaded.entry is the selector-generated pointer-to-cells adapter; all cells remain mapped through its return"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 48,
      "column": 5,
      "reason": "function is the resolved (ptr,len)->word runtime constructor; buffer remains readable until it returns"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 117,
      "column": 5,
      "reason": "word is a live String handle and both functions borrow it through the canonical runtime C ABI"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 124,
      "column": 9,
      "reason": "the runtime's borrowed UTF-8 projection holds length readable bytes until the owning String is dropped"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 144,
      "column": 13,
      "reason": "word is a live exact-Int carrier and stringify returns a fresh owning String"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/ComptimeValues.jet",
      "line": 148,
      "column": 13,
      "reason": "the temporary String is owned here and decoding has copied its bytes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 100,
      "column": 9,
      "reason": "the slot is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 106,
      "column": 9,
      "reason": "the relocation is an aligned word inside the writable mapping, checked above"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 127,
      "column": 5,
      "reason": "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/Image/Loader.jet",
      "line": 149,
      "column": 5,
      "reason": "the slot is an aligned word inside the mapping, now writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 38,
      "column": 1,
      "reason": "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 84,
      "column": 5,
      "reason": "the caller owns `count` writable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 91,
      "column": 5,
      "reason": "the caller owns `count` readable bytes at `address`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 99,
      "column": 5,
      "reason": "an anonymous mapping aliases no existing memory"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 106,
      "column": 5,
      "reason": "the caller owns the mapping it changes"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 112,
      "column": 5,
      "reason": "the caller owns the mapping and nothing uses it any more"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 119,
      "column": 5,
      "reason": "exit_group ends every thread of the process"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 124,
      "column": 1,
      "reason": "stores one word at an address the caller owns and has mapped writable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 131,
      "column": 1,
      "reason": "reads one word at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 138,
      "column": 1,
      "reason": "reads one byte at an address the caller owns and has mapped readable"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 161,
      "column": 9,
      "reason": "the caller owns the destination words"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 171,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 184,
      "column": 1,
      "reason": "calls machine code; the caller vouches for the code and its two-word System V signature"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 199,
      "column": 1,
      "reason": "dlopen reads the NUL-terminated path at `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 207,
      "column": 1,
      "reason": "dlsym reads the NUL-terminated name at `name`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 230,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `path`"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Source/OS/Linux.jet",
      "line": 242,
      "column": 5,
      "reason": "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table"
    },
    {
      "package": "jet",
      "file": "Compiler/JetBackend/Tests/ComptimeScalars.jet",
      "line": 29,
      "column": 9,
      "reason": "the identity returns its single owning String unchanged; decoding has copied it"
    },
    {
      "package": "jet",
      "file": "Core/auth/auth.jet",
      "line": 126,
      "column": 5,
      "reason": "auth PASETO v4.public verification uses the fixed Ed25519 primitive"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 785,
      "column": 5,
      "reason": "crypto expert: Argon2id password hashing"
    },
    {
      "package": "jet",
      "file": "Core/crypto/crypto.jet",
      "line": 812,
      "column": 5,
      "reason": "crypto expert: Argon2id password verification"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 70,
      "column": 5,
      "reason": "vault typed load provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 77,
      "column": 5,
      "reason": "vault typed current provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 84,
      "column": 5,
      "reason": "vault typed status provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 91,
      "column": 5,
      "reason": "vault typed versions provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 98,
      "column": 5,
      "reason": "vault typed generation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 105,
      "column": 5,
      "reason": "vault typed store provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 113,
      "column": 5,
      "reason": "vault key import: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 121,
      "column": 5,
      "reason": "vault key import: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 129,
      "column": 5,
      "reason": "vault wrapped import provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 136,
      "column": 5,
      "reason": "vault typed rotation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 144,
      "column": 5,
      "reason": "vault typed retirement provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 152,
      "column": 5,
      "reason": "vault typed revocation provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 160,
      "column": 5,
      "reason": "vault write authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 168,
      "column": 5,
      "reason": "vault wrapped authorization provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 176,
      "column": 5,
      "reason": "vault typed generation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 184,
      "column": 5,
      "reason": "vault typed store commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 192,
      "column": 5,
      "reason": "vault key import commit: signing key material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 200,
      "column": 5,
      "reason": "vault key import commit: X25519 secret material"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 208,
      "column": 5,
      "reason": "vault wrapped import commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 216,
      "column": 5,
      "reason": "vault typed rotation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 224,
      "column": 5,
      "reason": "vault typed retirement commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 232,
      "column": 5,
      "reason": "vault typed revocation commit provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 239,
      "column": 5,
      "reason": "vault passphrase export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/crypto/vault.jet",
      "line": 247,
      "column": 5,
      "reason": "vault recipient export provider boundary"
    },
    {
      "package": "jet",
      "file": "Core/email/email.jet",
      "line": 545,
      "column": 13,
      "reason": "read the DKIM key length, then wipe this copy"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 36,
      "column": 1,
      "reason": "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 40,
      "column": 1,
      "reason": "`pointer` names a live, aligned, readable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/mem/mem.jet",
      "line": 44,
      "column": 1,
      "reason": "`pointer` names a live, aligned, writable `T` for the whole access"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 158,
      "column": 1,
      "reason": "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 174,
      "column": 5,
      "reason": "posix getpgid: inspect a process group"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 180,
      "column": 5,
      "reason": "posix getsid: inspect a process session"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 185,
      "column": 1,
      "reason": "posix getpriority: the caller vouches that `which` names this process or one it controls"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 189,
      "column": 1,
      "reason": "posix setpriority: the caller owns the scheduling priority of the process it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 193,
      "column": 1,
      "reason": "posix utime: the caller owns the file whose timestamps it rewrites"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 199,
      "column": 1,
      "reason": "posix stop: the caller accepts that the process ends here after the shared cleanup boundary"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 206,
      "column": 1,
      "reason": "posix pipe: the caller owns both returned descriptors and closes each exactly once"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 210,
      "column": 1,
      "reason": "posix close_fd: the caller owns `fd` and nothing uses it after this call"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 214,
      "column": 1,
      "reason": "posix mkfifo: the caller owns `path` and removes the named pipe it creates"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 218,
      "column": 1,
      "reason": "posix fork: the caller handles both the parent and the child return and reaps the child"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 222,
      "column": 1,
      "reason": "posix setsid: the caller owns the process session topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 226,
      "column": 1,
      "reason": "posix wait: the caller owns the child processes it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 230,
      "column": 1,
      "reason": "posix waitpid: the caller owns the child process it reaps"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 234,
      "column": 1,
      "reason": "posix setuid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 238,
      "column": 1,
      "reason": "posix setgid: the caller drops or changes process credentials deliberately"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 242,
      "column": 1,
      "reason": "posix setpgrp: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 246,
      "column": 1,
      "reason": "posix setpgid: the caller owns the process group topology it changes"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 250,
      "column": 1,
      "reason": "posix kill: the caller owns or is entitled to signal the target process"
    },
    {
      "package": "jet",
      "file": "Core/sys/sys.jet",
      "line": 254,
      "column": 1,
      "reason": "posix initgroups: the caller changes supplementary groups deliberately"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 58,
      "column": 5,
      "reason": "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 113,
      "column": 5,
      "reason": "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Source/Sign.jet",
      "line": 137,
      "column": 5,
      "reason": "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Tests/Index.jet",
      "line": 266,
      "column": 5,
      "reason": "the test owns the planted lock file it ages"
    },
    {
      "package": "jet",
      "file": "Jetpack/Publication/Tests/Sign.jet",
      "line": 164,
      "column": 5,
      "reason": "original signing secret-erasure acceptance requires the Core volatile in-place wipe provider (#3597)"
    },
    {
      "package": "jet",
      "file": "Jetpack/Store/Tests/ClosureGraph.jet",
      "line": 59,
      "column": 13,
      "reason": "the test owns the journal file it just wrote"
    },
    {
      "package": "jet",
      "file": "Jetpack/Store/Tests/Seal.jet",
      "line": 31,
      "column": 5,
      "reason": "the store test owns the fixture file whose mtime it pins"
    },
    {
      "package": "jet",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet",
      "line": 4,
      "column": 5,
      "reason": "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word"
    },
    {
      "package": "jet",
      "file": "Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet",
      "line": 111,
      "column": 5,
      "reason": "host replay uses a stack cell as a stand-in for the board register"
    },
    {
      "package": "jetpack",
      "file": "crates/jetpack/batteries/embedded/embedded.jet",
      "line": 181,
      "column": 9,
      "reason": "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/aes256gcm_open.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/aes256gcm_seal.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed NIST AES-256-GCM vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/argon2id.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed Argon2id known-answer vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/ed25519_sign.jet",
      "line": 6,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/ed25519_verify_strict.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 8032 Ed25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/hkdf_sha256_raw.jet",
      "line": 8,
      "column": 5,
      "reason": "fixed RFC 5869 HKDF vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/migrate_v1.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed JETC v1 source failure"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/open_v1.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed JETC v1 AES vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/secret_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect fixed secret bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect generated signing-key bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/x25519_raw.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed RFC 7748 X25519 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/x25519_secret_bytes.jet",
      "line": 7,
      "column": 5,
      "reason": "inspect generated X25519 secret bytes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_open.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_seal.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed CFRG XChaCha20-Poly1305 vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/commit_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing key into the vault"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/commit_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret into the vault"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/prepare_import_signing.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed signing-key vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/vault/prepare_import_x25519.jet",
      "line": 5,
      "column": 5,
      "reason": "restore a fixed X25519 secret vector"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/crypto/x25519.jet",
      "line": 9,
      "column": 5,
      "reason": "inspect shared-secret length"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/mem/volatile_read.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/mem/volatile_write.jet",
      "line": 6,
      "column": 5,
      "reason": "value is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/close_fd.jet",
      "line": 6,
      "column": 9,
      "reason": "close an invalid descriptor without changing process state"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/fork.jet",
      "line": 5,
      "column": 5,
      "reason": "fork and let the child return immediately"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/getpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "read the current process priority"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/initgroups.jet",
      "line": 5,
      "column": 5,
      "reason": "probe initgroups with a stable missing user"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/kill.jet",
      "line": 5,
      "column": 5,
      "reason": "check the process group without sending a signal"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/mkfifo.jet",
      "line": 5,
      "column": 5,
      "reason": "probe FIFO creation with an empty path"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/pipe.jet",
      "line": 5,
      "column": 5,
      "reason": "open a pipe and observe its descriptor pair"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setgid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setgid with an invalid identifier"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpgid.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process into its existing process group"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpgrp.jet",
      "line": 5,
      "column": 5,
      "reason": "set the current process group"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setpriority.jet",
      "line": 5,
      "column": 5,
      "reason": "lower the current process priority for this short-lived witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setsid.jet",
      "line": 5,
      "column": 5,
      "reason": "create a session when the host permits it"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/setuid.jet",
      "line": 5,
      "column": 5,
      "reason": "probe setuid with an invalid identifier"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/stop.jet",
      "line": 6,
      "column": 9,
      "reason": "stop this dedicated witness successfully"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/umask.jet",
      "line": 5,
      "column": 5,
      "reason": "set a deterministic mask for this short-lived witness"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/utime.jet",
      "line": 5,
      "column": 5,
      "reason": "probe timestamp update with an empty path"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/wait.jet",
      "line": 5,
      "column": 5,
      "reason": "wait for a child that this witness does not have"
    },
    {
      "package": "tests",
      "file": "tests/conformance/corpus/core/sys/waitpid.jet",
      "line": 5,
      "column": 5,
      "reason": "poll for a child that this witness does not have"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/unsafe_obligations/run.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet",
      "line": 26,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet",
      "line": 10,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/ui/audited_gate_ladder_forbidden/run.jet",
      "line": 3,
      "column": 5,
      "reason": "the organization policy refuses this audited escape"
    },
    {
      "package": "tests",
      "file": "tests/ui/cffi_out_pointer_requires_unsafe.jet",
      "line": 12,
      "column": 1,
      "reason": "the local slot remains live for the complete C call"
    },
    {
      "package": "tests",
      "file": "tests/ui/checked_text_raw_requires_unsafe.jet",
      "line": 22,
      "column": 5,
      "reason": "audited literal"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_type_precedence.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed password-hash vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 7,
      "column": 5,
      "reason": "protocol-selected password policy"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_argon2id_literal_policy_valid.jet",
      "line": 15,
      "column": 5,
      "reason": "fixed password-hash vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length.jet",
      "line": 12,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_valid.jet",
      "line": 5,
      "column": 5,
      "reason": "protocol-selected interop material"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_material_length_valid.jet",
      "line": 17,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_callback_bound_precedence.jet",
      "line": 13,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_correct.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_bound_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_prohibition_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_effect_unknown_precedence.jet",
      "line": 10,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_length.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_region_authority_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet",
      "line": 9,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_expert_nonce_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_length.jet",
      "line": 7,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_type_precedence.jet",
      "line": 5,
      "column": 5,
      "reason": "fixed interop vector"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_valid.jet",
      "line": 6,
      "column": 5,
      "reason": "protocol-selected output length"
    },
    {
      "package": "tests",
      "file": "tests/ui/crypto_hkdf_output_valid.jet",
      "line": 14,
      "column": 5,
      "reason": "fixed interop vectors"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_duplicate_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "each discarded register is named once"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_float_signature.jet",
      "line": 3,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_missing_return_anchor.jet",
      "line": 3,
      "column": 1,
      "reason": "cycle counter"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_no_mem.jet",
      "line": 1,
      "column": 1,
      "reason": "register-only increment"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_return_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the result register is overwritten by the body"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_asm_unknown_clobber.jet",
      "line": 3,
      "column": 1,
      "reason": "the body declares only audited registers"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_body_not_string.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_inline_c_mismatch.jet",
      "line": 1,
      "column": 1,
      "reason": "deliberate mismatch fixture"
    },
    {
      "package": "tests",
      "file": "tests/ui/ffi_unknown_language.jet",
      "line": 1,
      "column": 1,
      "reason": "demo"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_e3102.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_e3103.jet",
      "line": 3,
      "column": 1,
      "reason": "reads through caller-provided raw pointer"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_without_mem.jet",
      "line": 2,
      "column": 5,
      "reason": "addr is valid"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_wrong_ptr.jet",
      "line": 4,
      "column": 5,
      "reason": "plain ints are not raw pointers"
    },
    {
      "package": "tests",
      "file": "tests/ui/lowlevel_volatile_write_wrong_value.jet",
      "line": 6,
      "column": 5,
      "reason": "slot is live and pointer never escapes"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_forbidden/run.jet",
      "line": 2,
      "column": 5,
      "reason": "not allowed"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_obligation_missing.jet",
      "line": 5,
      "column": 5,
      "reason": "local pointer"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_obligation_no_bleed.jet",
      "line": 6,
      "column": 5,
      "reason": "two independent pointer operations"
    },
    {
      "package": "tests",
      "file": "tests/ui/unsafe_per_site/run.jet",
      "line": 2,
      "column": 5,
      "reason": "selection required"
    }
  ]
}
-->

### Counts

| crate/package | regions |
| --- | ---: |
| Examples | 38 |
| jet | 89 |
| jetpack | 1 |
| tests | 87 |
| **total** | **215** |

### Regions

| crate/package | file | line | reason |
| --- | --- | ---: | --- |
| Examples | Examples/features/crypto/crypto_migration.jet | 31:5 | "AES-256-GCM protocol interoperability" |
| Examples | Examples/features/crypto/crypto_suite.jet | 36:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/expert_projection.jet | 30:5 | "expert cryptography is an explicit opt-in" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 96:5 | "known-answer check reads the RFC 7748 shared secret" |
| Examples | Examples/features/crypto/primitive_matrix.jet | 158:5 | "expert known-answer vectors need raw key bytes" |
| Examples | Examples/features/crypto/random_api_split.jet | 33:5 | "compare the typed and raw HKDF rungs" |
| Examples | Examples/features/crypto/vault_keys.jet | 41:5 | "restore audited raw signing-key material" |
| Examples | Examples/features/effects/audited_gate_ladder.jet | 6:5 | "the audited block has no low-level operation" |
| Examples | Examples/features/effects/single_use_discard.jet | 35:5 | "event cancelled; the ticket admits to nothing, so voiding it is correct" |
| Examples | Examples/features/io/cleanup_matrix.jet | 34:5 | "the stop runs the shared cleanup boundary" |
| Examples | Examples/features/io/cleanup_matrix.jet | 72:5 | "the exit callbacks have no captured state" |
| Examples | Examples/features/io/os_process_control.jet | 30:5 | "POSIX process and pipe control for the core.sys surface" |
| Examples | Examples/features/io/os_stop_cleanup.jet | 24:5 | "the stop callback uses the shared cleanup boundary" |
| Examples | Examples/features/io/process_exit_cleanup.jet | 20:5 | "the exit callback has no captured state" |
| Examples | Examples/features/io/system_privileged_entry.jet | 10:5 | "run only inside a disposable user namespace with a dedicated test identity" |
| Examples | Examples/features/io/system_projection.jet | 14:5 | "the clock file was created above in this test-owned directory" |
| Examples | Examples/features/io/system_projection.jet | 20:5 | "fork and reap both children before returning" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 8:1 | "the operands are scalar registers and the early-written r10 clobber does not overlap live inputs" |
| Examples | Examples/features/lowlevel/inline_asm.jet | 19:5 | "call the audited register-only assembly contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 6:1 | "the scalar ABI contract matches the C definition" |
| Examples | Examples/features/lowlevel/inline_c.jet | 13:1 | "the scalar ABI contract deliberately returns a mismatched result" |
| Examples | Examples/features/lowlevel/inline_c.jet | 19:5 | "call the audited inline C contract" |
| Examples | Examples/features/lowlevel/inline_c.jet | 26:9 | "call the audited inline C contract with bounded Int values" |
| Examples | Examples/features/lowlevel/inline_c.jet | 32:9 | "call the audited inline C contract with a deliberately wrong C result" |
| Examples | Examples/features/lowlevel/lowlevel.jet | 8:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/mmio_board_write.jet | 9:5 | "stand-in MMIO cell stays on this stack frame" |
| Examples | Examples/features/lowlevel/opaque_handles/run.jet | 34:5 | "fixed headers define handle close ownership, exclusive buffers, and bounded output lengths" |
| Examples | Examples/features/lowlevel/pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| Examples | Examples/features/lowlevel/unsafe_obligations.jet | 9:5 | "cell stays live and the pointer remains local" |
| Examples | Examples/features/memory/expiring_secret.jet | 20:5 | "vault key import: expiring secret lifetime" |
| Examples | Examples/features/memory/pin.jet | 43:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| Examples | Examples/features/memory/rawptr.jet | 7:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| Examples | Examples/features/memory/uninit.jet | 12:5 | "uninitialized plain-data storage is filled before every read" |
| Examples | Examples/features/memory/unsafe_sentries.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| Examples | Examples/features/memory/unsafe_sentries_package_off/run.jet | 7:5 | "the local cell is live for this package-policy proof" |
| Examples | Examples/features/memory/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| Examples | Examples/features/memory/unsafe_sentries_source_off.jet | 7:5 | "the local cell is live for this raw read" |
| Examples | Examples/features/tooling/checked_text_type.jet | 20:5 | "checked text example output" |
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 55:9 | "each argument cell is inside this call's writable mapping" |
| jet | Compiler/JetBackend/Source/Image/Calls.jet | 60:5 | "loaded.entry is the selector-generated pointer-to-cells adapter; all cells remain mapped through its return" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 48:5 | "function is the resolved (ptr,len)-&gt;word runtime constructor; buffer remains readable until it returns" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 117:5 | "word is a live String handle and both functions borrow it through the canonical runtime C ABI" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 124:9 | "the runtime's borrowed UTF-8 projection holds length readable bytes until the owning String is dropped" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 144:13 | "word is a live exact-Int carrier and stringify returns a fresh owning String" |
| jet | Compiler/JetBackend/Source/Image/ComptimeValues.jet | 148:13 | "the temporary String is owned here and decoding has copied its bytes" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 100:9 | "the slot is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 106:9 | "the relocation is an aligned word inside the writable mapping, checked above" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 127:5 | "`main` is the runtime's `int32_t jet_rt_main(void (*)(void))` and `entry` is the image's checked no-argument entry" |
| jet | Compiler/JetBackend/Source/Image/Loader.jet | 149:5 | "the slot is an aligned word inside the mapping, now writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 38:1 | "a raw Linux x86-64 system call; the caller passes the call number and arguments that call expects" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 84:5 | "the caller owns `count` writable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 91:5 | "the caller owns `count` readable bytes at `address`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 99:5 | "an anonymous mapping aliases no existing memory" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 106:5 | "the caller owns the mapping it changes" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 112:5 | "the caller owns the mapping and nothing uses it any more" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 119:5 | "exit_group ends every thread of the process" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 124:1 | "stores one word at an address the caller owns and has mapped writable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 131:1 | "reads one word at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 138:1 | "reads one byte at an address the caller owns and has mapped readable" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 161:9 | "the caller owns the destination words" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 171:1 | "calls machine code; the caller vouches for the code and its System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 184:1 | "calls machine code; the caller vouches for the code and its two-word System V signature" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 199:1 | "dlopen reads the NUL-terminated path at `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 207:1 | "dlsym reads the NUL-terminated name at `name`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 230:5 | "`name` is a NUL-terminated copy of `path`" |
| jet | Compiler/JetBackend/Source/OS/Linux.jet | 242:5 | "`name` is a NUL-terminated copy of `symbol`; dlsym only reads the library's symbol table" |
| jet | Compiler/JetBackend/Tests/ComptimeScalars.jet | 29:9 | "the identity returns its single owning String unchanged; decoding has copied it" |
| jet | Core/auth/auth.jet | 126:5 | "auth PASETO v4.public verification uses the fixed Ed25519 primitive" |
| jet | Core/crypto/crypto.jet | 785:5 | "crypto expert: Argon2id password hashing" |
| jet | Core/crypto/crypto.jet | 812:5 | "crypto expert: Argon2id password verification" |
| jet | Core/crypto/vault.jet | 70:5 | "vault typed load provider boundary" |
| jet | Core/crypto/vault.jet | 77:5 | "vault typed current provider boundary" |
| jet | Core/crypto/vault.jet | 84:5 | "vault typed status provider boundary" |
| jet | Core/crypto/vault.jet | 91:5 | "vault typed versions provider boundary" |
| jet | Core/crypto/vault.jet | 98:5 | "vault typed generation provider boundary" |
| jet | Core/crypto/vault.jet | 105:5 | "vault typed store provider boundary" |
| jet | Core/crypto/vault.jet | 113:5 | "vault key import: signing key material" |
| jet | Core/crypto/vault.jet | 121:5 | "vault key import: X25519 secret material" |
| jet | Core/crypto/vault.jet | 129:5 | "vault wrapped import provider boundary" |
| jet | Core/crypto/vault.jet | 136:5 | "vault typed rotation provider boundary" |
| jet | Core/crypto/vault.jet | 144:5 | "vault typed retirement provider boundary" |
| jet | Core/crypto/vault.jet | 152:5 | "vault typed revocation provider boundary" |
| jet | Core/crypto/vault.jet | 160:5 | "vault write authorization provider boundary" |
| jet | Core/crypto/vault.jet | 168:5 | "vault wrapped authorization provider boundary" |
| jet | Core/crypto/vault.jet | 176:5 | "vault typed generation commit provider boundary" |
| jet | Core/crypto/vault.jet | 184:5 | "vault typed store commit provider boundary" |
| jet | Core/crypto/vault.jet | 192:5 | "vault key import commit: signing key material" |
| jet | Core/crypto/vault.jet | 200:5 | "vault key import commit: X25519 secret material" |
| jet | Core/crypto/vault.jet | 208:5 | "vault wrapped import commit provider boundary" |
| jet | Core/crypto/vault.jet | 216:5 | "vault typed rotation commit provider boundary" |
| jet | Core/crypto/vault.jet | 224:5 | "vault typed retirement commit provider boundary" |
| jet | Core/crypto/vault.jet | 232:5 | "vault typed revocation commit provider boundary" |
| jet | Core/crypto/vault.jet | 239:5 | "vault passphrase export provider boundary" |
| jet | Core/crypto/vault.jet | 247:5 | "vault recipient export provider boundary" |
| jet | Core/email/email.jet | 545:13 | "read the DKIM key length, then wipe this copy" |
| jet | Core/mem/mem.jet | 36:1 | "`addr` is the address of a live, aligned `T` that stays valid for every use of the returned pointer" |
| jet | Core/mem/mem.jet | 40:1 | "`pointer` names a live, aligned, readable `T` for the whole access" |
| jet | Core/mem/mem.jet | 44:1 | "`pointer` names a live, aligned, writable `T` for the whole access" |
| jet | Core/sys/sys.jet | 158:1 | "posix umask: the caller owns the process file-creation mask and restores it when other code relies on it" |
| jet | Core/sys/sys.jet | 174:5 | "posix getpgid: inspect a process group" |
| jet | Core/sys/sys.jet | 180:5 | "posix getsid: inspect a process session" |
| jet | Core/sys/sys.jet | 185:1 | "posix getpriority: the caller vouches that `which` names this process or one it controls" |
| jet | Core/sys/sys.jet | 189:1 | "posix setpriority: the caller owns the scheduling priority of the process it changes" |
| jet | Core/sys/sys.jet | 193:1 | "posix utime: the caller owns the file whose timestamps it rewrites" |
| jet | Core/sys/sys.jet | 199:1 | "posix stop: the caller accepts that the process ends here after the shared cleanup boundary" |
| jet | Core/sys/sys.jet | 206:1 | "posix pipe: the caller owns both returned descriptors and closes each exactly once" |
| jet | Core/sys/sys.jet | 210:1 | "posix close_fd: the caller owns `fd` and nothing uses it after this call" |
| jet | Core/sys/sys.jet | 214:1 | "posix mkfifo: the caller owns `path` and removes the named pipe it creates" |
| jet | Core/sys/sys.jet | 218:1 | "posix fork: the caller handles both the parent and the child return and reaps the child" |
| jet | Core/sys/sys.jet | 222:1 | "posix setsid: the caller owns the process session topology it changes" |
| jet | Core/sys/sys.jet | 226:1 | "posix wait: the caller owns the child processes it reaps" |
| jet | Core/sys/sys.jet | 230:1 | "posix waitpid: the caller owns the child process it reaps" |
| jet | Core/sys/sys.jet | 234:1 | "posix setuid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 238:1 | "posix setgid: the caller drops or changes process credentials deliberately" |
| jet | Core/sys/sys.jet | 242:1 | "posix setpgrp: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 246:1 | "posix setpgid: the caller owns the process group topology it changes" |
| jet | Core/sys/sys.jet | 250:1 | "posix kill: the caller owns or is entitled to signal the target process" |
| jet | Core/sys/sys.jet | 254:1 | "posix initgroups: the caller changes supplementary groups deliberately" |
| jet | Jetpack/Publication/Source/Sign.jet | 58:5 | "export a fresh Core signing seed only to descriptor-relative owner-only key publication; the Core key owner is wiped on every outcome" |
| jet | Jetpack/Publication/Source/Sign.jet | 113:5 | "import the protected seed into Core signing without a second raw-secret copy; close the temporary Core Secret on success and failure" |
| jet | Jetpack/Publication/Source/Sign.jet | 137:5 | "package author signatures require Core's strict Ed25519 verification; no raw verifier or helper process" |
| jet | Jetpack/Publication/Tests/Index.jet | 266:5 | "the test owns the planted lock file it ages" |
| jet | Jetpack/Publication/Tests/Sign.jet | 164:5 | "original signing secret-erasure acceptance requires the Core volatile in-place wipe provider (#3597)" |
| jet | Jetpack/Store/Tests/ClosureGraph.jet | 59:13 | "the test owns the journal file it just wrote" |
| jet | Jetpack/Store/Tests/Seal.jet | 31:5 | "the store test owns the fixture file whose mtime it pins" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/mmio_fixed.jet | 4:5 | "board.sensor_v1 GPIO register 0x40000100 is a volatile MMIO word" |
| jet | Tools/agent-eval/domain-foundations/area-embedded/pkg/run.jet | 111:5 | "host replay uses a stack cell as a stand-in for the board register" |
| jetpack | crates/jetpack/batteries/embedded/embedded.jet | 181:9 | "host replay uses a stack cell for the CMSDK UART0 control register and never dereferences the target address" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_open.jet | 8:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/aes256gcm_seal.jet | 7:5 | "fixed NIST AES-256-GCM vector" |
| tests | tests/conformance/corpus/core/crypto/expert/argon2id.jet | 8:5 | "fixed Argon2id known-answer vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_sign.jet | 6:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/ed25519_verify_strict.jet | 7:5 | "fixed RFC 8032 Ed25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/hkdf_sha256_raw.jet | 8:5 | "fixed RFC 5869 HKDF vector" |
| tests | tests/conformance/corpus/core/crypto/expert/migrate_v1.jet | 10:5 | "fixed JETC v1 source failure" |
| tests | tests/conformance/corpus/core/crypto/expert/open_v1.jet | 7:5 | "fixed JETC v1 AES vector" |
| tests | tests/conformance/corpus/core/crypto/expert/secret_bytes.jet | 7:5 | "inspect fixed secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/shared_secret_bytes.jet | 9:5 | "inspect shared-secret length" |
| tests | tests/conformance/corpus/core/crypto/expert/signing_key_bytes.jet | 7:5 | "inspect generated signing-key bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_raw.jet | 7:5 | "fixed RFC 7748 X25519 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/x25519_secret_bytes.jet | 7:5 | "inspect generated X25519 secret bytes" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_open.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/expert/xchacha20poly1305_seal.jet | 9:5 | "fixed CFRG XChaCha20-Poly1305 vector" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_signing.jet | 5:5 | "restore a fixed signing key into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/commit_import_x25519.jet | 5:5 | "restore a fixed X25519 secret into the vault" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_signing.jet | 5:5 | "restore a fixed signing-key vector" |
| tests | tests/conformance/corpus/core/crypto/vault/prepare_import_x25519.jet | 5:5 | "restore a fixed X25519 secret vector" |
| tests | tests/conformance/corpus/core/crypto/x25519.jet | 9:5 | "inspect shared-secret length" |
| tests | tests/conformance/corpus/core/mem/volatile_read.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/conformance/corpus/core/mem/volatile_write.jet | 6:5 | "value is live on this stack frame and the pointer never escapes" |
| tests | tests/conformance/corpus/core/sys/close_fd.jet | 6:9 | "close an invalid descriptor without changing process state" |
| tests | tests/conformance/corpus/core/sys/fork.jet | 5:5 | "fork and let the child return immediately" |
| tests | tests/conformance/corpus/core/sys/getpriority.jet | 5:5 | "read the current process priority" |
| tests | tests/conformance/corpus/core/sys/initgroups.jet | 5:5 | "probe initgroups with a stable missing user" |
| tests | tests/conformance/corpus/core/sys/kill.jet | 5:5 | "check the process group without sending a signal" |
| tests | tests/conformance/corpus/core/sys/mkfifo.jet | 5:5 | "probe FIFO creation with an empty path" |
| tests | tests/conformance/corpus/core/sys/pipe.jet | 5:5 | "open a pipe and observe its descriptor pair" |
| tests | tests/conformance/corpus/core/sys/setgid.jet | 5:5 | "probe setgid with an invalid identifier" |
| tests | tests/conformance/corpus/core/sys/setpgid.jet | 5:5 | "set the current process into its existing process group" |
| tests | tests/conformance/corpus/core/sys/setpgrp.jet | 5:5 | "set the current process group" |
| tests | tests/conformance/corpus/core/sys/setpriority.jet | 5:5 | "lower the current process priority for this short-lived witness" |
| tests | tests/conformance/corpus/core/sys/setsid.jet | 5:5 | "create a session when the host permits it" |
| tests | tests/conformance/corpus/core/sys/setuid.jet | 5:5 | "probe setuid with an invalid identifier" |
| tests | tests/conformance/corpus/core/sys/stop.jet | 6:9 | "stop this dedicated witness successfully" |
| tests | tests/conformance/corpus/core/sys/umask.jet | 5:5 | "set a deterministic mask for this short-lived witness" |
| tests | tests/conformance/corpus/core/sys/utime.jet | 5:5 | "probe timestamp update with an empty path" |
| tests | tests/conformance/corpus/core/sys/wait.jet | 5:5 | "wait for a child that this witness does not have" |
| tests | tests/conformance/corpus/core/sys/waitpid.jet | 5:5 | "poll for a child that this witness does not have" |
| tests | tests/fixtures/unsafe_obligations/run.jet | 5:5 | "local pointer" |
| tests | tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet | 26:5 | "AES-256-GCM protocol interoperability" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| tests | tests/ui/audited_gate_ladder_forbidden/run.jet | 3:5 | "the organization policy refuses this audited escape" |
| tests | tests/ui/cffi_out_pointer_requires_unsafe.jet | 12:1 | "the local slot remains live for the complete C call" |
| tests | tests/ui/checked_text_raw_requires_unsafe.jet | 22:5 | "audited literal" |
| tests | tests/ui/crypto_argon2id_literal_policy.jet | 7:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_argon2id_literal_policy_type_precedence.jet | 7:5 | "fixed password-hash vector" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 7:5 | "protocol-selected password policy" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 15:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_expert_material_length.jet | 12:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_material_length_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 5:5 | "protocol-selected interop material" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 17:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_callback_bound_precedence.jet | 13:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_correct.jet | 5:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_effect_bound_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_prohibition_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_unknown_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_length.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_region_authority_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_length.jet | 7:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 6:5 | "protocol-selected output length" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 14:5 | "fixed interop vectors" |
| tests | tests/ui/ffi_asm_duplicate_clobber.jet | 3:1 | "each discarded register is named once" |
| tests | tests/ui/ffi_asm_float_signature.jet | 3:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_missing_return_anchor.jet | 3:1 | "cycle counter" |
| tests | tests/ui/ffi_asm_no_mem.jet | 1:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_return_clobber.jet | 3:1 | "the result register is overwritten by the body" |
| tests | tests/ui/ffi_asm_unknown_clobber.jet | 3:1 | "the body declares only audited registers" |
| tests | tests/ui/ffi_body_not_string.jet | 1:1 | "demo" |
| tests | tests/ui/ffi_inline_c_mismatch.jet | 1:1 | "deliberate mismatch fixture" |
| tests | tests/ui/ffi_unknown_language.jet | 1:1 | "demo" |
| tests | tests/ui/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| tests | tests/ui/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| tests | tests/ui/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
| tests | tests/ui/unsafe_forbidden/run.jet | 2:5 | "not allowed" |
| tests | tests/ui/unsafe_obligation_missing.jet | 5:5 | "local pointer" |
| tests | tests/ui/unsafe_obligation_no_bleed.jet | 6:5 | "two independent pointer operations" |
| tests | tests/ui/unsafe_per_site/run.jet | 2:5 | "selection required" |
<!-- unsafe-ratchet:end -->

## CoreLib unsafe audit

The ratchet checker also audits the CoreLib (`Core/**`), independently of
sema. Two rules hold there:

- Every raw operation sits inside an audited gate: postfix dereference
  `p.*`, `Ptr<T>.from_addr`, `core.mem.volatile_read`/`volatile_write`, and
  the POSIX controls sema gates with E3101 (`core.sys.fork`, `kill`, `setuid`,
  …). The scan ignores comments and string literals.
- A CoreLib function that opens an `#Unsafe` block without being an `#Unsafe`
  function discharges the obligation for every caller. Unless it is recorded
  below as a safe abstraction with its proof, it fails: a plain `pub fn` around
  a gated operation would let safe code reach that operation with no gate of
  its own. A wrapper whose caller must vouch for the operation is written
  `#Unsafe("<caller contract>") pub fn …` instead, so every call needs its own
  `#Unsafe` region (E3103), whether it goes through the module alias or a
  direct import. `core.mem.from_addr`, `volatile_read`, `volatile_write` and
  the `core.sys` POSIX controls are such functions.

The registry below is written by reviewers, one row per safe abstraction:
its file, function, and the proof that no caller can break memory safety
through it. The checker never writes a proof; it fails on a missing row and on
a row whose function no longer opens a gate.

<!-- unsafe-abstractions:begin -->
```json
[
  {"file": "Core/auth/auth.jet", "function": "verify_paseto", "proof": "The strict Ed25519 verifier only reads `key`, the PAE-encoded message and the 64-byte signature, all owned byte lists sliced and length-checked above; it returns a Bool and exposes no key material."},
  {"file": "Core/crypto/crypto.jet", "function": "password_hash_with_salt", "proof": "Password and salt lengths are bounded before Argon2id runs with fixed parameters (m=65536, t=3, p=1, 32-byte output); the derived key stays in a Secret that only feeds the PHC encoding."},
  {"file": "Core/crypto/crypto.jet", "function": "password_verify", "proof": "Every PHC field is parsed and bounded (memory, iterations, lanes, salt and digest lengths) before Argon2id runs; the derived key is compared in constant time and never leaves the function."},
  {"file": "Core/crypto/jetc.jet", "function": "seal", "proof": "Expert AEAD seal is safe here because the nonce is unique per (key, record): the writer owns a fresh random key and 16-byte prefix (:151-195); each record's nonce = prefix ∥ LE64(index) (:62-65), with the header at index 0 (:189) and chunks at index 1.. strictly increasing (:194-210), bounded by MAX_RECORDS, and the final state cannot be reused. AAD is a fixed domain per record kind (:73-94). Key / nonce lengths are enforced by expert (expert.jet:499-509)."},
  {"file": "Core/crypto/jetc.jet", "function": "open", "proof": "Expert AEAD open is safe here because inputs are bounded before the call (headers :215-226, record shape / count :264-269); plaintext becomes a Secret only from an authenticated open result (:270-275); the coordinator writes only after authentication (crypto.jet:652-653) and requires the final record (:277-279) before publish (crypto.jet:656-657)."},
  {"file": "Core/crypto/crypto.jet", "function": "__migrate_v1", "proof": "Reachable only from expert.migrate_v1 (expert.jet:624, the sole caller), which the checker restricts to an importing core.crypto.expert caller inside #Unsafe (CheckerCoreLib/core_call.rs:2553-2577). The legacy buffer is bounded (≤1 GiB + 34, :666-675); the plaintext is a Secret only after authentication (:676-680); the staged v2 output is verified (:919-929) before publication."},
  {"file": "Core/crypto/vault.jet", "function": "load", "proof": "`validate_key` rejects a forged or foreign key reference before the typed provider runs; the provider returns the typed key value, never raw key bytes."},
  {"file": "Core/crypto/vault.jet", "function": "current", "proof": "`validate_name` rejects an invalid vault name before the typed provider runs; the result is a typed key reference, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "status", "proof": "`validate_key` rejects a forged or foreign key reference before the provider runs; the result is a KeyStatus value."},
  {"file": "Core/crypto/vault.jet", "function": "versions", "proof": "`validate_name` rejects an invalid vault name before the provider runs; the result is a list of typed key references."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_generate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_store", "proof": "`validate_name` runs first and the key moves (`^key`) into the provider, so the caller keeps no copy; the result is a MutationPlan."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_signing", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move (`^bytes`) into the provider, so no raw copy remains with the caller."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_x25519", "proof": "`validate_name` runs and the material must be exactly 32 bytes; the bytes move (`^bytes`) into the provider, so no raw copy remains with the caller."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_import_wrapped", "proof": "`validate_name` runs and the wrapped blob is bounded to 16..8192 bytes before the provider decrypts it under `unlock`; the result is a WrappedImportPlan."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_rotate", "proof": "`validate_name` runs first; the provider returns a MutationPlan that only `authorize_write` and a commit can act on."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_retire", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "prepare_revoke", "proof": "`validate_key` and `validate_reason` run first; the provider returns a MutationPlan, not key material."},
  {"file": "Core/crypto/vault.jet", "function": "authorize_write", "proof": "`validate_plan` and `validate_reason` run first; the plan is lent read-only and the result is a single-use VaultWrite token."},
  {"file": "Core/crypto/vault.jet", "function": "authorize_wrapped_import", "proof": "`validate_wrapped_plan` and `validate_reason` run first; the result is a single-use VaultWrite token."},
  {"file": "Core/crypto/vault.jet", "function": "commit_generate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_store", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_signing", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_x25519", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_import_wrapped", "proof": "The write token and wrapped plan are validated and both move into the provider, so each is used once; the result is a typed key reference."},
  {"file": "Core/crypto/vault.jet", "function": "commit_rotate", "proof": "The write token and plan are validated and both move into the provider, so each is used once; the result is a Rotation of typed key references."},
  {"file": "Core/crypto/vault.jet", "function": "commit_retire", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/crypto/vault.jet", "function": "commit_revoke", "proof": "The write token and plan are validated and both move into the provider, so each is used once; nothing is returned but the error."},
  {"file": "Core/crypto/vault.jet", "function": "export_to_passphrase", "proof": "`validate_key` runs first and the passphrase is lent read-only; the result is passphrase-encrypted wrapped bytes, never the plain key."},
  {"file": "Core/crypto/vault.jet", "function": "export_to_recipients", "proof": "`validate_key` runs and the recipient list is bounded to 1..16; the result is recipient-encrypted wrapped bytes, never the plain key."},
  {"file":"Core/email/email.jet","function":"decode_dkim","proof":"The gate reads and writes Secret.bytes, not arbitrary memory. At email.jet:78-95 a nonempty length divisible by four bounds every quartet access and canonical padding/alphabet checks reject malformed input. The only decoded allocation is the fixed 32-byte Secret at :82; :96-104 guard every write with written < 32, never grow a secret-bearing List, and :107 rejects any non-32-byte result. Errors drop the zeroizing Secret; success moves it at :108. Callers supply only a borrowed Secret and cannot choose unchecked indices or receive a plain decoded byte list."},
  {"file":"Core/data/data.jet","function":"bridge_tool_on_path","proof":"__host_path_is_file invokes OsStr::from_encoded_bytes_unchecked (Top/DataStats.rs:11-13), so its input must retain native encoded-byte provenance. data.jet:1090 obtains PATH from the current host's encoded environment; :1092-1108 split only at ASCII colon/semicolon and remove Windows ASCII quotes. :1111-1117 append only ASCII separators and valid UTF-8 tool text, preserving that host encoding. The metadata leaf borrows the owned list and returns Bool/error, not a pointer; the caller cannot supply an unrelated byte allocation to this helper or retain a host borrow."},
  {"file":"Core/data/data.jet","function":"data_loader_render","proof":"The gate at data.jet:1729 only matches the compiler-private DataTree Number, TypedText and Bytes variants. Each payload is accessed after its matching discriminant at :1730, :1734 or :1738, then read through ordinary String/List operations into the caller's exclusively borrowed output; no unchecked memory, forged carrier or raw pointer is created. Other variants leave the gate for normal recursive rendering. Any typed DataTree input remains a valid enum; callers cannot select a mismatched native payload."},
  {"file":"Core/process/process.jet","function":"cmd","proof":"The __spec_fields leaf is layout transfer into ProcessSpec, not process execution (Top/ProcessSpec.rs:6-37). process.jet:92 takes all fields from default_fields(:419-428): paired environment columns are both empty; optional cwd, stdin, timeout, resource limits, terminal and policy are accompanied by their actual presence flags at :95-104, with valid typed placeholders. The sole present numeric limit is 67108864. argv is a typed owned String list; even empty argv is deferred to checked spawn, not indexed here. No descriptor or memory address is supplied or returned, and callers cannot alter the hidden initialized field set."},
  {"file":"Core/http/server.jet","function":"response_body","proof":"The __response_parts gate at server.jet:76 transfers typed Headers and one-shot Body ownership into the response without reading the body. :72-75 normalize every caller status to 100..599 before I64 conversion; all other transport fields at :78-79 are initialized typed constants, absent slots, or empty lists. The consuming body/headers parameters at :69 prevent the caller from reusing moved stream ownership. No pointer or foreign callback is accepted; invalid status cannot reach the unchecked head transfer."},
  {"file":"Core/http/server.jet","function":"access_log_template","proof":"The gate at server.jet:139 calls __route_template with a borrowed typed HTTPRequest. Its provider Top/HTTPServer.rs:6321-6322 only clones the request's stored Option<String>; it neither dereferences caller-supplied addresses nor mutates routing state, consumes the request, or exports a borrowed native reference. Every caller therefore receives owned optional text regardless of whether route metadata was present."},
{"file":"Core/db/db.jet","function":"close","proof":"db.jet:40 consumes the Close resource before passing its private raw field to __queue_raw_close(:42). The bridge uses a u64 registry key, not a memory address (jet-pkg-model/Prelude/DB.rs:66-67): removal drops the owned connection and absent keys return false. Thus even an already-absent handle cannot dereference freed storage; no connection borrow or raw pointer escapes the consuming close."},
{"file":"Core/db/db.jet","function":"__queue_open","proof":"__queue_raw_open accepts an ordinary String path and registers the owned SQLite connection under a fresh numeric key (jet-pkg-model/Prelude/DB.rs:40-48). db.jet:49 rejects the failure sentinel zero and :50 immediately adopts the key into the Close resource; no native pointer or connection reference is returned. Arbitrary caller paths produce a resource or None, not an unchecked memory capability."},
{"file":"Core/db/db.jet","function":"__queue_begin","proof":"The gate at db.jet:54 lends driver.raw to __queue_raw_begin without transferring the Close resource. The bridge fixes the statement to BEGIN and resolves the numeric key in DB_CONNS while its map borrow is live (jet-pkg-model/Prelude/DB.rs:95-100,115-117); missing keys return false. Callers cannot supply SQL or a pointer to this operation, and the native connection reference does not escape."},
{"file":"Core/db/db.jet","function":"__queue_commit","proof":"db.jet:57 lends only the typed driver's numeric key to __queue_raw_commit. The bridge selects the fixed COMMIT statement and performs the registry lookup under a live map borrow (jet-pkg-model/Prelude/DB.rs:95-100,119-121), returning false for an absent key. No raw address or native reference reaches the caller, including after failed transaction admission."},
{"file":"Core/db/db.jet","function":"__queue_rollback","proof":"db.jet:60 lends only the typed driver's numeric key to __queue_raw_rollback. Its bridge selects the fixed ROLLBACK statement and keeps the registry borrow through execution (jet-pkg-model/Prelude/DB.rs:95-100,123-125); absence is false rather than a dangling dereference. It exposes neither a native connection nor caller-selected memory."},
{"file":"Core/db/db.jet","function":"__queue_execute","proof":"db.jet:62-64 hold the exclusive __QueueDb borrow while __queue_raw_execute marshals typed String SQL and DBValue parameters. The bridge looks up the numeric key, rejects absence, owns decoded parameters and lends them only while the prepared statement executes (jet-pkg-model/Prelude/DB.rs:183-199). No caller address is interpreted and no statement or parameter reference escapes in the owned Int/DBError result."},
{"file":"Core/db/db.jet","function":"__queue_query","proof":"db.jet:67-69 hold the exclusive __QueueDb borrow through __queue_raw_query; SQL and parameters are typed owned values, not pointers. The bridge rejects an absent registry key and keeps connection, decoded parameters and statement alive through row reads (jet-pkg-model/Prelude/DB.rs:131-151), encoding owned results. Returned String/DBValue rows contain no references into SQLite storage, so callers cannot outlive a native row borrow."},
{"file":"Core/net/net.jet","function":"ip_addr","proof":"__ip_from_bytes expects exactly four or sixteen octets. net.jet:136 accepts only parse_ip_bytes success: parse_ipv4_bytes(:960-979) checks four decimal octets <=255, and the IPv6 parser(:982-1048) checks eight 16-bit words including compression before expanding to sixteen bytes. The exact owned list moves at :138; callers cannot substitute a differently sized list after validation."},
{"file":"Core/net/net.jet","function":"ip_to_string_impl","proof":"net.jet:143-144 call __ip_bytes on a borrowed typed IPAddr; Top/NetHTTP.rs:1944-1948 discriminant-matches its native IPv4/IPv6 enum and copies exactly its initialized octets into an owned list. Formatting only reads that list. No caller-selected index, raw pointer or borrowed native storage escapes."},
{"file":"Core/net/net.jet","function":"ip_is_ipv4","proof":"__ip_is_v4 at net.jet:149 only matches the valid native IPAddr discriminant (Top/NetHTTP.rs:1951-1952). Any typed address is admitted; no payload reinterpretation, mutation or memory borrow is returned, only Bool."},
{"file":"Core/net/net.jet","function":"socket_addr_parse","proof":"At net.jet:182-189 parsing produces exactly sixteen IPv6 bytes or four IPv4 bytes, with the IPv6 length checked at :183. parse_port(:846-856) bounds decimal port to 0..65535 and parse_scope(:1144-1154) bounds scope to 0..4294967295 before :193-194 consume the octets and IP into __ip_from_bytes/__socket_from_parts. Flow is the constant zero. Callers cannot select uninitialized representation fields or overflow the U16/U32 transfer."},
{"file":"Core/net/net.jet","function":"socket_host","proof":"__socket_ip at net.jet:199 borrows a valid SocketAddr and copies its native IP enum (Top/NetHTTP.rs:2038-2039). ip_to_string_impl then reads copied octets for source formatting; the caller receives owned text, not a reference into the socket carrier. No raw address is accepted."},
{"file":"Core/net/net.jet","function":"socket_port","proof":"__socket_port at net.jet:202 borrows a typed SocketAddr and widens its initialized native u16 port to Int (Top/NetHTTP.rs:2042-2043). This scalar projection accepts every valid carrier and neither exposes a borrow nor changes socket ownership."},
{"file":"Core/net/net.jet","function":"socket_scope","proof":"__socket_scope at net.jet:215 matches the valid socket enum, returning None for IPv4 or widening the IPv6 u32 scope (Top/NetHTTP.rs:2046-2050). Source substitutes zero for None. No reinterpretation or caller-controlled pointer is involved and only an owned scalar escapes."},
{"file":"Core/net/net.jet","function":"udp_packet_addr","proof":"__udp_packet_addr at net.jet:413 borrows a typed UDPPacket and clones its stored SocketAddr (Top/NetHTTP.rs:2851-2852); the packet remains live during the projection. The return carrier owns its address rather than pointing into packet storage, so releasing the packet cannot invalidate the result."},
{"file":"Core/jobs/jobs.jet","function":"queue","proof":"The gates at jobs.jet:130-136 call __path_kind and __create_dir_all_raw with owned String paths computed by queue_default_path/files.dirname, after endpoint authority validation at :124-126. Metadata is a scalar kind query and recursive mkdir returns owned IOError; neither leaf accepts or returns memory addresses. The kind check at :131-132 rejects observed symlinks before the checked SQLite open. Path races do not create a borrowed descriptor or dangling pointer because these are independent pathname operations, not identity-handle transfers."},
{"file":"Core/jobs/jobs.jet","function":"wait","proof":"The unsafe __sleep_ms leaf casts signed milliseconds to unsigned host Duration (Top/ServiceAuthority.rs:3792-3793). jobs.jet:386-387 reject negative/oversized timeout and :393 clamps the exact delay passed at :395 to 1..25, independently of wall-clock movement. No caller can reach the cast with a negative or oversized delay; the leaf only sleeps and exports no memory."},
{"file":"Core/jobs/jobs.jet","function":"queue_publish_transition","proof":"jobs.jet:486 constructs owned JSON fields and :487 clamps negative timestamps before __publish_parts(:489). The provider Top/ServiceAuthority.rs:3772-3787 validates JetDevtoolsEvent::from_parts before publication, owns entity/fields and borrows source/kind only during the call. No native pointer, callback or mutable queue storage is transferred to a sink; caller-chosen observation text cannot escape that validation boundary."},
{"file":"Core/jobs/jobs.jet","function":"queue_publish_status","proof":"jobs.jet:499-502 assemble owned status fields/entity and nonnegative timestamp, then __publish_parts(:504) invokes the existing validating JetDevtoolsEvent::from_parts gate (Top/ServiceAuthority.rs:3779-3786). The sink gets an owned validated event rather than references into JobQueueStatus, so callers cannot make event storage outlive a borrowed source value."},
{"file":"Core/testing/testing.jet","function":"testing_byte","proof":"__string_byte performs an unchecked native byte read. Its local helper testing.jet:353-355 is reached by testing_boundary only with 0 < position < text.count_bytes()(:362-363), by testing_diff_lines with cursor < end <= length(:387-393), by testing_append_bytes over a bounded prefix(:408-409,412-457), and testing_ascii_name over 0..<count_bytes(:508-509). These establish nonnegative in-storage indices before U64 conversion; none expose the index-taking helper as a public Core member or allow the String borrow to escape."},
{"file":"Core/testing/testing.jet","function":"testing_owned_text","proof":"__bytes_into_text at testing.jet:376 requires valid UTF-8. The only callers are testing_unified_diff(:503) and testing_ascii_name(:517). Diff output appends complete String bytes or scalar-aligned prefixes chosen by testing_boundary(:362-366,423-424,450-451); marker fragments are ASCII. Name output(:508-515) appends only ASCII alphanumerics, hyphen or underscore. Each exact owned list moves into String with no mutation between construction and transfer; arbitrary caller bytes cannot reach this local helper."},
{"file":"Core/tui/tui.jet","function":"capability_env_bytes","proof":"__cap_env_bytes at tui.jet:101 borrows a String name and returns an owned optional byte list. Its provider Ui.rs:514-515 obtains var_os and consumes OsString into encoded bytes; no borrowed environment storage is returned and source does not assume UTF-8 (capability_text uses checked String.from_bytes at :112). Arbitrary names cannot manufacture a native pointer or invalidate returned storage."},
{"file":"Core/tui/tui.jet","function":"capability_stdout_terminal","proof":"The gate at tui.jet:126-127 invokes __stdout_terminal with no caller-supplied values and returns only a copied Bool host terminal fact at :129. It lends no memory, descriptor ownership or callback to the caller; host terminal presence or absence cannot change the typed result's validity."},
{"file":"Core/tui/tui.jet","function":"normalized_style_test_node","proof":"The local test helper at tui.jet:651-655 calls __node_transfer with initialized text/dimensions, fixed kind/role, an empty child list, explicitly absent accessibility slots and present style slots. Its sole caller(:659-663) obtains StyleWords from style_words(:416-421), whose three flags are bounded bits 1/2/4 and whose colors are normalized by color_words. No arbitrary numeric representation or uninitialized optional slot is passed; the returned retained UINode owns its payload."},
  {"file":"Core/encoding/cbor.jet","function":"validate","proof":"The cbor.jet:95 gate only matches DataTree's private Number/TypedText variants to reject them(:97-98), and recursively validates initialized Array/Object payloads(:99-104). Discriminant matching precedes payload access; ordered Object storage is borrowed through object_entries while value remains live. No raw address, variant transmute or payload reference escapes in the error/unit result."},
  {"file":"Core/encoding/cbor.jet","function":"encode_value","proof":"The cbor.jet:157 gate grants access to private DataTree.Bytes only after its matching branch(:171); all byte reads iterate the owned list length(:173). Object entry views are taken only in the Object branch(:179-180) and retain the value borrow throughout bounded index ordering. encode(:111) validates the tree first, rejecting internal Number/TypedText. The caller's exclusively borrowed output receives copied bytes, never native payload references."},
  {"file":"Core/encoding/cbor_stream.jet","function":"retain_key","proof":"__bytes_into_text at cbor_stream.jet:208 consumes stored, which :205 fills with exactly text.count_bytes() bytes obtained through bounds-checked stream.byte_at from a valid String. It neither truncates a scalar nor edits its encoding. Only the exact owned copy moves into the retained key; arbitrary wire bytes cannot reach this transfer."},
  {"file":"Core/encoding/cbor_stream.jet","function":"next_inner","proof":"For a CBOR text item cbor_stream.jet:303 obtains the owned payload, :306 strictly validates that exact list's UTF-8 and returns Syntax on failure, then :308 immediately moves it into __bytes_into_text. No mutation intervenes, and binary items(:304) never use the text transfer. Host/wire callers cannot bypass the validator with malformed payload bytes."},
  {"file":"Core/encoding/csv.jet","function":"close_field","proof":"__bytes_into_text at csv.jet:124 consumes the whole-value parser's field. typed_records(:213-222) obtains complete scalar byte sequences from a valid String, and TypedCSV.push(:138-200) either appends the whole sequence or replaces/removes ASCII CSV syntax; doubled quotes and CRLF are ASCII. Thus field is empty or concatenated complete UTF-8 scalars at every close; it is moved and reset before reuse(:124-126), never exposed as arbitrary caller bytes."},
  {"file":"Core/encoding/csv.jet","function":"query_text","proof":"csv.jet:315-316 enforce the bounded read's size/list limits and :317 validates sample.bytes with the strict shared UTF-8 validator. The gate at :318-319 immediately moves that same owned list into __bytes_into_text; failures return FieldError before conversion. A caller's arbitrary file contents cannot produce an invalid String or retain a native read-buffer borrow."},
  {"file":"Core/encoding/csv.jet","function":"finish_field","proof":"CSVReader.character(:725-744) validates lead width, all continuation bytes, overlong/surrogate/out-of-range cases before next_inner(:880-882) calls push. push(:798-874) appends each entire validated scalar or only ASCII quote/CRLF escapes. Consequently finish_field's __bytes_into_text(:769) moves only complete UTF-8 storage, then resets field(:771). Partial/invalid wire scalars fail without reaching this gate."},
  {"file":"Core/encoding/json.jet","function":"emit_coercion","proof":"json.jet:101-102 calls __coerce_emit with ordinary borrowed String text; it supplies no pointer, descriptor or callback. The reserved leaf preserves the existing stderr emission boundary, and the helper returns no borrowed storage. Caller-selected message bytes remain valid UTF-8 and cannot become an unchecked address or outlive the call as a native reference."},
  {"file":"Core/encoding/json.jet","function":"coerce_line","proof":"The gate at json.jet:109-110 calls __coerce_now with no arguments and assigns its owned Int clock snapshot. Subsequent interpolation(:112) is safe String construction. No caller can choose a raw memory operand or receive a host borrow through this parameterless scalar query."},
  {"file":"Core/encoding/json.jet","function":"append_events","proof":"json.jet:707-709 match each private DataTree.Number/TypedText/Bytes discriminant before projecting its initialized String/list into the corresponding typed DataEvent. Only owned enum payloads are returned; no mismatched variant access, pointer formation or borrowed backing view escapes. Public variants are recursively handled outside the gate(:712-735)."},
  {"file":"Core/encoding/json.jet","function":"write_json","proof":"json.jet:855-870 discriminate private Number/TypedText/Bytes before reading each payload. Number/text are ordinary String inputs to output helpers; Bytes are read only over 0..<bytes.len()(:865-868). The caller holds an exclusive output-list borrow, which receives copied encoding bytes; no native storage reference or unchecked index is returned. Other variants leave the gate for normal recursive rendering."},
  {"file":"Core/encoding/stream.jet","function":"object_entries","proof":"__object_entries_view requires an Object and returns a borrowed ordered backing view. stream.jet:11 checks that exact discriminant immediately before :13; other variants panic before the intrinsic. The result's `from value` lifetime at :10 ties the view to the same borrowed DataTree, so callers cannot keep it after releasing its owner or reinterpret another payload."},
  {"file":"Core/encoding/stream.jet","function":"object_from_entries","proof":"__object_from_entries at stream.jet:21 receives a consuming list of typed String/DataTree entries, moving its sole allocation into an initialized Object carrier. No pointer, length or discriminant is supplied separately, and the caller cannot reuse the moved list. Arbitrary duplicate keys affect encoding policy, not the validity or ownership of the native payload."},
  {"file":"Core/encoding/stream.jet","function":"bytes_payload","proof":"stream.jet:27 checks private DataTree.Bytes before __bytes_view on the same value; all other variants panic(:29). The View<U8> result is declared `from value`(:25), retaining the owner borrow rather than exporting an independent pointer. Callers cannot select a different variant's backing or use the view after the carrier's lifetime."},
  {"file":"Core/encoding/stream.jet","function":"typed_number","proof":"The gate at stream.jet:33-34 constructs the compiler-private Number enum from a consuming typed String; it does not reinterpret memory. Its source caller json.jet:562 is in Parser.number after numeric grammar validation and checked String construction(:561). The owned lexeme is retained for typed decoding, never used as an address or unchecked index; consumers use normal checked numeric parsing. Callers cannot corrupt enum layout or retain the moved String's storage."},
  {"file":"Core/encoding/stream.jet","function":"typed_string","proof":"__typed_text at stream.jet:40 consumes an initialized String into the private typed-text carrier. UTF-8 is already guaranteed by the String type; this operation neither changes its bytes nor accepts a separately forgeable pointer/length. The caller loses ownership of the moved text and receives an owning DataTree enum, not an escaping borrow."},
  {"file":"Core/encoding/stream.jet","function":"read_string","proof":"stream.jet:294 validates the exact decoded byte list, returning Syntax and releasing tracked capacity on failure(:295-296). The __bytes_into_text gate(:299-300) immediately consumes that same list without intervening mutation. Escaped or raw wire bytes therefore cannot bypass UTF-8 validation or expose a dangling parser buffer."},
  {"file":"Core/encoding/stream.jet","function":"number","proof":"Both gates discharge separate obligations: stream.jet:360 rejects invalid JSON numeric grammar and :365 rejects invalid UTF-8 before :371 consumes that exact byte list into String. Only after those checks does :375-376 construct DataEvent.Number when typed_numbers is enabled. The valid initialized lexeme moves into its matching carrier; no unchecked numeric payload reinterpretation or raw buffer reference reaches callers."},
  {"file":"Core/encoding/stream.jet","function":"fold_text","proof":"stream.jet:555 consumes a valid String into __text_into_bytes solely to measure allocation capacity. That byte list is not modified; :557 can fail before reconstruction, otherwise :559 immediately moves the unchanged UTF-8 list into __bytes_into_text. Budget checks affect retention, not validity; no caller receives the intermediate storage or can mutate it between conversions."},
  {"file":"Core/encoding/stream.jet","function":"fold","proof":"stream.jet:566 checks the private DataEvent.Number discriminant before moving its initialized String through fold_text(:567) into matching DataTree.Number(:568). fold_text preserves UTF-8 while accounting allocation ownership. No other event variant is interpreted as a number; recursion and ordinary event handling occur outside the gate and no native payload borrow escapes."},
  {"file":"Core/encoding/stream.jet","function":"text_from_bytes","proof":"stream.jet:834 strictly validates the caller's exact owned byte list, returning EncodingError on failure; :836 then immediately moves it into __bytes_into_text. No mutation or alternate allocation occurs between validation and transfer. Even arbitrary external byte input cannot return a String violating UTF-8 or retain a pointer into the moved list."},
  {"file":"Core/encoding/stream.jet","function":"byte_at","proof":"The unchecked __string_byte at stream.jet:845 is dominated by :841-842, which reject index < 0 or index >= text.count_bytes() before U64 conversion. The borrowed String remains live for the one-byte read; only copied U8 returns. Callers cannot cause a negative, out-of-storage or dangling pointer read through this helper."},
  {"file":"Core/encoding/stream.jet","function":"ascii_text","proof":"Despite its formatter-oriented name, ascii_text admits no unchecked caller assumption: stream.jet:1402 runs strict __utf8_validate on the exact owned list and panics on invalid UTF-8. Only the succeeding path at :1404-1405 consumes it into __bytes_into_text, with no intervening write. Malformed formatter output cannot create an invalid String."},
  {"file":"Core/encoding/stream.jet","function":"wire_float","proof":"The gate at stream.jet:1415-1416 only transfers the owned valid String produced by Float interpolation(:1413) into __text_into_bytes. It is the safe direction of the UTF-8 transfer and accepts no raw bytes or caller pointer. Later digit editing uses a separate list and checked ascii_text reconstruction; no alias to the moved String is retained."},
  {"file":"Core/encoding/toml.jet","function":"toml_leaf","proof":"The gate at toml.jet:718 permits private carrier matching, not raw memory. Number/TypedText are explicitly rejected(:724); Bytes are read after their discriminant and within list bounds(:726-731). Arrays recurse on typed values and Objects use object_entries only in their matching branch(:742-745), keeping the owner live. The return is owned formatted text, never a native payload borrow."},
  {"file":"Core/encoding/xml_engine.jet","function":"text_byte","proof":"__string_byte at xml_engine.jet:135 is an unchecked byte read. Its only local callers are byte_prefix_text(:139-142) and unit_prefix(:2029-2032), both iterating 0..<needle.count_bytes(), and ascii_equal(:1386-1389), which first checks equal String storage lengths then iterates 0..<left.count_bytes(). Every index is nonnegative and below the corresponding borrowed String's byte length; no pointer or view escapes the copied U8 result."},
  {"file":"Core/encoding/xml_engine.jet","function":"byte_text","proof":"__bytes_into_text at xml_engine.jet:174 consumes a copy of source[start..<end]. Every local caller slices bytes of an existing String (DOCTYPE/entity/declaration helpers) or Scanner.source, whose sole construction path parse_document_with(:667-669) takes String.bytes(). Boundaries are zero/end or offsets adjacent to ASCII XML delimiters, quotes and whitespace, which cannot lie inside a UTF-8 continuation sequence. Checked list accesses(:172) prevent out-of-storage copies; the exact copied scalar-aligned allocation moves with no intervening mutation or escaping borrow."},
  {"file":"Core/encoding/xml_engine.jet","function":"normalize_attribute_text","proof":"xml_engine.jet:2204 starts with bytes of a valid String. The in-place loop(:2207-2215) replaces only ASCII CR/LF/tab with ASCII space and removes the ASCII LF following CR; all non-ASCII bytes keep their order and membership. write never exceeds read, and :2217 truncates only the finished compacted sequence. Thus __bytes_into_text(:2219) consumes valid complete UTF-8 for every caller text, not an assumed-valid wire allocation."},
  {"file":"Core/encoding/yaml.jet","function":"yaml_leaf","proof":"yaml.jet:320 grants private carrier matching. Number/TypedText explicitly panic(:326); Bytes are read only after their matching discriminant over 0..<bytes.len()(:328-333). Object payload views are obtained only in the Object branch(:344-347), retaining the owner; arrays recurse on typed values. The caller receives an owned String and cannot request a mismatched payload or retain native storage references."},
  {"file":"Core/service/service.jet","function":"__authority_validate","proof":"service.jet:155 projects owned identity fields; :156 rejects invalid identity before __endpoint_validation_snapshot(:160). Its native ServiceAuthority.rs:360-368 keeps the registry mutex across exact/candidate lookup and copies only String/scalar facts. No registry guard, endpoint pointer or mutable reference escapes; policy checks operate on owned snapshots, so caller identity cannot extend a native borrow."},
  {"file":"Core/service/service.jet","function":"endpoint_show","proof":"__endpoint_parts at service.jet:265 only clones the initialized endpoint tree/worker Strings and widens generation (Services.rs:3273-3274). Interpolation(:266) returns owned text. Any typed endpoint is valid for this projection; no unchecked index or native reference reaches callers."},
  {"file":"Core/service/service.jet","function":"set_delivery","proof":"service.jet:270 holds an exclusive tree borrow, :272 rejects a started tree, and :273 writes only a typed ServiceDelivery field via __tree_write_delivery. Services.rs:3259-3260 performs a normal field assignment under that borrow; no pointer or secondary tree alias is returned. Callers cannot change the validated tree concurrently with the transfer."},
  {"file":"Core/service/service.jet","function":"set_restart","proof":"service.jet:277 lends the tree exclusively; :279 checks not started before __tree_write_restart. __group_write_restart is indexed only by 0..<__tree_group_count(:281-282), while neither write changes group storage length (Services.rs:3258-3266). Every transferred policy is a typed enum, and no group reference escapes to invalidate that count."},
  {"file":"Core/service/service.jet","function":"drain_dead_letters","proof":"service.jet:286 holds the exclusive tree borrow and :288 checks started before counting and clearing the same dead-letter list(:289-290). Native __tree_dead_letter_count and __tree_clear_dead_letters(Services.rs:3249-3250,3268) return a copied length and clear owned storage without exporting an element borrow. Callers cannot hold a conflicting live list view across the clear."},
  {"file":"Core/service/service.jet","function":"set_state_empty","proof":"service.jet:295 retains an exclusive tree borrow. :297-300 reject a started tree or existing snapshot/events before __tree_write_empty_adapter and __tree_clear_state_authority(:301-302), whose provider(Services.rs:3269-3272) only assigns typed fields. No stored payload reference is returned; callers cannot interleave a state-record insertion between check and replacement."},
  {"file":"Core/service/service.jet","function":"select_state_adapter","proof":"The exclusive tree borrow at service.jet:315 covers :317-326 checks for not-started, no records, recognized migration, bounded visible store/schema and positive version before __tree_write_state_adapter/__tree_write_state_authority(:327-328). Their provider(Services.rs:3277-3284) initializes owning optional fields from typed String/scalars, not raw memory. The caller cannot replace a checked field set or retain an alias into the new authority."},
  {"file":"Core/service/service.jet","function":"restore_snapshot","proof":"__state_load expects a present state authority (Services.rs:3292-3295). service.jet:348 checks that exact option before :349, borrows the live typed tree, and :355 checks nonempty records before reading records[0]. read_state_record(:374-392) checks schema, framing, byte length and UTF-8. Returned snapshot text is owned, not a reference into tree/store storage."},
  {"file":"Core/service/service.jet","function":"replay_events","proof":"service.jet:362-364 call __tree_state_authority/__tree_event_records only as borrowed field projections; native Services.rs:3286-3290,3297 copy optional Strings and the event list. Each copied record passes checked framing(:367) before join. No list-element reference or native store handle is exported, so releasing or later editing the tree cannot invalidate the owned replay text."},
  {"file":"Core/service/service.jet","function":"upgrade_receipt","proof":"__tree_upgrade_receipt at service.jet:397 clones the existing optional typed receipt (Services.rs:3425). Source rejects None before returning the owning value. No native reference, rollback-store pointer or unchecked payload access escapes for any typed tree."},
  {"file":"Core/service/service.jet","function":"__upgrade_receipt_display","proof":"__upgrade_receipt_parts at service.jet:403 projects initialized scalar fields and cloned String/pinned-shard list (Services.rs:3426-3430), omitting rollback storage. :404-405 only format those owned fields. Caller-supplied typed receipts cannot choose a memory address or retain a borrow into the native receipt."},
  {"file":"Core/service/service.jet","function":"tree_show","proof":"__tree_display_parts at service.jet:411 copies the live tree's initialized fields and handler-name pairs (Services.rs:3432-3439); it exports neither worker tasks nor mailbox storage. :413 iterates the returned owned list and matches typed restart/delivery enums for text. No native payload borrow or separately supplied index reaches the caller."},
  {"file":"Core/service/service.jet","function":"workflow_history","proof":"__workflow_replay_parts at service.jet:429 uses the checked native workflow authority/replay and collects owned run-id/history/outcome records (Services.rs:3441-3445), propagating errors. :435-438 only search those copied records and join the matching list. Arbitrary run_id cannot be used as a native vector index or expose a replay guard."},
  {"file":"Core/service/service.jet","function":"workflow_outcome","proof":"service.jet:444 obtains owned records through __workflow_replay_parts, whose provider validates authority and collects owned payloads (Services.rs:3441-3445). :450-455 match run_id and require a present typed outcome rather than indexing with caller input. The result owns its panic text/enum payload and contains no reference into mutable workflow storage."},
  {"file":"Core/service/service.jet","function":"observe","proof":"__tree_observation_parts at service.jet:461 is a read-only projection into copied counts/booleans and typed workflow status codes (Services.rs:3447-3463). The local loop(:470-478) reads that owned list and formats text; no task, channel, mutable view or raw address is exposed. Every typed tree therefore remains valid regardless of caller observation timing."},
  {"file":"Core/service/service.jet","function":"tree","proof":"__tree_allocate at service.jet:542 materializes every native initializer field from typed arguments. :543-566 provide generation1, typed restart/delivery, positive fixed budget, empty initialized worker/group/record collections, false started, Empty state, absent authorities/receipts and a fresh Arc task group from __tree_task_group_new. Services.rs:1327-1352 only allocate/assign those fields. The caller supplies only owned name text, not uninitialized carriers or native pointers."},
  {"file":"Core/service/service.jet","function":"state_path_kind","proof":"__path_kind at service.jet:617 accepts a borrowed valid String path and returns copied metadata kind or owned IOError. It performs a final-component pathname query, not a descriptor/identity ownership transfer; no caller pointer or reference into OS metadata is returned. A path race can alter the metadata result but cannot invalidate a caller memory borrow."},
  {"file":"Core/service/service.jet","function":"directory_generation","proof":"service.jet:643 calls __tree_generation on a borrowed typed tree; Services.rs:6272-6273 only copy its initialized i64 generation into an owned Int. The caller supplies no index/address, and no scheduler or tree reference escapes this scalar projection."},
  {"file":"Core/service/service.jet","function":"event_count","proof":"__tree_event_count at service.jet:649 only reads the live tree's event_log length (Services.rs:4735-4736), returning an owned scalar. No record element, raw backing pointer or mutable borrow is exposed, so callers cannot outlive or invalidate event storage via this query."},
  {"file":"Core/service/service.jet","function":"dead_letter_count","proof":"__tree_dead_letter_count at service.jet:655 borrows the typed tree and reads its initialized owned list length (Services.rs:3249-3250). The result is a copied Int with no native payload reference or caller-controlled address; subsequent clear/drop cannot invalidate it."},
  {"file":"Core/service/service.jet","function":"workflow_start","proof":"service.jet:855 retains an exclusive tree borrow through workflow_prepare(:857), identity/version validation(:861-862), __workflow_count-bounded index loops(:864,872) and cursor/handle updates. __workflow_append receives a framed String only after run-id overflow rejection(:879-881); post-append cursor selection re-reads count(:885). Provider field/index leaves(Services.rs:7244-7258) use normal bounds-checked Vec access and owned projections, never reinterpret an id as a pointer. No mutable workflow view escapes."},
  {"file":"Core/service/service.jet","function":"workflow_step","proof":"The exclusive tree borrow at service.jet:892 covers sync(:894), bounded deterministic step checks(:898-902), and workflow_find(:903), which searches 0..<__workflow_count(:1061-1064). Cursor/history leaves operate only on that selected index; __workflow_event_at uses optional checked history.get (Services.rs:7253). :913-915 reject terminal/oversized history before framed append. No caller run_id becomes an unchecked address or borrowed history reference."},
  {"file":"Core/service/service.jet","function":"workflow_activity","proof":"service.jet:923 exclusively borrows the tree; workflow_prepare and workflow_token checks(:925-935) precede bounded max_attempts(:937) and count-bounded workflow_find(:938). Activity-definition helpers iterate __workflow_steps_len and parse field counts(:1114-1121), not caller indices. Native index leaves use checked Vec access (Services.rs:7244-7258); :965-967 reject terminal state before framed append. Returned TaskStatus is copied, with no workflow/storage borrow escaping."},
  {"file":"Core/service/service.jet","function":"workflow_activity_retry","proof":"service.jet:976 holds the exclusive tree borrow across sync, bounded key checks and workflow_find(:978-986). The selected activity definition is owned; :989-998 validate outcome, replay agreement, terminal state and retry bound before computing the next attempt and __workflow_append(:1000-1002). History lookups are optional checked projections and index leaves use normal Vec bounds (Services.rs:7253-7258). Callers cannot forge a vector pointer or retain mutable activity storage."},
  {"file":"Core/service/service.jet","function":"workflow_activity_complete","proof":"service.jet:1011 exclusively borrows the tree through sync, key/outcome checks and count-bounded workflow_find(:1013-1023). The two replay entries(:1027-1028) use __workflow_event_at's checked optional history.get (Services.rs:7253); only matching initialized events advance the cursor(:1029-1030). :1038-1047 reject conflicting completion before framed append. The return is an owned typed outcome; no raw event reference or unchecked caller index escapes."},
  {"file":"Core/service/service.jet","function":"group","proof":"service.jet:1189 rejects started state and :1190-1194 validate_group against owned topology facts before __group_reparent(:1196). validate_group(:1205-1230) proves every named worker exists and membership is unique, discharging the provider's expect at Services.rs:3310. Reparent and __group_push mutate only typed tree/task storage, with no worker reference escaping to caller code between validation and use."},
  {"file":"Core/service/service.jet","function":"chaos_fail","proof":"service.jet:1235 rejects unstarted trees and :1237 bounds the __tree_write_chaos_count increment before :1239. __first_running_endpoint(:1240) returns an owned optional endpoint (Services.rs:3302-3303), not a vector index or pointer, and fail_worker performs endpoint validation. Only a copied count returns; arbitrary caller state cannot overflow the native counter or obtain a mutable worker borrow."},
  {"file":"Core/service/service.jet","function":"append_event","proof":"__event_store_append requires a present authority (Services.rs:3316-3317). service.jet:1251-1255 establish started EventLog state, present authority, bounded valid payload and record-count bound before constructing the owned framed record(:1256). The durable append(:1257) must succeed before __tree_event_push(:1261); the native leaf owns its inserted String. No reference into storage or independently supplied memory length escapes."},
  {"file":"Core/service/service.jet","function":"commit_snapshot","proof":"service.jet:1267-1270 establish started Snapshot state, present authority and bounded valid payload before framing and __snapshot_store_write(:1271-1272). Native Services.rs:3320-3326 holds the authority mutex across store write and tree.snapshot assignment and owns the record String. Callers cannot bypass the present-option precondition, interpose a write inside the leaf, or receive a reference into replaced snapshot storage."},
  {"file":"Core/service/service.jet","function":"validate_tree_endpoint","proof":"service.jet:1295-1296 borrow typed tree/endpoint only to obtain owned route/identity parts (Services.rs:3328-3336; ServiceAuthority.rs:351-355). :1297-1301 enforce matching authority/tree and permitted generation before locked __authority_validate. Neither caller identity nor generation is interpreted as a pointer/index, and no registry/tree borrow escapes this error/unit result."},
  {"file":"Core/service/service.jet","function":"directory_resolve","proof":"__directory_entry at service.jet:1321 finds by String and clones an optional entry (Services.rs:3338-3339), rejecting absence. :1324-1335 check generation, signed directory proof, authority and live worker before returning its owned endpoint. __worker_route searches typed identity and returns owned facts, not a worker reference. An arbitrary name or stale endpoint cannot turn into an unchecked payload index or escaping native borrow."},
  {"file":"Core/service/service.jet","function":"send","proof":"service.jet:1345-1364 check started state, endpoint authority/generation, bounded message and live nondraining worker before __authority_try_send. That operation receives typed owned text/endpoint, not caller addresses. Post-send __worker_sent_account(:1374) uses only the index returned by a fresh __worker_route on the same tree (Services.rs:3344-3352); native Vec indexing is bounds checked and task writes retain their lock. No mailbox or task reference escapes to the caller."},
  {"file":"Core/service/service.jet","function":"delivery_status","proof":"__delivery_read at service.jet:1444 obtains the checked durable entry and materializes owned __DeliveryFacts (ServiceAuthority.rs:2727-2744). Failure returns before any field use; :1448 returns only the typed state enum. Callers cannot choose a log-memory address or outlive a registry/log lock through this projection."},
  {"file":"Core/service/service.jet","function":"delivery_receipt","proof":"service.jet:1459 first obtains checked owning __DeliveryFacts through __delivery_read(ServiceAuthority.rs:2727-2744). :1463-1465 require successful signature derivation before :1467 constructs a fully initialized source receipt from its owned typed fields. No native entry reference or lock guard is exposed, and invalid delivery identity/authority returns an error before a receipt is emitted."},
  {"file":"Core/service/service.jet","function":"delivery_events","proof":"__delivery_read(true) at service.jet:1473 returns owned record fields, not a borrowed log buffer (ServiceAuthority.rs:2727-2743). Every indexed record access is dominated by the exact fields.len()==10 or ==6 checks(:1480,1494); counters parse through checked workflow_i64, signatures verify before acceptance(:1491-1493,1505-1510), and :1514 guards events[0]. Malformed caller/log data cannot access an absent field or escape a mutable durable-entry borrow."},
  {"file":"Core/service/service.jet","function":"delivery_retry","proof":"service.jet:1522 invokes __delivery_probe, whose native ServiceAuthority.rs:2746-2747 checks the durable entry before success. :1526 materializes a typed runtime from the delivery's cloned store and constant zero retention, then __runtime_retry receives typed carriers only. The returned Delivery owns its identity fields; no log reference, pointer or guard escapes, including on retry errors."},
  {"file":"Core/service/service.jet","function":"endpoint_receive","proof":"service.jet:1533-1537 validate owned endpoint identity before __endpoint_mailbox. Native ServiceAuthority.rs:2751-2776 captures the channel in an owning Arc and returns initialized started/draining facts, so releasing the registry lock cannot dangle the mailbox. :1542 enforces lifecycle/depth policy before one __mailbox_receive; :1544-1547 decode its status into owned message/error. No caller can supply a raw channel address or receive a borrowed queue element."},
  {"file":"Core/service/service.jet","function":"local_delivery_ids","proof":"service.jet:1622 requires successful __worker_route before __mailbox_snapshot(:1623), using only the returned valid worker index. Native Services.rs:3362-3366 copies messages/idempotency records into owned lists; :1627-1634 match initialized identity/message fields before __delivery_id with a fully initialized zero-retention runtime. No pointer or mutable mailbox/record view is exported and arbitrary endpoint text is never used directly as an index."},
  {"file":"Core/service/service.jet","function":"send_durable","proof":"service.jet:1642-1658 check delivery mode, state authority, bounded key/message, started state and endpoint routing before __runtime_send. Mailbox snapshot/restore indices come only from __worker_named_index(:1676,1693), which searches current workers (Services.rs:3368-3369); native restores use checked Vec access(:3378-3379). Each authority operation retains its own locks and returns owning Delivery/String facts; mailbox replacement consumes its owned message list, never exporting channel references. Dead-letter/idempotency pushes are length bounded(:1683,1702)."},
  {"file":"Core/service/service.jet","function":"receive","proof":"service.jet:1715-1726 validate tree lifecycle/endpoint before selecting a worker. Capacity bounds :1728-1729 dominate __take_pending; each mailbox operation(:1751-1752,1774-1783) uses the index returned by a fresh __worker_route, and rollback reselects target(:1787-1790). Native Services.rs:3362-3423 use owned snapshots, checked Vec indexing and locked/Arc channel operations, not unchecked memory. :1780-1782 admit only a successful owned message; no queue element or task guard escapes to callers."},
  {"file":"Core/service/service.jet","function":"pending_delivery_rollback","proof":"The gate at service.jet:1806 passes typed endpoint/owned pending records to __requeue_pending, then an owned prior message snapshot to __mailbox_restore in both result branches(:1809,1815). Its local callers in receive(:1759,1762) pass the index obtained from __worker_route, never caller text as an address; native Services.rs:3378-3379 still bounds-check the Vec access. Both operations own/copy their inputs under existing locks and return errors, not references into restored mailbox storage."},
  {"file":"Core/service/service.jet","function":"worker","proof":"service.jet:1868-1876 reject started state, capacity outside1..1000000, invalid names, excess workers and duplicates before allocation. __endpoint_unchecked receives route fields/issued authority(:1878-1893), and __worker_mailbox_new receives bounded capacity plus an empty owned queue(:1894). :1906-1914 initialize task/topology and every __worker_push field; the mailbox moves rather than cloning its forkable channel. The handler is a typed checked function item, not a raw caller address; native Services.rs:7289-7305 only store initialized owned fields."},
  {"file":"Core/service/service.jet","function":"fail_worker","proof":"service.jet:2008 holds an exclusive tree borrow and :2010-2014 validate lifecycle/endpoint before obtaining owning topology/worker facts. Target indices derive from worker_scope/worker_index(:2019-2031), group/supervisor accesses are explicitly bounded(:2023,2092,2109), and previous/desired lists are constructed for exactly facts.workers.len()(:2059-2067). Native writes(Services.rs:7308-7329) use checked Vec indexing; task partition writes hold one mutex(:7339-7341). Replacement mailboxes own their queues and move at :2103, so close/restart cannot expose dangling channel storage."},
  {"file":"Core/service/service.jet","function":"restarts","proof":"service.jet:2125 validates the endpoint before owned topology/worker facts. :2134 checks supervisor against facts.groups.len() before selecting budget; :2136 iterates worker facts and matches tree/name before reading that worker's owning restart list. Providers preserve worker insertion order (Services.rs:3305-3307,7269-7277), and accesses are normal bounds-checked projections. The result is a copied count, not a restart-list or task reference."},
  {"file":"Core/service/service.jet","function":"mailbox_depth","proof":"service.jet:2146 validates the endpoint, then :2151-2155 iterate owning topology/worker facts and call __worker_mailbox_depth only with the matching in-range worker index. Native Services.rs:7328-7329 bounds-check workers and copies the live Arc channel's depth. No channel pointer/view or mutable mailbox reference reaches caller code, even for stale or unknown endpoints which return errors."},
  {"file":"Core/ui/host.jet","function":"transfer_request","proof":"__request_transfer requires kind 0/1 and explicit absent-directory state (Core/HostServices.rs:2135-2152). The local helper's only callers host.jet:172,176 supply exactly those constants, true directory_absent, an empty typed filter list and false multiple. The typed UIFSGrant is borrowed/cloned into an owning request, not interpreted as a memory address; title is valid String text. No current-host scope is accessed by this layout-only leaf and callers cannot supply unchecked tags to the local helper."},
  {"file":"Core/ui/ui.jet","function":"gtk_backend","proof":"ui.jet:126-127 invoke __gtk_backend without caller-supplied memory or callbacks. Its provider UiGtk.rs:2069-2070 calls JetGtkBackend::new(:1346-1371), allocating an owning Arc<Mutex<GtkState>> with every field initialized, empty queues and null not-yet-initialized GTK handles. Construction neither dereferences those handles nor installs/borrows the current host. The caller receives the owned backend carrier, not a pointer into a scoped provider."},
  {"file":"Core/ui/ui.jet","function":"tui_backend","proof":"__tui_backend at ui.jet:132 has no arguments and allocates JetTuiBackend through Ui.rs:1860-1861,1572-1591. The provider owns a fresh Arc<Mutex<State>> with initialized capability facts, absent frames/focus, zero counters and an empty grid. It performs no current-host scope dereference and exports no borrow of constructor-local storage; arbitrary safe callers cannot supply a forged native backing pointer."},
  {"file":"Core/ui/ui.jet","function":"null_backend","proof":"__null_backend at ui.jet:137 has no caller-selected fields and calls JetNullBackend::new (Ui.rs:1856-1857,1346-1356). That constructor owns a new Arc<Mutex<State>> containing initialized absent frame/event/focus slots and an empty command list; it neither accesses scoped host pointers nor returns references into temporary storage. The returned carrier keeps its allocation alive independently of the caller frame."},
  {"file":"Core/ui/ui.jet","function":"node_accessibility","proof":"ui.jet:148 calls __accessibility_transfer with typed UINode/UIAccessibility and the explicit present=true flag. Ui.rs:302-308 only assign Some(metadata) to the owning node then return that node; no discriminant reinterpretation, unchecked index, callback invocation or current-host access occurs. Caller metadata remains an initialized typed record and the result owns its Strings/lists rather than borrowing source storage."},
  {"file":"Core/ui/ui.jet","function":"node_shortcut","proof":"ui.jet:165 passes initialized typed node/shortcut with present=true to __shortcut_transfer. Its provider Ui.rs:311-317 only stores the typed shortcut in the node's Option and returns the owning node, with no scoped-host dispatch or raw address access. Any valid typed shortcut therefore preserves layout/lifetimes, and no native registry borrow escapes."},
  {"file":"Core/ui/ui.jet","function":"transfer_node","proof":"__node_transfer trusts numeric role/kind/absence/style representations (Ui.rs:1938-1985). ui.jet:173-184 derives role solely as -1 or0..3 and kind solely as0..4 from typed enums. :185-192 couples color presence to initialized text, fixes all six absent-slot bits to63, and marks style absent with zero initialized words. Native :1987-2002 own the copied label/color and typed children, never dereferencing caller addresses. Width/height are stored floats, not memory lengths; invalid geometry cannot cause an unchecked allocation transfer here."},
  {"file":"Core/ui/ui.jet","function":"transfer_viewport","proof":"__viewport_transfer converts numeric device/dimensions/scale to native enum/u32/u16 (Core/Preview.rs:895-908). The local helper's only callers ui.jet:115-117 supply device tags0,1,2, positive dimensions390..1280/800..1119 and scale1 or2; each fits its native width and all fields are initialized before the gate(:197-198). It accesses no current host and returns an owning plain viewport rather than a pointer or borrowed resource; arbitrary external numbers cannot reach this local transfer."},
  {"file": "Core/sys/sys.jet", "function": "getpgid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."},
  {"file": "Core/sys/sys.jet", "function": "getsid", "proof": "A read-only POSIX query: it changes no process state, touches no memory the caller owns, and returns an Int or IOError."}
]
```
<!-- unsafe-abstractions:end -->
