# Jet's memory model: one link, proven costs, no lifetimes

Status: research-backed proposal, 2026-10-05. Nothing here is ratified or
changes behavior. It builds on three earlier proposals, which it does not
repeat: [foundations](memory-model-foundations-2026-09-27.md) (the `@` link
ballots), [next level](memory-model-next-level-2026-10-02.md) (one owner
settles, checks prove or stop), and the
[greenfield concept](greenfield-memory-system-2026-10-02.md). Work state lives
on Tower cards #3645 (memory model), #4249 (ownership checker gaps) and #4619
(the post-Rust safety net). The four owner choices this note raises are ballot
drafts under `~/.cache/jet-dev/ballots/READY/`; section 6 lists them.

Evidence. Every "Jet today" verdict below comes from `jet check` on the
candidate binary `/mnt/jetscratch/candidates/dev-05ea86f65/jet` (Jet 0.0.1,
built 2026-10-05 14:16), run over the 64-case Rust-borrow-checker acceptance
corpus that the agent MemEvidence wrote at
`/mnt/jetscratch/scratch/memcorpus/cases/` (outputs in `out/`). Case numbers
such as **C14** refer to that corpus. A `jet check` pass is a sema verdict, not
proof that the program runs on every tier. Ratified law is quoted from Tower
(`tower decision show <ID>`). Claims about other languages cite primary
sources in the final section; anything not observed is marked
**[inference]**.

## The answer in one page

The owner asked three things. Here are the short answers.

**Is Jet at least as memory safe as Rust?** For the surface that exists today,
yes in design and mostly in practice. Every Rust rejection in the corpus that
guards memory safety is also a Jet rejection (C02, C03, C05, C12, C18, C23,
C25, C26, C29, C41–C43, C45, C51, C55, C58–C61). One program is a real gap:
**C62** iterates a list while a local closure pushes into it. `jet check`
accepts it, and only the backend's MIR legality verifier stops the build with
an internal compiler error. Today that fails closed because the backend
checks again. After Rust and its checks leave, a miss like this would become
silent memory corruption. That is why the independent re-proof on card #4619
is a hard prerequisite for removing Rust (section 5.8).

**Can everything Rust and Polonius accept be written in Jet without
hassle?** Not yet. Jet accepts the common shapes (C01, C04, C07, C09, C13,
C21, C24, C27, C28, C31, C35, C38, C44, C50, C63, C64), but rejects 17
Rust-accepted programs: C06, C14, C14b, C15, C16, C17, C20, C24b, C24c, C32,
C33, C39, C40, C48, C52 (growing the list), C53, and C54 (section 3). None
needs a lifetime annotation to fix. Each is an unbuilt ratified feature (`@`
links, `loop p in &list`, moving out of a field you can edit, borrowed captures
in task groups), a checker precision gap, or one of the four ballots. Jet
already accepts some programs that stable Rust rejects, all safely: C11
(constant-index disjoint writes), C36, C46, and C49, which needs Polonius in
Rust. C63 writes a lending iterator without generic associated types. After
the fixes and the four ballots, every row of the matrix has a Jet spelling
with no lifetime annotation, and several are shorter than Rust's.

**Do `@` live links replace `View<T>`, and do they suffice for stored
references?** Yes to replacing the spelling: ratified D-MEMREF1=A says `@T`
"replaces the spellings of D-MEM-VIEWRET1" (`View`/`ViewMut`), `Shared<T>`,
`Cell<T>`, and the `Pin<T>` field form. They suffice only once two rules are
pinned down that ratified law left open:

1. **A link freezes shape, not value.** While a link is live, nobody may do
   anything that frees or moves the storage it points into: growing or
   shrinking a list, replacing a whole string, or switching an enum's case.
   Writing a new value into the place is allowed, and the link sees it. This
   is the meaning D-MEMREF1's own example needs (`p.hp = 5` shows through
   `t.captain`). It is also the only rule that is safe, live, and free at the
   same time. Ante reached the same split between "shape-stable" and
   "shape-unstable" storage independently.
2. **Long-lived links point at objects, not into growable storage.** A link
   that the compiler cannot bound to a scope cannot keep a list from growing,
   so it cannot point at a list item. The item must be its own object
   (`[@Node]`). Ballot D-LINK-ITEM1 decides this. It is the one place where
   D-MEMREF-LIFE1's "keep the target alive in counted memory" does not work as
   written, because keeping a list's buffer alive does not stop it from moving.

With those two rules, one `@` type covers Rust's `&T`, `&mut T`, `&'a T`
fields, `Rc<RefCell<T>>`, `Arc<Mutex<T>>`, and `Weak<T>`. The compiler picks
the cheapest representation that its proof allows, reports the choice, and
lets an expert lock it (D-AUTOPIN1). Wherever Rust's borrow checker would
accept the program, the link costs exactly what Rust's reference costs: a plain
pointer with no count, no check, and no copy. Jet adds a count or a check only
in programs that Rust rejects. In those programs Rust would need `Rc`,
`RefCell`, an index, or `unsafe`, each of which costs at least as much. That
is the design. The current emitter is not there yet: the generated Rust copies
or allocates where Rust would borrow in about a dozen common sites, including
fold callbacks, function values, `Shared<T>` access, `map.get`, and printing
text (section 4.2). Each has a card draft in section 7.

## 1. Jet today and its exact acceptance boundary

### 1.1 The access verbs and places

Jet has one ownership contract (spec "Ownership and borrowing"). A parameter
`T` reads, `&T` writes exclusively, and `^T` takes ownership. The call site
mirrors `&` and `^`. `~x` is the only copy spelling. A *place* is a name plus
its maximal field, index, or range projection. A bare place is a read window,
`&place` is the exclusive write window, and `~place` is new owned storage.
Different known fields are disjoint, constant indexes and ranges are disjoint,
and dynamic indexes conservatively overlap (D-SHAPE-PLACE1).

Copy, move, and share are chosen by the compiler unless marked
(D-COPY-DEFAULT1=A). A last use moves. A non-last use shares the storage, and
the first write makes the copy ("share on reuse"). Storing a read view in an
owning slot is a semantic copy (D-MEM-COPYSEM1=A). `#Policy(copies:
.Explicit)` and `!Mem.Copy(above: N)` make hidden copies visible or illegal.

### 1.2 Views and provenance

`View<T>` and `ViewMut<T>` are the checked stored and returned references.
Sema keeps one provenance and alias graph. An owner is a declaration identity,
and a view fact records place, access, extent, source kind, and invalidation.
A function result may name several possible owners; the compiler records the
set (D-MEMPROVENANCE2=A), and an expert may declare it with a trailing `from`
clause on results, parameters, traits, and function types
(D-MEMPROVENANCE3=A). Type definitions never carry owner parameters. This is
the same idea as Niko Matsakis' 2024 "origins as sets of loans": an origin is
a set of places, not an abstract lifetime. Jet already ships that formulation
without lifetime syntax.

### 1.3 Identity and shared state

| Need | Jet today | Cost today |
|---|---|---|
| Many holders, mutable, thread-safe | `Shared<T>` (`shared x`) | Always an atomic count plus a read-write lock, even in one thread (foundations F7) |
| Local interior mutation | `Cell<T>` with guards | Runtime borrow flag, like Rust `RefCell` |
| Back links | `Shared.Weak<T>` | Atomic weak count |
| Graphs, entity systems | `Pool<T>` + `ID<T>` | Generation check per access, like Rust `slotmap` |
| Address-stable fields | `Pin<T>` | None |
| Arena data | `Arena` scope-bound views | None; E0631/E0632 checked |
| Cyclic object graphs | `#Policy(gc)` | Private collector, slated for retirement by D-MEMREF-LIFE1 |

