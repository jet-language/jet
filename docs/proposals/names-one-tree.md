# Names, modules, and visibility: one tree

Working proposal, 2026-08-07. A first-principles rethink of how anything in Jet gets a name, is found, and is seen. D-NAME-SIGIL1=A is ratified; the remaining owner choices are the other D-NAME ballots on card #1625, summarized in DECISIONS at the end.

## Executive summary

Jet never founded its name story. Every ratified rethink leans on names — the Core tree, `use` lists, computed modules, `@` facts — but the ground under them is five accidental systems that grew separately. D-ONCE-AT1=D supersedes the former prefix `$` fact spelling; infix `@` package references are unchanged.

**The finding.** A name reaches you through one of five doors today: a quoted file import (`use "scoring"`), a bare module import (`use math`), a file-reference declaration (`module math`), automatic project discovery, or the prelude — a closed table inside the compiler. Visibility has six spellings, and two of them give the underscore different meanings. Inside the compiler, six separate stores resolve names, two of them disagree about package visibility, and two tools re-derive imports by scanning raw source text. The shadowing rule has one law and four carve-outs.

**The one idea.** A name is a member at one point in one tree. A declaration attaches it. `.` walks the tree. `pub` fences an edge. `use` makes a local short name that only points — it never creates, moves, or changes meaning. The prelude is a short alias list you can read. `@` marks the compile-time grade of a name. The underscores are one ladder: `_` discards, `_name` marks internal, `__name` belongs to the machine. Reflection and diagnostics print the same path you type.

**The law.** *A declaration attaches; an alias only points. Declarations never collide. A declaration replaces an alias — never another declaration.* Every ratified naming rule falls out of this law, and the four shadowing carve-outs stop being carve-outs: prelude names, reserved Core names, and FFI modules are aliases, so your declaration wins by law, not by exception.

The concrete payoffs:

- **Zero-import projects with a full audit trail.** Your own files are already in the tree. A beginner writes a second file and calls `scoring.letter(91)` with no `use` line. An expert runs `jet imports main.jet` and reads exactly what resolved to what, spells any import explicitly (`use project.grades.curve`, or `use "../shared/tools.jet" as tools` for a file by path), and can require explicit imports project-wide with one switch.
- **One visibility story.** `pub` opens an edge, `pub(package)` fences at the package, `_name` means internal — one meaning everywhere. The file marker and its helpers become one spelling.
- **The lexical space claimed on purpose.** One underscore talks to humans, two underscores belong to the machine, and the dunder/sunder shapes mean nothing by ratified D-NAME-SIGIL1=A.
- **A prelude you can read.** The ambient names become a visible alias list in Core source, not a Rust table. Growth stays ballot-gated.
- **Names round-trip.** `T.reflect()` and every diagnostic print a path you can type back in. Today two same-named types reflect identically.
- **One resolver.** Six binding stores, two import-map builders, two call ladders kept in sync by comment, six mangling sites, and two source-text scrapers collapse into one sema-owned name ledger that every engine and tool reads. This fixes a live divergence: `pub(package)` is enforced by sema and invisible to codegen's filters.

What the nine ballots ask: adopt the model; make project files visible without imports; pick the audit switch for explicit imports; move the prelude into Core source; pick the visibility set; record the ratified underscore ladder; unpark `use` inside module bodies; finish the ratified retirement of role modules; make reflection print paths.

What does not change: private by default, no wildcards, no `namespace` keyword, the ratified Core tree and grouped `use` list, `@` law, casing law, generic modules, and the one-definition rule.

Six terms carry the whole doc. **Tree**: the single namespace of a program — your project, its packages, `core`, the FFI roots. **Attach**: what a declaration does — put a member at one tree point. **Alias**: a local short name made by `use` or the prelude; it points at a member and has no meaning of its own. **Fence**: a visibility mark on a tree edge. **Grade**: when a name exists — runtime, compile time (`@name`), or compiler fact (`@build`). **Ledger**: the compiler's one table mapping every mention to the member it resolved to; sema writes it, everything else reads it.

## The problem, briefly

Five doors to the same place. Each row is one way a name arrives in your file, with its home and its defect:

| # | Door | Home | Defect |
|---|------|------|--------|
| 1 | `use "scoring"` — quoted file import | S16; `crates/jet-driver/src/Loader.rs:1585` | duplicates door 4; path strings in source |
| 2 | `use math` — bare module import | S16; `Loader.rs:1626` | needed even for your own files |
| 3 | `module math` — file reference | S16/D-MOD1; `Loader.rs:1399` | a declaration that only imports |
| 4 | automatic discovery of `module` declarations | U3; `crates/jet-driver/src/ProjectParts.rs` | covers declarations, not plain files |
| 5 | the prelude | D-PRELUDE-LAW1; `crates/jet-foundation/src/Syntax/core_surface.rs:183` | closed Rust table; matched per call site, never in scope |

Six spellings of visibility, for three ideas:

