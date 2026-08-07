# Names, modules, and visibility: one tree

Proposal, 2026-08-07. First-principles audit of how anything in Jet gets a
name, is found, and is seen. Owner choices are in DECISIONS at the end.
Everything else implements existing ratified law.

## Executive summary

Jet has never founded its name story. Every ratified rethink leans on names —
the Core tree, `use` lists, computed modules, `$` facts — but the ground under
them is five accidental systems that grew separately.

The finding. Today a name reaches you through one of five doors: a quoted
file import (`use "scoring"`), a bare module import (`use math`), a file
reference declaration (`module math`), automatic project discovery (U3), or
the prelude (a closed compiler table). Visibility has six spellings. The
compiler resolves names in six separate stores, mangles them in six places,
and two tools re-derive imports by scanning raw source text. The shadowing
rule has one law and four carve-outs.

The one idea. **A name is a member at one point in one tree.** A declaration
attaches it. `.` walks the tree. `pub` fences an edge. `use` makes a local
short name that only points — it never creates, moves, or changes meaning.
The prelude is a small alias list at the root. `$` marks the compile-time
grade of a name. Reflection and diagnostics print the same path you type.

The law. *A declaration attaches; an alias only points. Declarations never
collide. A declaration replaces an alias — never another declaration.*
Every ratified naming rule falls out of this law. The four shadowing
carve-outs stop being carve-outs: prelude names, reserved Core names, and
FFI modules are aliases, so your declaration wins by law, not by exception.

The concrete payoffs:

- **Zero-import projects.** Your own files are already in the tree. A
  beginner writes a second file and calls `scoring.letter(91)` with no
  `use` line at all. Two import forms and one declaration form get deleted.
- **One visibility story.** `pub` fences an edge; `pub(package)` fences at
  the package; `_name` means "internal, not a promise" — one meaning
  everywhere. The file-mode marker and its helpers become one spelling.
- **A prelude you can read.** The ambient names become a visible alias list
  in Core source, not a Rust table. Growth stays ballot-gated.
- **Names round-trip.** `T.reflect()` and every diagnostic print a path you
  can type back in. Today two same-named types reflect identically.
- **One resolver inside the compiler.** Six binding stores, two import-map
  builders, two call ladders kept in sync by comment, six mangling sites,
  and two source-text scrapers collapse into one sema-owned name ledger
  that every engine and tool reads. This fixes a real divergence:
  `pub(package)` is enforced by sema and invisible to codegen's filters.

What the ballots ask: adopt the model; make project files visible without
imports; move the prelude into Core source; pick the visibility set; unpark
`use` inside module bodies; finish the ratified retirement of role modules;
make reflection print paths.

What does not change: private by default, no wildcards, no `namespace`
keyword, the ratified Core tree and grouped `use` list, `$` law, casing law,
generic modules, and the one-definition rule.

## Glossary

- **Tree** — the single namespace of a program: your project, its packages,
  `core`, and the FFI roots. Every name is a member at one point in it.
- **Attach** — what a declaration does: put a member at one tree point.
- **Alias** — a local short name made by `use` or by the prelude. It points
  at a tree member. It has no meaning of its own.
- **Fence** — a visibility mark on a tree edge: `pub` or `pub(package)`.
- **Grade** — when a name exists: runtime, compile time (`$name`), or
  compiler fact (`$layout`, `$build`).
- **Prelude** — the small set of names every file sees with no `use` line.
- **Name ledger** — the compiler's one table mapping every mention to the
  member it resolved to. Sema writes it; everything else reads it.

## The one idea

**A name is a member at one point in one tree; everything else reads or
fences that tree.**

Beginner story: where does a name come from? You declared it, your project
has it (a file is a module named by its filename), or it is one of the few
prelude names. Where can you see a name? Follow the dots from a root:
`core.`, a package name, or your own files. Who sees yours? Nobody outside
your module until you write `pub`.

Expert story: the tree is fully addressable and fully shapeable. `pub use`
builds a curated door. `pub(package)` narrows a fence to the package.
`_name` marks internals. Generic modules attach computed subtrees. `$build`
and `$layout` are the compiler's own read-only branch, reached with the same
dots. `jet project parts` prints the tree; reflection returns the same paths.