### 1.4 Concurrency

A task owns or copies its captures. `freeze(x)` makes one deeply immutable
owned snapshot. `task ^x` moves. A `task.group` child may borrow reads freely
and writes on proven-disjoint places (D-TASKBORROW1=A). `Shared<T>` locks per
statement, and `#Transact` commits several statements at once. One prover
answers every crossing (D-CONC-CROSS1=A). Data races are a compile error for
the covered surface (D-DATARACE1=C, architecture "Concurrency boundary
safety").

### 1.5 `@` links: ratified, not built

| Decision | Ruling |
|---|---|
| D-MEMREF1=A | `@T` is a checked live link; `@x` links to a place, field, or item, or makes a new object. Read access is transitive. `a == b` compares values, `a.same(b)` compares identity. Replaces the `View`, `ViewMut`, `Shared`, `Cell`, and `Pin`-field spellings. |
| D-MEMREF-LIFE1=A | When the owner provably outlives the link, the link is a plain pointer. Otherwise the target lives in counted storage from its creation, with cycle collection only for types that can form cycles. `jet gc report` lists each case with a fix. `!Mem.Rc` and `!Mem.Gc` forbid it. |
| D-MEMREF-EXCL1=A | Prove exclusivity where possible; elsewhere add a small runtime check at the lend or loop (Swift's model). `!Mem.Check` forbids the checks. |
| D-MEMREF-TASK1=A | A link type that reaches another task switches to thread-safe storage; `#Local`/`#Shared` pin it. |
| D-LINK-DEBT1=A | A link to a value that owes a job is generation-checked and never keeps it alive. |
| D-REACT-REF1=A | Writes through links notify tracked readers; untracked data has no notify code. |
| D-DENY-COST1=A, D-AUTOPIN1=A | Every hidden cost can be denied (`!Mem.Gc`, `!Lock`, `!Notify`, `!Dyn`, `!Mem.Check`, `!Mem.Copy(above: N)`); every automatic choice is reported and lockable. |

None of this is built. On the candidate binary, `t :: Team{captain: @p}`
reports E0107 "Nothing named `@p` exists here" (C33), and `@Node{...}` reports
E0119 "There's no type called `@Node`" (C32). The lexer and parser already
reserve prefix `@` for links (`Syntax.rs:30`; retirement diagnostics E0388 and
E0003 in `Compiler/JetLexer/Source/Lexer/Payload.jet:911,929`).

### 1.6 What is built, what is ratified but unbuilt, and what is broken

| Area | Built and observed | Ratified, not built | Defect against ratified law |
|---|---|---|---|
| Parameters and places | `T`/`&T`/`^T`, E0204 (C05, C58), two-phase `v.push(v.len())` (C04), NLL last use (C01), reborrow (C57) | — | — |
| Disjointness | Fields in one call (C07), constant indexes (C11), `split_write` (C09), `get_disjoint_write` (C13) | — | Two live `&` windows on sibling fields report E0212 when written through (C06); a whole-list write window cannot `push` (probes p2–p4) |
| Returned and stored views | `from` clauses (C27, C28, C50), struct with `View<str>` field from a parameter (C24), lending method (C63) | `to_views`, `*_views`, text views (D-ITER-VIEWCOLLECT1, D-COLLECTION-TOPK1, D-FOUND-VIEW1) | Same-scope `View<str>` field store says E2307 under a policy that is not set (C24b); a `View<str>` field has no string methods (C24c); a map value cannot be returned as `ViewMut` (C14b) |
| Flow sensitivity | Early return then mutate (C49, Polonius-only in Rust) | — | Matching on a write place does not bind write windows (C14, C15, C48; owner ruling of 2026-09-30, card #4249 criterion [#3974]) |
| Iteration | Nested read loops (C21), E0507 on push-while-iterating (C18) | `loop p in &list` (D-LOOP-STMT-ARROW1 text) | C20 rejects `loop p in &ps` with E0205; C62 is a sema miss that ICEs |
| Moves through `&` | Moves of locals and last-use fields (C47) | `old :: ^x; x = new` is the ratified replacement for `mem.replace` (D-CORESURF-SMALL1=A) | C53/C54 reject `^place` through `&` with E0201, so swap and take need deep copies |
| Tasks | Constant-window borrows in a group (C40b), sendability (C42, C43, C60) | — | `split_write` halves cannot be captured by group children (C40), contrary to D-TASKBORROW1=A |
| Read parameters | Lists by borrow | — | A range window cannot be passed to a `[T]` read parameter (C39: E0112 "wants [Int] … this is View<Int>") |
| Links | — | All of section 1.5 | — |

### 1.7 Does `@` replace `View<T>`, and is it enough?

The spelling question is settled: D-MEMREF1=A replaces `View`, `ViewMut`,
`Shared`, `Cell`, and `Pin` fields with `@`. Later ratified APIs
(D-ITER-VIEWCOLLECT1 `[View<T>]`, D-COLLECTION-TOPK1 `smallest_views`,
D-FOUND-VIEW1 text views) were written in the old spelling and move with the
cutover (card draft K2).

The meaning needs one clarification, which section 5 states in full. A
`View<T>` today freezes its owner's *value*: any overlapping write is E0212. A
link is *live*: D-MEMREF1's example writes `p.hp = 5` and reads the new value
through `t.captain`. One type cannot have both meanings. The resolution keeps
everything that matters for safety and speed:

- What a link forbids while it is live is **shape change**: anything that
  frees, moves, or reinterprets the storage it points into. That is exactly
  the set of writes that could make it dangle.
- Value writes are allowed and visible. Where the compiler proves no value
  write happens during a link's life (almost always, and always when Rust
  would accept the program), the link is lowered exactly like today's view:
  read-only and non-aliased.

Is the link enough for stored references? It covers every stored-reference
shape in the corpus once D-LINK-ITEM1 settles links into growable storage.
Generational references remain the right tool for one case only, a link to a
value that owes a job (D-LINK-DEBT1). `Pool`/`ID` remains a data structure for
entity systems, not a reference mechanism.

## 2. Research: what each system proved and what failed

### Rust: NLL, two-phase borrows, Polonius, view types

- **NLL (RFC 2094, 2018).** Lifetimes became sets of control-flow points
  instead of lexical scopes. It removed most "artificial block" errors.
  It deliberately deferred **problem case #3**: a function that returns a
  borrow on one path and mutates the owner on another (`get_default`). The
  RFC shows that the usual workaround does not even work there.
- **Two-phase borrows.** `v.push(v.len())` is accepted because the `&mut`
  for the receiver is reserved, then activated after the arguments run. A
  write inside the arguments is still an error. Jet matches both halves today
  (C04 accepts, C05 rejects).
- **Polonius.** The 2025h2 project goal ships a "stabilizable" alpha that
  accepts problem case #3 and the filtering lending iterator, passes crater,
  and is "still too slow". It still rejects the conditional linked-list cursor
  (`while let Some(now) = p { if c { p = &mut now.next } }`) and
  `remove_last_node_iterative`, which need full flow sensitivity. Lending
  iterators remain hard to use "due to the unrelated limitations in GATs".
- **Borrow checking without lifetimes (Matsakis, 2024).** An origin is a set
  of loans like `shared(a.b.c)`, computed with place liveness. The post names
  interior references (one field pointing into another) as the pattern Rust
  cannot express. Jet's provenance graph is this formulation already.
- **View types (Matsakis, 2021).** `&{golden_tickets} self` would let a method
  declare which fields it touches, so a caller can hold a borrow of one field
  while calling a method that uses another. Rust has not shipped it. Rust's
  per-function checking is the stated reason: a caller cannot see which
  fields a callee touches without a declaration.
- **Pain data.** Google's 2022 internal study of over 1,000 developers names
  "ownership and borrowing" as one of the top three hardest areas, with macros
  and async. The 2024 State of Rust survey lists slow compilation first and
  "Rust will become too complex" among the top worries. Ante's author
  describes aliasing-xor-mutability errors as "some of the most common errors
  in Rust", often fixed by "giving up and … inserting excessive calls to
  `clone`".

**What Jet takes:** place-based origins (already built), flow-sensitive loans
at least as precise as Polonius alpha, two-phase calls, and method footprints
in place of view-type syntax. **What Jet avoids:** lifetime parameters on
types, and the per-function blindness that makes view types necessary.

### Swift: exclusivity, `~Copyable`, `borrowing`/`consuming`, `~Escapable`

Swift 5 enforces "exclusive access to memory" statically where it can and
dynamically elsewhere, in release builds too (Trick, 2019). Dynamic checks
cover escaping closures, class properties, and globals. The cost is "small in
most cases", with advice to avoid class property access in hot loops. A
simple assignment is an instantaneous access; only `inout` and `mutating`
calls are long accesses. SE-0390 added noncopyable types, SE-0377 the
`borrowing`/`consuming` parameter conventions, and SE-0446 (Swift 6.2)
`~Escapable` types for `Span`. SE-0446 explains why: Swift's array iterator
"logically creates a copy of the Array" and holds a counted pointer, and
"these safety checks all incur runtime overhead".

**What Swift proved:** dynamic exclusivity is practical and cheap when the
compiler removes most checks. **What failed:** value semantics without
borrowing made Swift copy or retain where Rust borrows; a decade later Swift
added borrowing, consuming, noncopyable, and nonescapable types to win the
performance back. Jet must not repeat that order: borrowing comes first and
counting is the fallback, never the reverse.

### Hylo (Val): mutable value semantics, subscripts, projections

Hylo has no first-class references at all. A *subscript* yields a projection
of part of a value for the duration of the caller's use, and `inout`, `let`,
`set`, and `sink` variants are bundled under one name (Hylo language tour;
Racordon et al., JOT 2022). This removes stored references entirely, and with
them every lifetime question.

**What Hylo proved:** most code never needs a stored reference; scoped
projections cover accessors, `get_mut`, and map entries. **What it costs:**
graphs, parsers that keep pieces of their input, and observers need
indexes or copies. Jet keeps Hylo's lesson as the beginner default (read and
write parameters are scoped projections) but does not give up stored links.

### Mojo: origins

Mojo's lifetime checker tracks an *origin* per reference: which variable owns
the value and whether it is mutable. Origins are usually inferred, can be
named with `origin_of(x)`, and form unions for "one of these" results. It
also has "untracked" and "wildcard" origins; the documentation warns that a
wildcard "effectively disables Mojo's ASAP destruction … and prevents Mojo from
enforcing argument exclusivity". **Lesson:** an origin as a named set of
places is learnable; an escape hatch that silently widens to "anything"
poisons the whole scope. Jet's `from` clause has no wildcard, and must not get
one.

### Vale: generational references and regions

Every object carries a generation; a non-owning reference stores the
generation it expects and checks it on use. Measured overhead was 2% to
10.84% on one terrain benchmark. Regions then remove checks wholesale: a
`pure` function sees its inputs as one immutable region with no checks, and
isolates can be frozen and unfrozen as a unit. The authors call regions "a
borrow checker you can precisely enable and disable", and stress that "regions
don't change the semantics of the program". The next-level proposal records
that Vale abandoned hybrid-generational memory after more than 30 attempts.

**What Jet takes:** generation checks for links to values that owe a job
(already ratified, D-LINK-DEBT1), and "proven immutability removes checks" as
an optimization, never as new syntax. **What Jet avoids:** generation checks
as the default link representation; Jet's static proof already covers the
cases Vale's regions recover.

### Pony and Verona: reference capabilities and regions

Pony gives every reference one of six capabilities (`iso`, `trn`, `ref`,
`val`, `box`, `tag`) and proves data-race freedom between actors with
`consume` and `recover`. It works, but the capability lattice is the most
cited learning barrier in Pony. Verona (Microsoft Research) groups objects
into regions with one entry point, so ownership transfer of a whole object
graph between threads is one pointer move. **What Jet takes:** "a group of
objects crosses tasks as one" is how `freeze` and `^` should behave for
linked graphs. **What Jet avoids:** user-visible capability annotations.
D-MEMREF-TASK1 already infers the thread-safe form instead.

### Austral: linear types and borrow regions

Austral makes resources linear (used exactly once) and borrows them inside an
explicit lexical region. The specification's goal is "fits-in-head
simplicity". **What it proved:** linearity plus scoped borrows is enough for
safe resource handling. **What it costs:** explicit regions are ceremony that
NLL showed can be inferred. Jet already has linearity (D-LIN1, D-OWES1) and
infers regions.

### Lobster: compile-time reference counting

Lobster picks one owner per allocation, makes every other use a borrow, and
inserts a count increment only where a second owner appears. "Using this
analysis was able to remove around 95% of runtime reference count operations."
Its author needed changes in only two places across several dozen programs.
Lobster also reports cycles at program exit instead of collecting them.

**What Jet takes:** the exact shape of D-COPY-DEFAULT1 and D-MEMREF-LIFE1:
infer the owner, borrow everything else, count only where a second owner is
proven, and report each count.

### Cyclone: regions

Cyclone (Grossman et al., PLDI 2002) added region-annotated pointers to C with
inference inside functions and annotations at function boundaries. It proved
that region checking can make C-style code safe, and showed that annotation
burden at boundaries is what users feel. Jet's answer is boundary inference
plus an optional `from` declaration, never a required one.

### Ante: shape-stable shared mutability

Ante adds shared mutable references (`ref`, `mut`) next to Rust-style
exclusive ones (`imm`, `uniq`). Shared mutable references may point anywhere
whose *shape* is stable: struct fields, tuple members. Taking a reference into
a vector element, a union payload, or through a `Box` requires an exclusive
reference, because those are the places a write can free or move. Ante calls
the scheme "completely zero-cost".

**What Jet takes:** the shape rule, which is the missing half of D-MEMREF1.
It is the reason section 5 says "a link freezes shape, not value".

### Summary of lessons

| Lesson | Source | Jet rule |
|---|---|---|
| An origin is a set of places, inferred | Matsakis 2024, Mojo | Built: provenance graph, `from` |
| Flow-sensitive loans accept conditional returns | Polonius | Card K4 |
| Callers need to know which fields a method touches | View types | Ballot D-METHOD-FOOTPRINT1 |
| Dynamic exclusivity is cheap when rare | Swift 5 | Ratified D-MEMREF-EXCL1 |
| Count only where a second owner exists | Lobster | Ratified D-COPY-DEFAULT1, D-MEMREF-LIFE1 |
| Shared mutation is safe where shape is stable | Ante | Section 5.3, ballot D-LINK-ITEM1 |
| Borrow first, count second | Swift's history | Section 4 guarantee classes |
| Generations for values that must be settled | Vale | Ratified D-LINK-DEBT1 |
| Interior references are the next frontier | Matsakis 2024 | Ballot D-LINK-SIBLING1 |

## 3. Program shapes: Rust, Jet today, proposed Jet

Verdicts: **A** accept, **R** reject. "Rust" means stable NLL; "Polonius"
is the 2025 alpha. Jet today is the observed `jet check` verdict on the
candidate binary.

| # | Shape | Rust spelling | Rust / Polonius | Jet today | Proposed Jet |
|---|---|---|---|---|---|
| C01 | Borrow ends at last use | `let w = &mut c; w.n += 1; c.n` | A / A | A | unchanged |
| C04 | Two-phase call | `v.push(v.len())` | A / A | A | unchanged |
| C06 | Two live write borrows of sibling fields | `let a = &mut s.xs; let b = &mut s.ys;` | A / A | R: E0212, writing through `a` is misread as an owner change | `a :: &s.xs` … `&a.push(…)` accepted (K7) |
| C07 | Field read and sibling field write in one call | `record(&b.names, &mut b.notes)` | A / A | A | unchanged |
| C08 | Field borrow, then `&mut self` method touching another field | `let a = &mut s.xs; s.bump(); a.push(1)` | R / R (needs view types) | R: E0212/E0220 | A when `bump`'s footprint is `{count}` (D-METHOD-FOOTPRINT1) |
| C09 | `split_at_mut` | `let (a, b) = v.split_at_mut(mid)` | A / A | A: `(a, b) :: &v.split_write(mid) ?? …` | unchanged |
| C11 | Write borrows of two constant indexes | `&mut ps[0]`, `&mut ps[2]` | R / R | **A** | unchanged |
| C13 | Disjoint writes at runtime indexes | `ps.get_disjoint_mut([i, j])` | A / A | A: `get_disjoint_write` | unchanged |
| C14 | NLL problem case #3, `get_default` | `match map.get_mut(&k) { Some(v) => v, None => { map.insert(..); .. } }` | R / **A** | R: E0202, E2305, E0113 | A: `if map.get(key) == { .Val(v) -> return @v … }` (K4) |
| C15 | NLL problem case #2 | `match map.get_mut(&k) { Some(v) => v.push(1), None => .. }` | A / A | R: E0202 (payload of a write place is not a write window) | A (K4) |
| C16 | Entry / get-or-insert | `*counts.entry(w).or_insert(0) += 1` | A / A | R; idiom C16b costs two or three hash lookups | A: one lookup (D-MAP-SLOT1) |
| C17 | Update a map value in place | `*counts.get_mut("a").unwrap() += 1` | A / A | `counts["a"] += 1` is E0164 by ratified law; `&groups["a"].push(3)` works | `counts["a", or: 0] += 1` (D-MAP-SLOT1) |
| C18 | Push while iterating | `for x in &v { v.push(*x) }` | R / R | R: E0507 | unchanged |
| C20 | Mutable iteration | `for p in ps.iter_mut() { p.v += 1 }` | A / A | R: E0205; idiom C20b uses indexes | `loop p in &ps { p.v += 1 }` (K5, ratified spelling) |
| C21 | Nested read iteration | `for a in &v { for b in &v {..} }` | A / A | A | unchanged |
| C22 | Mutable chunks | `for c in v.chunks_mut(2)` | A / A | A, with a misleading L0501 "copies every time" lint on a write window | lint fixed (K7) |
| C24 | Struct holding a borrow | `struct Parser<'a> { src: &'a str }` | A / A | A from a parameter; R (E2307) in the same scope (C24b) | `struct Parser { src: @String }` in both (K2, K10) |
| C27/C28 | Return a borrow from one or either input | `fn longest<'a>(a: &'a str, b: &'a str) -> &'a str` | A / A | A: inferred, or `-> View<str> from a \| b` | `-> @String from a \| b` |
| C30 | Self-referential struct | not safe (`Pin` + `unsafe`, or `ouroboros`/`self_cell`) | R / R | R: E2307 | A: `keys: [@String] from text` (D-LINK-SIBLING1) |
| C31 | Tree with parent IDs | `Vec` arena + `usize` or `slotmap` keys | A / A | A: `Pool<Node>` + `ID<Node>` | unchanged |
| C32 | Tree with owning children and weak parents | `Rc<RefCell<Node>>` + `Weak` | A / A | `@` R (E0119); `Shared.Weak` works but locks (C32b) | `kids: [@Node]`, `parent: ?@Node` with a weak link (K1) |
| C33 | Field that refers to another value, owner keeps writing | `Rc<RefCell<Player>>` | A only with `Rc<RefCell>` | R: E0107 | `captain: @Player`; `@p`; no count when `p` outlives `t` |
| C34 | Many holders, one thread | `Rc<RefCell<T>>` | A / A | A: `shared` (locks) | `@Stats{…}` with no lock unless it crosses (D-MEMREF-TASK1) |
| C35/C38 | Closures capturing borrows | `\|\| count += 1` | A / A | A | unchanged |
| C36 | Closure holds a write capture while owner is read | `let add = \|\| v.push(2); v.len(); add()` | R / R | **A** (captures act at call time) | unchanged |
| C39 | Scoped threads reading halves | `thread::scope(\|s\| s.spawn(\|\| v[..2].iter().sum()))` | A / A | R: E0112, a window cannot be a `[T]` read argument | A (K7) |
| C40 | Scoped threads writing `split_at_mut` halves | `thread::scope` + `split_at_mut` | A / A | R: E1102/E0111 | A (K8, ratified D-TASKBORROW1) |
| C40b | Scoped writes to constant windows | needs `split_at_mut` in Rust | A with workaround | **A** | unchanged |
| C44 | Thread-shared counter | `Arc<Mutex<i64>>` | A / A | A: `shared` | `@Counter` shared into tasks switches to locked form |
| C46 | Store a value, keep using the original | must write `.clone()` | R / R | A: share on reuse | unchanged |
| C48 | Polonius cursor down a list | `loop { match cur.next { Some(ref mut n) => cur = n, None => return &mut cur.val } }` | R / A (no-condition form) | R: E0202, E0108, E0113 | A (K4) |
| C49 | Return a borrow on one path, mutate on the other | `if let Some(x) = v.first() { return x } v.push(..)` | R / **A** | **A** | unchanged |
| C50 | Returned `&mut`, then reuse the owner | `*slot(&mut v) += 1; v.push(9)` | A / A | A | unchanged |
| C52 | Struct holding `&mut` | `struct Ctx<'a> { out: &'a mut Vec<String> }` | A / A | A for one window (`ViewMut<String>`), but a whole-list write window cannot `push` | `out: @[String]` held with write access can `push` (K7) |
| C53 | `mem::swap` of fields | `mem::swap(&mut s.front, &mut s.back)` | A / A | R: E0201; idiom C53b deep-copies both lists | `t :: ^a; a = ^b; b = t` accepted (K6, ratified D-CORESURF-SMALL1) |
| C54 | `Option::take` | `self.head.take()` | A / A | R: E0201; idiom C54b copies | `old :: ^self.head; self.head = None` (K6) |
| C56 | Hold a map value across an insert | `let x = map.get("a"); map.insert(..); x` | R / R | **A**, because `get` returns a copy | A; `get` returns a link and the store materializes only when the map changes shape (K13) |
| C62 | Loop over a list while a closure pushes into it | `for x in &v { grow(*x) }` | R / R | **sema accepts; build ICEs** | R: E0507 at the loop (K9) |
| C63 | Lending iterator method | GAT `LendingIterator` | A / A (hard to use) | A: `fn next(&self) -> ViewMut<Int> from self` | `-> @[Int] from self` |
| — | Callbacks capturing borrows, stored | `Box<dyn Fn + 'a>` | A / A | A (C64); escaping capture copies (C64b) | unchanged; a captured link is a kept link |
| — | Arena allocation | `bumpalo` `&'bump T` | A / A | A: E0632 on use after reset (C55) | unchanged |
| — | Async borrows across await | `async` + scoped borrows | A / A | Jet has no `async`; `task.group` borrows | unchanged |

Summary of the corpus. Rust-accepted shapes that Jet rejects today become
accepted by ratified-but-unbuilt features or checker fixes (K1–K10), except
three that need the ballots in section 6: C08 (method footprints), C16 and
C17 (one-lookup map update), and C30 (a record holding links into its own
text). Of the programs Rust rejects for safety reasons, Jet rejects all but
C62, a sema miss that the backend still catches. Jet safely accepts some
programs Rust rejects (C11, C36, C40b, C46, C49), and accepts C56 only by
copying the map value.

## 4. Performance: never copy where Rust borrows or moves

### 4.1 The rule

Jet beats Rust only if no program pays a cost that the matching Rust program
does not pay. The rule this proposal sets is:

> For every program that Rust's borrow checker accepts, the Jet spelling of the
> same program lowers to the same representation: plain pointers for borrows,
> bitwise moves for moves, and no count, flag, lock, or copy that Rust does not
> have. Jet may add a cost only in a program Rust rejects, and then only a
> cost no larger than Rust's own workaround (`Rc`, `RefCell`, `Arc<Mutex>`,
> an index, or `clone`).

