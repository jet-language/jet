# Sidequest: E2-M9 — First-party library ring implementation

**Plan:** `docs/plans/epoch-2/m9-first-party-libraries.md`  
**Status:** all decisions ratified; ready to implement after M6 + M8  
**Depends on:** E2-M6 (clean API ergonomics), E2-M8 (version + API diff enforcement), E2-M14 ✅ (C FFI for sqlite)

## Ratified decisions summary

| Decision | What to implement |
|---|---|
| D-LR1 | Ship ALL ring libs in Epoch 2 (owner chose all-in; not just the first wave) |
| D-LR2 | sqlite via C FFI (E2-M14 ✅ done) |
| D-LR3 | Crypto: vetted hashes/HMAC/RNG only |
| D-LR4 **AMENDED** | Add `jet.yaml` in wave 1 (owner chose B against the original recommendation to defer YAML) |

## Wave order (per D-LR1 — ship ALL in Epoch 2)

1. `jet.csv`, `jet.toml`, `jet.yaml` (D-LR4=B), `jet.log`, `jet.time`
2. `jet.regex`, `jet.crypto`, `jet.archive`, `jet.db` (sqlite via C FFI)

## D-DEP1 architecture pattern

Third-party Rust libraries are wrapped as FFI-wrapping Jet packages, not compiler crates (I6).
This means ring libraries that need Rust internals (e.g. a fast YAML parser) follow the pattern:

```
[extern rust "some-rust-crate@version"]
// -> compiler wraps it, ships as jet.yaml package
// -> no compiler dep; goes through the same store as user packages
```

The `jet.tls` package (E2-M10) follows the same pattern. Document this pattern once and reference it from each package.

## D-JSON1 implementation

`jet.json` (if separate from core) must implement lenient coercion (D-JSON1-decode=B: `"8080"` → `8080` where unambiguous). See `json1-coercion-visibility.md` for the required coercion surfacing.

## Diagnostics to register (E27xx)

E2701 (malformed input with location), E2702 (crypto misuse), L2701 (catastrophic-backtracking regex advisory).

## Exit criteria

See `m9-first-party-libraries.md`. Key: five real-script examples run; docs generated; API diffs enforced by M8. `nix develop -c cargo test` green.