## Evidence: the shadow systems

Five ways to reach a name:

| # | Door | Home | Defect |
|---|------|------|--------|
| 1 | `use "scoring"` quoted file import | S16; `crates/jet-driver/src/Loader.rs:1585` | duplicates door 4; path strings in source |
| 2 | `use math` bare module import | S16; `Loader.rs:1626` | needed even for your own files |
| 3 | `module math` file reference | S16/D-MOD1; `Loader.rs:1399` | a declaration that only imports |
| 4 | automatic discovery of `module` declarations | U3; `crates/jet-driver/src/ProjectParts.rs` | covers declarations, not plain files |
| 5 | prelude table | D-PRELUDE-LAW1; `crates/jet-foundation/src/Syntax/core_surface.rs:168` | closed Rust table; matched per call site, never in scope |

Six spellings of visibility: `pub`, `priv`, `#PubFile`, `pub(package)`,
`pub _name` soft-public (L0601), `_name` discovery-skip (U3). Two of them
give `_` different meanings.

One shadowing law, four carve-outs: no shadowing (E0118, E0105/E0106);
except prelude names (D-PRELUDE-LAW1), reserved `Reader`/`Cursor`
(syntax-decisions.md:1084), FFI modules in-situ (syntax-decisions.md:2460),
and `_` (exempt).

Inside the compiler (file:line evidence from this audit):

| # | Duplicate | Homes |
|---|-----------|-------|
| B1 | six binding stores, no symbol table | `Checker.scopes` (jet-sema/src/Sema/mod.rs:1250), `LowerEnv.locals` (jet-codegen/src/Codegen/TIR/lower/env.rs:24), `EvalCtx` (TIR/eval/mod.rs:708), `SemanticSymbolIndex` (jet-semindex/src/Symbols.rs:70), `Session.scope` (jet-repl/src/lib.rs:572), `Cx` (Codegen/Context.rs:82) |
| B2 | import maps built twice from the same AST | sema Bundle.rs:1952–2148 vs Codegen/Imports.rs:139–251, including a second copy of the re-export walk |
| B3 | call-name ladder twice, aligned by comment | Sema CheckerInfer/calls/direct_calls.rs:91 vs Codegen TIR/subset/expressions.rs:74 ("MUST match") |
| B4 | visibility checked twice with different rules | sema checks `pub` + `pub(package)`; Codegen/Imports.rs:83–426 filters on bare `is_pub` — `pub(package)` is invisible there |
| B5 | six mangling sites plus ~30 inline `format!("user_…")` bypasses; JIT has its own scheme | Codegen/mod.rs:1302 canonical; jet-jit/src/jit/types_meta.rs:351 separate |
| B6 | module scoping by string convention | members stored as `{alias}__{method}` and recovered by byte slicing (Sema/CheckerCoreLib/imports.rs:49); sibling calls fixed by AST string rewrite (Bundle/InlineCalls.rs:6) — a sibling used as a value silently misses |
| B7 | Core surface as Rust string tables | predicates.rs:51 (~60 module paths), module_items.rs (1148 lines), fixed_sigs.rs (3236 lines); one tool string-parses sema's Rust source via `include_str!` (jet-devserver Canvas/query_actions.rs:583) |
| B8 | three unrelated things called "prelude" | ambient ident list; `Units.jet` re-parsed at every check (Bundle.rs:2677); the Rust runtime text under `Prelude/**` |
| B9 | "did you mean" search twice with different candidate sets | names_incdec.rs:66 vs direct_calls.rs:749 |

Why now: `modularize` is the third most frequent operation in real code, and
the one surface this audit family has never measured
(docs/audits/surface-frequency-audit-2026-08-04.md:448). Every ratified
rethink — Core tree, `use` lists, `$` facts, computed modules, build config —
lands on this ground. Founding it once means building them once.

## The model

Three axes. Every name has all three; nothing else exists.

1. **Place** — the tree point where the declaration attached it. Scopes are
   the same shape at every zoom level: block ⊂ function ⊂ module ⊂ package
   ⊂ tree. A type body is a namespace. A module body is a namespace. The
   project is a namespace.
2. **Fence** — who may cross the edge: private (default), `pub(package)`,
   `pub`. `_name` adds "not a promise" on top of any fence.
