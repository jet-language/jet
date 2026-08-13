# Mine-video synthesis: Logan Smith Rust series (9 videos)

*Mined 2026-07-24. Report-only; no Tower/repo changes. Companion to `2026-07-24-verse-video-mining.md`.*

## Videos (all one creator — Logan Smith)

| ID | Title | Len | Views | Comments |
|---|---|---|---|---|
| 8j_FbjiowvE | 5 Strong Opinions On Everyday Rust | 7m | 93k | 241 |
| KWB-gDVuy_I | Constructors Are Broken | 18m | 185k | 722 |
| Klq-sNxuP2g | Moves Are Broken | 52m | 38k | 412 |
| SMCRQj9Hbx8 | Comprehending Proc Macros | 48m | 95k | 410 |
| wU8hQvU8aKM | Two Ways To Do Dynamic Dispatch | 20m | 128k | 280 |
| s5S2Ed5T-dc | A Simpler Way to See Results | 19m | 178k | 316 |
| A4cKi7PTJSs | Use Arc Instead of Vec | 15m | 222k | 447 |
| SqT5YglW3qU | Rust Functions Are Weird (But Be Glad) | 20m | 213k | 447 |
| 6c7pZYP_iIE | Choose the Right Option | 18m | 110k | 234 |

**Extra source (added 2026-07-24):** matklad, *"Study of std::io::Error"* — https://matklad.github.io/2020/10/15/study-of-std-io-error.html. Folded into the error-design rows below. (Note: the two YouTube links the owner sent later — `s5S2Ed5T-dc` and `Klq-sNxuP2g` — were already in this batch of 9. Their ledgers were re-read for the deeper extraction below; no re-capture was needed.)

**Coverage/limits:** all 9 transcripts read in full (auto-captions only — no creator captions — medium confidence, technical tokens garbled but consistent; no claim rests on caption text alone). Stratified comment samples read per video (top-liked + technical + replies). 141 claims across 9 hand-built ledgers at `/tmp/<ID>.claims.json`. **Independent-corroboration caveat:** all nine share one speaker, so agreement across videos is NOT independent corroboration — it is one consistent viewpoint. Treated accordingly.

## Verdict

**No code gaps. This series is strong external validation of Jet's ratified design.** Across every video, the "good practice" Logan advocates is something Jet has already adopted — and in most cases Jet made the good pattern *structural* (the default or only spelling) rather than advisory. The one genuinely novel idea in the whole batch that Jet has not built (transactional rollback, from the companion Verse video) is already tracked as a "watch" lesson.

## The dominant cross-cutting theme

