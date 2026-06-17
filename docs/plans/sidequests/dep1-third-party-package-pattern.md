# Sidequest: D-DEP1 — Third-party dependency wrapping pattern

**Ratified:** 2026-06-17 (D-DEP1)  
**Affects:** E2-M9 (ring libs), E2-M10 (jet.tls), E2-M12 (OTel exporter), E2-M16 (signed cache)

## What was ratified

D-DEP1: Third-party dependencies (Rust crates) ship as **FFI-wrapping Jet packages**, not as compiler crates. The compiler stays zero external crates (I6).

## Pattern

```jet
// In jet.tls/payload.jet:
[extern rust]
rustls = "rustls#0.23.0"   // version-pinned via # syntax (VERSION-#)

// In jet.tls/src/lib.jet:
@extern module rustls { … }   // wraps the Rust crate's API surface
```

The wrapping package goes through the same hangar store and lockfile as user packages. It is a `jet.*` namespace package, not a compiler built-in.

## What agents must implement

1. **`[extern rust]` section in `payload.jet`** — parser support for Rust crate pinning in the package manifest
2. **`extern rust "crate@version"`** syntax in the package layer (distinct from C FFI's `@extern module c.<lib>`) — register in `src/syntax.rs` (I7)
3. **Codegen**: when resolving a `jet.*` package that wraps a Rust crate, the compiler must include the Rust crate as a `[dependencies]` entry in the generated `Cargo.toml` and wire the FFI surface

## Where this pattern is used

- `jet.tls` wraps `rustls` (E2-M10)
- `jet.otel` exporter (post-M12) wraps the OTel Rust SDK
- Any ring lib that needs a native Rust backend (regex engine, yaml parser, etc.)
- The pattern explicitly does NOT apply to compiler internals — only to user-installable packages

## Exit criteria

- At least one `jet.*` package (suggest `jet.tls`) successfully wraps a Rust crate via this pattern
- The Rust crate version is pinned in `payload.jet` and appears in `.jet/lock`
- `jet.tls` installs and links without any compiler source change (only package files)
- Pattern documented in `docs/spec/` or package authoring guide
