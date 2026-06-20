# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready):** every full decision card carries, in this order, (1) a **user
story** — a real person and what they're doing, so the owner sees why the decision
exists; (2) a **short tradeoff comparison** — a compact table, one row per option,
columns that actually differ (ceremony / failure mode / ratification cost /
familiarity); and (3) a **worked example of every option** in a fenced ```jet (or
```shell) block — what that person types, sees, and hits as an error. No abstract
option tables standing in for examples. Close with `**Recommendation:**` + a one-line
why. Decisions not yet drafted to that bar are listed below as one-liners with a
recommendation; expand one into a full card when it's time to decide it.

---

## Open decisions

> **14 open decisions across 4 cards.** Each card below is self-contained: a user
> story (why it exists), a tradeoff table, and a worked example per option. Cards
> **c25** (range sugar) and **c55** (REPL v2) turned out implement-only — every
> choice they raised is already covered by ratified decisions — so nothing is
> queued for them here. Submitting a decision records it in `syntax-decisions.md`
> and removes it from this file.

---

## Memory & capability model — board card c06


# Ballot c06 — Memory capability model

Source plan: `tools/Tower/docs/sidequests/memory-capability-model.md`. Replaces the
internal three-mode `AccessConvention` (`Read`/`Mutate`/`Move`) with a user-visible
four-capability vocabulary. `take` and `view` are already ratified ownership keywords
(S10, M2); the open work is parameter-position annotations, the copy/share verbs, the
manifest flag, and the inference defaults. Owner has final say on all syntax (I7, I8).

Cross-references ballot c07 (`D-TGT1..D-TGT5`, `lib-exe-targets-model.md`): **D-CAP5**
is blocked on **D-TGT1** (whether lib/exe survives), and the manifest spelling in
**D-CAP4** should match whatever field style **D-TGT3** ratifies for `targets:`.

---

### D-CAP1 — Capability keyword spellings (rec A)

**User story.** Mara is porting a Rust game loop to Jet. She never wants to write a
borrow or a lifetime, but she does want to read a function signature and know at a
glance whether `heal(player)` keeps the player, mutates it, or just looks at it. The
words on the page have to mean what a beginner thinks they mean — and they have to be
the *same* words the compiler uses in its error messages.

| Option | Keywords | Reuses S10? | New tokens | Familiarity |
|--------|----------|-------------|------------|-------------|
| A | `view` / `edit` / `take` / `share` | `view`,`take` | `edit`,`share` | plain-English, no overlap with `mut` |
| B | `view` / `mut` / `take` / `share` | `view`,`take`,`mut` | `share` | reuses ratified `mut`, but `mut` reads as Rust |
| C | `read` / `write` / `own` / `share` | none | all 4 | S10 already **rejected** `read`/`write`/`owned` |
| D | `look` / `change` / `keep` / `share` | none | all 4 | maximally beginner, but invents fresh jargon |

- **Option A — view / edit / take / share.** Two keywords already exist; add `edit` and
  `share`. The four words are distinct, plain, and map one-to-one onto the four
  capabilities.

```jet
fn heal(player: edit Player) { player.hp += 10 }
fn draw(scene: view Scene)   { render(scene) }
fn add(player: take Player)  { party.members.push(player) }
fn cache(tex: share Texture) { textures.insert(tex) }
```

- **Option B — reuse `mut` for the edit capability.** `mut` is already ratified (S10);
  spend zero new tokens on the mutate slot.

```jet
fn heal(player: mut Player) { player.hp += 10 }
// reads as Rust's `&mut`; the plan's diagnostics vocab explicitly bans
// surfacing &mut-flavored wording to beginners.
```

- **Option C — read / write / own / share.** Verb-symmetric set.

```jet
fn heal(player: write Player) { player.hp += 10 }
// S10 (ratified) already rejected `read`/`write`/`owned` as the canonical
// ownership words. Re-proposing them would reverse a ratified decision.
```

- **Option D — look / change / keep / share.** Most beginner-legible verbs.

```jet
fn heal(player: change Player) { player.hp += 10 }
fn draw(scene: look Scene)     { render(scene) }
fn add(player: keep Player)    { party.members.push(player) }
// but `take`/`view` are already ratified — this would orphan two live keywords.
```

**Recommendation:** A — keeps the two ratified keywords, adds the two genuinely-missing
verbs, and avoids re-litigating S10's rejection of `read`/`write`.

---

### D-CAP2 — `copy` / `share` as keywords vs. method calls (rec A)

**User story.** Theo calls `party.add(player)` and then prints `player.name`. The
compiler tells him `player was taken`. He needs a one-word fix he can paste at the call
site — and it has to be obvious in review that he chose to duplicate or to share, not
that the compiler did it silently behind his back (the plan kills the implicit-clone
path, L0201).

| Option | Form | Visible at glance | Ceremony | Discoverable in diagnostic |
|--------|------|-------------------|----------|----------------------------|
| A | prefix keyword `copy x` / `share x` | yes — leads the line | low | trivial to quote in fix-it |
| B | method `x.copy()` / `x.share()` | trailing, easy to miss | low | reads like any other method |
| C | function `copy(x)` / `share(x)` | yes | low | but looks like stdlib, not a capability verb |
| D | sigil `~x` (copy) / `^x` (share) | terse | lowest | opaque to a beginner |

