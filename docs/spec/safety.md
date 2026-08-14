# Safety

Jet keeps memory-unsafe operations behind explicit `#Unsafe("reason")` gates.
The reason is part of the review record. Generated Rust may use `unsafe` only
for an approved gate or in a vetted runtime or standard-library internal.

## Unsafe-region ratchet

The repository records every user-written unsafe region in one baseline. The
baseline splits regions by crate or package and names each file, position, and
reason. `scripts/agent/check-unsafe-ratchet.mjs` checks the baseline.

The check scans semantic `.jet` source forms: block/function
`#Unsafe("reason")` gates and grouped `#[Unsafe("reason"), …]` function gates.
It ignores comments, strings, invalid marker placements, and generated FFI
files under `.jet/bindings/`. A higher count fails with each new region. Use
`--update` in the same change when the new region is approved. A lower count
updates the baseline automatically. A same-count edit that changes a recorded
file, position, or reason fails as stale until the baseline is refreshed.

<!-- unsafe-ratchet:begin -->
<!-- unsafe-ratchet:data
{
  "schema": 1,
  "total": 67,
  "counts": {
    "docs": 2,
    "examples": 22,
    "tests": 43
  },
  "regions": [
    {
      "package": "docs",
      "file": "docs/reference/syntax-surface.jet",
      "line": 504,
      "column": 1,
      "reason": "reads through a raw pointer; addr must be live and valid"
    },
    {
      "package": "docs",
      "file": "docs/reference/syntax-surface.jet",
      "line": 507,
      "column": 5,
      "reason": "addr is the address of a live Int on this frame"
    },
    {
      "package": "examples",
      "file": "examples/features/crypto/crypto_migration.jet",
      "line": 26,
      "column": 5,
      "reason": "AES-256-GCM protocol interoperability"
    },
    {
      "package": "examples",
      "file": "examples/features/crypto/random_api_split.jet",
      "line": 28,
      "column": 5,
      "reason": "compare the typed and raw HKDF rungs"
    },
    {
      "package": "examples",
      "file": "examples/features/crypto/vault_keys.jet",
      "line": 36,
      "column": 5,
      "reason": "restore audited raw signing-key material"
    },
    {
      "package": "examples",
      "file": "examples/features/effects/audited_gate_ladder.jet",
      "line": 7,
      "column": 5,
      "reason": "the audited block has no low-level operation"
    },
    {
      "package": "examples",
      "file": "examples/features/effects/single_use_discard.jet",
      "line": 39,
      "column": 5,
      "reason": "event cancelled; the ticket admits to nothing, so voiding it is correct"
    },
    {
      "package": "examples",
      "file": "examples/features/io/os_process_control.jet",
      "line": 25,
      "column": 5,
      "reason": "POSIX process and pipe control for the core.os surface"
    },
    {
      "package": "examples",
      "file": "examples/features/io/process_exit_cleanup.jet",
      "line": 15,
      "column": 5,
      "reason": "the exit callback has no captured state"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/inline_asm.jet",
      "line": 3,
      "column": 1,
      "reason": "the operands are scalar registers and add does not access memory"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/inline_asm.jet",
      "line": 8,
      "column": 5,
      "reason": "call the audited register-only assembly contract"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/inline_c.jet",
      "line": 1,
      "column": 1,
      "reason": "the scalar ABI contract matches the C definition"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/inline_c.jet",
      "line": 6,
      "column": 5,
      "reason": "call the audited inline C contract"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/lowlevel.jet",
      "line": 10,
      "column": 5,
      "reason": "`cell` is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/mmio_board_write.jet",
      "line": 9,
      "column": 5,
      "reason": "stand-in MMIO cell stays on this stack frame"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/pointer_cast_deref.jet",
      "line": 11,
      "column": 5,
      "reason": "flag is live on this stack frame and the pointer never escapes"
    },
    {
      "package": "examples",
      "file": "examples/features/lowlevel/unsafe_obligations.jet",
      "line": 5,
      "column": 5,
      "reason": "cell stays live and the pointer remains local"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/pin.jet",
      "line": 41,
      "column": 5,
      "reason": "node storage is fixed for the returned pin; self_addr names this place"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/rawptr.jet",
      "line": 9,
      "column": 5,
      "reason": "`cell` is a live Int on this stack frame; the pointer never escapes"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/uninit.jet",
      "line": 12,
      "column": 5,
      "reason": "uninitialized plain-data storage is filled before every read"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/unsafe_sentries.jet",
      "line": 7,
      "column": 5,
      "reason": "pointer is used only after arena reset to prove quarantine"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/unsafe_sentries_package_off/run.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this package-policy proof"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/unsafe_sentries_provenance.jet",
      "line": 6,
      "column": 5,
      "reason": "external address must be a live allocation"
    },
    {
      "package": "examples",
      "file": "examples/features/memory/unsafe_sentries_source_off.jet",
      "line": 7,
      "column": 5,
      "reason": "the local cell is live for this raw read"
    },
    {
      "package": "tests",
      "file": "tests/fixtures/unsafe_obligations/main.jet",
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
      "line": 13,
      "column": 1,
      "reason": "the local slot remains live for the complete C call"
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
      "line": 12,
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
      "file": "tests/ui/crypto_expert_nonce_region_caps_precedence.jet",
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
      "file": "tests/ui/unsafe_forbidden/main.jet",
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
      "file": "tests/ui/unsafe_per_site/main.jet",
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
| docs | 2 |
| examples | 22 |
| tests | 43 |
| **total** | **67** |

### Regions

| crate/package | file | line | reason |
| --- | --- | ---: | --- |
| docs | docs/reference/syntax-surface.jet | 504:1 | "reads through a raw pointer; addr must be live and valid" |
| docs | docs/reference/syntax-surface.jet | 507:5 | "addr is the address of a live Int on this frame" |
| examples | examples/features/crypto/crypto_migration.jet | 26:5 | "AES-256-GCM protocol interoperability" |
| examples | examples/features/crypto/random_api_split.jet | 28:5 | "compare the typed and raw HKDF rungs" |
| examples | examples/features/crypto/vault_keys.jet | 36:5 | "restore audited raw signing-key material" |
| examples | examples/features/effects/audited_gate_ladder.jet | 7:5 | "the audited block has no low-level operation" |
| examples | examples/features/effects/single_use_discard.jet | 39:5 | "event cancelled; the ticket admits to nothing, so voiding it is correct" |
| examples | examples/features/io/os_process_control.jet | 25:5 | "POSIX process and pipe control for the core.os surface" |
| examples | examples/features/io/process_exit_cleanup.jet | 15:5 | "the exit callback has no captured state" |
| examples | examples/features/lowlevel/inline_asm.jet | 3:1 | "the operands are scalar registers and add does not access memory" |
| examples | examples/features/lowlevel/inline_asm.jet | 8:5 | "call the audited register-only assembly contract" |
| examples | examples/features/lowlevel/inline_c.jet | 1:1 | "the scalar ABI contract matches the C definition" |
| examples | examples/features/lowlevel/inline_c.jet | 6:5 | "call the audited inline C contract" |
| examples | examples/features/lowlevel/lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| examples | examples/features/lowlevel/mmio_board_write.jet | 9:5 | "stand-in MMIO cell stays on this stack frame" |
| examples | examples/features/lowlevel/pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| examples | examples/features/lowlevel/unsafe_obligations.jet | 5:5 | "cell stays live and the pointer remains local" |
| examples | examples/features/memory/pin.jet | 41:5 | "node storage is fixed for the returned pin; self_addr names this place" |
| examples | examples/features/memory/rawptr.jet | 9:5 | "`cell` is a live Int on this stack frame; the pointer never escapes" |
| examples | examples/features/memory/uninit.jet | 12:5 | "uninitialized plain-data storage is filled before every read" |
| examples | examples/features/memory/unsafe_sentries.jet | 7:5 | "pointer is used only after arena reset to prove quarantine" |
| examples | examples/features/memory/unsafe_sentries_package_off/run.jet | 7:5 | "the local cell is live for this package-policy proof" |
| examples | examples/features/memory/unsafe_sentries_provenance.jet | 6:5 | "external address must be a live allocation" |
| examples | examples/features/memory/unsafe_sentries_source_off.jet | 7:5 | "the local cell is live for this raw read" |
| tests | tests/fixtures/unsafe_obligations/main.jet | 5:5 | "local pointer" |
| tests | tests/fuzz/sema/differential/ex_crypto_crypto_migration.jet | 26:5 | "AES-256-GCM protocol interoperability" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_lowlevel.jet | 10:5 | "`cell` is live on this stack frame and the pointer never escapes" |
| tests | tests/fuzz/sema/valid/ex_lowlevel_pointer_cast_deref.jet | 11:5 | "flag is live on this stack frame and the pointer never escapes" |
| tests | tests/ui/audited_gate_ladder_forbidden/run.jet | 3:5 | "the organization policy refuses this audited escape" |
| tests | tests/ui/cffi_out_pointer_requires_unsafe.jet | 13:1 | "the local slot remains live for the complete C call" |
| tests | tests/ui/crypto_argon2id_literal_policy.jet | 7:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_argon2id_literal_policy_type_precedence.jet | 7:5 | "fixed password-hash vector" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 7:5 | "protocol-selected password policy" |
| tests | tests/ui/crypto_argon2id_literal_policy_valid.jet | 15:5 | "fixed password-hash vectors" |
| tests | tests/ui/crypto_expert_material_length.jet | 12:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_material_length_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 5:5 | "protocol-selected interop material" |
| tests | tests/ui/crypto_expert_material_length_valid.jet | 17:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_callback_bound_precedence.jet | 12:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_correct.jet | 5:5 | "fixed interop vectors" |
| tests | tests/ui/crypto_expert_nonce_effect_bound_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_prohibition_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_effect_unknown_precedence.jet | 10:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_length.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_region_caps_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_trait_dispatch_precedence.jet | 9:5 | "fixed interop vector" |
| tests | tests/ui/crypto_expert_nonce_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_length.jet | 7:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_type_precedence.jet | 5:5 | "fixed interop vector" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 6:5 | "protocol-selected output length" |
| tests | tests/ui/crypto_hkdf_output_valid.jet | 14:5 | "fixed interop vectors" |
| tests | tests/ui/ffi_asm_float_signature.jet | 3:1 | "register-only increment" |
| tests | tests/ui/ffi_asm_missing_return_anchor.jet | 3:1 | "cycle counter" |
| tests | tests/ui/ffi_asm_no_mem.jet | 1:1 | "register-only increment" |
| tests | tests/ui/ffi_body_not_string.jet | 1:1 | "demo" |
| tests | tests/ui/ffi_inline_c_mismatch.jet | 1:1 | "deliberate mismatch fixture" |
| tests | tests/ui/ffi_unknown_language.jet | 1:1 | "demo" |
| tests | tests/ui/lowlevel_e3102.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_e3103.jet | 3:1 | "reads through caller-provided raw pointer" |
| tests | tests/ui/lowlevel_volatile_write_without_mem.jet | 2:5 | "addr is valid" |
| tests | tests/ui/lowlevel_volatile_write_wrong_ptr.jet | 4:5 | "plain ints are not raw pointers" |
| tests | tests/ui/lowlevel_volatile_write_wrong_value.jet | 6:5 | "slot is live and pointer never escapes" |
| tests | tests/ui/unsafe_forbidden/main.jet | 2:5 | "not allowed" |
| tests | tests/ui/unsafe_obligation_missing.jet | 5:5 | "local pointer" |
| tests | tests/ui/unsafe_obligation_no_bleed.jet | 6:5 | "two independent pointer operations" |
| tests | tests/ui/unsafe_per_site/main.jet | 2:5 | "selection required" |
<!-- unsafe-ratchet:end -->
