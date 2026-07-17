# The shape law — a uniformity paradigm for all of Jet

One decision: **the rule that assigns a syntactic shape to every construct in
the language**, so that an expert who has learned one construct can predict the
shape of a sibling and be right most of the time.

Scope is the whole language: core syntax, stdlib API shapes, manifest surface,
CLI verb grammar, diagnostics vocabulary. The metric is **guessability** — not
minimalism. Keywords stay where a keyword is the honest shape (control flow,
declarations). This proposal picks a *law* first; application waves follow it.

Every ratified decision is challengeable in this pre-1.0 window. Where a
candidate overturns law, the overturn is a flagged ballot row. Migration cost is
never weighed here (philosophy: effort is not a deterrent).

Source material: the three shape inventories (`audit/core-syntax.md` F1–F17,
`audit/stdlib-shapes.md` findings 1–7, `audit/manifest-cli-shapes.md` F1–F10),
`docs/spec/philosophy.md`, `docs/spec/syntax-decisions.md`,
`docs/proposals/package-ecosystem-frameworks.md` (the held D-ECO wave that
triggered this).

---

## 1. The disease

One semantic job wears many syntactic shapes, with no rule an expert can
internalize to predict which. Consolidated into cross-cutting classes, ranked by
blast radius (how much of the surface each poisons).

### Class A — "named typed block" has 6 shapes [blast: highest — manifest + lang]

Job: bind a name to a body of typed fields.

```jet
module env.dev { … }              // MOD, dotted namespace     U3
overlay dev { … }                 // NAMEBLK, kw + name        D-JPK-OVERLAY1
executable { entry: "…" }         // KWBLK, unnamed            D-TGT1-4
payload: { name: …, version: … }  // F:V colon-block           U10
build { release: … }              // bare-word block, no colon D-BUILDPROFILE1
Wrap.{ path_prefix: … }           // TCTOR-DOT typed literal   D-DOTCTOR1
```

Plus the lang side: `struct S { }`, `state T { }`, `taskgroup g { }`,
`layout n { }` — same "keyword + optional name + typed body" job, four more
grammars. **Wrong guess:** after `module env.dev { }`, an expert writes
`module overlay.dev { }`; the language wants `overlay dev { }` — no `module`,
space for the dot. Cite manifest F1, core F10/F11.

### Class B — "construct a value" has 5+ shapes [blast: high — stdlib + lang + manifest]

Job: make a value of a named type.

```jet
Wrap.{ path_prefix: … }        // dot-literal            D-DOTCTOR1
BigInt(100)                    // bare PascalCase call    D-BIGINT1 / D-API-CTOR1
Deque.new()                    // static method (fresh)   D-ALLOC1 / D-API-CTOR1
Set.from([1,2,3])              // static method (convert) D-API-CTOR1
time.clock(42)                 // lowercase module factory D-DET1
vault.rotting_new(v, ttl, clk) // verb-compound module fn stdlib finding 1
X25519SecretKey.generate()     // 5th verb "generate"     stdlib finding 1
Recipe.cargo(lock: "…")        // namespaced fn (sum-ish) D-ECO2
Ok(v) / Err(e)                 // PascalCase Result variants  D-SHAPE3b
Val(x) / None                  // PascalCase option ctors D-OPT-SPELL1
```

D-API-CTOR1 codifies four idioms, but shipped code has at least three that fit
none (`time.clock`, `rotting_new`, `generate`), and the option/result ctors
disagree on casing for the identical job. **Wrong guess:** an expert who read
the ctor law guesses `Expiring.new(…)` and `Clock.new(42)`; the language has
`expiring.new(…)` and `time.clock(42)`. Cite core F4/F5/F17, stdlib 1/2,
manifest F3.

### Class C — "property of a function" spans 4 planes [blast: high]

Job: attach a fact/promise/instruction to a fn.

```jet
@Pure                          // contract marker         S60
@Sanitizer / @Replayable       // directive marker        D-TAINT-SAN / D-REPLAY1
pub / priv                     // keyword                 S18
#(Net, !Fs)                    // effect parens           D-EFF1
effects: { allow: […] }        // manifest field          D-EFFBUDGET1
```

The two-plane law (D-MARKER-FAMILY1: `@` = checkable promise, `#` = compiler
instruction) is a *good* rule that its own assignments break. `@Sanitizer` is a
promise about return taint; `@Replayable` is a determinism promise — both are
`@Pure` siblings, both stranded on `#`. **Wrong guess:** "promise → @" is right
for `@Pure`/`@MustUse`, wrong for `@Sanitizer`/`@Replayable`. Cite core F1/F7.

### Class D — "external reference" has 4 punctuations [blast: medium-high]

```jet
github@NixOS/nixpkgs           // @ sigil                 U6
textkit#1.2.0                  // # sigil (shares [T#N])  U6
nixpkgs:fastfetch              // : sigil, CLI-only       U6
source.package / infra/logging / logging   // 3 member-ref forms  D-MONOREF1
from: packages.jet  vs  from: oci("…")      // accessor vs call, one field  D-JPK-IMAGE1
```

No single visual signature for "this is a reference." Cite manifest F4/F9,
core F8.

### Class E — "CLI subcommand" has 6 shapes + a spec/impl split [blast: high]

```
jet fmt                        // bare verb
jet trust list                 // noun → verb
jet os vm prove <host>         // noun → noun → verb
jet push <fleet>               // verb absorbs a positional noun
jetos user plan                // a whole separate binary for one family
jet store verify               // self-relabeling deprecated alias
```

