# Polyglot — replace every language, in phases

Goal (north star, philosophy.md): no reason to reach for another language.
Two prongs. **Reach**: call anything from Jet, today, ergonomically —
better inline/FFI than any host language offers. **Replace**: migrate any
codebase into Jet with importers, native-replacement proof, and no
call-site rewrites. The mechanism for both is already ratified law —
this proposal fills the language matrix and adds the two missing tiers.

## Ratified substrate (do not re-decide)

- **D-FFI-UNIFY1** — every foreign language mounts as `<lang>.<lib>` with
  three tiers: script (`use "raylib.h" as rl` — bind on first compile),
  project (`use py.h5 as h5`, pinned in `pkg.jet`), overlay
  (`#Extern module <lang>.<lib>`). Per-language binder
  `jet inspect bind <lang>`; generated bindings are safe wrappers by
  construction (I1); binder diagnostics are Jet diagnostics (I2/I4);
  in-situ replacement by a Jet package exporting the same surface.
- Shipped/ratified binders: **c** (S59 + D-CABI-*), **rust** (S50),
  **py** (D-FFI-PY1: sidecar default, `py@embed` opt-in),
  **js** (D-FFI-JS1: target-dependent host, `.d.ts` stubs),
  **swift** (D-FFI-SWIFT1). Roots reserved: `r.*`, `gpu.*`
  (D-DATA-BRIDGE1).
- Providers: npm/PyPI/SwiftPM/Cargo/Nix/GitHub federated under jetpack
  authority (D-JPK-EXTPROV1, D-WD6). Metadata importers: `jet import
  <ecosystem>` (D-JPK-IMPORTCMD1). Native replacement requires proof
  (D-WD15). Legacy build wrappers: CMake/Make/Gradle/npm/cargo as Tier-2
  actions (D-BUILDLEGACY1).

## The two missing tiers

**Inline tier (D-FFI-INLINE1).** The script tier binds a foreign *file*;
nothing lets a Jet file carry ten lines of foreign code. Proposed: a
fourth D-FFI-UNIFY1 tier — one directive shape for every language,
typed boundary, checked at build:

```jet
#FFI(c) fn crc32(data: [U8], seed: U32) -> U32 {
    """
    uint32_t crc32(const uint8_t* data, size_t data_len, uint32_t seed) {
        uint32_t c = ~seed;
        for (size_t i = 0; i < data_len; i++) { /* … table walk … */ }
        return ~c;
    }
    """
}
```

The Jet signature is the contract: sema type-checks call sites against
it, the language's binder compiles the body on cache miss, marshaling is
generated exactly like the script tier, and a body/signature mismatch is
a Jet diagnostic. Same shape for `#FFI(asm)`, `#FFI(py)`, …
Effects: a `#FFI` fn declares its effect row like any extern.