Everything below checks Jet against this rule. The rows come from the
agent MemEvidence's interim audit: `jet emit --rust` on the corpus cases,
reading the generated Rust (`/mnt/jetscratch/scratch/memcorpus/out/<case>.rs`).
Its full report will be `/mnt/jetscratch/scratch/memcorpus/AUDIT.md`. The same
audit confirms four places that already match Rust: a disjoint field call
becomes a real `&`/`&mut` split (C07), returned views are real Rust references
(C27, C28), last-use moves are moves (C47), and `task ^x` moves.

### 4.2 Where Jet copies, counts, locks, or checks today

| Site | Jet today | Rust equivalent | Verdict | Fix |
|---|---|---|---|---|
| Read and write parameters | Borrow, no copy | `&T`, `&mut T` | Parity | — |
| Last use into a store | Move (C47) | Move | Parity | — |
| Non-last use into a store | Eager deep clone (`Vec<String>.clone()` in the emitted Rust, C46), not the copy-on-first-write that D-COPY-DEFAULT1 ratified | Rust rejects; user writes `.clone()` | Parity with Rust's workaround, **loss** against ratified law | K14: build the shared counter and copy on first write |
| Swap, take, replace through `&` | Two deep copies (C53b, C54b) | Three pointer moves | **Loss** | K6: `^place` plus refill |
| `freeze(x)` | Deep copy into the snapshot | `Arc<T>` (one allocation) or a scoped borrow | **Loss** when `x` is not used again | K12: a last-use `freeze` moves into immutable storage |
| `map.get(k)` in a branch | `map.get(k).cloned()` (C56) | `Option<&V>` | **Loss** for large values | K13: return a link; materialize only on store |
| Map get-or-insert | Two or three hash lookups (C16b, C15b) | One (`entry`) | **Loss** | D-MAP-SLOT1 |
| Mutable iteration | Index loop, bounds check per access (C20b) | `iter_mut`, no checks | **Loss** | K5: `loop p in &ps` |
| Range into a `[T]` read parameter | `sum(v[0..1])` lowers to `.to_vec()`; a bound window is rejected (C39) | `&v[..2]`, no copy | **Loss** | K7: one slice type for read parameters |
| `xs.fold(…)` on a read parameter | Clones the whole list, then calls the callback through `Rc<RefCell<Option<Box<dyn FnMut>>>>` per element (C39c) | Borrow plus an inlined closure | **Loss** | K15: iterate by borrow; non-escaping lambdas are static calls |
| Every function value | Two allocations (`Rc::new(RefCell::new(Some(Box::new(…))))`); a non-escaping closure that writes a local boxes that local (C35) | Stack closure, `&mut` capture, static call | **Loss** | K15 |
| `Shared<T>` in one thread | `shared S{…}` clones the new struct; every field access allocates a closure and takes the lock (C34, C44) | `Rc<RefCell<T>>`: plain count plus flag; the struct moves in | **Loss** | `@T` local form (D-MEMREF-TASK1); a last use moves in |
| `print(x)` and comparisons on text | Clone the `String` first (C56, C64b) | Borrow | **Loss** | K15: read parameters of Core output and comparison take borrows |
| `Int` reads | `JetInt` is a tagged word that is not `Copy`; every read is `.clone()` (a branch; an atomic retain if it spilled to a big integer) | `i64` copy | **Loss** on hot paths | Out of scope here; the packed-Int rail work owns it |
| `Cell<T>` guards | Runtime flag | `RefCell` | Parity | — |
| `Pool[ID]` | Generation plus bounds check | `slotmap` | Parity | — |
| Stored escaping lambda capturing a window | Copies the window's data (C64b) | `move` closure moves; borrowing closure must not escape | Parity or better | — |
| Lint L0501 on `&v[i..i+1]` | Claims "copies every time" on a write window (C22) | — | Possibly a false report; if true, a **loss** | K7: verify and fix |
| `jet audit copies` coverage | Reports C46 but misses the copies in the rows above and escaping-closure view copies (`jet_view_copy`, C64b) | — | Visibility gap | K14: every implicit copy, count, and lock is reported |