And the sharpest one: the **spec commits to `jet registry publish` /
`jet inspect dossier` namespacing; the shipped binary implements every one as a
bare top-level verb** (`jet publish`, `jet dossier`), with the dispatch-arm
comments quoting the namespaced spec form one line above the flat implementation
(`grep -n '"inspect"' Source/main.rs` → nothing). Cite manifest F5/F6.

### Class F — "handle-binding block" has 3 slots [blast: medium]

```jet
@Transact(order) { … }         // handle inside the parens   D-TXN4
@Grant(Fs) { caps -> … }       // handle after {, via arrow  D-SCAP1
taskgroup g { … }              // handle after the keyword   D-TASKSCOPE1
```

All three are "a scoped block binding one handle." D-TXN4's own doc says it
"mirrors `region r { }`" — yet region is `@Region(r)`. Expert can't guess the
slot. Cite core F2.

### Class G — "manage a value's lifecycle" has 3 shapes [blast: high]

Job: duplicate, consume, or discard a value. Three unrelated grammars:

```jet
copy x                         // prefix keyword    D-CAP2
consume(x)                     // free builtin      D-DROP-WORD1
result.drop("reason")          // method            D-MARK-DISCARD1
```

The `copy` prefix keyword is the worst offender, and shipped usage proves it. In
real code the keyword is dropped *mid-argument-list*:

```jet
ops.transform(pool, copy input, 5000)      // shipped
ops.Transform(session, copy input, 5000)   // shipped
```

Under D-MEM1 a bare `input` is a read-borrow; a consuming callee needs an owned
copy, so the caller must prepend `copy` — but nothing at the call site signals
that, and the keyword breaks the left-to-right read of the argument list (owner:
the keyword "looks bad," its placement "seems almost arbitrary," and he
"probably wouldn't have guessed they needed to be there"). **Wrong guess:** an
expert reads `ops.transform(pool, input, 5000)` and cannot see that `input`
needed an owned copy; and having learned `result.drop("reason")` as a method,
would sooner write `input.copy()` than a bare prefix keyword. Cite core F6,
D-CAP2 / **S4** (shipped 2026-07-04), D-DROP-WORD1, D-MARK-DISCARD1, D-MEM1.

**S4 caveat — this reopens a recent, deliberate decision.** S4 chose `copy x`
as a real prefix-verb *on purpose* and made `.clone()` deliberately
un-typable (it falls through to no-such-method by I8), with every fix-it
steering to `copy name`. So any respelling here is not fixing an oversight —
it is *reopening S4*, which the owner explicitly asked for this session
("find a better way than the `copy` keyword"). §4 treats it as an owner-gated
reopen with S4's rationale on the table, not a silent flip. The three lifecycle
verbs also do **not** share a gating regime: `copy` is ungated on any cloneable
type, `drop("reason")` is the `@MustUse` discard, but `consume(x)` is the
**linear finisher** legal only inside `@Unsafe` for `@SingleUse`/`#NoCopy`
(D-LIN1). That difference is load-bearing (I1: footguns stay structurally
visible), so the family does not fully collapse to one shape — see §4.

### Class H — manifest field-nesting rule contradicts itself [blast: medium]

U8 says `sources:` / `imports:` **must** nest inside a `module name { }` body
and are never file-top-level. U10/S52 puts `sources:` / `payload:` /
`packages:` at file top level in `pkg.jet` with no enclosing module. Same field
name, opposite placement rule, no local cue. Cite manifest F2.

### Smaller classes (lower blast, same disease)

- **I — duration spelled 3 ways:** `@Every(5min)`, `.timeout(2s)`,
  `time.ms(n)` (core F13).
- **J — derive spelling history:** D-SHAPE2 collapses prefix derives into the
  one `@[…]` rule list; in-body `derive` remains the explicit body form.
- **K — naming drift:** `constant_time_eq` vs `constant_time_equal` (arg-type
  suffix), `watcher.files` noun-first vs `fs.read` verb-first, RAII vs explicit
  `.close()` with no visible rule (stdlib 2/3/4).
- **L — one-off grammars:** `.view(a..b)`, `.take_pattern("{h}")`, `@Test`'s
  private `.setup`/`.timeout`/`.skip` scope-members, `wrapping(a+b)` as a
  free-fn wrap (core F15/F16/F17).
- **M — invokable entry: name vs marker:** `fn run/dev/build` are magic by
  name; `@Task fn deploy` is magic by marker (core F14).
- **N — hygiene drift:** registry doc comments write `#layout`/`#grant`
  (lowercase) against PascalCase constants; informal planning-number citations
  are reused for unrelated features with no canonical header (core F12,
  manifest F10).

---

## 2. Bright spots to preserve

Already guessable. A paradigm that churns these to hit a clean count is
**disqualified** — don't break working code for a tidier table.

- **One `@Rule` application law** — D-SHAPE2 makes the rule name and its legal
  attachment site carry meaning instead of asking readers to classify sigils.
- **`.{` / `.[` dot-adjacency family** — `T.{}` ctor, `.Variant.{}`, `f.[a,b]`
  fan-out all read as "dot then bracket = structured operation" (D-DOTCTOR1,
  S75).
- **One `if`, one `loop`** — single branching keyword (incl. multi-arm
  `if x == {}`), single loop keyword (S68/S19). Keep keywords for control flow.