3. **Grade** — when the name exists: runtime, compile time (`$name`,
   D-META-STAGE1), or compiler fact (`$layout`, `$build`; D-FACT law).

The law, restated: *a declaration attaches; an alias only points.
Declarations never collide. A declaration replaces an alias — never another
declaration.*

Ratified rules that become theorems of the law:

- E0118/E0105 no-shadowing — "declarations never collide."
- D-GENMOD-IDENTITY1 "aliases and display names never enter identity" —
  "an alias only points."
- D-PRELUDE-LAW1 "user shadowing wins" — the prelude is aliases, and "a
  declaration replaces an alias."
- The `Reader`/`Cursor` user-type-wins guard and FFI in-situ shadowing —
  same theorem, same rule, no carve-out text needed.
- D-CORE-USELIST1's "local name = last segment unless `as`" — an alias is a
  pointer, so its default spelling is the member's own leaf name.
- D-CALLDUAL1's "never a global search" — lookup walks scopes and aliases,
  never scans the world. Auto-visible project files extend what is in
  scope; they do not change how lookup walks.

The "ohhh" connections:

- **`use core.[files, http]` and `prefix.[a, b]` are the same operation.**
  D-SPREAD1 member spread and the D-CORE-USELIST1 use list both fan one
  prefix across bracketed members. One meaning for `.[ ]`: "these members
  of that prefix." In expression position it builds values; after `use` it
  builds aliases. The sigil is not overloaded; the law is shared.
- **The prelude is not a mechanism; it is a module.** A ratified alias list
  (`pub use`) in Core source gives the same names, keeps growth
  ballot-gated, and deletes a closed compiler table. `#NoPrelude` means
  "do not open that list."
- **The four shadowing carve-outs are one rule.** They were all aliases.
- **`$` is a grade, not a place.** `$build.deps` and `T.$layout` live in
  the same tree and resolve by the same walk; sema checks the grade. This
  is the name-plane twin of type-system v2's carriers × knowledge.
- **A file is a module.** U3 already says modules merge into one whole and
  never import each other. Finishing that thought deletes both file-import
  forms: the tree covers every file, and `use` returns to its one job —
  making short names.

## The surface

Every change as before/after. Status marks: **(ratified)** already law,
**(amends X)** changes ratified law via the named ballot, **(new)** new.

### 1. Your own files: no import lines — (amends S16, D-MOD1/2, D-MOD-DIR; ballot D-NAME-FILES1)

Before (today):

```jet
// main.jet
use "scoring"
use util as text

fn run() {
    print(scoring.letter(91))
    print(text.shout("hello"))
}
```

After (proposed):

```jet
// main.jet — scoring.jet and util.jet are already visible
fn run() {
    print(scoring.letter(91))
    print(util.shout("hello"))
}
```

`use` remains for shortening and for packages:

```jet
use util as text                  // alias, when you want one   (proposed)
use scoring.[letter]              // unqualified item (ratified list form)
use markdown                      // an external package (ratified, U17)
```

Deleted spellings: `use "path"`, `use "path" as x`, and the file-reference
declaration `module math`. A directory is a namespace; `grades/scoring.jet`
is `grades.scoring`. `module name { }` stays as the way to group names
inside a file — philosophy already calls inline and file modules one
feature with two entry points.

### 2. One visibility story — (amends D-VISDEFAULT2, D-SHAPE-INTERNAL1; ballot D-NAME-FENCE1)

Before (today, six spellings):

```jet
pub fn api() { }                  // public
fn helper() { }                   // private
#PubFile                          // flips one file public-by-default
priv fn secret() { }              //   …with per-item opt-out
pub(package) fn shared() { }      // package-only
pub fn _legacy() { }              // soft-public, warns outside
module _draft { }                 // skipped from discovery
```

After (proposed, three marks with one meaning each):

```jet
pub fn api() { }                  // public (ratified)
fn helper() { }                   // private (ratified)
pub(package) fn shared() { }      // package fence (ratified)
pub module text { … }             // public-by-default subtree (proposed,
                                  // replaces #PubFile + priv)
_name                             // one meaning anywhere: internal —
                                  // skipped from discovery, callable,
                                  // warns outside, never a promise
```