- **Option A — prefix keywords.** Matches the four-capability vocabulary; the verb leads
  the expression so the intent is the first thing read.

```jet
party.add(copy player)   // duplicate, keep my own
party.add(share player)  // both of us own it
print(player.name)       // ok — I kept a copy / a share
```

- **Option B — method calls.** No new keywords; rides existing method syntax.

```jet
party.add(player.copy())
party.add(player.share())
// duplication hides at the tail of the expression; in `f(g(x).copy())`
// it is easy to miss in review.
```

- **Option C — free functions.** `copy`/`share` as ordinary stdlib calls.

```jet
party.add(copy(player))
// indistinguishable from a user-defined helper; the capability story is invisible.
```

- **Option D — sigils.** Single-character prefixes.

```jet
party.add(~player)   // copy
party.add(^player)   // share
// terse but unteachable; violates the plain-vocabulary goal.
```

**Recommendation:** A — the prefix verb is the only form that is both leading-visible and
quotable verbatim in the post-take fix-it (`use copy player` / `use share player`).

---

### D-CAP3 — Annotation order: `player: edit Player` vs. `edit player: Player` (rec A)

**User story.** Priya reads a signature `fn write(file: edit File, data: view Bytes)`.
She parses it left-to-right as "the param `file`, which is an editable File." The
capability is a property of *what the value is here*, not of its name — so it should sit
where the rest of the type information already lives.

| Option | Form | Groups with | Reads as |
|--------|------|-------------|----------|
| A | `player: edit Player` | the type | "player is an edit-Player" |
| B | `edit player: Player` | the binding | "edit the player (a Player)" |
| C | `edit Player player` | C-style | type-first, unlike all other Jet params |

- **Option A — type-side.** Capability attaches to the type, mirroring every other type
  annotation in Jet.

```jet
fn write(file: edit File, data: view Bytes) {
    file.append(data)
}
```

- **Option B — binding-side.** Capability prefixes the parameter name (Rust's `mut`
  pattern position).

```jet
fn write(edit file: File, view data: Bytes) {
    file.append(data)
}
// the keyword now sits where `pub`/`mut`-on-bindings live; a beginner
// can misread `edit file` as an imperative "edit the file" statement.
```

- **Option C — C-style type-first.** Capability and type both precede the name.

```jet
fn write(edit File file, view Bytes data) {
    file.append(data)
}
// inverts Jet's `name: Type` ordering everywhere; non-starter for consistency.
```

**Recommendation:** A — consistent with `name: Type` everywhere else and reads as a
property of the value, not a command.

---

### D-CAP4 — `api` manifest spelling (rec A)

**User story.** Devi publishes a library and wants its public capability signatures
locked so a future refactor that flips `view` to `edit` is flagged as an API break, not
shipped silently. She opens `pkg.jet` and needs one line that looks like every other
field she already sets in the `payload:` identity block (`name:`, `version:`). `pkg.jet`
is Jet syntax, not TOML (S52 amended; U1 → U10 → D-JPK-FILES).

| Option | Spelling | Matches existing manifest style | Ratification cost |
|--------|----------|---------------------------------|-------------------|
| A | field `api: stable` / `api: explicit` | yes — colon fields like `name:`, `targets:` | low |
| B | statement `package api = stable` | introduces `=` assignment into the manifest | new manifest grammar |
| C | attribute `#[api(stable)] package` | attribute syntax on the package decl | new attribute surface |
| D | per-target field inside `targets:` | granular, but couples to D-TGT shape | blocked on D-TGT3 |

- **Option A — `api:` field.** A plain manifest field alongside the rest of `payload:`.

```jet
// pkg.jet
payload: { name: "raylib-jet", version: "0.4.0", api: stable }
//                                                ^^^^^^^^^^^ record public
//                                          capability signatures; flag breaks
packages: { raylib_jet: library }
```

- **Option B — `payload api = ...` statement.** The plan's literal first draft.

```jet
// pkg.jet
payload api = stable
// introduces `key = value` assignment; the rest of pkg.jet uses `key: value`.
```

- **Option C — attribute.** Attribute on the package declaration.

```jet
#[api(stable)]
payload: { name: "raylib_jet", version: "0.4.0" }
// adds an attribute grammar to the manifest that nothing else there uses.
```

- **Option D — per-target.** Capability mode declared inside each target.

```jet
// pkg.jet — speculative per-target shape proposed by c07 (D-TGT3)
targets: [
    library { api: stable },
    executable,
]
// most precise once targets land, but its exact shape depends on D-TGT3
// (bare keyword vs. block). Do not ratify ahead of c07.
```

**Recommendation:** A — `api: stable` / `api: explicit` matches the ratified colon-field
manifest style and costs no new grammar. If c07 ratifies per-target blocks (D-TGT3),
Option D can later layer on top without contradicting A.

---

### D-CAP5 — Which targets emit capability metadata (rec A, provisional — defer to D-TGT1)

