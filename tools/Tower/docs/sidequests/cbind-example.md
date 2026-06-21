# Plan: C-header binding example (`use c.<lib>` / `jet bind`) — no decision

**Status: plan — NO owner decision needed; ratified, just missing an example (I5).**

Unblocks: **Yuki** (verify a C-header binding end-to-end).

---

## Goal

The native C-prototype binder is **ratified and shipped**: D-CBIND3 (native
std-only parser in `Source/CBind.rs`), and c53 shipped auto-bind-on-cache-miss +
header-hash invalidation. But there is **no `examples/features/` entry** that
binds a real C header end-to-end — `22_ffi.jet` is `extern rust` (base64 crate),
not a C-header binding, and `lowlevel.jet` is raw memory. Yuki could not verify
the C-interop story because nothing demonstrates it.

This is purely an **I5 gap**: the feature exists; the executable-spec example does
not. No syntax, no decision.

Verified: `22_ffi.jet` = `extern rust "base64@0.22"`; D-CBIND3 + c53 ratified and
implemented (`syntax-decisions.md:2235`, board c53 done); `grep "use c\." examples/`
→ nothing.

## Pipeline touch points

- **examples only** — author a small C header + a `.jet` file that binds it via
  the ratified path (`use c.<lib>` / `jet bind`), calls a function, prints the
  result; golden-tested output (I5).
- **docs**: ensure the binding workflow is documented where a user would look
  (`spec.md` / stdlib FFI section) if not already.
- Possibly surfaces real bugs in the binder when exercised — if so, those become
  their own bug cards (cf. c43 "FFI u32→Int boundary untested").

## Invariants in play

- **I5** every feature ships with an example + golden output — this *closes* an
  I5 hole, doesn't open a decision.
- **I2** if the binder rejects/ICEs on the chosen header, that's a P0 bug to file,
  not something to paper over.

## Open questions

None for the owner. Implementation choices only:
1. Which C function to bind (pick something with a clean scalar/`char*` signature
   inside D-CBIND3's supported subset — scalars, `char*`→String, `void`).
2. Whether the example needs a vendored header or can bind a libc symbol portably.
3. CI: does the golden test need a C toolchain available (Nix shell provides one)?

## Test plan

1. `examples/features/cbind.jet` (+ a fixture `.h`) — bind one C function, call
   it, print the result; golden-tested (I5).
2. If binding exercises a u32/unsigned boundary, coordinate with c43.