| Spelling | What it means today | Defect |
|----------|--------------------|--------|
| `pub` | public | — |
| `priv` | private again, inside a `#PubFile` file | exists only to undo a marker |
| `#PubFile` | flips one file public-by-default | a file marker fighting the module tree |
| `pub(package)` | visible to the package only | enforced by sema, invisible to codegen's filters |
| `pub _name` | soft-public: callable, warns, no promise (D-SHAPE-INTERNAL1) | underscore meaning one |
| `module _draft` | skipped from discovery (U3) | underscore meaning two |

One shadowing law, four carve-outs: no shadowing (E0118, E0105/E0106) — except prelude names (D-PRELUDE-LAW1), reserved `Reader`/`Cursor` (syntax-decisions.md:1084), FFI modules in-situ (syntax-decisions.md:2527), and `_` (exempt).

Inside the compiler, the same fragmentation (file:line evidence from this audit):

| # | Duplicate | Homes |
|---|-----------|-------|
| B1 | six binding stores, no symbol table | `Checker.scopes` (jet-sema/src/Sema/mod.rs:1250), `LowerEnv.locals` (jet-codegen/src/Codegen/TIR/lower/env.rs:24), `EvalCtx` (TIR/eval/mod.rs:708), `SemanticSymbolIndex` (jet-semindex/src/Symbols.rs:70), `Session.scope` (jet-repl/src/lib.rs:572), `Cx` (Codegen/Context.rs:82) |
| B2 | import maps built twice from the same AST | sema Bundle.rs:1952–2148 vs Codegen/Imports.rs:139–251, including a second copy of the re-export walk |
| B3 | call-name ladder re-implemented in codegen, aligned by comment | Sema CheckerInfer/calls/direct_calls.rs:91 vs Codegen TIR/subset/expressions.rs:74–235 (alignment comment at :219); third partial copy at method_calls.rs:1118 |
| B4 | visibility checked twice with different rules | sema checks `pub` + `pub(package)`; Codegen/Imports.rs:83–426 filters on bare `is_pub` — `pub(package)` is invisible there |
| B5 | six mangling sites plus ~30 inline `format!("user_…")` bypasses; JIT has its own scheme | Codegen/mod.rs:1302 canonical; jet-jit/src/jit/types_meta.rs:351 separate |
| B6 | module scoping by string convention | members stored as `{alias}__{method}` and recovered by byte slicing (Sema/CheckerCoreLib/imports.rs:49); sibling calls fixed by AST string rewrite (Bundle/InlineCalls.rs:6) — a sibling used as a value falls outside the rewrite and can only error |
| B7 | Core surface as Rust string tables | predicates.rs:51 (~60 module paths), module_items.rs (1148 lines), fixed_sigs.rs (3236 lines); one tool string-parses sema's Rust source via `include_str!` (jet-devserver Canvas/query_actions.rs:583) |
| B8 | three unrelated things called "prelude" | ambient ident list; `Units.jet` re-parsed at every check (Bundle.rs:2677); the Rust runtime text under `Prelude/**` |
| B9 | "did you mean" search twice with different candidate sets | names_incdec.rs:66 vs direct_calls.rs:749 |

Why now: `modularize` is the third most frequent operation in real code, and the one surface this audit family never measured (docs/audits/surface-frequency-audit-2026-08-04.md:448). Every ratified rethink — Core tree, `use` lists, `@` facts, computed modules, build config — lands on this ground. Founding it once means building them once.

## The proposal

Three axes. Every name has all three; nothing else exists.