**User story.** Sol builds a project that produces both a runnable game and a reusable
engine crate from one `pkg.jet`. The engine half must publish capability metadata so
downstream consumers compile against a checked contract; the game half should just infer
everything and stay ergonomic. Today that split rides one `pkg.jet`'s `packages:` block —
`library` vs `executable` (U10/D-ILE1) — but c07's `lib-exe-targets-model.md` may dissolve that into
fine-grained `targets:`. This card decides which *target* carries metadata emission once
that happens.

This decision is **blocked on D-TGT1** (ballot c07): if lib/exe is merely augmented
(D-TGT1 Option A) the existing rule stands; if it is replaced (D-TGT1 Option B) the rule
must move onto the new target vocabulary. The options below are the same rule expressed
against each possible c07 outcome.

| Option | Carrier of "emit metadata" | Depends on | Ergonomic default applies to |
|--------|----------------------------|-----------|------------------------------|
| A | any target that produces a consumable library artifact | D-TGT1=A or B | binary/app targets |
| B | only an explicit `library` target; all else infer-only | D-TGT1=B | every non-library target |
| C | metadata always emitted, gated purely by `api:` (D-CAP4) | independent of D-TGT | n/a — `api:` decides |

- **Option A — library-producing targets emit; binaries infer.** Preserves today's
  intent (`Library` emits, `Executable` does not) regardless of how D-TGT1 reshapes the
  vocabulary.

```jet
// pkg.jet  — targets per D-TGT (c07, unratified)
targets: [library, executable]
// `library` artifact ships capability metadata; `executable` infers and emits nothing.
pub fn step(world: World) { world.tick() }   // metadata: step(world: edit World)
```

- **Option B — only an explicit `library` target emits.** Tighter: anything not named
  `library` is infer-only, even if it is consumable.

```jet
targets: [staticlib]   // not literally `library` → no metadata under Option B
// risks a consumable artifact with no published contract.
```

- **Option C — decouple from targets; let `api:` decide.** Metadata emission is governed
  only by D-CAP4's `api:` field, ignoring target kind.

```jet
targets: [executable]
api: stable          // emits capability metadata even for an executable
// simplest rule, but emits contracts for artifacts nobody consumes.
```

**Recommendation:** A, provisional — ratify D-TGT1 first (c07), then confirm "any
library-producing target emits, binaries infer." A holds under either D-TGT1 outcome and
keeps today's behavior intact.

---

### D-CAP6 — When does `api: explicit` become the library default (rec A)

**User story.** Two years from now a beginner runs `jet new mylib` and writes one `pub`
function with no annotations. The question the owner is deciding: does that just work
(inference fills the contract), or does the toolchain refuse until she hand-writes
`edit`/`view` on every public signature? This is the simplicity-ratchet call (I8): make
the easy thing the default unless safety demands otherwise — and capability *safety* is
already guaranteed by inference, so explicitness here buys documentation, not safety.

| Option | Default for libraries | Beginner friction | When explicit is needed |
|--------|-----------------------|-------------------|-------------------------|
| A | inference; `api: explicit` opt-in forever | none | author opts in for visible contracts |
| B | inference now, flip to explicit at v1.0 | none now, breaks later | forced on everyone at 1.0 |
| C | explicit required for libraries from day one | high — every `pub` annotated | always |

- **Option A — opt-in forever.** Inference is always the library default; `api: explicit`
  is a tool authors reach for deliberately.

```jet
// pkg.jet has no `api:` line — inference fills the contract
pub fn heal(player: Player) { player.hp += 10 }
// published metadata: heal(player: edit Player)   — inferred, no error
```

- **Option B — flip to explicit at 1.0.** Ergonomic now, mandatory later.

```jet
// pre-1.0: compiles. post-1.0: same code errors —
//   error: pub `heal` must declare capabilities under api = explicit
//   fix:   pub fn heal(player: edit Player)
// a silent future break to every library in the ecosystem.
```

- **Option C — explicit from day one.** Every public signature annotated, always.

```jet
pub fn heal(player: Player) { player.hp += 10 }
// error: pub `heal` must declare capabilities
//   fix: pub fn heal(player: edit Player) { … }
// taxes the beginner for documentation inference already provides — fails I8.
```

**Recommendation:** A — inference already guarantees safety, so explicitness is a
documentation preference, not a correctness gate. Keep it opt-in (I8); never auto-flip.

---

## Cross-references to ballot c07 (D-TGT)

- **D-CAP5** is gated on **D-TGT1** (replace vs. augment lib/exe). Ratify D-TGT1 before
  finalizing which target emits capability metadata.
- **D-CAP4 Option A** (`api:` field) should match the colon-field manifest style; if
  **D-TGT3** ratifies per-target blocks, **D-CAP4 Option D** (`library { api: stable }`)
  becomes available as a follow-on without contradicting A.

---

## Library / executable targets — board card c07


These five cards reshape the `packages:` block in `pkg.jet` (D-JPK-FILES, latest;
renamed from `payload.jet`/`pack.jet` — see U1 → U10 → D-JPK-FILES) from a
single `kind:` per package into fine-grained *targets*. They reuse the ratified
manifest shape (U10: bare keyword `name: kind`, or block `name: { kind: …, … }`)
and the `#test` prefix attribute (S82 marker, sigil `@`→`#` per D-ATTR1, latest).
None of D-TGT1..D-TGT5 is ratified.