### 4.3 Zero-copy guarantee classes

Every value access in Jet falls into exactly one class. The compiler reports
the class of each site (`jet inspect choices`, D-AUTOPIN1) and an effect
denial locks it.

| Class | When | Machine cost | Rust equivalent | Lock it with |
|---|---|---|---|---|
| **Z0 borrow** | Read or write parameter; scoped link or window; group-scoped capture; link returned under a proven `from` | Plain pointer; read-only and non-aliased when no value write overlaps | `&T` / `&mut T` | `!Mem.Copy(above: 0), !Mem.Rc, !Mem.Check` |
| **Z1 move** | Last use; `^x`; `^place` with refill | Bitwise move of the header | move | `!Mem.Copy(above: 0)` |
| **S share** | Non-last use into an owning slot | One count increment; copy deferred to the first write, if any | `.clone()` (eager deep copy) | `!Mem.Copy(above: N)` reports; `~` makes it explicit |
| **K kept link** | A link the compiler cannot bound to its owner's scope | Count on the target (atomic only when it crosses tasks) | `Rc<RefCell<T>>` / `Arc` | `!Mem.Rc` |
| **C checked** | Two links might reach one place during a long access | One identity compare at the lend, or one access-set entry per long access | `RefCell` flag, or a rejection | `!Mem.Check` |
| **G generation** | Link to a value that owes a job | One generation compare per use | `slotmap` key | (always on; ratified) |
| **L lock** | Mutable link shared across tasks | Lock per statement | `Arc<Mutex<T>>` | `!Lock` |