**Assembly floor (D-FFI-ASM1).** True master-of-ALL bottoms out in asm
(kernels, crypto, intrinsics SIMD doesn't cover). Inline asm is the
`asm` instance of the inline tier, gated by `#Unsafe` (S58) with typed
operands — no bare string globals:

```jet
use core.mem

#Unsafe("cycle counter — no Core surface")
#FFI(asm) fn rdtsc() -> U64 {
    """
    rdtsc
    shl rdx, 32
    or  rax, rdx        ; -> return
    """
}
```

## Language matrix — phased

Phase order: systems first (owner pick 2026-07-11). Each language is one
ballot (binder depth + host model), following the D-FFI-PY1/JS1/SWIFT1
precedent. Worked target spelling shown per row; all rows reuse the same
three ratified tiers plus the inline tier above.

### Phase 1 — systems floor

| Lang | Root | Host model | Ballot |
|---|---|---|---|
| Assembly | `#FFI(asm)` only (no lib namespace) | rustc `asm!` lowering per target | D-FFI-ASM1 |
| C++ | `cpp.*` | clang bindgen; classes → opaque handles, methods → fns, exceptions → `T ? CppError`; templates instantiated on demand | D-FFI-CPP1 |
| Inline tier (all langs) | `#FFI(<lang>)` | per-lang binder compiles body | D-FFI-INLINE1 |

```jet
use cpp.opencv as cv                       // project tier
img :: cv.imread("scan.png")?              // exceptions surface as `? CppError`
out :: img.gaussian_blur(kernel: 5)?
```

### Phase 2 — managed & popular back ends

| Lang | Root | Host model | Ballot |
|---|---|---|---|
| Go | `go.*` | `go build -buildmode=c-archive` shims; goroutine-blocking calls get `#(…)` effects | D-FFI-GO1 |
| Java/JVM (Kotlin, Scala ride along) | `java.*` | embedded JVM (JNI) default, sidecar opt-in like py; classes → opaque handles | D-FFI-JVM1 |
| C#/.NET | `cs.*` | hostfxr embed; async → Jet tasks | D-FFI-DOTNET1 |
| Fortran | `fortran.*` | ISO C binding via gfortran; arrays map to `[T]`/`Tensor<T>` column-major-aware | D-FFI-FORTRAN1 |

```jet
use java.pdfbox as pdf
doc :: pdf.load(files.read_bytes("form.pdf")?)?
print(doc.text())
```

### Phase 3 — scripting & shell

| Lang | Root | Host model | Ballot |
|---|---|---|---|
| Lua | `lua.*` | in-process (tiny VM); zero-copy tables ↔ `[K: V]` | D-FFI-LUA1 |
| Ruby | `ruby.*` | sidecar worker (py precedent) | D-FFI-RUBY1 |
| Perl | `perl.*` | sidecar worker; regex-heavy legacy scripts callable as-is | D-FFI-PERL1 |
| PHP | `php.*` | sidecar php-fpm-style worker | D-FFI-PHP1 |
| R | `r.*` (root already reserved) | sidecar Rserve-style; data.frame ↔ `core.data.Table` | D-FFI-R1 |
| Shell | `sh` typed text (D-TYPEDTEXT1 instance) | `Sh` checked value: `{hole}` = injection-safe argv item, never word-split | D-FFI-SH1 |

```jet
// D-FFI-SH1 — the Sql/Html mechanism, third instance (I8: same engine)
cmd: Sh :: "rsync -a {src} {dest}"     // holes become argv items, no quoting bugs
process.run(cmd)?
```

### Phase 4 — legacy & enterprise replacement

| Lang | Root | Host model | Ballot |
|---|---|---|---|
| COBOL | `cobol.*` | GnuCOBOL compiles to C ABI; copybooks → `@Codable` structs via binder | D-FFI-COBOL1 |
| MATLAB/Octave | `octave.*` | sidecar Octave; matrices ↔ `Matrix<M,N>`/`Tensor<T>` | D-FFI-OCTAVE1 |
| Source importers (all langs) | `jet import <lang> ./src` | semantic transpile-in: editable canonical Jet + TODO diagnostics (extends D-WD5/D-JOS-NIXIMPORT1 discipline to source code) | D-MIGRATE-SRC1 |

```shell
$ jet import cobol ./payroll
payroll/ → jet/payroll/            37 programs, 214 copybooks
  copybooks → @Codable structs     (fixed-width via #[Layout(cobol)] facts)
  PERFORM graph → fn call graph
  12 TODO diagnostics (JT01xx): GO TO fan-in at PR-DIST-030, …
```

Erlang/Elixir, Haskell, OCaml, Zig, D: reachable through the C ABI tier
already shipped; each gets a root only when a real project demands more
than the C surface (no speculative binders — D-STDLIBLEDGER1 spirit).

## Why Jet wins this

Nobody else has all four at once: (1) one structure for every language
(vs. per-language ad-hoc crates/gems), (2) generated bindings that are
safe wrappers by construction with the host language's errors laundered
into native diagnostics, (3) in-situ native replacement with proof — the
migration story is *gradual and call-site-stable*, (4) package manager
that already federates the foreign ecosystems' registries. The inline
tier beats C++'s `asm`, Rust's `asm!`, and Python's ctypes on ergonomics
because the signature is the checked contract and the toolchain is
provisioned by jetpack, not by the user's PATH.

## Ballot index (one decision per ballot)

Phase 1 card: D-FFI-INLINE1, D-FFI-ASM1, D-FFI-CPP1.
Phase 2 card: D-FFI-GO1, D-FFI-JVM1, D-FFI-DOTNET1, D-FFI-FORTRAN1.
Phase 3 card: D-FFI-LUA1, D-FFI-RUBY1, D-FFI-PERL1, D-FFI-PHP1,
D-FFI-R1, D-FFI-SH1.
Phase 4 card: D-FFI-COBOL1, D-FFI-OCTAVE1, D-MIGRATE-SRC1.