D-TGT1 gates D-CAP5 in `memory-capability-model.md` (which target emits
capability metadata). Resolve D-TGT1 first; the capability defaults follow.

---

### D-TGT1 — Replace `kind:` or augment it? (rec B)

**User story.** Priya ships `httpx`: a request library other packages `use`, plus
a `httpx` CLI that probes endpoints. Today U10 forces one `kind:` per package, so
she has to split it into two packages or bolt the CLI on as a special case. She
wants one package that openly declares it is both a library and an executable.

| Option | Concepts the author learns | Migration cost | Failure mode |
|---|---|---|---|
| A — augment | two (`kind:` *and* `targets:`) | none (old form stays) | two ways to say the same thing diverge over time |
| B — replace | one (`targets:`) | one-line lint per old entry | brief churn while `kind:` is deprecated |

- **Option A — augment: keep `kind:`, add `targets:` alongside.** Both forms stay
  first-class; `targets:` is merely preferred.

```jet
// pkg.jet — old kind: still canonical, targets: optional
packages:
  httpx: { kind: library }        // still valid, no warning
  probe: { targets: [executable] }    // new form
```

- **Option B — replace: `targets:` is canonical, `kind:` is a deprecation lint.**
  `kind:` is parsed and rewritten to a one-element `targets:` list with an
  advisory.

```jet
// pkg.jet — kind: triggers a migration advisory
packages:
  httpx: { kind: library }
//        ^^^^^^^^^^^^^^^ advisory: `kind: library` is deprecated; write
//        `targets: [library]`. (run `jet fix` to migrate)
  probe: { targets: [executable] }    // canonical
```

**Recommendation:** B — one concept (targets) is simpler than two; `kind:`
collapses to a single-line migration the compiler can auto-fix.

---

### D-TGT2 — Which targets ship in the first increment? (rec A)

**User story.** Marco is wiring up the first release of the targets model. He needs
the targets that real packages reach for on day one — without blocking on tooling
that does not exist yet (`benchmark` harness, `plugin` loader).

| Option | Targets shipped | Tooling required now | Risk |
|---|---|---|---|
| A — core four | library, executable, test, example | none beyond build + `jet test` | none |
| B — all six | + benchmark, plugin | benchmark harness, plugin loader | ships keywords with no working backend |

- **Option A — core four: `library`, `executable`, `test`, `example`.** The four whose
  build paths already exist. `benchmark`/`plugin` are rejected as unknown target
  keywords until their tooling lands.

```jet
packages:
  httpx: {
    targets: [
      library,
      executable { name: "httpx", entry: "src/cli.jet" },
      example { name: "probe", entry: "examples/probe.jet" },
    ]
  }
// `jet test` runs the package's #test fns; no target needed (see D-TGT5)
```

- **Option B — all six now, including `benchmark` and `plugin`.** Keywords accepted
  immediately, even though their backends are stubs.

```jet
packages:
  httpx: {
    targets: [library, benchmark { entry: "bench/throroughput.jet" }, plugin]
  }
// error: target `plugin` has no backend yet — its tooling design is unresolved.
//        Declared targets must be buildable.
```

- **Option C — library + executable only this increment.** Defer `test`/`example` too,
  matching today's two-kind surface exactly.

```jet
packages:
  httpx: { targets: [library, executable { entry: "src/cli.jet" }] }
// example { … } -> error: unknown target `example` (not in this increment)
```

**Recommendation:** A — `test` and `example` have working build paths and high
demand; `benchmark`/`plugin` correctly wait on their own designs (I8).

---

### D-TGT3 — Manifest spelling: bare keyword vs. block (rec A)

**User story.** Lena maintains a package whose `library` target needs no options at
all, but whose `executable` target needs an explicit entry module. She wants the simple
target to stay one word and only reach for a block where she actually sets a field —
the same rule U10 already gives her for `kind:`.

| Option | Zero-field target | Familiarity | Ceremony |
|---|---|---|---|
| A — bare allowed, block when fields | `library` | matches U10 `name: kind` | minimal |
| B — block always | `library {}` | new rule to learn | empty braces everywhere |

- **Option A — bare keyword allowed; block required only when fields are set.**
  Mirrors U10's ratified `name: kind` vs `name: { kind: …, … }`.

```jet
packages:
  app: {
    targets: [
      library,                                  // bare — no fields
      executable { name: "app", entry: "src/main.jet" },  // block — has fields
    ]
  }
```

- **Option B — block-only: every target is a block, even with no fields.**

```jet
packages:
  app: {
    targets: [
      library {},                               // mandatory empty block
      executable { name: "app", entry: "src/main.jet" },
    ]
  }
// library -> error: target entries must be blocks; write `library {}`
```

**Recommendation:** A — consistent with the already-ratified U10 shorthand; empty
`{}` is pure noise.

---

### D-TGT4 — Convention for default executable entry point (no rec)

**User story.** Sam writes `targets: [executable]` with no `entry:` and expects it to
just build. The owner has ruled against designs that dictate file structure, so the
real question is whether a bare `executable` is even allowed, and if so how its entry is
found — without the manifest mandating where Sam's files live.