- **Closed prelude** — no-prefix ambient surface is a small fixed list; no
  silent library injection (D-PRELUDE-LAW1).
- **Teaching-error discipline** — every retired spelling has an E-code that
  names the replacement (I4). Any overturn below must ship its teaching error.
- **Casing convention** — snake_case fns/fields, PascalCase types, held
  uniformly (D-API naming). The exceptions are `ok`/`err` and `@[allow]`.
- **Diagnostics render shape** — `Error [E….]:` / `Why:` / `Fix:`, fixed and
  disciplined; no drift found at render level.
- **Naming laws that already hold** — `has` for membership (D-API-CONTAINS1),
  `add`/`add_new` for storage (D-API-STORE1), closed `len` abbreviation list
  (D-API-LEN1). These are the *model* for what a shape law does; extend them,
  don't disturb them.
- **Injectable-capability *convention*** — a capability is minted as a value and
  passed as an ordinary typed param, no DI container (stdlib §8). This is the
  bright spot and is preserved. Note the distinction: the *convention* (mint →
  pass) is untouchable; the *spelling* of the minting call (`time.clock(seed)`
  as a lowercase module factory) is a Class B item, and aligning it to
  `Clock.new(seed)` keeps the convention exactly (still a minted value, still a
  param) while fixing only the factory shape. Preserving the convention is not
  the same as freezing the spelling.
- **Magic-default + named-arg override** — `app.auth(users: db)`,
  `Server.serve(addr, mux, tls: …)`: one call, tune a knob. The canonical
  dual-facet ergonomics (I8) — every candidate must keep it.

---

## 3. The consistency-law candidates

Four cohesive, distinct laws. Each: the one-sentence law, the mechanical
job→shape rule an expert internalizes, the same worked corpus rewritten,
a guessability score against a concrete sibling-prediction test, what it
overturns, the kill-criteria check, and steelmanned attacks.

The corpus, six fixtures, shown once per candidate:
**(a)** the jet-repo manifest port (from the ecosystem proposal),
**(b)** a marker-heavy fn signature,
**(c)** a stdlib construction cluster,
**(d)** a type-decl set,
**(e)** a control-flow snippet (shows what stays keyword),
**(f)** a value-lifecycle call site (the `copy`/`consume`/`drop` family, Class G).

Baseline **(b)**–**(f)** as they read today:

```jet
// (b)  @Pure @MustUse  @Replayable @Sanitizer
//      fn parse(input: Tainted<Str>) -> Record ? Error #(Net, !Fs) { … }
// (c)  Deque.new()   Set.from([1,2,3])   time.clock(42)   Wrap.{…}   BigInt(100)   Ok(v)/Val(x)
// (d)  struct Point { x: Int, y: Int }   alias Pair<T> = (T,T)
//      UserId :: distinct Int            state Door { Open, Closed }
// (e)  if x == { .A -> …, .B -> … }       loop i in 0..10 step 2 { … }
// (f)  ops.transform(pool, copy input, 5000)   consume(handle)   result.drop("reason")
```

---

### Candidate 1 — Value-uniform ("everything is a typed value") — owner's seed

**Law:** every named thing that has fields is a *value* of a named type,
constructed by exactly one literal shape `Type.{ … }`; the manifest, a package,
a dev shell, a target are all just typed values; only control flow and binding
stay keywords.

**Rule:** has fields → `Type.{}` (product) / `.Variant(…)` (sum). Runs code →
keyword (`fn`, `if`, `loop`). Effects/contracts become fields or typed values,
not a separate plane.

**(a) manifest = one typed literal:**

```jet
Manifest.{
    payload:  Payload.{ name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 },
    sources:  [ Source.{ name: "pkgs", from: github@NixOS/nixpkgs/nixos-unstable } ],
    packages: [
        Package.{ name: "jet", target: .Executable,
                  build: .Cargo(lock: "Cargo.lock"),
                  wrap:  Wrap.{ path_prefix: self.runtime },
                  meta:  Meta.{ platforms: .Unix } },
        Package.{ name: "jetpack", alias_of: packages.jet, bin: "jetpack" },
    ],
    envs: [ Env.{ name: "dev", packages: […], banner: Banner.{ lines: […] } } ],
}
```

`executable` → `target: .Executable` (a Target value). `alias(…)` → a
Package-valued field. `module env.dev {}` → an `Env.{}` list element.

**(b) fn:** promises/effects have nowhere natural to live but a field bag:

```jet
fn parse(input: Tainted<Str>) -> Record ? Error
    with Contract.{ pure: true, must_use: true, replayable: true,
                    sanitizes: true, effects: Effects.{ net: true, fs: false } }
{ … }
```

Strictly more verbose than `@Pure`. **This is where the seed breaks.**

**(c) construction — the win:** one shape.

```jet
Deque.{}                 // was Deque.new()
Set.{ items: [1,2,3] }   // was Set.from([1,2,3])
Clock.{ seed: 42 }       // was time.clock(42)
Wrap.{ … }               // unchanged
```

But `Deque.{}` either exposes `head`/`tail`/`buf` (a footgun literal) or runs
constructor logic behind literal syntax (then it is not a literal). `Set` must
dedup; a literal cannot. **Encapsulation break.**

**(d) type decls:** untouched — a type declaration is not a value in v1 (no
first-class types). `struct`/`alias`/`distinct`/`state` stay four keyword forms.