Rust-acceptable programs only ever produce Z0 and Z1. Classes S, K, C, G,
and L appear only in programs Rust rejects or rewrites with a heavier type.

### 4.4 When `~` is needed

`~x` is never needed for speed or to satisfy the checker. It is needed only to
*mean* "an independent value that will not see later changes" in a place
where the compiler would otherwise share or link. Three cases remain:

1. Keeping a snapshot of a value you will keep changing (`saved :: ~board`).
2. Under `#Policy(copies: .Explicit)`, where every hidden copy is spelled.
3. Giving a task its own copy of a value the parent keeps using and changing.

Because `~x` lowers to a share when the source is not changed afterwards
(class S), an explicit copy also costs nothing until someone writes.

### 4.5 What share-on-reuse costs and when it is free

A shared heap value needs a counter and a uniqueness test before each write.
D-COPY-DEFAULT1 already requires that "values proven unique at compile time
carry no counter and no uniqueness check". This proposal adds three
implementation rules so the counter never reaches Rust-shaped code:

- The counter lives in the heap header, not in each handle, and is allocated
  only for values that sema proves may be shared. A function that never
  shares a list sees a list without a counter field in use **[inference: the
  layout choice is an implementation detail for card K14]**.
- The uniqueness test is hoisted out of loops: one test before a write loop,
  then plain writes, as Swift's array optimizer does with `isUniquelyReferenced`.