**"Make invalid states unrepresentable / strong invariants as the unit of reasoning."** It runs through Constructors (build-then-construct-valid; ctors can't signal failure → return Optional), Moves (strong invariants delete corner cases; destructive moves preserve them), Options (non-null refs via niche optimization), and Results (typed error enums). Jet's entire safety story is this theme made structural.

## Per-theme cross-check

| Video theme | Jet status | Evidence |
|---|---|---|
| **Errors as typed values, not exceptions/erased** (Results, Constructors) | **Aligned, structural** | `T ? E` typed payload, `?`, `??`, `.drop()`, `#MustUse`, E0401-05. No exceptions. The Results video's own top pains (lib errors skipping the `Error` trait; anyhow-infects-downstream; `core` fragmentation) are things Jet's language-native error carrier pre-empts. `docs/spec/spec.md:784-821` |
| **`Option<&T>` not `&Option<T>`** (Options) | **Aligned, structural** | `&User?` grammar = "write access over an optional User" — the reference sits *inside* the optional by construction. The bad pattern is awkward to even spell. Niche optimization inherited via Rust lowering. `docs/spec/spec.md:528-557` |
| **Reference the borrowed view, not the container** (`&[T]`/`&str`, 5-Opinions, Arc) | **Aligned, arguably safer** | `View<T>`/`ViewMut<T>`/`View<str>` are owner-provenance-tracked borrowed views; `~` for an owned copy across a boundary. Owner tracking (E2305/E2307) catches use-after-free the C++ commenter warned about. `docs/spec/spec.md:329,371,391-405` |
| **Make invalid states unrepresentable** (Constructors, Moves, Verse parse-don't-validate) | **Aligned, rich** | `validate { }` in-struct rules accumulating `[FieldError]` (D-VALIDATE1=A); `#Invariant(...)` refinements (D-REFINE1); range/distinct-Int types (D-RANGETYPE1) that lower without bounds checks; `require(...)`. `newtype` keyword deliberately **declined**. `docs/spec/syntax-decisions.md:646,652,2707,1752` |
| **Named constructors that build-then-validate** (Constructors) | **Aligned** | `Type.new(...)` for hidden-state construction; `Type.{...}` literals; `.new(...)` receiver-elision (D-SHAPE3a). Note: commenters insist the correct term is "named constructor," **not** "factory" — worth honoring in Jet docs. `docs/spec/spec.md:520-560` |
| **Dynamic dispatch is a deliberate choice** (DynDispatch) | **Aligned, dual-facet** | S48: trait in type position (`fn f(s: Shape)`) = auto-boxing + dynamic dispatch (beginner magic); `<T: Shape>` = monomorphization (expert static). **No user-facing `dyn`.** `docs/spec/syntax-decisions.md:963-965` |
| **Least-powerful mechanism; bounded metaprogramming** (ProcMacros, 5-Opinions impl-Into) | **Aligned by invariant** | Jet has comptime + entry-local user `derive` + `#[Codable]`, but **no proc-macro-style arbitrary-syntax invention**. The ProcMacros comment "insane that arbitrary syntax can be added to a compiled language" is exactly the wow-factor I7/I8 reject. `docs/spec/spec.md:616-671` |
| **Destructive moves + strong invariants** (Moves) | **Aligned** | `^T` take = owned/consumed; lowers to Rust's destructive move. `View<T>` provenance handles the non-null/relocation invariants Logan spends 52m on. Self-referential/pinning is not a user surface. `docs/spec/spec.md:329,371` |
| **Don't overload sigils confusingly** (Moves: `&`/`&&` "unlearnable"; Verse: `[]`/`()`) | **Aligned** | E0029 — one capability sigil per parameter; single call syntax; ratified sigils bare/`&`/`^`/`~` with decision IDs (I7). `docs/spec/spec.md` E0029 block |
| **Function identity / monomorphization** (Functions) | **N/A to user surface** | Rust-internal (zero-sized fn types, merging). Jet hides rustc (I2); effect rows `--[…]->` are Jet's function-type annotation axis. Not a gap. |
| **Arc-vs-Vec as a *default*** (Arc) | **Correctly NOT a Jet default** | The video's own comments push back hard on defaulting to `Arc` ([403]/[371]/[53]). Jet keeps magic collection defaults for beginners; allocation/sharing control is expert opt-in (`#Policy(no_alloc/zero_rc/arena_bounded)`). Aligned with two-facet. |

## The two open threads (already tracked — not new gaps)

1. **Transactional rollback** (companion Verse video). Disposition = **"watch"**: only via explicit checked transaction regions, sema-proven rollback-safe. `docs/proposals/language-shape-research.md:354`, `docs/archive/language-lessons-and-regrets.md:399-407`.
2. **Optional narrowing / flow-typing after presence checks.** The Options ergonomics theme touches this; there is an **OPEN (unratified) audit card** for it in Tower. Not decided.

## Corrections worth noting (video-internal, don't affect Jet findings)
- Moves: on-screen error at 1:35 (host acknowledged); prvalue = "pure rvalue."
- Functions: multiple commenters call the C++ comparison a strawman (removing `noinline` reproduces identical codegen); function merging is an LLVM feature, not rustc-unique.
- Arc: `Arc<[T]>` clone == *sharing* (`&[T]`), not a Vec deep-clone — apples-to-oranges; building `Arc<[T]>` costs an extra allocation.
- 2024 edition already ships the unsafe-op-in-unsafe-fn lint (5-Opinions #5 partly landed upstream).

## Full value extraction (every drop, by category)

The table above gave the headline themes. This section pulls the smaller,
concrete ideas too. Each row says what Jet does now. Tags: **ALIGNED** (Jet
already has it), **CHECK** (worth verifying — I did not confirm it in code;
the cross-check ran out of session budget), **REJECTED** (Jet chose otherwise
on purpose), **DOC** (a wording or teaching fix).

### Performance

1. **Niche-packed optionals.** `Option<&T>` is pointer-sized: a reference can
   never be all-zeros, so all-zeros means `None`. Rust *guarantees* this
   layout (not incidental). matklad goes further: `io::Error` packs an OS
   errno, a simple kind, and a boxed custom error into one pointer, and could
   set a low bit to stay pointer-sized even for `io::Result<i32>`. **CHECK:**
   does Jet's lowering guarantee niche packing for `&T?` / `^T?`, so an
   optional reference costs the same as a raw pointer?
2. **Error stack size is not free.** matklad: *"on-the-stack size of errors is
   important: you pay for it even if there are no errors."* Keep `E` small; box
   the large variant. The DynDispatch video shows `anyhow::Error` uses a thin
   pointer to a heap struct whose first field is a vtable pointer, so
   `Result<T, anyhow::Error>` stays one pointer wide on the happy path.
   **CHECK:** does Jet's default `T ? E` stay one word when `E` is the default
   `Error`? Is there an expert thin-error option?
3. **O(1) shared-immutable clone.** `Arc<[T]>` / `Arc<str>` clone = refcount
   bump + pointer copy, no allocation. Pick non-atomic `Rc` when single-thread,
   atomic `Arc` only across threads; drop the capacity word when growth is
   unused (16 vs 24 bytes); `Box<[T]]>` is tightest when you never clone.
   **CHECK / ALIGNED:** Jet's `#Policy(no_alloc / zero_rc / arena_bounded)` and
   the read-vs-write facet cover this axis; confirm Jet can *auto-pick* the
   cheap backing when it proves no mutation and prove the thread need.
4. **Static dispatch is the zero-cost default.** A trait call on a concrete
   type resolves statically; the dynamic cost starts only at the coercion.
   Wide-pointer trait objects have no data dependency between value and vtable
   (a pipelining win) vs the intrusive-vtable dependent load chain. **ALIGNED:**
   Jet generics monomorphize; S48 makes dynamic explicit.
5. **Monomorphization bloat → function merging.** LLVM folds identical machine
   code (even `x+x` vs `x*2`), so unique function types do not always cost
   binary size. Jet inherits this through rustc/LLVM. Honest cost to state, not
   a win to over-claim.
6. **Generic wrapper → monomorphic core.** matklad's `new<E>` delegates to a
   non-generic `_new` so less code duplicates per instantiation, which cuts
   compile time. **CHECK:** Jet codegen could apply this to generic
   constructors.
7. **Large-value / in-place construction.** Rust leans on return-value
   optimization; heap init of a big value needs `MaybeUninit`, and a big value
   built on the stack can overflow it. Jet lowers to Rust, so it inherits this.
   **CHECK:** Jet's story for building a large value in place.
8. **Const/static promotion.** A locally-built table that depends only on
   compile-time constants gets a `'static` lifetime with no heap allocation.
   The transpiler can lean on this for vtable-like tables.

### Safety

1. **Make invalid states unrepresentable.** The strongest cross-video theme
   (a 505-like top comment). **ALIGNED:** `validate { }`, `#Invariant(...)`,
   range/distinct-`Int` types, no null, `T?` / `T ? E`.
2. **Single-motion construction.** Build every field as a local, then make the
   whole value in one step. No half-built object, no two-phase init, no
   reachable zero-value default (the Go footgun). **ALIGNED:** struct literals,
   dotless is an error, no uninitialized path. **CHECK:** is any implicit
   zero-value of a struct reachable in Jet? It must not be.
3. **A constructor must be able to fail.** C++ constructors cannot signal
   failure by their signature; the fix (throw) hides an exit at every call
   site. **ALIGNED:** `Type.new(...) -> T ? E`.
4. **Non-null types, checked once.** Check for absence at the boundary, then
   carry a stronger type inward, instead of a null check at every step.
   **ALIGNED:** `T?` as an outer layer.
5. **Destructive moves keep invariants.** A non-destructive move forces a type
   to allow an extra empty state, which weakens its invariant, spreads null
   checks program-wide, and runs a do-nothing destructor. **ALIGNED:** `^T`.
6. **`unsafe` has two meanings — split them.** On a block it *permits* unsafe
   operations; on a function it *imposes* a caller contract. Rust's old default
   made the whole body of an unsafe function an implicit unsafe block, which
   hides where the real risk is. Rust 2024 turned on a lint
   (`unsafe_op_in_unsafe_fn`) to require an explicit inner block. **CHECK — a
   direct design lesson:** does `#Unsafe("reason") fn` in Jet permit unsafe
   operations across its whole body, or does it still need an explicit inner
   `#Unsafe { }`? If the former, adopt the split so risk stays traceable to the
   smallest block.
7. **`unsafe` is greppable — that is the point.** *"Grep for unsafe to find
   where you might have made a mistake"* (289 likes). **ALIGNED:**
   `#Unsafe("reason")`. Caveat: the real mistake can sit *outside* the block
   (an invariant set up elsewhere), so grep is necessary, not sufficient.
8. **Keep exhaustiveness load-bearing.** Never glob-import enum variants above
   a match: dropping a variant then silently turns its arm into a catch-all
   binding, and the compiler stops checking exhaustiveness. This bit real
   engineers (421-like pain thread). **CHECK:** is Jet's match exhaustiveness a
   hard error, and can a glob or catch-all binding silently swallow a missing
   variant? Keep it a hard signal.
9. **All errors must be uniformly handleable.** A library error type that does
   not implement the `Error` trait breaks generic handling (213-like pain).
   **ALIGNED:** Jet's language-native error carrier makes every error uniform by
   construction, and sidesteps the Rust `thiserror`-for-libs /
   `anyhow`-for-apps split entirely.
10. **Absence is not failure.** `HashMap::get` returning nothing is not an
    error. **ALIGNED:** Jet keeps `T?` (absence) and `T ? E` (failure)
    separate — exactly the distinction Rust users hack around with
    `Result<T, ()>`.
11. **Self-referential types break under a move.** A value that stores its own
    address is invalid after a byte-copy move; this is the case Rust's `Pin`
    exists for, and commenters call `Pin` confusing. **CHECK — likely a real
    gap:** how does Jet handle a self-referential value under `^T`, without
    Rust's `Pin` and without beginner lifetime syntax? It must be automatic.

### Surface and API design

1. **`Option<&T>`, never `&Option<T>`.** The reference belongs *inside* the
   optional: it hides the storage type, drops the `.as_ref()` at every call
   site, is `Copy`, allows a `.filter()` before yielding, and stays callable
   whatever the caller stores. **ALIGNED:** `&User?` in Jet means "write access
   over an optional `User`" — the reference is inside by construction.
   **CHECK:** the mutable fork — `&User?` gives "mutate the value" (like
   `Option<&mut T>`); the other shape, "let the caller set it to `None`" (like
   `&mut Option<T>`), has real uses in the video. Can Jet express the second
   when needed, and is which-is-which unambiguous?
2. **Reference the view, not the container.** Take `&[T]` not `&Vec`, `&str`
   not `&String`, `&T` not `&Box`. **ALIGNED, arguably safer:** Jet's
   `View<T>` / `View<str>` carry owner provenance. Add a signature guideline:
   accept a view, not an owned container.
3. **Do not return `impl Into<T>`.** It lets the caller only call `.into()`, so
   `.into()` scatters across every call site — call it inside instead. But
   return-position `impl Trait` *is* right for closures and iterators, whose
   real types cannot be named. **CHECK:** Jet should absorb trivial conversions
   internally, yet still allow an `impl`-style return for unnameable
   closure/iterator types.
4. **Call them "named constructors," not "factories."** A factory holds
   configuration and *picks* an implementation; a plain named constructor does
   not. **DOC:** where Jet docs describe `Type.new`, use "named constructor."
5. **Struct-update fill.** Rust fills the rest of a struct from another value
   or from `Default`. **CHECK:** does Jet have a record-update sugar, or is it
   deliberately rejected? (Weigh against the Go zero-value footgun — an
   implicit default must not silently produce an invalid value.)
6. **No star imports** (they shadow names), except a sanctioned prelude or a
   test module. **CHECK:** Jet's import surface rule.
7. **A named "never fails" marker.** Rust uses a zero-variant error
   (`Infallible`) to fill a trait slot that demands a fallible signature but
   cannot fail; `Result<T, Infallible>` is just `T`. `Result<Infallible, E>`
   means "if this returns, it failed" (a server loop). **CHECK:** how does a
   never-failing Jet impl satisfy a `T ? E` trait slot?
8. **Separate the matchable kind from the display detail** (matklad). Expose a
   small, `#[non_exhaustive]`, fieldless, `Copy` *kind* enum to match on, keep
   the rest encapsulated, and give hooks (`get_ref` / `into_inner`) for the raw
   detail without over-exposing it. **CHECK / guidance:** Jet's `E` design
   should offer a matchable kind plus a display plus an optional payload.
9. **Avoid kitchen-sink error enums** (matklad). Exposing a dependency's error
   as one of your variants makes it your public API — the dependency's next
   major version forces yours, and boxing it later is a breaking change. Push
   the dependency's error to the caller when you can. **Guidance** for Jet
   stdlib and user error types.
10. **Design the data model first in metaprogramming.** Model the concept as
    types before you parse or generate. **Guidance** for Jet's computed-module
    and derive machinery.

### Syntax

1. **A trailing expression without a semicolon carries meaning.** No semicolon =
   "return whatever this evaluates to" (the return types stay coupled, so a
   later type change is a compile error); a semicolon = "return unit, discard."
   Some viewers dislike the implicit-return rule as a footgun and force an
   explicit `return`. **CHECK:** is Jet's trailing-expression-return rule
   explicit enough, and does it overload one token with two meanings? Jet
   already has `.drop("reason")` for a deliberate discard, which is the explicit
   shape these viewers want.
2. **No user-facing `dyn`.** **ALIGNED:** S48 — a trait in type position means
   dynamic dispatch; `<T: Trait>` means static.
3. **Keep sigils distinct.** C++ overloaded `&` and `&&` for value categories,
   which made moves unlearnable (63-like comment); Verse's `[]` vs `()` and
   `logic` drew the same complaint. **ALIGNED, cautionary:** keep Jet's `&`,
   `^`, `~` distinct and intuitive; E0029 already forbids two capability sigils
   on one parameter.
4. **Express relocatability as an inferred fact, not a keyword.** C++26's
   `trivially_relocatable_if_eligible` was called clunky. If Jet ever needs a
   relocatability notion, infer it; do not add verbose keywords.
5. **Case-based disambiguation (idea).** A commenter asks why a variant and a
   binding are not told apart by case (as Haskell does), which would turn the
   glob-match footgun into a compile error. Weigh against the owner's dislike
   of arbitrary rules. **Idea only.**

## Open items worth verifying (I could not confirm these in code)

The cross-check agent hit the session limit, so these are honest CHECKs, not
confirmed gaps. In rough priority:

1. **`#Unsafe` two-meanings split** (Safety #6) — the clearest actionable
   design lesson. Confirm whether an unsafe *contract* also silently *permits*
   unsafe operations across the whole body.
2. **Self-referential / relocation story** (Safety #11) — likely a real gap;
   how Jet moves a self-referential value without `Pin`.
3. **One-word `T ? E` on the happy path** (Performance #2) — error stack size
   and an expert thin-error option.
4. **Niche packing for optional references** (Performance #1).
5. **Match exhaustiveness hardness + no silent catch-all** (Safety #8).
6. **Mutable optional composition** — can `&User?` express both "mutate value"
   and "set to `None`"? (Surface #1).
7. **Reachable zero-value default** (Safety #2) and **struct-update sugar**
   (Surface #5).
8. **Never-fails trait-slot fill** (Surface #7).
9. **Large-value in-place construction** (Performance #7).

These are candidates for a short verify pass or, where a design choice is
involved (#1, #2, #6), an owner ballot. None is urgent; none blocks current
work.

## Recommendations
- **Implement:** nothing. The series validates existing ratified design end-to-end.
- **Doc nicety (optional):** where Jet docs describe `Type.new`, prefer the term **"named constructor"** over "factory" (commenter correction, and it is more accurate).
- **Avoid:** the anti-patterns these videos name — untyped/erased errors, `&Option<T>`, `&Vec`/`&String` in signatures, `Arc`-as-default, arbitrary user syntax, sigil overloading. Jet already avoids all of them; the value is a checklist to *keep* avoiding them as the surface grows.

**Owner gates:** none newly raised by these 9. See the separate note (in chat) on the effect-abstracted-control-flow design question, which *is* owner-gated if pursued.
