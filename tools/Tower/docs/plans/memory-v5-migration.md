# Migration plan: current implementation → memory model v5

**Gate:** D-MEM1 = A, ratified 2026-07-03 (card #187). Model spec:
[../proposals/memory-greenfield.md](../proposals/memory-greenfield.md); usage:
[../proposals/memory-v5-gallery.md](../proposals/memory-v5-gallery.md).
**Workflow per stage:** failing ui fixtures / golden examples FIRST (I4/I5), then
parser → sema → codegen, full targeted tests green, docs updated, checkpoint
commit. One stage at a time; full suite at the end of each stage before the next.

**Greenfield rule (owner, 2026-07-03; matches syntax-decisions.md "no compat,
ever"):** pre-v5 spellings leave zero trace in the release. No deprecation
paths, no compat shims, and no diagnostics that mention a former spelling —
`~`, `-> &T` returns, stored `&T` fields, `#Ref`, `api:`, `.clone()` simply are
not part of the grammar/stdlib and fail as ordinary unknown syntax. Teaching
diagnostics exist only for genuine user mistakes (writing to a read param,
use-after-`^`, exclusivity conflicts) and for foreign-language guesses
(`mut`/`take`/`view`, E0056–E0058, re-pointed at the v5 sigils).

## Current seams (audited 2026-07-03)

- `crates/jet-foundation/src/AST.rs:18` — `AccessConvention` (Read / Mutate /
  Move / Infer).
- `crates/jet-foundation/src/Syntax.rs:135-137` — `SIGIL_MUTATE "~"`,
  `SIGIL_MOVE "^"`, `SIGIL_VIEW "&"`.
- `crates/jet-sema/src/Sema/CheckerOwnership.rs` — conventions, moved-map, L0201.
- `crates/jet-sema/src/Sema/CapabilityFreeze.rs` + `ApiFreeze.rs` — D-CAP8
  freeze/E0912, `api:` manifest plumbing.
- `crates/jet-codegen/src/Codegen/Context.rs:774` — `rust_param_type` lowering.
- Live syntax that ceases to exist: `examples/features/memory/ownership.jet`
  (`~Int`, `-> &String` borrow return), `ref_field.jet` (stored `&String` +
  `#Ref`, D-REF-SHORTHAND).

## Stages

**S1 — Glyph flip (foundation + parser + formatter). DONE (2026-07-04).**
`&` becomes the write sigil (rename `SIGIL_VIEW` → `SIGIL_WRITE`, decision row
D-MEM1, I7). `~` is removed from the grammar entirely — it fails as ordinary
unknown syntax, no special-case message. `^` unchanged. Call sites mirror
`&`/`^` only. Receivers: `&self` / `^self` (D-MUTSELF1 pattern kept). Formatter
emits the v5 glyphs + fmt STABILITY test (round-trip rule). E0056–E0058
foreign-guess errors (`mut`/`take`/`view`) re-pointed at the v5 sigils.

**S2 — Signatures can't lie (sema). DONE (2026-07-04).**
Unmarked param = Read, enforced: a body write is an error with a fix-it (add
`&` at param + call sites) — free once elevation was cut, since E0111/E0205
already gate on `info.mutable` and only ever saw `true` for an unmarked param
via elevation. `AccessConvention::Infer` deleted (enum variant gone, every
match arm exhaustive without it); `Capability.rs` (the elevation pass) deleted
outright, no fallback. `CapabilityFreeze.rs` (E0912) deleted outright; the
`api:` manifest field (`ApiMode`) no longer exists — an ordinary unknown-field
error (E1216) like any other typo'd key. `ApiFreeze.rs`'s snapshot mechanism
remains — it now backs pub-metadata semver diffing unconditionally on every
publish (E1218/E2601), no `api:` opt-in gate. L0201 deleted: passing a named
binding to a Move param without `^` (or a std constructor consuming a
borrowed binding) is now E0209, a hard error regardless of liveness — no
silent clone ever. Fix menu: `^name` when this is `name`'s last use;
`name.clone()` (today's actual clone spelling — `copy name` per D-CAP2 isn't
parseable until S4) or reorder to make this the last use, when `name` is
still live after.

**S3 — Second-class borrows (deletions).**
`-> &T` return types and `&T` struct fields are simply not in the v5 grammar
(ordinary syntax errors); `#Ref` marker, E0207, and E0427 are deleted outright.
The "how do I store a reference?" question is answered by the ownership
diagnostics suggesting owned field / `Shared<T>` / `Id<T>` — forward-looking
guidance, no mention of former spellings. `ownership.jet` and `ref_field.jet`
rewritten to the v5 surface (their diffs are the acceptance test).

**S4 — `copy` verb. DONE (2026-07-04).**
D-CAP2 keyword form lands (`copy x`); it is the one copy spelling in the
release surface — `.clone()` does not exist there (one way to mean it, I8).
Rule: `^` marks giving away a *named binding*; temporaries (`copy x`, literals,
call results) pass without `^` — nothing survives to be used-after.
`Expr::Copy` is a real prefix-verb AST node (parser/formatter/codegen), and is
also the sole lowering target the compiler's own internal duplication
rewrites (val-binding-from-read-param, owning struct-field reads) now
build — one mechanism, whether the user writes it or the compiler inserts
it. `.clone()` is not a callable Jet method: `infer_method_call` no longer
special-cases `clone`, so it falls through to the ordinary "no such method"
path. A non-cloneable type under `copy x` is E0211 (new code); a scalar is
legal but redundant (already `Copy`, no error). Every E0121/E0201/E0209/etc.
fix-it that used to suggest `.clone()` now suggests `copy name`.

**S5 — View values (stdlib). DONE (2026-07-04).**
`[T]` slice views were already live (`list.view(a..b)`, D-DYNARRAY1, shipped
2026-07-01, predates this migration) — nothing to do there; "same machinery"
turned out to mean the same scope-liveness *reasoning*, reused below, not a
shared code path (`String` has no distinct view type the way `View<T>` does).

`String.trim()`/`.after(sep)`/`.before(sep)` bound to a local (`d :: s.trim()`)
now return a zero-copy `&str` window into the receiver's buffer instead of an
eagerly-allocated `String`, whenever sema proves the binding can't outlive its
owner — `Binding::string_view` (AST.rs), tracked in a `list_views`-shaped map
(`Checker::string_views`), escape-checked as **E2307** (new code). Unlike
`View<T>`, `String` stays ONE Jet-level type end to end (D-MEM1 gallery: "one
String type") — the view-ness lives on the *binding*, invisible to the type
system, so codegen (TIR `lower.rs`'s `Stmt::Val` handling) picks the borrowed
`_view` prelude helper (`jet_string_after_view`/`_before_view`/`_trim_view`,
`&str -> &str`) instead of the owned one only for THIS shape. A view's legal
surface is deliberately narrow — chain another `.trim()/.after()/.before()`,
interpolate it (`"{d}"`, backed by new `impl JetDisplay for str` etc.), or
`copy d` to materialize an owned `String` — every other read (return, rebind,
struct field, call argument, list/tuple literal element, any other method)
is E2307, caught at ONE general choke point (the `Expr::Ident` inference arm,
gated by `allow_string_view_read`) rather than enumerated per call site.

`split` stays eager (`Vec<String>`, unchanged) — a zero-copy `[View-of-String]`
needs the same generalized List-of-views representation work as `Shared<T>`/
`Pool<T>` (S6-scale), not a stage-S5-sized change; deferred, named here as a
gap, not silently dropped.

Golden pinning: `examples/features/memory/string_view.jet` (view binding,
original string still readable after, `copy` escaping a function boundary) +
two ui snapshots (`tests/ui/string_view_outlives_owner.jet` — E2307 on
`return d`; `tests/ui/string_view_task_capture.jet` — E1102 on a task
capturing a view, mirroring `task_detach_view_capture.jet`'s existing
`View<T>` case). Perf: `jet bench` fixture (`#Bench` block, D-BENCH1/D-TOOL5,
the existing convention — no new benchmarking harness) in the same example.

**S6 — `Shared<T>` and `Pool<T>`/`Id<T>`.**
CoreLib types + lowering (`Arc<RwLock>` class; generational arena). Ownership
diagnostics suggest them by name. Examples + goldens for: shared config across
tasks, entity world, parent-pointer tree. Any owner-facing spelling not already
fixed by the ratified proposal text (e.g. `@Resource` naming) gets a mini-ballot
BEFORE this stage builds it.

**S7 — `policy` floors.**
Module-level `policy` item (parser + manifest). `no_alloc` first (allocation
sites error inside the module). The final policy list is a follow-on ballot;
only ratified members ship.

**S8 — Diagnostics + docs sweep (rides every stage, finishes here).**
Every new/changed error: code + what/why/fix in docs/spec/diagnostics.md + ui
snapshot (I4). spec.md memory chapter rewritten to v5. syntax-decisions.md:
D-MEM1 row + supersession notes on D-CAP7 (reshaped), D-CAP8, D-REF-SHORTHAND1/2,
L0201. examples/features/memory/* is the executable spec of v5 (I5).

**S9 — Verification gate.**
Full `nix develop -c cargo test`, goldens, fmt stability, diagnostics coverage,
`./target/debug/jet run` smoke on rewritten examples (fresh binary, not the
stale Nix-store `jet`). Checkpoint commit per stage throughout.

## Ordering & interactions

- S1 → S2 → S3 sequential (same seams). S4 anytime after S1. S5 after S3.
  S6/S7 after S2. S8 accumulates; S9 final.
- Card #188 (syntax decrees: `[]`, `Val/None`, bare lambdas, tuple destructure,
  `files`/`.write`, `.after`) is independent; if it lands first, S5 inherits
  `.after`/`.before` for free. Both cards touch examples/snapshots — do not run
  their sweeps concurrently.
- jetpack (D-JPK track) is unaffected and may proceed in parallel.

## Out of scope (post-v1 / reserved)

First-class (storable/returnable) borrows — philosophy C1 tier 2, behind
explicit syntax, only if real programs demand them. No work here.