| Option | File-structure mandate | Bare `executable` allowed | Failure mode |
|---|---|---|---|
| A — explicit `entry:` always | none | no | clear error: "specify `entry:`" |
| B — convention search | yes (fixed search paths) | yes | ambiguous match across conventions |
| C — single-root-file rule | soft (root layout) | yes, when one `.jet` at root | breaks the moment a second root file appears |

- **Option A — require `entry:` on every `executable` (no convention).** No path is ever
  assumed; a bare `executable` is rejected with a fix.

```jet
packages:
  app: { targets: [executable] }
// error: an `executable` target needs an entry module.
//   fix: executable { entry: "src/main.jet" }
```

- **Option B — bare `executable` allowed; compiler searches fixed conventions.** Tries
  e.g. `src/main.jet`, then `<package>.jet`; errors only if none or several match.

```jet
packages:
  app: { targets: [executable] }
// resolves: found src/main.jet -> entry for `executable`
// (if both src/main.jet and app.jet exist:)
// error: ambiguous entry for `executable` — src/main.jet and app.jet both match;
//        add `entry:` to disambiguate.
```

- **Option C — bare `executable` valid only when exactly one `.jet` sits at package root.**

```jet
packages:
  app: { targets: [executable] }   // valid: only app/main.jet at root
// add a second root file and:
// error: `executable` has no `entry:` and the package root has 2 `.jet` files;
//        add `entry:`.
```

**Recommendation:** none — genuine owner call. Option A is the safest (no
file-structure assumption, no ambiguity) but trades away the zero-config
convenience that B/C buy with a layout convention the owner has resisted.

---

### D-TGT5 — `test` target vs. `#test fn` (S82) (rec C)

**User story.** Dahlia has unit tests written as ratified S82 `#test` functions next
to her code, and a separate end-to-end script under `tests/`. She wants `jet test` to
just run her `#test` fns with no manifest entry, while still being able to point at
the standalone integration file when she has one.

| Option | Unit `#test` fns | Integration file | Manifest entry for units? |
|---|---|---|---|
| A — explicit test target | not auto-run | declared target | yes (required) |
| B — implicit only | auto-collected | no path for it | no |
| C — both (hybrid) | auto-collected | optional `test { entry: … }` | no |

- **Option A — separate `test` target carries everything; `#test` fns aren't auto-run.**
  Nothing runs unless a `test` target names a file.

```jet
packages:
  app: { targets: [library, test { entry: "tests/all.jet" }] }
// `jet test` runs only tests/all.jet; the #test fn below is ignored unless that
// file imports and invokes it.
#test
fn reversing_twice(xs: [Int]) { require_eq(reverse(reverse(xs)), xs) }
```

- **Option B — implicit only: `jet test` collects `#test` fns; no `test` target exists.**
  No way to point at an out-of-tree integration file as a target.

```jet
packages:
  app: { targets: [library] }
// `jet test` auto-collects every #test fn in the package — no target declared.
#test
fn reversing_twice(xs: [Int]) { require_eq(reverse(reverse(xs)), xs) }
// tests/e2e.jet -> not built (no target slot for a standalone file)
```

- **Option C — hybrid: `#test` fns auto-collected; `test { entry: … }` optional for
  out-of-tree files.** Both coexist.

```jet
packages:
  app: { targets: [library, test { entry: "tests/e2e.jet" }] }
// `jet test` runs BOTH: every #test fn in src/ (auto) AND tests/e2e.jet (declared)
#test
fn reversing_twice(xs: [Int]) { require_eq(reverse(reverse(xs)), xs) }
```

**Recommendation:** C — honors ratified S82 (unit `#test` fns always just work, no
ceremony) while still giving integration files that live outside the source tree a
declared home.

---

## Step-through debugger — board card c52


# Draft ballot cards — c52 (DAP debugger) + c25 (range arms)

> Status: draft — not yet queued in `decision-ballots.md`
> Date: 2026-06-20
> Prerequisite ratified decisions: D-DBG1 (`jet debug <file>` verb), D-OBS1 (source maps + Jet-line panics), D-RANGE1 (range arms reuse `..`), D-RANGE2 (ownership split), D-PATR (range patterns + exhaustiveness)

---


Two choices reach the owner. D-DBG1 (the `jet debug` command name) is already ratified. D-OBS1 scheduled the DAP debugger as a GA gate. The open decisions below cover the line-table artifact format and the policy for generated/library frames that have no Jet source line.

---

### D-OBS2 — Debug line-table format (rec A)

**User story.** A Zed extension author wants to write a third-party DAP adapter that reads Jet's source map and translates VS Code breakpoints to Jet lines. They need to know where to find the line table and whether it is stable enough to rely on between compiler versions.

| Option | Location | Parser complexity | Third-party stable? | Debug-build overhead |
|--------|----------|-------------------|---------------------|----------------------|
| A — inline `// jet:line` comments in generated `.rs` | Same file as codegen output | Scan lines for prefix; linear pass | Fragile: any Rust reformatter strips comments | Near-zero: comments don't affect compilation |
| B — sidecar `.jetmap` JSON file | `<file>.jetmap` beside the temp Rust file | Parse one JSON object | Stable: a versioned schema; third parties read one file | Negligible: written once per compile; schema-versioned |
| C — embed line table in the binary as a custom section | ELF/Mach-O `.jet_lines` section | Needs a binary reader (not std-only without careful scoping) | Stable at the binary level, invisible to source tools | Binary size: small table per translation unit |