1. **Place** — the tree point where the declaration attached it. Scopes are the same shape at every zoom level: block ⊂ function ⊂ module ⊂ package ⊂ tree. A type body is a namespace. A module body is a namespace. The project is a namespace.
2. **Fence** — who may cross the edge: private (default), `pub(package)`, `pub`. `_name` adds "not a promise" on top of any fence.
3. **Grade** — when the name exists: runtime, compile time (`@name`, D-META-STAGE1), or compiler fact (`@layout`, `@build`; D-LAYOUT-FACTS1, D-CONF-READ1, and the ratified fact law on card #1620).

The law, restated: *a declaration attaches; an alias only points. Declarations never collide. A declaration replaces an alias — never another declaration.*

Ratified rules that become theorems of the law:

- E0118/E0105 no-shadowing — "declarations never collide."
- D-GENMOD-IDENTITY1 "aliases and display names never enter identity" — "an alias only points."
- D-PRELUDE-LAW1 "user shadowing wins" — the prelude is aliases, and "a declaration replaces an alias."
- The `Reader`/`Cursor` user-type-wins guard and FFI in-situ shadowing — same theorem, no carve-out text needed.
- D-CORE-USELIST1's "local name = last segment unless `as`" — an alias is a pointer, so its default spelling is the member's own leaf name.
- D-CALLDUAL1's "never a global search" — lookup walks scopes and aliases, never scans the world. Auto-visible project files extend what is in scope; they do not change how lookup walks.

Here is the tree itself, for the small project this doc keeps coming back to. Files on the left, the one tree on the right:

```text
  ON DISK                          THE TREE
  app/
  ├── main.jet                     project
  ├── util.jet                     ├── main          (fn run)
  ├── scoring.jet                  ├── util          (fn shout, fn _pad)
  ├── grades/                      ├── scoring       (fn letter, fn curve)
  │   └── curve.jet                ├── grades
  └── package.jet                  │   └── curve     (fn apply)
                                   ├── markdown      (package, via jet.lock)
  core (ships with jet)            └── core
                                       ├── prelude   (print, panic, use core.time.[…])
                                       ├── files, http, time, …
```

Every question about names is a question about this picture. Where does a name come from? It is on the tree. Who sees it? Whatever the fences on the path allow. What does `use` do? It draws a shortcut arrow — nothing more.

Now the surface, element by element. Every change is a before/after pair. Status marks: **(ratified)** already law, **(amends X)** changes ratified law via the named ballot, **(proposed)** new.

### 1. Your own files: zero imports, full ledger — (amends S16, D-MOD1/2, D-MOD-DIR; ballots D-NAME-FILES1, D-NAME-AUDIT1)

The beginner rung first. Before — today, splitting a file breaks the program until you learn two import spellings:

```jet
// main.jet — today
use "scoring"
use "grades/curve" as curve

fn run() {
    print(scoring.letter(91))
    print(curve.apply(80))
}
```

After — the files were already in the tree, so there is nothing to type:

```jet
// main.jet — proposed: no import lines
fn run() {
    print(scoring.letter(91))
    print(grades.curve.apply(80))
}
```

`use` keeps its one real job — making short names — and packages keep their line:

```jet
use grades.curve as curve         // alias, when you want one
use scoring.[letter]              // unqualified item (ratified list form)
use markdown                      // an external package (ratified, U17)
```

Now the expert side, because magic without a ledger is a design hole. Zero-import needs three exits, and each one is concrete:

**Exit 1 — see exactly what the magic did.** One command prints the resolution ledger for a file: every name it did not declare, the member it resolved to, the source, and the door it came through (proposed):

```text
$ jet imports main.jet
main.jet resolves 3 names it did not declare:

  scoring         project.scoring         scoring.jet:1        auto (project tree)
  grades.curve    project.grades.curve    grades/curve.jet:1   auto (project tree)
  print           core.prelude.print      core/prelude.jet:3   prelude

out-of-tree files: none
```

The middle column is typeable: paste it after `use` and you have the explicit form. `jet project parts` (ratified, U3) stays the authoritative whole-tree listing, with `--skipped` for `_` internals.

**Exit 2 — the explicit spelling that always works.** Every project member has a canonical path under the `project` root, and `use project.…` is already ratified spelling (U3 uses it to reach skipped internals). For a file that is *not* in the tree — a shared script two directories up, a generated file, a vendored one-off — the quoted path form stays, now with one job and a required alias:

```jet
use project.grades.curve                // explicit tree path — always works
use "../shared/metrics.jet" as metrics  // by path: any file on disk (proposed:
                                        // `as` required; the ledger records it,
                                        // and `jet imports` flags it as out-of-tree)
```

In-tree, quoted paths are never needed — the tree path is shorter and survives file moves. Out-of-tree, the quoted form is the expert's escape and the audit trail knows it was used.

**Exit 3 — refuse the magic project-wide.** One switch in `package.jet` requires an explicit `use` line for every cross-file name. It is a deny-level lint, not a compile mode — the same source means the same thing everywhere; the switch only refuses to pass until the lines exist (proposed; ballot D-NAME-AUDIT1):

```jet
// package.jet
policy: .{ imports: .Explicit }
```

```text
$ jet check
Warning [L0xxx]: `scoring` resolves through the project tree, but this package requires explicit imports
  --> main.jet:4:11
 Fix: add `use project.scoring`, or run `jet fix` to add every missing line.
```

(`L0xxx` stands for a code allocated at implementation, with its registered text and snapshot per I4.)

Deleted spellings: the file-reference declaration `module math` (a declaration that only imports), and quoted imports for in-tree files. A directory is a namespace; `grades/curve.jet` is `grades.curve`. `module name { }` stays as the way to group names inside a file — philosophy already calls inline and file modules one feature with two entry points. This re-frames U3's "single outermost construct" clause: a declared module nests under its file's namespace, and the common case — one declaration matching the filename — keeps its path unchanged. The FILES1 ballot names that amendment.

### 2. `use` has one job — (unparks D-GENMOD-BODY1's exclusion; writes down D-SPREAD1 + D-CORE-USELIST1; ballot D-NAME-WALK1)

The whole `use` grammar, before and after:

| Form | Today | After |
|------|-------|-------|
| `use "scoring"` | file import | deleted in-tree; out-of-tree by-path form keeps quotes, requires `as` (FILES1) |
| `use "grades/curve" as g` | file import + alias | same — out-of-tree only |
| `module math` | file reference that only imports | deleted (FILES1) |
| `use math` | own module or package | package only — own files need no line (FILES1) |
| `use math.clamp` | selective import | unchanged |
| `use math.{sin, cos as c}` | brace group (S16) | `use math.[sin, cos as c]` — the ratified list form (D-CORE-USELIST1, #1575) |
| `use core.[files as fs, http]` | ratified, unbuilt | built on the one resolver |
| `use project._name` | ratified reach into skipped internals (U3) | unchanged; the same `project.` root now names every member |
| `pub use path` | re-export at file top only | legal at every namespace level (WALK1) |
| `use` inside `module name { }` | rejected — E0003 cites a ratification that shipped in June | legal; a module body is a namespace like any other (WALK1) |
| `use math.*` | rejected (E0612) | still rejected — no wildcards |

The module-body unpark, concretely:

```jet
module report {
    use core.text.fmt              // proposed: legal here
    pub use tables.render          // proposed: curated door at the right level
    pub fn build() => String { fmt.pad("…", 8) }
}
```

And one sentence enters the spec: `.[ ]` always means "these members of that prefix." In expression position it builds values (`point.[x, y, z]` is `[point.x, point.y, point.z]`, ratified D-SPREAD1); after `use` it builds aliases (ratified D-CORE-USELIST1). The entry grammars stay as ratified on each side — bare members in expressions (E0961), `as` and dotted paths after `use` — so neither ruling is amended. The connection just gets said.

### 3. One visibility story — (amends D-VISDEFAULT2, D-SHAPE-INTERNAL1; ballot D-NAME-FENCE1)

Before — today, six spellings:

```jet
pub fn api() { }                  // public
fn helper() { }                   // private
#PubFile                          // flips one file public-by-default
priv fn secret() { }              //   …with per-item opt-out
pub(package) fn shared() { }      // package-only
pub fn _legacy() { }              // soft-public, warns outside
module _draft { }                 // skipped from discovery
```

After — four marks with one meaning each:

```jet
pub fn api() { }                  // public (ratified)
fn helper() { }                   // private (ratified)
pub(package) fn shared() { }      // package fence (ratified)
pub module text { … }             // public-by-default subtree (proposed,
                                  // replaces #PubFile; priv opts out
                                  // inside it, exactly as today)
_name                             // one meaning anywhere: internal —
                                  // skipped from discovery, callable,
                                  // warns outside, never a promise
```

`pub module` puts the public-by-default flip at the attach point — the module — instead of a file marker. `priv` keeps its one existing meaning (opt out inside a public-by-default region) and simply moves with the flip. Note: `pub module` parses today with a weaker meaning — the module is visible but members still need `pub` — so this is a redefinition of that spelling, named in the FENCE1 ballot. `#PubFile` and the separate soft-public rule fold in.

One thing `_` never does in Jet: change access. Dart fused privacy into the underscore, so renaming `_x` to `x` there is an API change. In Jet the fence (`pub`) and the promise (`_`) stay separate marks, and `use project._name` still reaches an internal on purpose, with the warning.

### 4. The underscore ladder: claim the lexical space on purpose — (ratified D-NAME-SIGIL1=A)

Greenfield means the unclaimed shapes are part of the design, not leftovers. Here is the whole underscore space, what each shape means, and its status:

| Shape | Meaning in Jet | Status |
|-------|----------------|--------|
| `_` | discard: matches anything, binds nothing, exempt from shadowing | ratified |
| `_name` | internal: out of discovery, callable, warns outside, never a promise | one meaning via FENCE1 |
| `pub _name` | the same internal signal on a public member (soft-public) | ratified D-SHAPE-INTERNAL1; folds into the one `_` story |
| `__name` — any double underscore | the machine's space: rejected in user source; reserved for compiler-generated binders, debugger and serializer metadata, and tools | ratified D-SHAPE-DUNDER2 |
| `__name__` (dunder) | nothing, ever — rejected with the rest of `__`; protocol members are trait members, compiler facts read as `@facts` | SIGIL1 |
| `_name_`, `name_` (sunder, trailing) | no meaning on purpose; a trailing underscore is just a character | SIGIL1 |

The ladder reads aloud in one sentence: **zero underscores is a name, one underscore is a message to humans, two underscores is the machine talking.**

What the peers did with this space, and what it teaches:

| Shape | Language | What happened | Lesson |
|-------|----------|---------------|--------|
| `__init__`, `__len__` | Python | protocol members in a visible magic namespace | it works — no collisions, discoverable — but it is ugly, typo-silent (`__lne__` never fires, nothing warns), and users mint their own dunders against the docs' advice |
| `__name` in a class | Python | name-mangled to `_Cls__name` | privacy by rewriting the user's spelling confuses everyone it touches |
| `_name` | Python | convention-only privacy | an unchecked convention drifts; Jet's `_` is checked (discovery, warning) |
| `_Name`, `__name` | C / C++ | reserved to the implementation by rulebook | nobody reads the rulebook; collisions are undefined behavior. Jet rejects at parse instead (D-SHAPE-DUNDER2) |
| `_` | Go | blank identifier, one meaning | the floor Jet keeps: `_` never surprises |
| `_x` | Rust | unused-but-named binding, lint-silenced | the underscore as an "I mean this" signal to the compiler — same spirit as Jet's `_name` |
| `_name` | Dart | library privacy fused into the name | renaming changes access; Jet keeps fence and promise as separate marks |
| `#name` | JavaScript | true privacy via a new sigil | a whole sigil spent on one job; Jet spends none |

Python's regret is the sharpest: dunders gave the language a protocol namespace but put the machine's names in the user's mouth — everyone types `__init__` daily and typos fail silently. Jet already has both halves of the answer, ratified: protocol members ride traits with ordinary names, and compiler-held facts are read through `@` (`T.@range`, `f.@effects` — D-FACT-READ1), so there is nothing left for a dunder to do. The machine's namespace exists (`__`), but no human ever types into it.

The ratified SIGIL1 ruling writes the ladder into the spec as one law, settles the dunder/sunder shapes (no meaning, stated on purpose — an explicit wall, so nobody "discovers" a use for `_name_` in year three), and gives the reserved `__` space one visible product — every compiler-generated symbol a tool can show you (stack traces, dumps, generated-code review) starts with `__jet`, so a machine name is recognizable on sight:

```text
$ jet run app.jet --trace
  at project.scoring.letter (scoring.jet:4)
  at __jet_lambda_7 (main.jet:9)          // machine-made, and it says so
  at project.main.run (main.jet:8)
```

The cleanup target is now one ledger-derived scheme under the `__jet` prefix;
inline `format!("user_…")` mangling bypasses are retired.

### 5. The prelude in the open — (amends the D-PRELUDE-LAW1 mechanism, not its list; ballot D-NAME-ALIAS1)

Before: `print`, `input`, `panic`, `require` live in a Rust table (`core_surface.rs:183`) and are matched by name at each call site. To read the ambient surface of the language, you read compiler source.

After — Core ships a readable prelude module; the compiler opens it for every file (proposed):

```jet
// core/prelude.jet (Core source; the ratified list, ballot-gated growth)
pub fn print(value: Any) { … }         // ambient basics are declared here
pub fn panic(message: String) { … }    // (their meaning stays in Prelude
pub fn require(cond: Bool) { … }       //  per I9, as today)
pub use core.time.[Clock, Instant, Date, Duration]
pub use core.files.[Path, read as read_file, write as write_file]
pub use core.comptime.[embed_file, embed_bytes, find, fetch]
```

Exact member spellings follow the ratified Core tree (D-CORE-TREE1) and are settled by #1576; the lines above show the shape, not the final list. The comptime-gated four stay gated: the gate is a property of those declarations, and an alias never changes what it points at.

Your declaration still wins — now by the alias law, with the ratified shadow warning. `#NoPrelude` still opts a file out. The D-CORE-PRELUDE1 criteria and epoch-gated growth stay exactly as ratified; only the home moves from a Rust table to Core source everyone can read.

### 6. Names round-trip — (amends D-METAREFLECT1 and D-ANY-JAI1 lightly; ballot D-NAME-REFLECT1)

Before: `reflect.of(x).type_name()` returns a bare name; two `Point` types in different modules reflect identically. Diagnostics mostly print bare names.

After — every reflected and diagnosed name is the canonical path you can type (proposed). `T.reflect().name` stays the leaf; `T.reflect().path` is the full spelling. `jet project parts`, hover, and errors print the same path. Diagnostics use the leaf when it is unique in scope and the path when it is not, so short errors stay short.

```jet
print(geo.Point.reflect().path)   // "project.geo.Point"   (proposed)
print(geo.Point.reflect().name)   // "Point"               (ratified leaf)
```

### 7. Role modules finish their ratified retirement — (finishes D-ECO-DECL1/D-ECO-FILEROOT1; amends U3/U8/D-JPK-MODBODY1; ballot D-NAME-ROLEMOD1)

Before — the `module` keyword serves a second grammar for env/system/image/workspace, while D-ECO-DECL1 (ratified) already says Packages and Configs are typed values and `package.jet` is the one reserved file. Code ships both worlds (`ENV_FILE`, `pkg.jet` readers, `env.jet` at the repo root):

```jet
// env.jet, live at the repo root today
module env.dev {
    sources: { default: github@NixOS/nixpkgs/nixos-unstable }
    packages: [ default.[ cargo, clippy, rustc ] ]
}
```

After — `module` means code namespace, nothing else; ecosystem entries are the ratified typed values in `package.jet` (D-CONF-NAME1 vocabulary):

```jet
// package.jet (proposed final form, ratified vocabulary)
name: "jet"
envs: .{
    dev: Env.{
        sources: .{ default: github@NixOS/nixpkgs/nixos-unstable },
        packages: [default.[cargo, clippy, rustc]],
    },
}
```

The reserved-namespace paragraph in U3 and the U8 field rule are edited to match the later ruling.

### 8. The whole surface on one page

Everything above, plus every name-adjacent form the audit touched, in one table. This is the full syntax, grammar, and API pass — what a reader needs to check that nothing was missed:

| Surface | Today | After | Via |
|---------|-------|-------|-----|
| declare a member | `fn` / `struct` / `enum` / `trait` / … | unchanged | — |
| group names in a file | `module name { }` | unchanged; nests under the file's namespace | FILES1 |
| mount a file | `module math` (reference) or `use "math"` | nothing — the file is already `math` | FILES1 |
| import a project file | `use "grades/curve" as g` | nothing, or `use project.grades.curve` to be explicit | FILES1 |
| import a file by path | `use "path"` (also in-tree) | `use "…/file.jet" as name` — out-of-tree only, `as` required | FILES1 |
| import a package | `use markdown` | unchanged (U17) | — |
| selective import | `use math.clamp` | unchanged | — |
| grouped import | `use math.{sin, cos as c}` | `use math.[sin, cos as c]` | ratified, #1575 |
| member spread | `point.[x, y, z]` | unchanged; same `.[ ]` law as `use` lists, one spec sentence | WALK1 |
| re-export | `pub use`, file top only | every namespace level | WALK1 |
| `use` in a module body | rejected (stale E0003) | legal | WALK1 |
| wildcard import | rejected (E0612) | still rejected | — |
| public | `pub` | unchanged | — |
| package fence | `pub(package)` | unchanged, and finally enforced in one place | TREE1 |
| public-by-default region | `#PubFile` + `priv` | `pub module` + `priv` | FENCE1 |
| soft-public | `pub _name` (L0601 special) | folded into the one `_` story | FENCE1 |
| internal module | `module _draft` | same `_` story, every declaration kind | FENCE1 |
| discard | `_` | unchanged | — |
| machine names | `__name` rejected (D-SHAPE-DUNDER2) | unchanged, plus the visible `__jet` scheme | SIGIL1 |
| dunder / sunder | undefined accidents | defined as nothing, on purpose | SIGIL1 |
| ambient names | closed Rust table | `core/prelude.jet`, readable | ALIAS1 |
| refuse ambient | `#NoPrelude` | unchanged | — |
| role declarations | `module env.dev { }` | typed values in `package.jet` | ROLEMOD1 |
| reflected name | `.name` (leaf only) | `.name` + `.path` | REFLECT1 |
| tree listing | `jet project parts [--skipped]` | unchanged (ratified) | — |
| per-file ledger | — | `jet imports <file>` | FILES1/AUDIT1 |
| explicit-imports switch | — | `policy: .{ imports: .Explicit }` | AUDIT1 |
| generic modules | `module cache<K>(n) { }`, `module hot :: cache<…>` | unchanged (ratified) | — |
| FFI modules (`python.…`, `go.…`, …) | importable; in-situ user shadowing is a carve-out (syntax-decisions.md:2527) | same spelling; FFI roots are tree members and their ambient names are aliases, so user-wins is the law, not a carve-out | TREE1 |
| compiler facts | `@build.…` reads (ratified), `T.@layout` | unchanged; facts are the `@` grade of the same tree, resolved by the same walk | — |

## Beginner magic, expert control

The ladder. Each rung is opt-in. No upper rung changes what a lower rung does.

**Rung 0 — type nothing.** One file. Prelude names and your own declarations. No imports, no visibility marks, everything private.

```jet
fn run() { print("hi") }
```

**Rung 1 — add a file, still type nothing.** `scoring.jet` beside `main.jet` is `scoring.` immediately (proposed).

```jet
fn run() { print(scoring.letter(91)) }
```

**Rung 2 — shorten.** Aliases when paths get long (ratified forms).

```jet
use core.[files as fs, encoding.json]
use grades.curve.[apply]
```

**Rung 3 — share.** Fences when others arrive.

```jet
pub fn api() { }
pub(package) fn shared() { }
```

**Rung 4 — shape.** Curated doors and internals.

```jet
pub module text { pub use wrap.wrap }   // door (pub module proposed)
pub fn _legacy() { }                    // internal, warns outside
```

**Rung 5 — compute.** Generic modules and facts, all ratified.

```jet
module cache<K>(capacity: Int) { … }
module hot :: cache<String>(64)
module tuned :: cache<Int>(@build.settings.cache_slots)
```

**Rung 6 — control the authority.** Refuse the defaults; audit everything.

```jet
#NoPrelude                        // no ambient names in this file
fn print(x: String) { … }         // your name; replaces the alias, warns
```

```jet
// package.jet — the project-wide refusals
policy: .{ imports: .Explicit }   // every cross-file name needs its line (proposed)
```

`jet imports <file>` prints what resolved and how; `jet project parts` lists the whole tree; `--skipped` shows `_` internals; reflection prints the same paths.

The two failure modes, checked by name:

- **Ceremony creep**: none. No default gained a word. Rungs 0 and 1 lose words: two import forms and one declaration form disappear.
- **Magic without an exit**: auto-visible files can be refused per file (`_name` keeps a file out of discovery; `use project._name` still reaches it), seen (`jet imports` per file, `jet project parts` for the tree), and switched off project-wide (`imports: .Explicit`). The prelude keeps its three exits: shadow it, `#NoPrelude`, read it in Core source.

## The final vision

The same small program, today and proposed, side by side. Job: grade files, shared helpers, one public API, one internal.

Today — three files, two import spellings, one file marker, six visibility words:

```jet
// main.jet
use "scoring"
use "util"

fn run() {
    print(scoring.letter(91))
    print(util.shout(scoring.curve(80)))
}

// scoring.jet
pub fn letter(score: Int) => String { … }
pub fn curve(score: Int) => Int { … }

// util.jet
#PubFile
fn shout(s: String) => String = s.upper() + "!"
priv fn pad(s: String) => String { … }
```

Proposed — the same three files, zero import lines, one visibility story:

```jet
// main.jet
fn run() {
    print(scoring.letter(91))
    print(util.shout(scoring.curve(80)))
}

// scoring.jet
pub fn letter(score: Int) => String { … }
pub fn curve(score: Int) => Int { … }

// util.jet
pub module util                        // header: public-by-default file,
                                       // replaces #PubFile (proposed)
fn shout(s: String) => String = s.upper() + "!"       // public by default
fn _pad(s: String) => String { … }     // internal, one `_` story (proposed)
```

And the tree they live in — what `jet project parts` shows, what reflection prints, what every diagnostic spells:

```text
project
├── main            fn run                      private
├── util            pub module (public-by-default)
│   ├── shout       fn                          pub
│   └── _pad        fn                          internal: no promise, warns
├── scoring
│   ├── letter      fn                          pub
│   └── curve       fn                          pub
├── grades
│   └── curve
│       └── apply   fn                          pub
└── core
    └── prelude     print, panic, require, …    readable aliases
```

The rich middle — a library package with a curated door:

```jet
// text/module.jet (directory module, ratified)
pub use wrap.wrap                      // the door
pub use style.[bold, dim]              // ratified list form

// text/wrap.jet
pub fn wrap(s: String, width: Int) => String { … }
fn measure(s: String) => Int { … }     // private, invisible outside
```

The expert extreme — computed subtree, facts, no prelude, everything audited:

```jet
#NoPrelude
use core.prelude.[print]               // take back just one name (proposed)
use "../shared/metrics.jet" as metrics // out-of-tree, by path, on the record

module cache<K>(capacity: Int) {       // ratified generic module
    pub struct Entry { key: K }
    pub fn get(k: K) => K? { … }
}
module hot :: cache<String>(@build.settings.slots)   // ratified splice

fn run() {
    print(hot.Entry.reflect().path)    // "project.hot.Entry"  (proposed)
    metrics.tick()
}
```

And the expert's audit session over that file — every magic default answered by a command:

```text
$ jet imports expert.jet
expert.jet resolves 3 names it did not declare:

  print      core.prelude.print    core/prelude.jet:3     use line (expert.jet:2)
  metrics    "../shared/metrics.jet"  ../shared/metrics.jet  use line (expert.jet:3)
  @build     the build facts       package.jet            compiler fact (ratified)

out-of-tree files: ../shared/metrics.jet   (1)

$ jet project parts --skipped
(internal members, reachable via `use project._name`)
  project.util._pad
```

If this section were the whole proposal: one tree, no import lines until you want them, four visibility marks, one underscore ladder, a readable prelude, paths that round-trip, and a command that answers "what did the magic do" for any file.

## What this unlocks

- **Scripts and teaching**: a two-file program with zero ceremony. The first lesson about modules becomes one sentence: "files are modules."
- **Libraries**: one obvious way to shape an API — `pub use` doors at the module head, `_name` for internals, `pub(package)` for the workspace.
- **Auditable magic**: `jet imports` gives reviewers the Go property — every name traces to a declaration, a `use` line, or a listed auto-resolution — without Go's import ceremony. `imports: .Explicit` turns the ceremony back on where policy wants it.
- **Tooling**: rename, hover, go-to-definition, Canvas, and the REPL read one ledger instead of five models; the devserver stops parsing sema's Rust source with `include_str!`.
- **Metaprogramming**: derives emit paths that resolve anywhere, closing the unqualified-name traps in derive output and the inline-module Codable gap by construction — generated code names members by path, and paths mean the same thing everywhere.
- **Critical builds**: `pub(package)` enforced in one place ends the sema/codegen divergence; machine names are recognizable on sight (`__jet`), so generated-code review and stack traces stop guessing.
- **The ratified backlog lands once**: the Core tree (#1574), use lists (#1575), prelude policy (#1576), generic modules (#1523), and the `@` fact planes all sit on one resolver instead of five.

## What stays

- Private by default (S18); no wildcards (D-GLOBIMPORT1, E0612); no `namespace` keyword (D-NAMESPACE1); one definition per name (D-CAP10).
- The ratified Core tree and doctrine (D-CORE-TREE1, D-CORE-DOCTRINE1), grouped `use` lists (D-CORE-USELIST1), prelude membership and criteria (D-CORE-PRELUDE1/2).
- `__name` belongs to Jet (D-SHAPE-DUNDER2) — SIGIL1 structures the reserved space; it does not reopen it.
- `@` law: the mark on the name (D-META-STAGE1), `@build` reads (D-CONF-READ1), `@layout` (D-LAYOUT-FACTS1); users never declare `@` members on types.
- Generic modules: declaration, `::` binding, instance identity (D-CONF-GENSPELL1, D-GENMOD-IDENTITY1, D-META-MODNAME1).
- Casing law (D-SHAPE-CASE1), acronym law, kebab name positions (S84).
- `module name { }` inline grouping — it earns its place as the one way to put several namespaces in one file.
- `#NoPrelude` — the refusal stays; only the thing it refuses gets a readable home.

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Full worked examples per option live in the Tower ballots on card #1625.

| ID | Question | Options (first = recommended) |
|----|----------|-------------------------------|
| D-NAME-TREE1 | Adopt the model: one tree, attach/alias law, one sema name ledger inside the compiler? | A adopt / B adopt the law but keep today's import surface / C reject |
| D-NAME-FILES1 | Are project files visible without imports, and what are the explicit forms? | **Ratified C, 2026-08-07** — manual named imports stay; no invisible auto-import. Owner: "Let's just stick with manual named imports like we used to have rather than the magic auto imports that are invisible." |
| D-NAME-AUDIT1 | ~~The explicit-imports switch~~ | Withdrawn — moot under FILES1=C; with no magic there is nothing to refuse |
| D-NAME-ALIAS1 | Where does the prelude live? | A a readable Core module of `pub use` aliases / B today's compiler table / C mode-gated: bigger list for single files, small in packages |
| D-NAME-FENCE1 | Which visibility set? | A `pub`, `pub(package)`, `pub module` (+ `priv` inside it), one `_` story / B keep today's six spellings / C minimal: `pub` and `_` only |
| D-NAME-SIGIL1 | **Ratified A, 2026-08-07:** one underscore ladder and settled dunder/sunder shapes | `_` human, `__` machine (visible `__jet` scheme), dunders/sunders mean nothing on purpose |
| D-NAME-WALK1 | Admit `use`/`pub use` inside module bodies, and write the one `.[ ]` sentence into the spec? | A both / B only inline `use` / C neither |
| D-NAME-ROLEMOD1 | Finish the ratified retirement of role modules (`module env.dev`) into typed values? | A yes, edit U3/U8, delete `ENV_FILE`/`pkg.jet` readers / B keep role modules and amend D-ECO-DECL1 back / C defer |
| D-NAME-REFLECT1 | Do reflection and diagnostics print canonical typeable paths? | A yes, add `.path`, keep `.name` as leaf / B no, keep bare names / C paths in tools only |

Ratified rulings each ballot amends are named inside the ballot text: FILES1 amends S16 (D-S16-USE, D-MOD1/2, D-MOD-DIR) and U3/D-SHAPE-MODULEINTERNAL1's outermost-construct and discovery-naming clauses, and touches D-CALLDUAL1's scope wording; AUDIT1 adds a policy key under the ratified config plane (D-CONF-*) and a lint under D-LINTPOLICY1's law; ALIAS1 amends the D-PRELUDE-LAW1 mechanism and D-PRELUDEX1 wording; FENCE1 amends D-VISDEFAULT2 and D-SHAPE-INTERNAL1 and redefines the current `pub module` spelling; SIGIL1 builds on D-SHAPE-DUNDER2 (A keeps it; B and C amend it); WALK1 lifts D-GENMOD-BODY1's exclusion clause; ROLEMOD1 amends U3, U8, and D-JPK-MODBODY1 to match D-ECO-DECL1/D-ECO-FILEROOT1; REFLECT1 amends D-METAREFLECT1 and the D-ANY-JAI1 runtime reflection surface.

## Implementation shape

**Phase A — internal re-founding, no surface change, all tests green.** One resolver in sema produces the name ledger (generalize the existing reference and import fact stores); codegen, JIT, interpreter, LSP, REPL, and devserver consume it. One `mangle` function under the `__jet` scheme; delete the ~30 inline `format!("user_…")` bypasses and the JIT's private drift (the JIT derives the same `__jet` prefix from the ledger). Delete the import-map rebuild, the second call ladder, the `{alias}__{method}` byte slicing, the AST string rewrite for sibling calls, and the devserver's source scrapers. Codegen visibility filters read the ledger, closing the `pub(package)` divergence. I3 holds: resolution is checking, so it lives in sema; engines stay dumb readers. I6 holds: the ledger is plain data in an existing seam crate.

**Phase B — land ratified-but-unbuilt work on the new substrate**: the Core tree (#1574), grouped use lists (#1575), prelude policy (#1576), generic module respelling (#1523), `@build` facts (#1518). Built once, on one resolver.

**Phase C — balloted surface unifications, each a coherent greenfield migration that deletes the replaced form**: FILES1 (delete the file-reference `module` and in-tree quoted imports; land `jet imports`), AUDIT1 (the switch and `jet fix` support), FENCE1 (one visibility story), SIGIL1 (the spec law and the `__jet` scheme), ALIAS1 (prelude module), WALK1, ROLEMOD1 (finish the ecosystem ruling; delete `ENV_FILE`, `pkg.jet` readers, migrate the repo's own `env.jet`), REFLECT1. Every example, snapshot, and doc migrates in the same change.