`pub module` puts the "public by default" flip at the attach point — the
module — instead of a file marker plus a second keyword. `priv`, `#PubFile`,
and the separate soft-public rule fold in. `__name` stays Jet's.

### 3. The prelude in the open — (amends the D-PRELUDE-LAW1 mechanism, not its list; ballot D-NAME-ALIAS1)

Before: `print`, `input`, `panic`, `require` live in a Rust table
(`core_surface.rs:168`) and are matched by name at each call site.

After (proposed) — Core ships a readable prelude module; the compiler opens
it for every file:

```jet
// core/prelude.jet (Core source, ratified list, ballot-gated growth)
pub use core.io.[print, eprint, input]
pub use core.run.[panic, require, assert, assert_eq]
pub use core.time.[Clock, Instant, Date, Duration]
pub use core.files.[Path, read_file, write_file, file_exists]
```

Your declaration still wins — now by the alias law, with the ratified
shadow warning. `#NoPrelude` still opts a file out. The D-CORE-PRELUDE1
criteria and epoch-gated growth stay exactly as ratified; only the home
moves from a Rust table to Core source everyone can read.

### 4. `use` inside a module body — (unparks D-GENMOD-BODY1's exclusion; ballot D-NAME-WALK1)

Before (today, E0003): "`use` inside an inline code module is not yet
supported… call items by their qualified path for now." The message still
cites D-MOD4 ratification, which happened 2026-06-18.

After (proposed): a module body is a namespace like any other; `use` and
`pub use` work at every level.

```jet
module report {
    use core.text.fmt              // proposed: legal here
    pub use tables.render          // proposed: curated door
    pub fn build() => String { fmt.pad("…", 8) }
}
```

### 5. Names round-trip — (amends D-METAREFLECT1 lightly; ballot D-NAME-REFLECT1)

Before: `reflect.of(x).type_name()` returns a bare name; two `Point` types
in different modules reflect identically. Diagnostics mostly print bare
names.

After (proposed): every reflected and diagnosed name is the canonical path
you can type. `T.reflect().name` stays the leaf; `T.reflect().path` is the
full spelling. `jet project parts`, hover, and errors print the same path.

```jet
print(geo.Point.reflect().path)   // "project.geo.Point"   (proposed)
```

### 6. Role modules finish their ratified retirement — (finishes D-ECO-DECL1/D-ECO-FILEROOT1; amends U3/U8/D-JPK-MODBODY1; ballot D-NAME-ROLEMOD1)

Before: `module env.dev { packages: […] }` — the `module` keyword serves a
second grammar for env/system/image/workspace, while D-ECO-DECL1 (ratified)
already says Packages and Configs are typed values and `package.jet` is the
one reserved file. Code ships both worlds (`ENV_FILE`, `pkg.jet` readers,
`env.jet` at the repo root).

After (proposed): `module` means code namespace, nothing else. Ecosystem
entries are the ratified typed values in `package.jet`. The reserved
namespace paragraph in U3 and the U8 field rule are edited to match the
later ruling.

### 7. What `.[ ]` means, once — (writes down D-SPREAD1 + D-CORE-USELIST1; ballot D-NAME-WALK1)

```jet
point.[x, y, z]                   // [point.x, point.y, point.z] (ratified)
use core.[files as fs, http]      // aliases fs, http            (ratified)
```

One sentence enters the spec: `.[ ]` always means "these members of that
prefix"; expression position yields values, `use` position yields aliases.

## Beginner magic, expert control

The ladder. Each rung is opt-in. No upper rung changes what a lower rung
does.

**Rung 0 — type nothing.** One file. Prelude names and your own
declarations. No imports, no visibility marks, everything private.

```jet
fn run() { print("hi") }
```

**Rung 1 — add a file, still type nothing.** `scoring.jet` beside
`main.jet` is `scoring.` immediately. (proposed)

```jet
fn run() { print(scoring.letter(91)) }
```

**Rung 2 — shorten.** Aliases when paths get long. (ratified forms)

```jet
use core.[files as fs, encoding.json]
use grades.scoring.[letter]
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
module tuned :: cache<Int>($build.settings.cache_slots)
```

**Rung 6 — control the authority.** Refuse the defaults; audit everything.