**(e) control flow:** unchanged (owner's constraint — correct here).

**(f) value-lifecycle:** value-uniform governs construction, not operations, so
it offers no rule to unify `copy`/`consume`/`drop` — the three shapes survive.
`copy input` stays a prefix keyword mid-argument-list. Class G unsolved.

**Guessability:** construction becomes near-perfectly predictable (learn
`T.{}`, predict every sibling — ~95%). But the fn-property class *regresses*
(fields are longer and unordered vs terse markers), and the type-decl and
manifest-declaration classes are untouched. Net: fixes 1 of the top-4 classes,
worsens 1. Sibling test: `Wrap.{}` → `Meta.{}` ✓; but `@Pure` →
`Contract.{ pure: true }` is a downgrade.

**Overturns:** D-API-CTOR1 (all `.new`/`.from`/`.over` → `.{}`), D-ALLOC1,
D-EMAIL1 factories, D-OPT-SPELL1 / S34 casing, D-MARKER-FAMILY1 partial (markers
→ fields), U3/U10 (modules → list values).

**Kill-criteria:** hollow defaults? **at risk** — forcing `.{}` on
invariant-bearing containers either leaks internals (footgun) or lies about
being a literal. Dictate structure? no. Invariant carve-out? encapsulation
tension, not a formal I-carve-out.

**Attacks:** ① Encapsulation: a literal can't dedup a `Set` or maintain a
`Deque`'s invariants — the one construction shape is a lie for every stateful
type. ② Contracts-as-fields is longer than `@Pure`, hitting priority #2. ③ It is
"everything is a *value*," but declarations (module/struct/if/loop) are not
values — so it leaves Class A's declaration half unsolved: half a law. ④
Sum-type ctors still need `.Variant(…)`, so even construction keeps two shapes.
⑤ Multi-file namespace merge (`module env.dev` contributed from several files,
U14) is lost when envs are elements of one literal.

---

### Candidate 2 — Plane-total (keep the planes; make the law gap-free)

**Law:** there is a small fixed set of shape *planes*, each owning one semantic
domain; a total, published job→plane function assigns every construct, and every
existing exception is deleted or re-ruled so the function has no gaps.

**Planes and their domains:**

| Plane | Shape | Domain (the job that maps here) |
|---|---|---|
| keyword block | `kw name { }` | introduces a name/type/namespace into scope |
| value literal | `Type.{ }` / `.Variant(…)` | a value with fields (product / sum) |
| method | `.verb(…)` | an operation on a value |
| `@` marker | `@Name` | a checkable promise about behavior |
| `#` marker | `#Name` | a compiler build/run instruction |
| data | `field: value` | plain data and named args |
| ref sigil | one chosen sigil | a reference to something external |
| control flow | keyword | branch / loop / return |

**Rule (the whole law in one line):** *name the job; the job names the plane;
the plane names the shape.* No construct may take a shape its job doesn't
dictate; a job that maps to two shapes means a plane boundary is wrong — fix the
boundary, never add a special case.

Deletions/re-rulings this forces (each closes a Class):
- `@Sanitizer`/`@Replayable` → `@Sanitizer`/`@Replayable` (promises) — Class C.
- `overlay name { }` → `module overlay.name { }` (introduces a namespace) —
  Class A.
- product type → `.{}`, sum variant → `.Variant(…)`, and the ctor-verb choice
  becomes mechanical (`new` fresh / `from` convert / `over` view); bare `T(…)`
  and lowercase module factories (`time.clock` → `Clock.new`) retired — Class B.
- one ref sigil for all external refs — Class D.
- one handle slot: the handle is always the keyword-block's name
  (`transact order { }`, `region r { }`), markers never bind handles — Class F.
- value-lifecycle onto the method plane: `copy x` → `x.copy()`, `consume(x)` →
  `x.consume()`, joining the shipped `.drop()` — Class G.

**(a) manifest** — mostly already lands correctly; the fixes are local:

```jet
payload:  { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources:  { pkgs: github@NixOS/nixpkgs/nixos-unstable }
packages: {
    jet: executable {
        build: Recipe.Cargo(lock: "Cargo.lock"),   // sum variant → .Variant() form, by rule
        wrap:  Wrap.{ path_prefix: self.runtime },  // product → .{}, by rule
        meta:  Meta.{ platforms: .Unix },
    },
    jetpack: alias(packages.jet, bin: "jetpack"),
}
module env.dev     { packages: […]  banner: Banner.Lines([…]) }
module overlay.dev { package("R").with += [cran.jsonlite] }   // was `overlay dev`
```

The product-vs-sum rule (`Wrap.{}` vs `Recipe.Cargo(…)`) is now *stated and
checked*, which resolves manifest F3 and core F4 by rule instead of memory.

**(b) fn** — every promise on `@`, every instruction on `#`, effects in `#(…)`:

```jet
@Pure @MustUse @Replayable @Sanitizer
fn parse(input: Tainted<Str>) -> Record ? Error #(Net, !Fs) { … }
```

"Promise → @" is now always right. Cleanest possible fix for Class C, tiny
churn.

**(c) construction** — one ctor law, mechanical verb choice:

```jet
Deque.new()          // fresh stateful     (rule: fresh → new)
Set.from([1,2,3])    // convert existing   (rule: from existing → from)
Clock.new(42)        // was time.clock(42) (module factory retired)
Wrap.{ … }           // product literal
Int.from(bigval)     // was BigInt(100) narrowing → from
Option.Val(x) / Option.None    // D-SHAPE3b owner substitution: never Some
```

**(d) type decls** — sub-rule: bodied types use blocks, aliases use `=`:

```jet
struct Point { x: Int, y: Int }   // bodied → block
state  Door  { Open, Closed }     // bodied → block
alias  Pair<T> = (T, T)           // transparent rename → =
distinct UserId = Int             // was `UserId :: distinct Int` — same `=` rule
```

**(e) control flow** — unchanged, keyword plane, explicitly preserved:

```jet
if x == { .A -> …, .B -> … }
loop i in 0..10 step 2 { … }
```

**(f) value-lifecycle — the plane law moves the *ungated* verbs to methods, and
deliberately does not touch the gated one.** Duplicating and discarding are
plain operations on a value → the method plane, matching shipped `.drop()`:

```jet
ops.transform(pool, input.copy(), 5000)   // was `copy input` prefix keyword (reopens S4)
result.drop("reason")                     // already a method
consume(handle)                           // UNCHANGED — linear finisher, @Unsafe-gated (D-LIN1)
```

`input.copy()` reads left-to-right and carries its own call-site signal (the
`.copy()` is visibly *on* `input`), which the bare prefix never did; no new
sigil (respects D-CAP2). But `consume` is **not** collapsed: it is the linear
finisher legal only inside `@Unsafe` (D-LIN1), and giving it a bare `.consume()`
would hide that gate — a footgun I1 requires to stay visible. So Class G lands
as "copy and drop share the method shape; consume stays distinct and gated,"
not "one shape for three." And `copy`'s move reopens the 10-day-old S4 (see §4),
so it is an owner-gated ballot, not a mechanical consequence.

**Guessability:** highest of the four across the top-4 classes, because it fixes
A, B, C, D, F with *rules that keep the existing vocabulary* — nobody relearns
the planes, ~15 exceptions vanish. Sibling test: `@Pure` → `@Sanitizer` ✓;
`module env.dev` → `module overlay.dev` ✓; `Deque.new()` → `Ring.new()` ✓;
`Wrap.{}` → `Meta.{}` ✓; `Recipe.Cargo(…)` → `Recipe.Prebuilt(…)` ✓;
`result.drop("reason")` → `input.copy()` ✓. (No numeric score is claimed — the
percentages an earlier draft cited were narrative, not measured; the evidence is
the passing sibling pairs above versus the enumerated misses in §1, and a real
count would be run over the inventory's construct list before ratifying.)

**Overturns:** D-TAINT-SAN, D-REPLAY1 (→ @), D-JPK-OVERLAY1 (overlay → module
overlay.), D-DET1/D-EMAIL1 (module factories → type statics), D-API-CTOR1 (make
verb choice mechanical, kill bare `T()`), D-OPT-SPELL1/S34 (Option/Result
variant casing), D-MONOREF1 (3 ref forms → 1), D-TXN4/D-SCAP1 (handle slot),
plus the CLI-drift and hygiene fixes.

**Kill-criteria:** hollow defaults? no. Dictate structure? no. Invariant
carve-out? no — it *repairs* I8's "one way to mean it" and keeps I1's opt-in
tiers. Clean.

**Attacks:** ① "Total" is aspirational — the locally-justified I8 islands
(`.view(a..b)`, `.take_pattern`, `@Test` scope-members) are still exceptions;
plane-total must either kill useful DSLs or admit a bounded exception list,
softening "gap-free." ② The product-vs-sum ctor rule needs the type's kind to
predict its ctor; for an opaque stdlib type the user can't see the definition —
still a doc lookup. ③ Moving `@Sanitizer`/`@Replayable` to `@` is only honest if
they are statically *checked* like `@Pure`; if enforcement differs, the plane
line re-blurs. ④ Two marker glyphs survive (`@` and `#`) — a beginner still
learns which is which before the law pays off. ⑤ Effects keep both `#(…)` and
`@Pure` touching one lattice (Class C not fully closed) — needs a follow-on
consolidation (D-SHAPE8 below).

---

### Candidate 3 — Declaration-uniform (every named block is `kw name { }`)

**Law:** every named block in the language is `<keyword> <name> <body>` — no
dotted `module x.y`, no bare `executable {}`, no `alias(…)` fn, no
`overlay name`. package/env/overlay/system/target all read `kw name { }`.

**Rule:** introduces a name → `keyword name { … }`, always.

**(a) manifest** — every tier a keyword block:

```jet
package jet {
    target executable
    build  Recipe.cargo(lock: "Cargo.lock")
    wrap   { path_prefix: self.runtime }
    meta   { platforms: .Unix }
}
package jetpack { alias jet  bin "jetpack" }
source  pkgs { from github@NixOS/nixpkgs/nixos-unstable }
env dev { packages […]  banner […] }
overlay dev { package R { with [cran.jsonlite] } }
```

Class A is unified hard. But this overturns the entire `field: value` manifest
model and `module <ns>.<name>` namespacing.

**(b) fn:** untouched — markers are not blocks. Class C unsolved.

**(c) construction:** untouched — values are not declarations. Class B unsolved.

**(d) type decls** — `alias`/`distinct` forced into block form:

```jet
struct Point { x Int  y Int }
alias  Pair  { = (T, T) }     // awkward — a rename shoved into a block
distinct UserId { = Int }
state  Door  { Open  Closed }
```

**(e) control flow:** stays keyword — declaration-uniform is keyword-friendly,
consistent here.

**(f) value-lifecycle:** operations aren't named blocks, so the law is silent —
`copy input` / `consume(x)` / `.drop()` stay three shapes. Class G unsolved.

**Guessability:** high for Class A (all named blocks identical — learn
`env dev {}`, predict `system halcyon {}` ✓), but Classes B and C are entirely
untouched and `alias`/`distinct` are mangled. Fixes 1 of top-4.

**Overturns:** U3 (dotted module namespaces), U10 (compact `packages:` map),
D-TGT (bare `executable`), D-JPK-OVERLAY1, D-TYPEALIAS1/D-DIST1 (`=`/`::`), and
the whole `field: value` manifest surface.

**Kill-criteria:** hollow defaults? **yes** — kills the beginner one-liner
`packages: { hello: executable }`, which becomes a multi-line block. Direct hit
on priority #2. Dictate structure? borderline. Carve-out? no.

**Attacks:** ① Destroys the compact `packages: { hello: executable }` magic
(priority #2). ② `alias`/`distinct` are naturally `=`/type-expr; block form is
noise. ③ Loses namespace-merge: `module env.dev` contributed across files (U14)
is core to multi-file manifests; `env dev {}` blocks don't obviously merge. ④
Only 1 of 4 top classes fixed — construction and markers untouched. ⑤ Data
(`field: value` args, map entries) still needs colons, so the world still splits
into decl-blocks vs data — the law isn't actually total.

---

### Candidate 4 — Register-split (manifests are pure data; code keeps expressions)

**Law:** two registers. A *manifest* is pure data — only `field: value`,
`Type.{}`, and enum-dot `.Variant`; no fn calls, no bare keywords, no
dotted-module declarations. *Program code* keeps full expression syntax. One
predictable rule per register.

**Rule:** in a manifest, if you're not writing `field: value` / `Type.{}` /
`.Variant`, you're wrong. In code, ordinary expression rules.

**(a) manifest = pure data:**

```jet
payload:  Payload.{ name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources:  [ Source.{ name: "pkgs", from: .Github("NixOS/nixpkgs/nixos-unstable") } ]
packages: [
    Package.{ name: "jet", target: .Executable,
              build: .Cargo(lock: "Cargo.lock"),
              wrap:  Wrap.{ path_prefix: […] },
              meta:  Meta.{ platforms: .Unix } },
    Package.{ name: "jetpack", alias_of: .Package("jet"), bin: "jetpack" },
]
envs:     [ Env.{ name: "dev", packages: […], banner: Banner.{ lines: […] } } ]
overlays: [ Overlay.{ name: "dev", packages: [ .{ target: "R", with: [cran.jsonlite] } ] } ]
```

Inside a manifest everything is one shape (data). No `module env.dev`, no
`overlay dev`, no `executable {}`, no `alias(…)`, no `Recipe.cargo(…)`. Resolves
Classes A/B/D/H **within manifests** by banning the exceptions. Very high
in-manifest guessability.

**(b)–(f):** all live in the *code* register — **unchanged**. `Deque.new()`,
`@Pure`, `struct`, `if`, and `copy input` / `consume(x)` / `.drop()` keep exactly
today's shapes. Register-split is a scoped law: it fixes only the manifest side
and touches nothing in stdlib/core, so Class G survives untouched.

**Guessability:** near-perfect inside manifests (learn `Package.{}`, predict
`Env.{}`/`Overlay.{}` ✓, ~98%), zero change to code — so Classes B/C/K/L survive
everywhere an expert spends most of their day.

**Overturns:** U3 (module namespaces in manifests), U10 (`packages:`
bare-keyword map), D-TGT (bare `executable`), D-JPK-OVERLAY1, D-BUILDPROFILE1
(bare `build{}`), D-ECO2 (`Recipe.cargo` → enum-dot). Contained to manifests,
but deep — supersedes the whole D-ECO spelling wave.

**Kill-criteria:** hollow defaults? **yes** — `packages: { hello: executable }`
becomes `packages: [ Package.{ name: "hello", target: .Executable } ]`; more
ceremony for the beginner. Dictate structure? no. Carve-out? no.

**Attacks:** ① Beginner regression: the two-line magic manifest gets
noticeably more verbose. ② Only manifests — the code/stdlib surface (most of
what an expert types) keeps all its drift; the owner asked for the *entire*
language. ③ `hook: fn(sh) {}` (the ratified expert escape, D-ECO7) is code
inside a manifest — the pure-data register forbids it; the boundary leaks. ④
Computed modules (`platform.linux:` desugaring to comptime `if`, `git.root`,
`find()` — the direction the owner just pulled up) are code in a manifest; a
pure-data register bans calls and kills them. ⑤ Two registers is two mental
models; a `Wrap.{}` looks identical in both but the surrounding rules differ, so
"which register am I in" is itself a new tax.

---

## 4. Recommendation

**Adopt Candidate 2 (Plane-total) as one language-wide shape law that governs
manifests and code identically.** A manifest is not a separate "data-plane
register" — it uses the *same* full plane set as code: keyword blocks for
namespaces (`module env.dev`, `module overlay.dev`), value literals for values
(`Wrap.{}`), `field: value` for data, `.Variant` for sums. Register-split is a
rejected candidate, not a sub-profile folded in; the earlier "manifest is pure
data, one law not two" framing is dropped because it contradicted its own
example (the recommended manifest keeps `module` blocks and the bare-keyword
`executable`, neither of which is data-plane). One plane law, applied
everywhere, with no second manifest rule to learn.

**The metric this optimizes is the owner's, stated this session: can an expert
who has learned one construct guess the shape of a sibling and be right most of
the time?** That is expert predictability — it serves I8 ("one way to mean it")
and the expert-reach rank, not the beginner-learnability rank directly. The
beginner still benefits (fewer total shapes to meet), but the claim is not
"this is priority #2"; the claim is that a single job→plane function is the
mechanism I8 has always implied, now made total.

- **Why plane-total:** it is the only candidate that addresses all four
  highest-blast classes (A/B/C/D) while *keeping* every bright spot — it repairs
  the two-plane `@`/`#` law instead of replacing it, keeps keywords for control
  flow (the owner's constraint), keeps marker terseness and the magic/named-arg
  dual facet (I8), and states the product-vs-sum and ctor-verb rules the current
  surface only implies. It moves the surface from many per-construct shapes to
  one derivable rule; the sibling-prediction tests in §3 pass for the pairs an
  expert actually hits.
- **On Class B, honestly:** the product-vs-sum rule (`Wrap.{}` vs
  `Recipe.Cargo(…)`) does not make an *opaque* type's constructor pure-inference
  — you still must know whether `Deque` is a plain record or a stateful
  container to know its shape. What it fixes is replacing *five arbitrary verbs*
  (`.new`/`.from`/lowercase-factory/`generate`/bare-`T()`) with *one axis*
  (product vs sum) plus a mechanical verb rule (`new` fresh / `from` convert /
  `over` view). That is a more learnable axis, not a claim of zero lookup — the
  honest gain is "one fundamental distinction" over "memorize per type."
- **Value-lifecycle (Class G) — an owner-gated S4 reopen, not a silent flip.**
  `copy x` reads badly mid-argument and gives no call-site signal (the owner's
  complaint, verified in shipped `examples/interop/*/main.jet`). The natural
  respelling aligns it to the method plane the already-shipped
  `result.drop("reason")` uses: `ops.transform(pool, copy input, 5000)` →
  `ops.transform(pool, input.copy(), 5000)`. **But S4 (2026-07-04) deliberately
  chose the prefix verb and made method-`.clone()` un-typable by I8**, so this
  reverses a 10-day-old decision and resurrects the very method-copy S4
  rejected. That reversal is exactly what the owner asked to reconsider — so
  D-SHAPE-LIFECYCLE presents it as a live reopen with S4's rationale on the
  ballot (options: keep `copy x`; method `x.copy()`; other), recommending the
  method form on guessability grounds while naming what it costs S4.
  **`consume` does not join the collapse:** it is the linear finisher gated to
  `@Unsafe` (D-LIN1); giving it a bare `.consume()` would hide that gate and
  break I1's "footguns stay visible." So the family lands as: `copy` respelled
  (owner's ask), `drop` already a method, `consume` kept distinct and visibly
  gated. The three do *not* force to one shape — the gating difference is real
  and load-bearing.
- **Tradeoff — "gap-free" has an asterisk.** Two marker glyphs survive, and a
  small set of I8 DSL islands (`.view(a..b)`, `@Test` scope-members) are real
  exceptions. §5's I9 does not wave these away; it gives a *hard qualifying
  test* for what may be an island, so the escape clause cannot be used to
  re-admit ordinary drift.
- **Why not Value-uniform (owner's seed):** it fixes construction beautifully
  but breaks value encapsulation (a literal can't maintain a `Set`/`Deque`
  invariant — the same "then it isn't really a literal" objection applies),
  regresses marker terseness into field bags, and only covers values — leaving
  every declaration and control-flow shape untouched. Plane-total adopts its
  good instinct (one `Type.{}` literal plane) without paying its costs elsewhere.
- **Why not Declaration-uniform:** kills the beginner `packages: { hello:
  executable }` one-liner, mangles `alias`/`distinct`, and leaves construction
  and markers unsolved.
- **Why not Register-split:** fixes only manifests (most of what an expert types
  is code), forbids the ratified `hook` escape and the computed-module direction
  (`platform.linux:`, `git.root`, `find()`), and regresses the beginner
  manifest. Rejected outright, not folded in.

---

## 5. The durable artifact — a proposed invariant

**I9 — The shape law (guessability).** Every user-typeable construct is assigned
its syntactic shape by a single, published, total job→plane function. A
construct may not take a shape its job does not dictate. A new syntax decision
passes only if an expert who knows the plane law can predict the new construct's
shape from its job *without being told*. When a job appears to need two shapes,
the plane boundary is wrong and must be repaired; a per-construct exception is a
defect, not a carve-out. The **same** plane law applies in manifests and in
code — there is no second manifest register.

**The island test (so the exception clause can't be gamed).** A bounded
exception ("DSL island" — e.g. `@Sql<Row> { }`, `@Test`'s `.setup`/`.timeout`
members, `.view(a..b)`'s range grammar) is admissible *only* if it meets all of:
(1) it is a **closed, stdlib-only, statically-checked** grammar — no user code
may mint a new one; (2) it is **registered by name in the law itself**, so the
island list is finite and reviewable; (3) it does not change the shape of any
construct *outside* the island. Anything failing these three is ordinary drift
and is rejected — relabeling a special case as an "island" does not pass. The
island list is part of I9's text, not an open-ended escape hatch.

**Shape rule (the paragraph every future decision passes):** *Name the job; the
job names the plane; the plane names the shape.* Introduces a name into scope →
keyword block `kw name { … }`. A value with fields → `Type.{ … }` (product) or
`.Variant(…)` (sum). An operation on a value → `.verb(…)`. A checkable promise
about behavior → `@Marker`. A compiler build/run instruction → `#Marker`. Plain
data or a named argument → `field: value`. A reference to something external →
the one ref sigil. Branch / loop / return → keyword. If a construct maps to two
shapes, fix the boundary — never special-case; if it needs a bounded grammar,
it must pass the island test above.

---

## 6. Staged application plan → cards / ballots

Law first; then one application ballot per area. Every ballot that changes
user-typeable syntax is owner-gated; the impl card follows each ratification
(parser → sema → codegen → teaching error → snapshot → example, per I4/I5).

**Each area is its own ballot the owner decides — D-SHAPE1 does not pre-decide
them.** D-SHAPE1 records which uniformity principle the owner prefers as a
default lean, but it does not settle any spelling. Every wave below is a
separate ballot with its own options, including "leave as-is," so the owner
picks each resolution and can pick against the D-SHAPE1 lean on any given area.
The principle guides; it does not prescribe.

| Card | Scope (one line) | Depends on | Closes |
|---|---|---|---|
| **D-SHAPE1** | **Ratify the plane-total shape law + invariant I9** (§4/§5) — the job→plane function + the island test; commits wave directions per the note above | — | the disease framing; gates all below |
| **D-SHAPE2** | Marker-plane repair: move `@Sanitizer`/`@Replayable` → `@` (**precondition: confirm both are statically checked like `@Pure`; if enforcement differs, they are not promises and stay `#`**), one handle-binding slot, one derive spelling | D-SHAPE1 | core F1, F2, F9 |
| **D-SHAPE-LIFECYCLE** | **Reopen S4/D-CAP2** for `copy`: ballot keep-`copy x` / method-`x.copy()` / other, with S4's rationale on the card; `drop` already a method. **`consume` stays distinct and `@Unsafe`-gated (D-LIN1) — explicitly *not* collapsed** | D-SHAPE1 | core F6 |
| **D-SHAPE3a** | Construction *shape* law: product→`.{}`, sum→`.Variant()`, mechanical `new`/`from`/`over` verb choice, retire bare `T(…)` + lowercase module factories (`time.clock`→`Clock.new`, keeping the injectable *convention*) | D-SHAPE1 | core F4, F17; stdlib 1 |
| **D-SHAPE3b** | Option/Result variant casing alone (`Val`/`None`/`ok`/`err` → uniform) — **split out because it touches nearly every program**; decide independently | D-SHAPE1 | core F5; stdlib 2 |
| **D-SHAPE4** | Stdlib naming/resource law: `eq`/`equal` split, `watcher.*` verb-first, RAII-vs-`.close()` rule, duration unification (`5min`/`2s`/`time.ms`) | D-SHAPE1, D-SHAPE3a | stdlib 2, 3, 4; core F13 |
| **D-SHAPE5a** | Manifest *structure* under the law: `overlay dev`→`module overlay.dev`, resolve bare `executable{}`, **fix the U8-vs-U10 nesting contradiction**, one block/colon rule | D-SHAPE1, D-SHAPE3a | manifest F1, F2, F7, F8 |
| **D-SHAPE5b** | **Rework the HELD D-ECO wave under the law**, per D-ECO cluster (Recipe/Wrap/Meta/Banner/Tool/alias/…) — each cluster stays its own decision; includes the `Recipe.cargo`→`.Cargo` casing flip | D-SHAPE1, D-SHAPE3a, D-SHAPE5a | manifest F3; supersedes D-ECO2–19 |
| **D-SHAPE6** | CLI grammar law + **fix the spec/impl drift** (`jet inspect …`/`jet registry …` vs shipped flat verbs): pick noun-verb or ratify-flat-and-amend-spec, unify `jetos`/`jetpack` depth, retire self-relabeling aliases | D-SHAPE1 | manifest F5, F6 |
| **D-SHAPE7** | External-ref sigil unification across `provider@target`, version-pin, CLI `:` ref, and the three `D-MONOREF1` member forms. **Scope guard: must NOT disturb `[T#N]`, `#(E)`, or the `.{`/`.[` family (bright spots) — the chosen sigil is named and bounded** | D-SHAPE1 | manifest F4, F9; core F8 |
| **D-SHAPE8** | Effects-plane consolidation: reconcile `@Pure` vs `#(…)` into one signature effect-annotation spelling | D-SHAPE1, D-SHAPE2 | core F7 |
| **D-SHAPE9** | Invokable-entry law: reserved `fn run/dev/build` names vs `@Task fn` marker — pick one mechanism | D-SHAPE1, D-SHAPE2 | core F14 |
| **SHAPE-HYGIENE1** *(card, no owner gate)* | Registry doc casing (`#layout`→`@Layout`), ambiguous planning-number citations → canonical headers, historical `W`-prefix diagnostics note — pure correction, can start now | — | core F12; manifest F10 |

Order: **D-SHAPE1 ratifies first** (nothing else is a real decision until the
law exists, and it commits the wave directions per the note). Then the
application ballots fan out; SHAPE-HYGIENE1 can run in parallel immediately.
D-SHAPE5b supersedes the held D-ECO wave rather than deciding it as-proposed —
the ecosystem spellings are re-derived from the ratified law, each cluster its
own ballot.