- **Option A — inline `// jet:line <rust>=<jet>` comments in the generated `.rs` file.**
  The adapter scans the temp `.rs` for lines matching `// jet:line <rust>=<jet>` and builds the translation table in memory. No new file to manage.

    ```
    $ jet debug examples/features/05_loops.jet
    # adapter reads build/tmp/05_loops.rs:
    #   // jet:line 12=7
    #   // jet:line 14=8
    # translates editor breakpoint loops.jet:7 → rust line 12
    breakpoint hit  loops.jet:7  in main()
    ```

    Third-party tools that want to read the line table must parse the generated Rust source (which may be reformatted or stripped of comments by `rustfmt`). I6: std-only, trivially.

- **Option B — sidecar `.jetmap` JSON file beside the generated Rust.**
  Codegen writes `build/tmp/05_loops.jetmap` alongside `build/tmp/05_loops.rs`. Schema: `{ "version": 1, "source": "examples/features/05_loops.jet", "lines": [[12, 7], [14, 8], …] }`. The adapter reads one file. Third-party tools have a stable, schema-versioned contract.

    ```
    $ ls build/tmp/
    05_loops.rs   05_loops.jetmap

    # .jetmap content:
    { "version": 1, "source": "examples/features/05_loops.jet",
      "lines": [[12, 7], [14, 8], [16, 9]] }

    $ jet debug examples/features/05_loops.jet
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
       8 |     total += i
    locals:  n = 5   total = 0   i = 1
    ```

    I6: hand-written JSON serialization in `Source/Debug/linemap.rs`, zero crates. Codegen stays dumb (I3): records `(jet_line, rust_line)` pairs it already has; the formatter writes a small JSON object.

- **Option C — custom binary section in the compiled artifact.**
  Codegen emits DWARF-adjacent line information in a custom ELF/Mach-O section (`__jet,__lines`). The adapter reads the binary. No extra file, fully self-contained artifact.

    ```shell
    $ jet debug examples/features/05_loops.jet
    # adapter reads jet_lines section from the compiled binary
    breakpoint hit  loops.jet:7  in main()
    ```

    Requires a binary format writer and reader in `Source/Debug/` (std-only but non-trivial); platform binary format differences (ELF vs Mach-O vs PE) require per-platform branches. Viable for GA; heavyweight for a first iteration.

**Recommendation:** B. A versioned sidecar JSON is the simplest stable contract for both the built-in adapter and any third-party tools, requires no binary format work, and is trivially std-only. The schema version field future-proofs format evolution without breaking existing adapters. If the intermediate Rust files are cleaned up after compilation, the adapter reads the map during the session and the file can be discarded; the binary retains full DWARF from `rustc` for lldb's use.

---

### D-DBG2 — Policy for frames with no Jet source line (rec A)

**User story.** A developer is stepping through a Jet program that calls `core.fs.read_file(path)`. Execution steps into generated glue code or a Rust `std` function that has no Jet source line. What does the editor show?

| Option | Editor display | Beginner experience | Expert escape hatch | I2 compliance |
|--------|---------------|---------------------|---------------------|---------------|
| A — step over silently; surface only frames with a Jet line | Next Jet frame shown | Clean; no Rust noise | None | Yes — no Rust paths/lines ever surface |
| B — show a synthetic frame `[jet runtime]` with no file/line | Placeholder frame visible | Slightly noisy but honest | No detail | Yes — still no Rust identity |
| C — show the raw Rust frame (file + line) | Rust file/line in editor | Confusing; breaks I2 | Yes | No — I2 violation |

- **Option A — step over any frame that has no Jet source line; resume at the next Jet frame.**
  The adapter walks the lldb frame list, skips every frame whose Rust line does not appear in the `.jetmap` table, and surfaces only the first (innermost) Jet frame.

    ```
    # user is in main(), steps into core.fs.read_file — no Jet line
    # adapter silently steps over the generated glue
    # next stop: back in main() after the call returns
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
       9 |     total += i       ← next Jet line; glue was invisible
    locals:  n = 5   total = 5   i = 2
    ```

    Fully I2-compliant. The cost: a user cannot inspect Jet stdlib internals at the source level (they see the call complete atomically). Acceptable for v1; expert source-level stdlib debug is a post-GA concern.

- **Option B — surface a synthetic frame `[jet runtime]` at any depth with no Jet source.**
  The adapter inserts a placeholder frame when a non-Jet frame is innermost, showing a label but no file or line.

    ```
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    [jet runtime] — inside core.fs.read_file (no Jet source available)
    (jet-dbg) step
       9 |     total += i
    ```

    I2-compliant (no Rust paths). More visible about what is happening, but adds an extra step/frame the user must work through. Useful if users need to know "I am inside a runtime call."

- **Option C — show the raw Rust frame.**
  The adapter passes the lldb frame through as-is when no Jet line is found. The editor shows `src/std/fs.rs:418`.

    ```
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    /rustc/.../src/std/fs.rs:418   ← Rust path surfaced to user
    ```

    Direct I2 violation. Listed to be explicitly closed.