```jet
#NoPrelude                        // no ambient names in this file
fn print(x: String) { … }         // your name; replaces the alias, warns
```

`jet project parts` lists the whole tree, `--skipped` shows `_` internals,
and reflection prints the same paths.

The two failure modes, checked by name:

- **Ceremony creep**: none. No default gained a word. Rung 0 and rung 1
  lose words: two import forms and one declaration form disappear.
- **Magic without an exit**: auto-visible files can be refused per file
  (`_name` keeps a file out of discovery; explicit `use project._name`
  still reaches it), seen (`jet project parts` names every contributed
  member and its source file), and switched (a package that wants explicit
  imports everywhere sets it once in `package.jet` — the FILES1 ballot
  carries this switch). The prelude keeps its three exits: shadow it,
  `#NoPrelude`, read it in Core source.

## What it looks like

The same small program, today and proposed. Job: grade files, shared
helpers, one public API, one internal.

Today:

```jet
// main.jet
use "grades/scoring" as scoring
use "util"

fn run() {
    print(scoring.letter(91))
    print(util.shout(scoring.curve(80)))
}

// grades/scoring.jet
pub fn letter(score: Int) => String { … }
pub fn curve(score: Int) => Int { … }

// util.jet
#PubFile
fn shout(s: String) => String = s.upper() + "!"
priv fn pad(s: String) => String { … }
```

Proposed:

```jet
// main.jet — no import lines
fn run() {
    print(grades.scoring.letter(91))
    print(util.shout(grades.scoring.curve(80)))
}

// grades/scoring.jet
pub fn letter(score: Int) => String { … }
pub fn curve(score: Int) => Int { … }

// util.jet
pub module util {                      // proposed: pub module = public
    fn shout(s: String) => String = s.upper() + "!"   // default here
    _pad(s: String) … // internal helper, one `_` story           (proposed)
}
```

The rich middle — a library package with a curated door:

```jet
// text/module.jet (directory module, ratified)
pub use wrap.wrap                      // the door
pub use style.[bold, dim]              // ratified list form

// text/wrap.jet
pub fn wrap(s: String, width: Int) => String { … }
fn measure(s: String) => Int { }       // private, invisible outside
```

The expert extreme — computed subtree, facts, no prelude:

```jet
#NoPrelude
use core.io.[print]

module cache<K>(capacity: Int) {       // ratified generic module
    pub fn get(k: K) => K? { … }
}
module hot :: cache<String>($build.settings.slots)   // ratified splice

fn run() {
    print(hot.reflect().path)          // "project.hot"      (proposed)
}
```

## What this unlocks

- **Scripts and teaching**: a two-file program with zero ceremony. The
  first lesson about modules becomes one sentence: "files are modules."
- **Libraries**: one obvious way to shape an API — `pub use` doors at the
  module head, `_name` for internals, `pub(package)` for the workspace.
- **Tooling**: rename, hover, go-to-definition, Canvas, and the REPL all
  read one ledger instead of five models; the devserver stops parsing
  sema's Rust source with `include_str!`.
- **Metaprogramming**: derives emit paths that resolve anywhere, closing
  the latent unqualified-emission bug family (E2710) and the inline-module
  Codable gap by construction — generated code names members by path, and
  paths mean the same thing everywhere.
- **Critical builds**: `pub(package)` enforced in one place ends the
  sema/codegen divergence; provenance is greppable (every non-prelude name
  traces to a declaration or a `use` line — the Go property, kept even
  with auto-visible files because `jet project parts` names every source).
