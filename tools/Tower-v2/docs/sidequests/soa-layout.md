# Plan: Cache-friendly data layout — SOA (D-SOA1)

**Status: ratified — D-SOA1 + D-SOA2A–D all ratified 2026-06-22. No owner decision open.**
**Tier: Later (syntax ratified; implementation deferred post-v1).**
**Keyword: `#layout(columnar)`** (D-SOA2A=C renamed it from `soa`; this doc predates the
rename — read `soa` below as `columnar`).

---

## Goal

Let a struct opt in to structure-of-arrays storage layout with a single annotation.
Field access syntax (`particle.x`, `particle.y`) stays identical to AOS layout;
the compiler generates the index arithmetic over separate arrays. The feature
targets data-oriented loops over large, hot collections (particle systems, ECS
components, audio frames, simulation ticks).

No change to default behavior: plain `struct` is AOS (one struct value per array
slot). SOA is an explicit, visible opt-in.

---

## Pipeline touch points

### 1. Parser (assuming Option A is ratified: `#layout(soa)`)

`#layout(soa)` is parsed as a standard attribute (D-ATTR1) on a struct item:

```
struct-item = "#layout" "(" layout-kind ")" "struct" Name "{" fields "}"
layout-kind = "soa" | "aos"   // "aos" is the default; explicit for documentation
```

`#layout` on a non-struct item is a sema error (E0430).

### 2. Sema — layout tag

A struct with `#layout(soa)` carries a `Layout::SOA` tag in its type descriptor.
Array types `[T]` where `T` is `Layout::SOA` are SOA collections. A function
expecting `[Particle]` where `Particle` is `#layout(soa)` receives an SOA array;
no call-site annotation required.

Field access `arr[i].x` on a SOA collection is valid: sema desugars it to an
index into the `x`-field array. The user never sees this transformation.

Type mismatches that would require silent layout conversion at a call boundary
are a type error (same nominal type, incompatible layout — possible only if
Option B is also supported as a future-reserved form).

### 3. Codegen — layout transform

For a struct:

```jet
#layout(soa)
struct Particle {
    x: Float
    y: Float
    z: Float
    color: u32
}
```

The codegen emits a parallel Rust struct:

```rust
// Generated
struct ParticleSoa {
    x:     Vec<f64>,
    y:     Vec<f64>,
    z:     Vec<f64>,
    color: Vec<u32>,
}
```

Field access `particles[i].x` lowers to `particles.x[i]`. Iteration `loop p in
particles` lowers to an index loop that produces a proxy struct (or individual
field references if the body only touches specific fields — v1 can use a simple
index loop with a full proxy struct; optimization of field-subset access is
deferred).

Resize, push, pop, and slice operations on a SOA collection apply uniformly to all
field arrays. Lengths must stay in sync; sema enforces this by making all mutation
go through the collection API rather than direct field-array access.

---

## v1 scope

- Whole-struct SOA only. Partial-field SOA (`#layout(soa: x, y, z)`) deferred.
- Fixed-length SOA arrays (`[Particle#N]`) deferred; v1 supports only growable
  `[Particle]` collections.
- No SOA slice borrowing syntax in v1; iteration is by index loop or value loop.
- The layout tag does not affect serialization in v1; `#[Serialize]` on a SOA struct
  serializes as if AOS (one object per element). Revisit when serialization is
  spec'd for M10+.
- No SIMD auto-vectorization pass in v1; the layout transform is the feature; SIMD
  is a later optimization layer.

---

## Deferred status — rationale

SOA layout requires:
1. A codegen layout transform (non-trivial struct splitting in the Rust output).
2. A proxy type for field access in loops (ensuring `p.x` still compiles post-transform).
3. Sema tracking of layout tags on array types and enforcement at function boundaries.
4. At least one example + golden test (I5).

None of this blocks v1 correctness. The syntax decision (D-SOA1) is worth ratifying
now so plans written against it use a stable spelling. Implementation waits until
after v1 ships and the M12+ roadmap slot opens.

---

## Test plan (for when implementation begins)

1. **Happy-path example** — `examples/features/soa_particles.jet` creates a
   `[Particle]` collection, updates x/y/z in a loop, prints a checksum. Golden test
   verifies output (I5).
2. **AOS/SOA mismatch** — function expecting `[PlainParticle]` receives a SOA-tagged
   collection → type error snapshot.
3. **`#layout` on non-struct** — applying `#layout(soa)` to an enum or fn → E0430
   snapshot.
4. **Field access** — `particles[i].x` lowers correctly; validate via integration test
   that touches individual fields, not just a loop.
5. **Push/pop sync** — all field arrays grow together; a direct mutation of one field
   array (if accessible) is blocked.

---

## Resolved — D-SOA2 follow-ons ratified 2026-06-22 (no owner decision open)

1. **Partial SOA → deferred.** D-SOA2B=A: **whole-struct only** in v1. Partial
   (`#layout(columnar: x, y, z)`) is a future feature with its own ballot.
2. **Per-container form reserved.** D-SOA2C=A: `columnar [T]` in type position is
   **reserved** — emits "not yet supported", not a parse error.
3. **Serialization transparent.** D-SOA2D=A: a `columnar` struct serializes **AOS**
   (one object per element, portable); the layout is invisible to serde.
4. **Ownership atomic.** A `columnar` collection moves/borrows all field arrays as a
   unit; sema treats the value atomically, no partial-field borrows in v1.
