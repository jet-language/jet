### D-FIXARR1 — Should fixed-size lists `[T#N]` become real stack arrays (so `#Uninit` is sound)? (rec B)

1. **Gist:** Decide whether the existing fixed-size list `[T#N]` lowers to a true fixed stack array (no heap, no zero-fill) instead of today's growable `Vec`, which is what `#Uninit` needs to be safe.

2. **Story.** Walter writes firmware for a soil-moisture sensor. He needs a 4096-byte scratch buffer that lives on the stack and is filled by a DMA read, so he writes `use core.mem` and `#Uninit scratch: [U8#4096]`. Today that buffer secretly becomes a heap `Vec<u8>` that the compiler must zero-fill before his DMA write — a heap allocation and a 4096-byte memset he explicitly asked to skip, on a chip with 16 KB of RAM. He wants `[U8#4096]` to mean what it says: 4096 bytes, on the stack, no init, no allocator.

3. **In the wild:**
```jet
use core.mem

// A fixed protocol frame parsed off the wire, never resized.
struct Frame {
    header: [U8#8]      // exactly 8 bytes, on the stack inside Frame
    crc:    [U8#4]
}

fn read_frame(dev: edit Device) -> Frame {
    #Uninit raw: [U8#12]          // 12 uninitialized bytes, stack-allocated
    dev.fill(edit raw)            // hardware writes all 12 — proven by E0420 dataflow
    Frame { header: raw[0..8], crc: raw[8..12] }   // slices copy out (S40)
}
```

4. **Other languages:**
```rust
// Rust: distinct types. Fixed array vs growable vec are different types entirely.
let a: [u8; 12] = [0; 12];   // stack, Copy if T: Copy, no resize
let v: Vec<u8>  = vec![];     // heap, growable
let m = MaybeUninit::<[u8; 12]>::uninit();   // sound only because [u8;12] has a fixed layout
```
```zig
var raw: [12]u8 = undefined;   // stack, fixed, uninit — exactly Walter's case
```

5. **Tradeoffs (subagent-reviewed):**

| Option | Stack-allocated / zero-cost | `#Uninit` sound without zero-fill | Beginner mental model | One-path consistency with `[T]` |
|---|---|---|---|---|
| A — keep `Vec` lowering | no (heap) | no (must zero-fill, defeats feature) | "same as a list" | full (already shipped) |
| B — real stack array, same `[T#N]` type (rec) | yes | yes | "a list whose size is locked" | one type, widens to `[T]` |
| C — separate new array type + new spelling | yes | yes | two collection types to learn | two types, conversions to manage |

6. **Options.**

- **Option A — keep the `Vec<T>` lowering (status quo).** `[T#N]` stays sugar over `Vec<T>`; `#Uninit` falls back to allocating + zero-filling.
```shell
$ jet build sensor.jet
# compiles, but [U8#4096] is a heap Vec, and #Uninit silently zero-fills 4096 bytes.
# Walter's "skip the init" request is ignored. On a 16KB-RAM chip this is a bug, not a nuance.
```

- **Option B — `[T#N]` lowers to a real fixed stack array `[T; N]` (recommended).** Same surface spelling `[T#N]` (S76, ratified). Codegen changes `Type::FixedList { elem, len }` at `Source/Codegen/Context.rs:256` from `Vec<{T}>` to Rust `[{T}; {N}]`. Semantics: **copy** when `T` is copyable (Int/Float/Bool/Char/fixed-of-copy), **move** otherwise — mirroring how Jet already treats values (no new keyword). A `[T#N]` **widens to `[T]`** by copying its elements into a fresh list when passed to a `[T]` slot (S76 rule c, unchanged surface; now an explicit `.to_vec()` in codegen). **Slicing** `raw[a..b]` copies elements into a `[T]` (S40, unchanged). `#Uninit raw: [U8#12]` lowers to `MaybeUninit::<[u8; 12]>::uninit()` — sound, stack, no zero-fill. A beginner still writes `[Int#3]` and reads "a list of exactly 3 Ints"; nothing in the surface changes, the size guarantee just becomes real.
```jet
use core.mem
fn main() {
    pts: [Int#3] :: [10, 20, 30]   // exactly 3, stack-allocated, copies on assign
    first :: pts[0..2]             // first: [Int], a copied 2-element list (S40)
    print("{pts.len}")             // 3 — compile-time constant (S76 rule e)
}
```
```shell
$ jet run pts.jet
3
```
Error surface (provisional codes, memory range):
```jet
fn main() {
    a: [Int#3] :: [1, 2, 3]
    print("{a[5]}")     // literal index past the end
}
```
```shell
$ jet run pts.jet
error[E0965]: index 5 is out of range for [Int#3]
  --> pts.jet:3:13
   |
 3 |     print("{a[5]}")
   |             ^^^^ valid indexes are 0 through 2
   = a fixed-size list [Int#3] has exactly 3 elements, known at compile time.
   = fix: use an index from 0 to 2, or check at runtime with a condition.
```
(E0965 already exists for compile-time out-of-range. The new codegen surfaces two *additional* provisional memory-range codes: **E0613** — `#Uninit` requires a fixed-size element type with a known layout, not a growable `[T]`; **E0614** — `[T#N]` element type `T` is not stack-sized / has no fixed layout. Both **provisional** until implemented, both in the E0613–E0617 memory band, neither reuses a taken code.)

- **Option C — a separate fixed-array type with its own spelling.** Introduce `[T; N]` (or another spelling) as a *distinct* type from `[T#N]`, so the language has growable `[T]`, refined-`Vec` `[T#N]`, and stack-array `[T; N]`.
```jet
fn main() {
    refined: [Int#3] :: [1, 2, 3]   // S76 fixed-size-but-heap list
    stacked: [Int; 3] :: [1, 2, 3]  // a THIRD collection type, stack-allocated
}
```
```shell
$ jet run three.jet
# works, but a beginner now has three array-ish types and S76 already
# rejected the [T; N] spelling — ';' collides with S6 statement terminators.
```

**Recommendation:** B. S76 already ratified `[T#N]` as the one fixed-size spelling and explicitly *rejected* `[T; N]` (the `;` collides with S6 terminators), so the open question is not the spelling but the lowering: making the *existing* type a genuine stack array is the only path that keeps one collection model, makes `#Uninit` (c76/D-UNINIT1) sound without a silent heap-allocate-and-zero-fill, and costs beginners nothing — they keep writing `[Int#3]` and it simply means what it says.

**Owner Q1 — copy threshold.** B copies `[T#N]` on assignment when `T` is copyable and moves otherwise, matching Jet's existing value semantics. Confirm you want the copy/move line drawn exactly where `T`'s own copyability draws it (no separate "arrays always copy" or "arrays always move" rule).

**Owner Q2 — `var [T#N]`.** S76 rule (b) says a `var` initialized from a literal *widens to growable `[T]`*. Under B that means a mutable fixed-size stack array is never directly expressible by a beginner (you get a growable `[T]` instead); experts reach a true mutable stack array only via a `var` annotated `: [T#N]`. Confirm that's the intended split, or whether `var x := [..]` should stay fixed when annotated.