- **The ratified backlog lands once**: the Core tree (#1574), use lists
  (#1575), prelude policy (#1576), generic modules (#1523), and the `$`
  fact planes all sit on one resolver instead of five.

## What stays

- Private by default (S18); no wildcards (D-GLOBIMPORT1, E0612); no
  `namespace` keyword (D-NAMESPACE1); one definition per name (D-CAP10).
- The ratified Core tree and doctrine (D-CORE-TREE1, D-CORE-DOCTRINE1),
  grouped `use` lists (D-CORE-USELIST1), prelude membership and criteria
  (D-CORE-PRELUDE1/2).
- `$` law: the mark on the name (D-META-STAGE1), `$build` reads
  (D-CONF-READ1), `$layout` (D-LAYOUT-FACTS1), users never declare `$`
  members on types.
- Generic modules: declaration, `::` binding, instance identity
  (D-CONF-GENSPELL1, D-GENMOD-IDENTITY1, D-META-MODNAME1).
- Casing law (D-SHAPE-CASE1), acronym law, kebab name positions (S84).
- `module name { }` inline grouping — it earns its place as the one way to
  put several namespaces in one file.
- `#NoPrelude` — the refusal stays; only the thing it refuses gets a
  readable home.

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Full worked examples
per option live in the Tower ballots.

| ID | Question | Options (first = recommended) |
|----|----------|-------------------------------|
| D-NAME-TREE1 | Adopt the model: one tree, attach/alias law, one sema name ledger inside the compiler? | A adopt / B adopt the law but keep today's import surface / C reject |
| D-NAME-FILES1 | Are project files visible without imports? | A yes, whole project tree / B yes, same directory only / C no, keep explicit imports |
| D-NAME-ALIAS1 | Where does the prelude live? | A a readable Core module of `pub use` aliases / B today's compiler table / C mode-gated: bigger list for single-file programs, small list in packages |
| D-NAME-FENCE1 | Which visibility set? | A `pub`, `pub(package)`, `pub module`, one `_` story (deletes `#PubFile`/`priv`/L0601 special) / B keep today's six spellings / C minimal: `pub` and `_` only, delete `pub(package)` and `#PubFile` |
| D-NAME-WALK1 | Admit `use`/`pub use` inside module bodies, and write the one `.[ ]` sentence into the spec? | A both / B only inline `use` / C neither |
| D-NAME-ROLEMOD1 | Finish the ratified retirement of role modules (`module env.dev`) into typed values? | A yes, edit U3/U8, delete `ENV_FILE`/`pkg.jet` readers / B keep role modules and amend D-ECO-DECL1 back / C defer |
| D-NAME-REFLECT1 | Do reflection and diagnostics print canonical typeable paths? | A yes, add `.path`, keep `.name` as leaf / B no |

Ratified rulings each ballot amends are named inside the ballot text:
FILES1 amends S16 (D-S16-USE, D-MOD1/2, D-MOD-DIR) and touches D-CALLDUAL1's
scope wording; ALIAS1 amends the D-PRELUDE-LAW1 mechanism and D-PRELUDEX1
wording; FENCE1 amends D-VISDEFAULT2 and D-SHAPE-INTERNAL1; WALK1 lifts
D-GENMOD-BODY1's exclusion clause; ROLEMOD1 amends U3, U8, and
D-JPK-MODBODY1 to match D-ECO-DECL1/D-ECO-FILEROOT1; REFLECT1 amends
D-METAREFLECT1.

## Implementation shape

Phase A — internal re-founding, no surface change, all tests green.
One resolver in sema produces the name ledger (generalize the existing
`reference_anchors` + `import_targets`); codegen, JIT, interpreter, LSP,
REPL, and devserver consume it. One `mangle` function; delete the ~30
inline `format!("user_…")` bypasses and the JIT's private scheme's drift
(the JIT keeps its symbol prefix, derived from the ledger). Delete the
import-map rebuild, the second call ladder, the `{alias}__{method}` byte
slicing, the AST string rewrite for sibling calls, and the devserver's
source scrapers. Codegen visibility filters read the ledger, closing the
`pub(package)` divergence. I3 holds: resolution is checking, so it lives in
sema; engines stay dumb readers. I6 holds: the ledger is plain data in an
existing seam crate.

Phase B — land ratified-but-unbuilt work on the new substrate: the Core
tree (#1574), grouped use lists (#1575), prelude policy (#1576), generic
module respelling (#1523), `$build` facts (#1518). Built once, on one
resolver.

Phase C — balloted surface unifications, each a coherent greenfield
migration that deletes the replaced form: FILES1 (delete quoted imports and
file-reference `module`), FENCE1 (one visibility story), ALIAS1 (prelude
module), WALK1, ROLEMOD1 (finish the ecosystem ruling; delete `ENV_FILE`,
`pkg.jet` readers, migrate the repo's own `env.jet`), REFLECT1. Every
example, snapshot, and doc migrates in the same change (A4 rule: never
replacement without removal).