**Recommendation:** A. Silent step-over is the cleanest beginner experience and is the hardest I2 guarantee to weaken later. The adapter can log a debug-level trace (visible only in `jet debug --verbose`) so developers of the adapter itself can see skipped frames, while users never do. Option B is a reasonable upgrade once users report that the opaqueness is confusing.

---

---

## Allocators — board card c05

### D-ALLOC2 — What `arena.alloc(value)` returns, and how `reset`/`free` relate to outstanding allocations (rec A)

D-ALLOC1 ratified the *spelling* (`arena :: mem.Arena.new()`, `node :: arena.alloc(value)`)
and D-ALLOC-D ratified *two verbs* (`reset` keeps the buffer, `free` returns it to the OS).
Neither said what `alloc` **returns** or what the type system does when you touch an
allocation after `reset`/`free`. The c05 runtime shipped a stub where `alloc(v)` just
returns `v` — no real arena. This card picks the real return semantics. It is the load-bearing
decision: it determines whether arenas in a safe-by-default language are *statically* safe
(Rust's bar) or rely on a runtime trap, and whether Jet needs a region/lifetime story it
does not yet have.

**User story.** Priya is writing a game in Jet. Each frame she allocates thousands of
short-lived scratch objects — collision pairs, particle nodes, path segments — into a
per-frame arena, then wipes the whole thing at end-of-frame with one `reset` and reuses
the buffer next frame. She wants the cheap bulk-free that makes arenas worth using, and she
*never* wants a dangling read: if she stashes a frame-N node in a list that outlives the
`reset`, the compiler must stop her — by name, at the use site — not hand her a corrupted
read at runtime. She has never written a lifetime and does not want to start.

**Prior art (confirmed current APIs).**

- **Zig `std.heap.ArenaAllocator`** — `allocator.create(T)` returns `*T`, `alloc(T, n)` returns
  `[]T` (a real pointer/slice). `deinit()` frees everything; `reset(.retain_capacity)` rewinds
  the bump pointer but keeps the buffer. Outstanding pointers after a reset are silently
  dangling — the language does nothing; correctness is on the programmer. Fast, **not statically safe.**
- **Rust `bumpalo::Bump` / `typed-arena`** — `alloc(value)` returns `&'bump mut T`, a reference whose
  lifetime is tied to the arena's borrow. `reset(&mut self)` takes `&mut self`, which the borrow
  checker can only grant when **no** outstanding `&'bump T` references are live. Use-after-reset is
  a *compile error*, not a trap. **Statically safe — this is the gold standard, and the cost is a
  real lifetime/region in the type system.**
- **Odin `mem.Arena`** — allocations come back as `rawptr` / typed pointers via `context.allocator`;
  `free_all` / `arena_free_all` rewinds and keeps the first block. Outstanding pointers after
  `free_all` dangle; safety is by convention. **Not statically safe.**
- **C++ `std::pmr::monotonic_buffer_resource`** — `allocate()` returns `void*`; `deallocate()` is a
  no-op; `release()` (or destruction) returns everything upstream at once. Pure pointer + bulk-free,
  **no static guard at all.**
- **Jai** — temporary-storage / pool allocators give the same per-frame bump-and-reset ergonomics
  (`reset_temporary_storage`); pointer-based, manual lifetime discipline, no compiler enforcement.

**The spectrum.** One axis: *pointer + bulk-free* (Zig, Odin, C++, Jai) is trivial to implement and
maximally flexible, but the language cannot tell you when you've outlived your allocation — it's
exactly the class of bug a memory-safe language exists to delete. *Lifetime-bound reference* (Rust)
makes use-after-reset a compile error, but only because Rust pays for a borrow checker with regions.
Jet sits in between: it has ownership (S10 `take`/`view`/`mut`/`ref`) and the c06 capability model,
but **no user-facing region/lifetime mechanism today** and **no raw pointers exposed to users ever.**

| Option | `alloc(value)` returns | Static safety (use-after-reset) | reset / free model | Fits S10 / c06 ownership | Prior art |
|--------|------------------------|---------------------------------|--------------------|--------------------------|-----------|
| A scope-bound view | a `view` into arena storage, valid only inside the arena's lexical scope | **Yes** — checker forbids escaping the scope and forbids any use after `reset`/`free` | `reset`/`free` take the arena by `mut`; legal only when no live view escapes the binding | extends `view` (S10) with a region tied to the `arena ::` binding scope | Rust bumpalo / typed-arena |
| B opaque handle | an opaque `Handle<T>` (index into arena-owned storage), not a reference | **Yes** — `reset`/`free` bump a generation; sema marks every handle from the old generation dead, use names the site | `reset`/`free` invalidate all outstanding handles; arena *owns* the values | generational-index arenas (Rust `slotmap`/ECS); no new region machinery | 
| C owned clone (stub) | an owned `T` (the value, cloned into arena-managed storage if it lived there at all) | N/A — nothing dangles because nothing is shared | `reset`/`free` drop arena-owned copies; callers hold independent owned values | trivially fits — it's just `take`/owned values | current c05 stub (barely an arena) |

- **Option A — scope-bound view (à la Rust, the safe gold standard).** `alloc` returns a `view T`
  bound to the *region* introduced by the `arena ::` binding. You may read/write the allocation
  freely **inside** that scope; the checker forbids it from escaping (storing it somewhere that
  outlives the arena) and forbids any use after `reset`/`free`, which require `mut arena` and are
  only legal once no escaping view is live. This is bumpalo's `&'bump T` reworded in Jet's
  capability vocabulary — no lifetimes typed by the user, but a real region behind the scenes.

```jet
use core.mem

fn frame(world: view World) {
    arena :: mem.Arena.new(capacity: 1 << 20)

    node :: arena.alloc(CollisionPair{ a: 1, b: 2 })
    node.a += world.gravity            // ok — used inside the arena's scope

    arena.reset()                      // ok — `node` is not used after this point
}
```

```jet
fn frame(world: view World, out: mut [view CollisionPair]) {
    arena :: mem.Arena.new(capacity: 1 << 20)
    node :: arena.alloc(CollisionPair{ a: 1, b: 2 })

    out.push(node)   // error[E0631]: arena allocation escapes the arena it was allocated in
                     //  --> frame.jet:4:14
                     //   |
                     // 4 |     out.push(node)
                     //   |              ^^^^ `node` is a view into `arena`, which is freed
                     //   |                   when this function returns
                     //   = note: an arena allocation may not outlive the `arena ::` that made it
                     //   help: store an owned copy instead — `out.push(node.copy())`
}
```

```jet
fn frame() {
    arena :: mem.Arena.new()
    node :: arena.alloc(Node{ id: 7 })
    arena.reset()           // wipes the buffer
    print(node.id)          // error[E0632]: use of arena allocation after `reset`
                            //  --> frame.jet:5:11
                            //   |
                            // 4 |     arena.reset()
                            //   |     ----- `arena` reset here, invalidating `node`
                            // 5 |     print(node.id)
                            //   |           ^^^^ `node` was allocated before the reset
                            //   help: move the `reset` after the last use of `node`
}
```

- **Option B — opaque generational handle.** `alloc` returns an opaque `Handle<T>` — an
  index plus a generation, never a reference. Reads go through `arena.get(handle) -> view T`.
  `reset`/`free` bump the arena's generation; sema knows every handle minted before the bump is
  dead and reports a use-after at the access site. Handles are plain values, so they *can* be
  stored anywhere and outlive the arena — using a stale one is the error, caught statically when
  the generation is statically known, and trapped at runtime otherwise (still memory-safe: a dead
  handle never reads freed memory, it returns a checked failure). No region machinery needed.

```jet
fn frame() {
    arena :: mem.Arena.new()
    h :: arena.alloc(Node{ id: 7 })   // h : Handle<Node>
    arena.reset()                     // generation bumps
    n :: arena.get(h)                 // error[E0633]: handle invalidated by `reset`
                                      //  --> frame.jet:5:21
                                      //   |
                                      // 4 |     arena.reset()
                                      //   |     ----- generation bumped here; `h` is from the old generation
                                      // 5 |     n :: arena.get(h)
                                      //   |                    ^ `h` no longer names a live allocation
                                      //   help: re-allocate after the reset, or move the reset later
}
```

- **Option C — owned clone (the current c05 stub).** `alloc(value)` returns an owned `T`; the
  arena owns at most a managed copy. `reset`/`free` drop the arena's copies, but callers already
  hold independent owned values, so nothing can dangle — because nothing was ever shared. It is
  trivially safe and trivially fits S10 (`take`/owned), but it is **barely an arena**: there is no
  shared bump-allocated storage, every `alloc` is a clone, and `reset` frees nothing the caller
  can see. It buys none of the per-frame win Priya came for. Listed for honesty and as the
  fallback if a region story can't land this milestone.

```jet
fn frame() {
    arena :: mem.Arena.new()
    node :: arena.alloc(Node{ id: 7 })   // node : Node — an owned value, not shared
    arena.reset()                        // drops the arena's copy; `node` is untouched
    print(node.id)                       // prints 7 — no aliasing, so no use-after to catch.
                                         // also: this allocated nothing into a shared buffer.
}
```

**Recommendation:** A — scope-bound `view`. Safe-by-default (philosophy P1) means use-after-reset
must be a *compile error*, not a runtime trap or undefined behavior; only A and B clear that bar,
and only A gives the genuine shared-buffer bump allocation that makes arenas worth shipping
(B's per-access indirection and runtime fallback are a weaker, slower compromise). A is also the
design every language *lauded* for allocator safety converged on (Rust bumpalo / typed-arena).

**Interaction with c06 / S10 — and the gap this exposes.** A extends `view` (S10) but needs
something S10 does **not** have: a **region** — the lifetime of the `arena ::` binding scope — that
the checker can attach to each allocation and use to forbid escape and use-after-`reset`. That is a
new piece of the capability model. It should be specified as part of c06 (the `view` capability
already there is the natural home) or as an explicit follow-on region card; **D-ALLOC2-A cannot be
implemented until that region rule exists.** B is the escape hatch if the owner wants real arenas
*this* milestone without taking on regions — it's safe and needs no new type-system machinery, at
the cost of indirection and a runtime check on statically-unknown handles. C needs nothing new but
isn't really an arena. Recommend ratifying A as the target and sequencing the region work into c06;
fall back to B only if regions slip the milestone.