- A value that is only shared in one branch keeps the plain representation on
  the other branches.

Each automatic choice needs its paired performance cell before it ships
(D-COPY-DEFAULT1 technical; AGENTS.md performance gate). Card K14 lists them.

### 4.6 Aliasing facts are the real prize, and they need a second proof

Rust's speed advantage over C comes partly from `noalias`: a `&mut` cannot
alias, and a `&` cannot change during the borrow, so the optimizer keeps
values in registers and vectorizes. Jet earns the same facts from its own
proof: read parameters are frozen for the call (a conflicting write traps or
is rejected, D-MEMREF-EXCL1), `&` parameters are exclusive, and Z0 links are
read-only during their life. Emitting those facts is also the most dangerous
thing a backend can do: one wrong `noalias` is silent corruption. Today rustc
re-checks every borrow we emit. After Rust leaves, the native backend must
emit an aliasing fact only when MIR Lint has independently verified it
(section 5.8).

## 5. Recommended design

### 5.1 The model in four sentences

1. Every value has one owner; a call changes only what its caller marked `&`,
   and takes only what its caller marked `^` (built).
2. A link (`@`) reaches a value without owning it; while it lives, nobody may
   change the *shape* of what it points into, and every change of *value* is
   seen through it.
3. The compiler proves each link free when it can, and otherwise picks the
   cheapest safe fallback: a count, a check, a generation, or a lock. It
   reports every fallback and lets an expert forbid it.
4. No program ever states a lifetime. An expert may state *where a link comes
   from* (`from`), which the compiler checks.

### 5.2 Beginner path: no annotations, errors that teach

A beginner writes plain values, `&` where something changes, and `@` where two
things must see the same object. Nothing else.

```jet
struct Player {
    name: String
    hp: Int
}

struct Team {
    captain: @Player
    roster: [@Player]
}

fn run() {
    ann := @Player{name: "Ann", hp: 10}
    team :: Team{captain: ann, roster: [ann]}
    ann.hp -= 3
    print(team.captain.hp)        // 7: the same player
}
```

Every rejection names the shape change, the live link, and the smallest fix:

```text
error[E0212]: `players` can't grow while `best` points at one of its items
  --> game.jet:9:5
   |
 7 |     best :: @players[i]
   |             ----------- `best` points into `players` here
 9 |     &players.push(newcomer)
   |     ^^^^^^^^^^^^^^^^^^^^^^^ growing may move every item
11 |     print(best.name)
   |           ---------- `best` is still used here
 Why: a link sees changes to the item's value, but it can't follow the item
      if the list moves its storage
 Fix: use `best` before the push, or make each player its own object:
      `players: [@Player]`
```

### 5.3 The shape rule

A link points at a place. Writes to that place fall in two kinds:

- **Value writes** replace the bits in the place without freeing or moving
  any storage the link points into: assigning a field, assigning a list
  element, `+=` on a number. They are allowed while a link is live, and the
  link sees them.
- **Shape writes** may free, move, or reinterpret storage: `push`, `insert`,
  `remove`, `clear`, or `resize` on the list the link points into; replacing
  a whole `String` or list that a window points into; assigning a different
  enum case over a payload the link points into; moving or dropping the owner.
  They are rejected while a link into that storage is live (E0212, unchanged
  code).

This is Ante's "shape-stable" rule applied to Jet's places. It is sound for
the same reason Rust's rule is sound: the only writes that can make a pointer
dangle are shape writes. It is strictly more permissive than Rust's rule,
which also forbids value writes. Where no value write overlaps a link, the
compiler lowers the link as a read-only, non-aliased pointer, so there is no
cost compared with Rust.

Links into storage that is *always* shape-stable never freeze anything: a
struct field, a tuple member, a fixed-size list item, or a counted object's
field.

### 5.4 Link classes are representations, not types

| Class | Chosen when | Representation |
|---|---|---|
| Scoped | Owner provably outlives the link and no shape write overlaps it | Plain pointer (today's view) |
| Kept | The link may outlive the owner's scope | Owner promoted to counted storage at creation (D-MEMREF-LIFE1); link is a counted pointer plus field offset |
| Generational | Target's type owes a job | Pointer plus expected generation (D-LINK-DEBT1) |
| Shared across tasks | A link to the type reaches another task | Atomic count, lock per statement (D-MEMREF-TASK1) |

Any class may additionally get a check site (class C in section 4.3) when two
links might reach one place during a long access.

A program's meaning never depends on the class. A beginner never sees it. An
expert sees it with `jet inspect choices` and locks it with a denial or a
`from` clause. If an edit later breaks the lock, the build fails and names the
fact that changed (D-AUTOPIN1).

### 5.5 Places, windows, and the cutover from `View`

The link type is the type of the place it points at:

| Today | Proposed | Meaning |
|---|---|---|
| `View<T>` (a range of a list) | `@[T]` | A link to a list or to part of one |
| `View<str>` | `@String` | A link to a string or to part of one |
| `ViewMut<T>` | `@[T]` held with write access | Write access comes from the holder (`:=`, `&`, a writable path), as D-MEMREF1 rules |
| `Shared<T>` | `@T` | Thread-safe form chosen by D-MEMREF-TASK1 |
| `Cell<T>` | `@T` with a local-only pin (`#Local`) | Checked access per D-MEMREF-EXCL1 |
| `Pin<T>` field | `@T` field | Links to counted objects are address-stable by construction |
| `[View<T>]` from `to_views` | `[@[T]]` | Per-element provenance unchanged |

A link to a whole list place (`@xs`) supports the whole list API, including
`push` when held with write access, because the link points at the list
value, not into its buffer. A link to a range (`@xs[2..5]`) supports the
slice API only. This removes the C52 and p2–p4 gaps.

### 5.6 Inference: what the checker computes

The checker that makes all of this work is the one Jet already has, extended
in four ways. None adds syntax.

1. **Location-sensitive loans.** A loan is live at a point only if a value
   that carries it is live there. That is Polonius alpha's rule, and Jet's
   place-based facts make it simpler than in Rust: a loan returned on one
   branch does not exist on the other branch. This accepts C14, C15, and C48.
   Jet should match Polonius alpha at minimum, and go further on the cursor
   pattern because Jet's facts are per place rather than per region.
2. **Method footprints.** For each method, sema records which fields of
   `self` it reads and writes (D-METHOD-FOOTPRINT1). A caller may hold a link
   to any field the callee does not write.
3. **Shape effects.** Each operation carries a fact: value write or shape
   write, and on which place. Core collection methods declare theirs once in
   the Prelude (I9).
4. **Link class selection.** After the proof, each link gets the cheapest
   class its facts allow, recorded in MIR for the second checker.

### 5.7 Expert path

- **State provenance:** `fn longest(a: String, b: String) -> @String from a | b`
  (ratified D-MEMPROVENANCE3, respelled).
- **Forbid fallbacks:** `fn frame(world: &World) -[!Mem.Rc, !Mem.Check, !Mem.Gc, !Lock, !Mem.Copy(above: 0)]>`.
  The error names the line that would have needed the fallback and why.
- **Pin a representation:** `#Local` and `#Shared` on a type (D-MEMREF-TASK1).
- **Break cycles:** a weak link for back edges, so `!Mem.Gc` holds.
- **Disjoint writes at runtime indexes:** `split_write`, `get_disjoint_write`,
  `edit_disjoint` (D-MEMDISJOINT1).
- **Entity systems:** `Pool<T>` and `ID<T>`, unchanged.
- **Raw memory:** `*T` and `mem.Ptr<T>` inside `#Unsafe` (D-RAWPTR2).

### 5.8 After Rust: MIR Lint re-proves the model

Today rustc's borrow checker and `Send`/`Sync` re-check the Rust that Jet
emits; a rejection is an internal error (I2). C62 shows that this backstop
catches real sema misses. Card #4619 makes MIR Lint the replacement. Its
planned rule families are `alias.exclusive`, `view.valid`, and `send.boundary`
(agent SafetyNet's design, not yet landed). Links need four additions:

| Rule | Proves | MIR facts needed |
|---|---|---|
| `link.scoped` | A link marked Scoped is never live across a shape write to, a move of, or the end of any owner in its source set | Explicit loan-begin and loan-end operations (SafetyNet found MIR has none today); a shape-effect tag on each operation |
| `link.fallback` | A Checked access is dominated by its check; a Kept link targets counted storage and its retains and releases balance; a Generational deref is dominated by its generation compare | Link class tag on each link operation |
| `send.link` | A link value that crosses a task boundary has the thread-safe class, or is group-scoped with the join dominating every owner invalidation | Existing task-group scope markers |
| `noalias.justified` | Every read-only or non-aliased attribute the native backend emits is backed by a verified Scoped or exclusive fact | The attribute set per function argument and pointer |

Each rule is a forward dataflow over the same bit rows as `ownership.move`, so
the lint stays linear in program size. Every failure is an internal error
naming the pass. #4619's mutation test (delete each sema check in turn and
show MIR Lint still rejects the program) must include C62 and one case per
shape-write kind. MIR also needs the frozen-value fact that SafetyNet reports
missing (Rust lowers `freeze` to a plain copy; the Jet compiler has no
`freeze` at all), or `freeze` stops being provable after Rust leaves.

## 6. Owner decisions

### 6.1 Ballots raised by this proposal

| Ballot | Question | Recommendation |
|---|---|---|
| D-LINK-ITEM1 | What happens when a list item that a long-lived link points at may move because the list grows? | A: short-lived item links freeze the list's shape (compile-checked, like Rust); a long-lived link must point at an object, so the list holds `[@Node]`; the error teaches the change |
| D-METHOD-FOOTPRINT1 | May you call a method that changes one field while you hold a link to another field? | A: yes; the compiler records each method's field footprint, public methods publish it like provenance, and an expert may state it in the receiver with the ratified member spread (`fn reset(&self.[log, hits])`) |
| D-LINK-SIBLING1 | May a record hold text and links into that same text? | A: yes, with a `from field` clause on the link field (`keys: [@String] from text`), reusing the provenance word; the named text field is frozen for the record's life |
| D-MAP-SLOT1 | How do you read-or-create a map entry with one lookup? | A: `counts[w, or: 0] += 1`; the default runs only when the key is missing |

Each ballot is a short-profile draft under `~/.cache/jet-dev/ballots/READY/`
(`D-LINK-ITEM1.json`, `D-METHOD-FOOTPRINT1.json`, `D-LINK-SIBLING1.json`,
`D-MAP-SLOT1.json`), attached to card #3645. All four pass the read-only
validator (`node ~/.cache/jet-dev/ballots/READY/validate.mjs …`, exit 0, no
gaps, dry-run `add`). No reader pass has run on them yet. D-MAP-SLOT1 is
performance-motivated, so its paired candidate/plain cell must exist before
ratification.

One more owner spelling will be needed when the cutover (K2) starts:
D-MEMREF-LIFE1=A names the weak back link `Shared.Weak<T>` (with `downgrade`
and `upgrade`), while D-MEMREF1=A retires the `Shared<T>` spelling. Until a
short ballot picks the weak link's `@` spelling, `Shared.Weak<T>` stays.

### 6.2 Questions this note treats as settled

- **Does `@` replace `View`, `ViewMut`, `Shared`, `Cell`, and `Pin` fields?**
  Settled by D-MEMREF1=A. The mapping in section 5.5 is the cutover plan.
- **Do links see value writes?** Settled by D-MEMREF1=A's example. The shape
  rule is what keeps that safe; D-MEMREF-EXCL1=A covers dynamic cases.
- **Moving out of a field through `&` and refilling it.** Settled by
  D-CORESURF-SMALL1=A ("replace … duplicates the take operator (^) plus
  assignment"). The checker rejection in C53/C54 is a defect. The refill must
  happen before any operation that can fail, so an unwinding stop never sees
  an empty place.
- **`loop p in &list`.** Ratified spelling (D-LOOP-STMT-ARROW1 text). C20's
  rejection is a defect.
- **Group children capturing `split_write` halves.** Settled by
  D-TASKBORROW1=A. C40's rejection is a defect.
- **Copy on first write.** Ratified by D-COPY-DEFAULT1=A. The eager deep clone
  in C46's emitted Rust is a defect, not a design choice.

## 7. Implementation consequences (card drafts)

These are drafts for the orchestrator to place in Tower. Each names its proof.

- **K1 — Build `@` links.** Parse `@T` and `@place`, type them, select the
  class (section 5.4), lower each class, and add `jet inspect choices` rows.
  Proof: C32 and C33 run on every tier with the expected output; a UI snapshot
  for each new diagnostic; `!Mem.Rc` rejects a kept link with its call path.
- **K2 — Cut over `View`/`ViewMut`/`Shared`/`Cell`/`Pin` fields to `@`.**
  Migrate every caller, example, Core API (`to_views`, `*_views`, text views,
  `from`), snapshot, and spec section in one change; remove the old
  spellings. Proof: corpus cases C24, C27, C28, C50, C52, C63 respelled and
  green; no `View<` in `Examples/` or `Core/`.
- **K3 — Shape rule.** Narrow E0212 for links to shape writes; declare the
  shape effect of every Core collection operation once in the Prelude. Proof:
  value write through owner visible via link; `push` while an item link is
  live is E0212 with the section 5.2 text.
- **K4 — Location-sensitive loans.** Accept C14, C15, C48, and the
  Polonius-alpha cases; bind payloads of a write place as write windows
  (card #4249 criterion [#3974]). Proof: those cases accept; C02, C12, C23,
  C51 still reject.
- **K5 — `loop p in &list`.** Proof: C20 accepts and runs; pushing to the
  list inside the loop is E0507.
- **K6 — `^place` with refill through `&`.** Allow a move out of a write place
  when every path refills it before the next read and before any operation
  that can fail. Proof: C53 and C54 accept with no copy in `jet audit copies`;
  a refill after a fallible call is rejected with a teaching fix.
- **K7 — Window and whole-place gaps.** A range link passes to a `[T]` read
  parameter (C39); writing through a field window is not an owner change (C06,
  p2–p4); a whole-list write link can `push`; verify or remove the L0501 claim
  on write windows (C22).
- **K8 — Group children capture `split_write` halves** (C40, D-TASKBORROW1).
- **K9 — Close the C62 sema miss.** A local closure's write footprint counts as
  a write to the iterated list. Proof: C62 is E0507 in `jet check`; no ICE.
- **K10 — View fields in the same scope and their methods** (C24b, C24c).
- **K11 — MIR Lint link rules** (`link.scoped`, `link.fallback`, `send.link`,
  `noalias.justified`) plus explicit loan operations, shape-effect tags, link
  class tags, and the frozen-value fact, as an extension of #4619.
- **K12 — `freeze` of a last use moves** into immutable storage instead of
  deep-copying. Proof: `jet audit copies` shows no copy; paired cell.
- **K13 — Read accessors return links.** `map.get` and `list.get` return
  `?@V`; storing the result materializes under D-MEM-COPYSEM1. Proof: C56
  shows no copy unless the map changes shape while the result is live.
- **K14 — Copy on first write, full cost reporting, and paired cells.** Build
  the shared counter and copy-on-first-write that D-COPY-DEFAULT1 ratified
  (today C46 deep-clones). Make `jet audit copies` report every implicit copy,
  count, and lock (today it misses most rows in section 4.2). Add paired
  performance cells for every automatic choice: share on reuse vs Rust
  `clone`/borrow, kept link vs `Rc`, checked access vs `RefCell`, thread-safe
  switch vs `Arc<Mutex>`, mutable iteration vs `iter_mut`, swap vs `mem::swap`.
- **K15 — Borrowing calls and static closures.** A read parameter that is
  iterated or folded is borrowed, not cloned (C39c). A lambda that does not
  escape lowers to a direct call with borrowed captures, with no `Rc`,
  `RefCell`, or `Box` (C35). Core output and comparison operations borrow
  their text arguments (C56, C64b). Proof: the emitted code for those cases
  has no `.clone()` of the argument and no closure allocation; paired cells
  against the Rust programs in the case headers.
- **After the ballots:** one build card per ratified outcome (item links,
  method footprints, sibling links, map slots).

## Sources

Primary sources fetched for this note:

- Rust RFC 2094, non-lexical lifetimes, problem cases #1–#3:
  <https://rust-lang.github.io/rfcs/2094-nll.html>
- Rust project goal 2025h2, "Stabilizable Polonius support on nightly":
  <https://goals.rust-lang.org/2025h2/polonius.html>
- Niko Matsakis, "View types for Rust" (2021):
  <https://smallcultfollowing.com/babysteps/blog/2021/11/05/view-types/>
- Niko Matsakis, "Borrow checking without lifetimes" (2024):
  <https://smallcultfollowing.com/babysteps/blog/2024/03/04/borrow-checking-without-lifetimes/>
- 2024 State of Rust Survey results:
  <https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/>
- Google Open Source, "Rust fact vs. fiction" (2023):
  <https://opensource.googleblog.com/2023/06/rust-fact-vs-fiction-5-insights-from-googles-rust-journey-2022.html>
- Andrew Trick, "Swift 5 Exclusivity Enforcement" (2019):
  <https://www.swift.org/blog/swift-5-exclusivity/>
- Swift SE-0446, nonescapable types:
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md>
- Hylo language tour, subscripts:
  <https://docs.hylo-lang.org/language-tour/subscripts>
- Mojo manual, "Lifetimes, origins, and references":
  <https://mojolang.org/docs/manual/values/lifetimes/>
- Evan Ovadia, "Zero-Cost Borrowing with Vale Regions (Preview)" (2022):
  <https://verdagon.dev/blog/zero-cost-borrowing-regions-overview>
- Wouter van Oortmerssen, "Memory Management in Lobster":
  <https://aardappel.github.io/lobster/memory_management.html>
- Jake Fecher, "Achieving Safe, Aliasable Mutability with Unboxed Types"
  (Ante, 2024): <https://antelang.org/blog/safe_shared_mutability/>
- Fernando Borretti, The Austral Language Specification:
  <https://austral-lang.org/spec/spec.html>

Cited from the literature, not re-fetched for this note:

- Swift SE-0176 (exclusive access), SE-0377 (`borrowing`/`consuming`),
  SE-0390 (noncopyable types): <https://github.com/swiftlang/swift-evolution>
- Dimi Racordon et al., "Implementation Strategies for Mutable Value
  Semantics", Journal of Object Technology 21(2), 2022.
- Sylvan Clebsch et al., "Deny Capabilities for Safe, Fast Actors", AGERE
  2015; Pony tutorial, reference capabilities:
  <https://tutorial.ponylang.io/reference-capabilities/>
- Ellen Arvidsson et al., "Reference Capabilities for Flexible Memory
  Management" (Verona), OOPSLA 2023 (the ACM page refused automated access).
- Dan Grossman et al., "Region-Based Memory Management in Cyclone", PLDI 2002.
- Vale generational references: <https://verdagon.dev/blog/generational-references>

Jet sources: `Docs/spec/spec.md` (Ownership and borrowing; Expert memory tier;
Concurrency), `Docs/spec/architecture.md` (Concurrency boundary safety),
`Docs/spec/mir-lint.md`, Tower decisions named inline, cards #3645, #4249,
#4392, #4619, and the corpus at `/mnt/jetscratch/scratch/memcorpus/`.
