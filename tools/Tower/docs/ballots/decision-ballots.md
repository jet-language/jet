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

> **34 open decisions across 20 cards** (incl. testing ergonomics + jet.regex), plus a deferred-ballots list and informational notes.
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

---

## Qualifier system: traits, effects & tags — board card c62

### D-QUAL1 — Organizing traits, effects & tags across three reader-split surfaces (rec A — Core D, with Roles)

This is the c62 linchpin: a single rule for where every "label" concept lives — traits, effects, value-facts, capabilities, markers, prohibitions — so each surface stays sparse and every declaration stays legible. The proposal (Variant D) routes by **who reads it**, and the hybrids are optional surfaces layered on top.

**User story.** Two people read the same `checkout` service. Priya, a feature dev, opens a function and needs to know *at a glance* what it touches (does it hit the network? the DB?) and what its types *are* (is `Receipt` serializable? can it be silently dropped?). Sam, the security owner, never reads function bodies — he needs *one* auditable place that says what the coupon plugin is allowed to do. Today these concerns would pile onto the same declaration and drown each other. D-QUAL1 picks a routing rule so each reader sees only their surface.

**The routing rule (the whole mnemonic — shape mirrors meaning).**

- **`#(…)` round parens → what it *touches*** (effects). On the signature line — a per-caller contract everyone must see.
- **`#[…]` square brackets → what it *is*** (a static *list* of tags: derives, traits, value-facts, markers). Above the declaration, for library users.
- **`module { … }` manifest → what it's *allowed* to do** (capability policy). One auditable place, for security/ops.

Round = runtime reach. Square = a static attribute list. Manifest = permissions. A beginner needs four facts: *types hold data, `#(…)` says what a function touches, `#[…]` is the tag list, the manifest walls things off.*

| Option | Effect surface | Grouping power | Beginner read | Signature-glance contract | Best when |
|---|---|---|---|---|---|
| **A Core D** | inline `#(…)` | good | clear | **yes** | sensible default |
| **B × Roles** | `#(Role)` | **highest (DRY)** | high at use-site | yes (via role) | large codebases, shared policy |
| **C × Unified block** | `#[ effects: … ]` | **highest (visual)** | high | no (look above) | declaration-heavy review |
| **D × Grammar** | `does (…)` | good | **highest** | yes | onboarding-first teams |
| **E × Type-row** | `! {net, db, log}` | good | lower | yes | effects must flow through generics |

These compose: A is the base; B/D are surface skins; C regroups everything into the bracket block. The strongest practical combo is **A + B**, with **D** as an optional grammar skin.

- **Option A — Core D (recommended).** Three surfaces, sigil-spelled. Effects inline on the signature, tag lists in `#[…]` above the declaration, capability policy in the `module { }` manifest. Inline `#fact` attaches a value-fact locally to one value.

```jet
// ── manifest: the security owner reads only this ──
module shop.checkout {
    plugins.coupon: deny(fs, db)        // policy collected here, never inline
}

// ── data: a list of tags reads like a list ──
#[
    derive(Comparable, Serialize),
]
struct Order { id: OrderId, total: Usd }

#[
    derive(Serialize),
    linear,                             // value-fact: can't be silently dropped
]
struct Receipt { order: OrderId, paid: Usd }

// ── logic ──
fn cart_total(items: [Item]) -> Usd {   // bare: pure, inferred
    items.sum(Item.price)
}

fn charge(o: take Order #unpaid) #(db) -> Receipt #paid ?   // typestate + effect

pub fn checkout(req: Request) #(net, db, log) -> Response ? {  // contract on line 1
    raw  :: req.body() #tainted         // value-fact rides the value inline
    rcpt :: charge(parse(sanitize(raw))?.order)?   // sanitize strips #tainted
    record(rcpt)?                       // MUST consume the linear receipt
    Response.ok()
}
```

- **Option B — Core D × Roles (named bundles).** Define a contract once in the manifest; wear it by name. A *role* is a named effect-set or tag-set, referenced wherever its members would go. The DRY answer for a many-route service. Cost: indirection — you open `Handler` to see what it touches (mitigate with `jet explain #(Handler)`).

```jet
module shop.checkout {
    role Handler = #(net, db, log)                            // an effect role
    role Money   = #[derive(Comparable, Serialize), linear]   // a tag role
    plugins.coupon: deny(fs, db)
}

#[ Money ]                              // expands to the tag list above
struct Receipt { order: OrderId, paid: Usd }

pub fn checkout(req: Request) #(Handler) -> Response ? { ... }   // one word = full contract
pub fn refund(req: Request)   #(Handler) -> Response ? { ... }   // change Handler once, both update
```

- **Option C — Core D × Unified labeled block.** Group *everything* — effects included — in the bracket list using labeled sections that self-route. Cost: effects leave the signature line, so a caller glances *above* the function instead of *at* it.

```jet
#[
    effects: net, db, log,
    panics:  never,
    marker:  route("/checkout"),
]
pub fn checkout(req: Request) -> Response ? { ... }

#[
    derive: Comparable, Serialize,
    facts:  linear,
]
struct Receipt { order: OrderId, paid: Usd }
```

- **Option D — Core D × Grammar keywords.** Keep the three surfaces but spell the two inline ones in English. Most readable for newcomers; effects (`does`) vs traits (`is`) are visually unmistakable. Cost: `is / does / forbids / as` are four keywords doing what one sigil family did, and reads less "systematic" to experts.

```jet
module shop.checkout {
    plugins.coupon forbids (fs, db)
}

struct Receipt is (Serialize), linear { order: OrderId, paid: Usd }

pub fn checkout(req: Request) does (net, db, log) -> Response ? {
    raw :: req.body() as tainted
    ...
}
```

- **Option E — Core D × Type-row effects.** Make the effect surface a type row on the return. Strictly more composable (effects flow through generics uniformly), but the heaviest-looking. Worth it only if effects must propagate through generic code.

```jet
pub fn checkout(req: Request) -> Response ! {net, db, log} ? {
    raw :: req.body() #tainted
    ...
}
```

**Recommendation:** **A (Core D)** as the ratified base — it keeps each surface sparse, puts the effect contract on the signature line where every caller sees it, and needs only four facts to teach. Adopt **B (Roles)** alongside it for any codebase with shared policy across many routes (the strongest practical combo is A + B). **D (Grammar)** is an optional skin to ratify later if onboarding wins over expert density; **C** and **E** are situational and can stay declined unless a review style or generic-effect-propagation need forces them.

**Interactions with ratified decisions (read before ratifying — A would amend these).**
- **D-ATTR2** ratified the multi-marker list as **bare** `#[Serialize, Comparable]` and explicitly *rejected* the Rust-literal `#[derive(…)]` form. The examples above use `#[ derive(Comparable, Serialize) ]` — ratifying D-QUAL1 as written would **reverse D-ATTR2** on that point. Decide whether tag lists keep the bare form (`#[Comparable, Serialize, linear]`) or adopt the `derive(…)`-grouped form here.
- **S60** deliberately rejected a full effects system (`pure fn` is the one ratified effect-tag). The `#(net, db, log)` effect surface **reopens S60**. D-QUAL1 is the place to decide that reopening explicitly.
- **S56 / S83** ratified user-defined derives via the external connector `~~` (`derive Point~~Serialize`); `#[…]` is for built-in derive *markers*. Keep the two distinct: `#[…]` lists markers, `~~` attaches a derive impl.
- **Manifest surface**: the `module shop.checkout { … }` block overlaps `pkg.jet` (`payload:`/`packages:`, D-JPK-FILES) and module paths (D-MOD1 uses `.`). Decide whether capability policy lives in `pkg.jet`, in an in-source `module { }` block, or both.

---

## Qualifier taxonomy (decide first) — board card c62

### D-QUAL2 — How many kinds of qualifier? (rec B)

> **Decide this before D-QUAL1.** D-QUAL2 is the taxonomy foundation; D-QUAL1
> (c62) asks *where* qualifiers live across three surfaces (signature /
> declaration / manifest), and that routing rule only makes sense once the
> owner has decided *how many distinct kinds of qualifier exist*. Ratify
> D-QUAL2 first to give D-QUAL1 a stable vocabulary to route. D-QUAL2 does not
> duplicate D-QUAL1's surface decision — it feeds it.

**User story.** Priya is a mid-level dev joining the Jet project. She reads
the spec page on "qualifiers" and finds four overlapping terms: *trait*,
*attribute*, *tag*, *effect*. She can't tell which apply to types, which to
functions, which carry methods, which are just markers. She asks a colleague
and gets a different answer. The taxonomy decision is the fix: one mental model
that lets anyone answer her question from first principles.

**How this relates to D-QUAL1.** D-QUAL1 decides the *surface* — `#(…)` for
effects, `#[…]` for tags, manifest for policy. That surface routing is sound
regardless of which taxonomy option wins here, but the *names* and *beginner
explanation* change: under Option B the one-liner is "methods → trait, no
methods → tag"; under Option A it requires four sentences; under Option C it
requires a footnote about why traits are special labels. Ratify D-QUAL2, then
confirm the D-QUAL1 vocabulary matches.

| Option | Kinds | Beginner rule | Dispatch story | Re-teaching cost |
|--------|-------|---------------|----------------|-----------------|
| A — four kinds | trait / attribute / tag / effect | four sentences, four words | each kind dispatched differently | status quo; complexity already present |
| B — two kinds | trait (has methods) / tag (no methods) | one sentence | `#Name` attaches either; traits dispatch via vtable, tags erase at runtime | one doc pass + small sema change to first-class `tag` |
| C — one kind | "label" (labels with methods = traits) | one word, one footnote | hides vtable-vs-marker distinction | maximally small vocabulary; may confuse experts |

#### How other languages do this

- **Rust** — splits the space three ways: `trait` (has methods, dispatches),
  `derive`/attribute macros (`#[derive(..)]`, compiler-driven codegen), and
  marker traits (`Send`, `Copy` — empty bodies, no methods). Beginners conflate
  all three. Takeaway: the marker-vs-method split is the real line; Jet draws it
  once as tag-vs-trait instead of leaving three half-overlapping concepts.
- **Swift** — `protocol` (methods, dispatch) vs. marker protocols
  (`@_marker`, no requirements, erased) vs. property wrappers / attributes.
  Takeaway: even Swift bolts on a separate "marker protocol" concept; Jet's tag
  *is* that, first-class, no special case.
- **Haskell** — type classes (methods) vs. empty/marker classes vs. `DataKinds`
  promoted constructors used as phantom labels. Takeaway: the "class with no
  methods" is already used as a pure label everywhere; Jet names that the tag.
- **Go** — interfaces (methods, dispatch) and struct tags (string metadata, no
  methods, read by reflection). Takeaway: Go already has exactly two kinds and a
  beginner can say which is which — proof the two-kind split is teachable.
- **Java** — interfaces (methods) vs. marker interfaces (`Serializable`, empty)
  vs. annotations (`@Override`, metadata). Takeaway: three kinds where two would
  do; the marker-interface idiom shows "no methods = pure label" is well-trodden.

Across all five, the load-bearing distinction is *has methods (dispatches) vs.
no methods (pure label, erases)*. Option B is the only one that makes that the
whole vocabulary.

- **Option A — Four kinds: trait, attribute, tag, effect.** Keep the four
  existing concepts distinct and separately named. Traits have methods and
  enable dispatch. Attributes are compiler-understood markers (`#Serialize`,
  `#Comparable`). Tags are user-attached value-facts or typestate markers
  (`#paid`, `#tainted`). Effects are runtime-reach annotations (`#(db, net)`).

```jet
// A trait: has methods, enables dispatch
trait Drawable {
    fn draw(self, canvas: mut Canvas)
}

// An attribute: compiler-understood marker on a declaration
#Serialize
struct Order { id: Int, total: Float }

// A tag: user value-fact, no methods, attaches to a value
fn charge(o: Order) -> Receipt #paid ? {
    // ...
}

// An effect: runtime-reach annotation on a signature
fn save(o: Order) #(db) -> Unit ? {
    // error if you call this from a pure context:
    // error[E0730]: `save` declares effect `#(db)`; caller must also
    //               declare `#(db)` or wrap the call in an effect region.
}
```

The problem: a beginner who asks "what's the difference between a tag and an
attribute?" has no clear answer. Both are `#Name`-shaped, neither has methods.
The distinction is implementation-level (compiler-known vs user-defined),
which the beginner doesn't have context to reason about yet. Four overlapping
concepts fight the simplicity ratchet (I8).

- **Option B — Two kinds: trait (has methods) + tag (no methods).
  (RECOMMENDED)** Collapse attributes, tags, effects, typestate markers,
  units, taint, must-use, and tool-markers into one concept: **tag**. A tag is
  any `#Name` that carries no method body. Traits are anything with at least
  one method. The `#Name { … }` scoped-region form (S82, D-ATTR1) attaches
  the same way; the region's *body* is the method-vs-no-method discriminator.
  Derives are traits (they attach method impls). Effects like `#(db)` are tags
  whose propagation rule is tracked by sema — they erase at runtime, no
  dispatch needed.

```jet
// A trait — has methods; dispatch is real
trait Drawable {
    fn draw(self, canvas: mut Canvas)
}

// Tags — no methods; erase at codegen
#[Serialize, Comparable]   // built-in tags that trigger compiler-generated impls
struct Order { id: Int, total: Float }

fn charge(o: Order #unpaid) -> Receipt #paid ? {
    // #unpaid / #paid are user-defined tags; they are typestate
    // sema checks them; codegen erases them.
    // ...
}

fn save(o: Order) -> Unit ? {
    // declared effect tag via D-QUAL1's #(…) surface:
    // #(db) is a tag that propagates along call sites
}

// Error: tag used where a trait (dispatch) is expected
fn render_all(items: [#Drawable]) {
    //               ^^^^^^^^^^
    // error[E0731]: `#Drawable` is a tag (no methods); to dispatch on it,
    //               declare it as a trait: `trait Drawable { fn draw(…) }`
    //               and use it as a type: `fn render_all(items: [Drawable])`
}
```

```jet
// Error: trying to write methods on a tag declaration
tag Comparable {
    fn compare(self, other: Self) -> Int  // error[E0732]: a `tag` may not
                                          // declare methods; use `trait`
}
```

Cost: sema gains a first-class `tag` keyword (or policy: a `#Name` with no
method body is automatically a tag). One documentation pass renames "attribute"
and "effect" to "tag" in teaching material. The codegen change is zero —
tags already erase.

- **Option C — One kind: "label" (traits are labels-with-methods).** Every
  `#Name` is a label. Traits are the subset of labels that also carry a method
  body. There is no user-facing distinction; the compiler figures it out from
  context.

```jet
// C spells everything the same; the compiler infers kind from usage
#Drawable           // label — has methods somewhere? trait. Else: tag.
#Serialize          // label — compiler-known: generates impls
#paid               // label — value-fact: typestate

fn render(item: #Drawable) {
    item.draw(canvas)  // ok if #Drawable turns out to have a `draw` method
    // but: how does the beginner know whether #Drawable will dispatch
    // or be erased? They have to look up whether #Drawable has methods.
    // The "label" term hides the most load-bearing distinction in the type system.
}
```

The failure mode: "label" is a smaller vocabulary but demands the learner
understand the vtable-vs-erase distinction anyway, just with no word for it.
The term "trait" is load-bearing in error messages, docs, and impl blocks; a
rename to "label" would break all of them.

**Recommendation:** B — "methods → trait, no methods → tag" is the shortest
rule that correctly captures the dispatch-vs-marker split. Option A is the
current reality and the source of beginner confusion. Option C erases the one
distinction that actually matters for understanding dispatch.

---

---

## Effect system — board card c66

### D-EFF1 — An effect system, expressed as tags on functions (rec B)

**User story.** Lena maintains a 200-file Jet service. A junior just landed a PR
where a function deep inside the pricing logic — a function everyone assumed was a
pure calculation — quietly grew a `core.net.fetch(...)` call to hit a currency API.
Nothing flagged it. Now the pricing path makes a network round-trip per line item
and nobody noticed until production latency spiked. Lena wants the compiler to know
which functions touch the network, the disk, the clock, the RNG — and to *stop* a
function she has declared pure from silently gaining a side effect. She does not
want to hand-annotate 200 files to get it.

| Option | Who writes effects | Failure mode it catches | Ceremony | Reopens S60? |
|--------|--------------------|-------------------------|----------|--------------|
| A — none (status quo) | nobody (`pure fn` only) | only "this `pure fn` isn't pure" | zero | no |
| B — inferred, annotate at boundaries (rec) | compiler infers; you assert/restrict | hidden effect creep, capability leaks, taint sinks | low — boundaries only | **yes** |
| C — explicit always | every function, every effect | same as B | high — the coloring tax | yes |

#### How other languages do this

- **Koka** — row-polymorphic effect *inference*: every function's type carries an
  inferred effect row (`<console,exn>`); you rarely write them, the compiler
  propagates. Takeaway: inference + propagation is the proven way to avoid the
  coloring tax — this is the model B copies.
- **Frank / Eff / Effekt** — algebraic effects with **runtime handlers**: an effect
  is *performed* and a dynamically-installed handler resumes the computation.
  Takeaway: powerful but a runtime mechanism; Jet wants none of the handler runtime
  — effects are a static fact, then erased.
- **OCaml 5** — effect handlers in the runtime (used for the scheduler/concurrency),
  but effects are **not yet tracked in the type system**. Takeaway: even a flagship
  ML shipped handlers before static checking; Jet inverts that — static checking,
  no handlers.
- **Unison** — "abilities" are typed effects (`{IO, Exception}`) checked at compile
  time and discharged by handlers. Takeaway: closest to a clean typed-effect surface
  on a function signature; Jet borrows the surface, drops the handler discharge.
- **Haskell (mtl / monad transformers)** — effects encoded as type-class
  constraints (`MonadReader`, `MonadIO`) stacked in a transformer tower. Takeaway:
  expressive but the stack is the coloring tax made manifest; Jet refuses to make
  beginners thread a monad stack.
- **Rust** — *no* effect system: `unsafe` and the `Send`/`Sync` auto-traits are the
  only coarse "capability" propagation, and async is famously a function color.
  Takeaway: the gap D-EFF1 fills — Rust users feel the missing effect layer most.

**Jet's is unlike all of these at runtime: STATIC + INFERRED + ERASED.** There is no
handler, no monad, no runtime effect value. The effect set is computed in sema,
checked against any assertion the user wrote, and then thrown away — codegen emits
plain Rust with no trace of it (I3). An effect is just a compile-time tag on a
function; `pure fn` (S60) is the empty set.

- **Option A — no effect system; keep only `pure fn`.** S60 stands as-is. The only
  thing the compiler knows is "this one function claimed purity." It cannot tell you
  what an impure function touches, cannot wall the network out of a subtree, and
  cannot back D-SCAP1 or D-TAINT1 (both need propagation).

```jet
pure fn price(items: [Item]) -> Usd {
    items.sum(Item.price)          // ok — provably pure
}

fn quote(items: [Item]) -> Usd {   // just "not pure" — touches WHAT? unknowable
    log(items.len())               // network? disk? clock? the type can't say
    price(items)
}
// A junior adds core.net.fetch(...) inside quote(). No diagnostic. Nothing to
// assert against, because there is no effect to name.
```

- **Option B — inferred effect tags; annotate at boundaries + cap regions (rec).**
  The compiler infers each function's effect set from its body and propagates it
  along calls (an effect of a callee is an effect of the caller, exactly like Koka's
  rows). You only *write* an effect to **assert** ("this function touches at most
  `#net`") or to **restrict** a region. A `pure fn` whose body gained an effect is a
  compile error. A scoped cap region `#caps(net) { … }` (S82 + D-ATTR1 marker-region
  form) bounds what the enclosed code may touch — anything outside the allowed set is
  rejected at the call site. All compile-time; erased in codegen.

```jet
pure fn price(items: [Item]) -> Usd {
    items.sum(Item.price)
}

fn quote(items: [Item]) -> Usd {   // inferred effect set: {#net} (from fetch_rate)
    rate :: fetch_rate()?          // fetch_rate is inferred #net; quote inherits it
    price(items) * rate
}

// Boundary assertion: the public entry point declares its contract on line 1.
pub fn checkout(req: Request) #(net, db) -> Response ? {
    rate :: fetch_rate()?          // #net — allowed
    save(order)?                   // #db  — allowed
    Response.ok()
}

// Restrict a region: inside here, only #net is permitted.
fn render_card(c: view Card) {
    #caps(net) {
        thumb :: fetch_image(c.url)?    // ok — #net
        write_temp(thumb)?              // error[E0701]: effect `#fs` not permitted
                                        //   in this `#caps(net)` region
                                        //  --> card.jet:4:9
                                        //   |
                                        // 4 |         write_temp(thumb)?
                                        //   |         ^^^^^^^^^^ `write_temp` touches the
                                        //   |                    disk (#fs); this region allows
                                        //   |                    only #net
                                        //   help: widen the region — `#caps(net, fs) { … }` —
                                        //         or move the write outside it
    }
}

// The bug Lena hit, now caught:
pure fn price(items: [Item]) -> Usd {
    rate :: fetch_rate()?          // error[E0702]: `pure fn price` performs effect `#net`
                                   //  --> price.jet:2:13
                                   //   |
                                   // 2 |     rate :: fetch_rate()?
                                   //   |             ^^^^^^^^^^^^ `fetch_rate` touches the
                                   //   |                          network; `price` is declared pure
                                   //   help: drop `pure`, or pass the rate in as a parameter
    items.sum(Item.price) * rate
}
```

  **Flag — this REOPENS S60.** S60 ratified `pure fn` as the *one* effect tag and
  "deliberately rejected a full effects system." Option B is that full system. It does
  not contradict `pure fn`'s spelling or meaning (purity becomes the empty effect set,
  the natural bottom of the lattice) but it *does* reverse S60's "no further effects"
  stance. The owner must reopen S60 to ratify B. **B is recommended but gated on
  resolving five sub-questions** before implementation: (1) **effect polymorphism /
  coloring** — does a higher-order fn like `map(f)` propagate `f`'s effects, and how is
  that written (Koka does it with effect-row variables; Jet needs a beginner-legible
  answer or an explicit "effects don't cross the `fn(...)` type boundary in v1"
  limitation); (2) **trait-bound interaction** — can a trait method declare/forbid
  effects, and does an `impl` have to honor it; (3) **diagnostic quality** — the
  whole value is in errors like E0701/E0702 reading well at scale; (4) **surface
  spelling** — `#net` inline tags vs. a `! {net, fs}` return-row slot (D-QUAL1's
  Option E) — pick one and pin it; (5) **overlap with D-QUAL1's `#(…)`** — these are
  the same surface and must not ship two spellings.

- **Option C — explicit effects always.** Every function annotates every effect it
  performs; no inference. This is the coloring tax in full: a one-line refactor that
  adds a `log()` call forces an effect annotation onto that function *and every
  transitive caller*, all the way up.

```jet
fn deep(x: Int) #log -> Int {      // add one log()...
    log(x)
    x + 1
}
fn mid(x: Int) #log -> Int { deep(x) }      // ...now mid must say #log...
fn top(x: Int) #log -> Int { mid(x) }       // ...and top, and its callers, forever.
// error[E0703]: `mid` calls `deep` (#log) but does not declare effect `#log`
//   help: add `#log` to mid's signature  — repeated up the entire call chain
```

**Recommendation:** B — inference kills the coloring tax (the thing that makes C
unlivable and A's `pure fn` an island), keeps `pure fn` meaningful as the empty set,
and is the only option that can carry D-SCAP1 and D-TAINT1. Ratify only after pinning
the surface spelling against D-QUAL1 and answering effect-polymorphism, and reopen
S60 explicitly.

---

---

## Scoped capabilities — board card c67

### D-SCAP1 — Lend a power, get it back: scoped capabilities (rec A)

**User story.** Devi runs a plugin host. A third-party coupon plugin needs to read
*one* config file, once, during init — and nothing else, ever. She does not want the
plugin holding a filesystem handle it can stash, pass around, or reuse after init.
She wants to hand it the power to read that file, watch it use it inside a bounded
scope, and have the power *evaporate* the instant the scope ends — the same shape as
the `#audit`/`#unsafe` gate (S58) she already trusts, and the same RAII "cleanup at
scope end" story Jet teaches for files (S63).

This is **not** the c06 D-CAP capabilities (`take`/`view`/`edit`/`share`), which are
about *value ownership* — who may read or mutate a value. D-SCAP1 is about handing
out a revocable **power** — authority to perform an effect (filesystem access,
network access) — into a scope, then taking it back. D-CAP answers "who owns this
`Player`?"; D-SCAP1 answers "is this code *allowed* to touch the disk right now?"

| Option | Capability is | Granted/revoked by | Can it be stashed & reused? | Built on |
|--------|---------------|--------------------|-----------------------------|----------|
| A — capability *value*, RAII-revoked (rec) | a first-class value you hold | granted into a scope; auto-revoked at scope end (S63) | no — escaping it is a compile error | D-EFF1 + S63 |
| B — capability = effect tag only | a `#fs` tag on a function | the `#caps(fs){ }` region (D-EFF1) | no — there's no value at all | D-EFF1 only |

#### How other languages do this

- **Object-capability model (E, Caja; Pony reference capabilities)** — authority *is*
  an unforgeable reference; you can only do what you hold a capability object for, and
  you pass it explicitly. Takeaway: the canonical "no ambient authority" model — a
  capability is a value, exactly Option A.
- **Austral** — *linear* capability values: a capability is consumed/threaded
  linearly so it can't be duplicated or leaked, and regions bound its scope.
  Takeaway: linearity is how you make "get it back" enforceable at compile time; A
  borrows the scope-bound version without requiring full linearity.
- **Java SecurityManager** — *deprecated for removal* (JEP 411, deprecated in Java 17).
  A runtime, stack-inspecting, global permission monitor that proved unmaintainable
  and was a frequent CVE source. Takeaway: the cautionary tale — ambient,
  runtime-checked, global permissions are exactly what *not* to build; D-SCAP1 is
  static and erased.
- **POSIX capabilities / Capsicum (FreeBSD)** — process-level rights: Capsicum drops a
  process into "capability mode" where it can act only through pre-granted file
  descriptors. Takeaway: OS-level proof that scoping authority to held handles works;
  Jet does it at the language level, at compile time.
- **Wasm / WASI preview 2** — a component starts with **no ambient authority** and can
  only touch resources the host explicitly grants; not providing a resource implicitly
  revokes the capability. Takeaway: the modern, mainstream endorsement of cap-based
  security as the default — Jet's `#grant(fs) { … }` scope is the same idea, in-language.

- **Option A — capability values, revoked by scope / RAII (rec).** A capability is a
  first-class value granted into a lexical scope. Inside the scope you hold it and may
  perform the effect it authorizes; at scope end it is revoked by the same RAII rule
  S63 already teaches ("when a value goes out of scope, Jet cleans it up"). Letting the
  capability *escape* — storing it somewhere that outlives the grant — is a compile
  error, so it can't be stashed and reused. Layered on D-EFF1: the capability is what
  authorizes a `#fs`/`#net` effect inside the region; without it the effect is rejected.

```jet
fn init_plugin(p: view Coupon) {
    #grant(fs) { caps ->                 // grant fs authority into this scope
        cfg :: caps.read("coupon.toml")? // ok — caps authorizes the #fs effect
        p.configure(cfg)
    }                                    // caps revoked here (RAII, S63)
    // p can no longer touch the disk — it never held a capability value
}

// Escape is rejected — the power can't outlive the grant:
fn leaky(store: mut [FsCap]) {
    #grant(fs) { caps ->
        store.push(caps)   // error[E0711]: capability `caps` escapes the `#grant` that lent it
                           //  --> plugin.jet:3:20
                           //   |
                           // 3 |         store.push(caps)
                           //   |                    ^^^^ `caps` is revoked when this `#grant`
                           //   |                         scope ends; storing it would outlive
                           //   |                         the authority
                           //   = note: a lent capability may not be stored, returned, or shared
                           //   help: do the filesystem work inside the `#grant` scope, or take a
                           //         capability parameter on a function the caller grants per-call
    }
}

// Using a #fs effect with no capability in scope is rejected at the call site:
fn sneaky(p: view Coupon) {
    cfg :: read("secret")?   // error[E0712]: `#fs` effect requires a filesystem capability
                             //   help: wrap the access in `#grant(fs) { caps -> … }`,
                             //         or take an `FsCap` parameter
}
```

- **Option B — capabilities as effect tags only (no first-class value).** No
  capability *value* exists. Authority is purely the D-EFF1 region: inside
  `#caps(fs) { … }` the `#fs` effect is permitted, outside it isn't. Simpler — it's
  just D-EFF1 — but you can't *hand* a capability to a callee as a value, can't model
  "this plugin holds an fs cap and nothing else for the duration of a call it makes,"
  and the grant/revoke is implicit in lexical nesting rather than an explicit lent
  thing.

```jet
fn init_plugin(p: view Coupon) {
    #caps(fs) {
        cfg :: read("coupon.toml")?   // ok — region permits #fs
        p.configure(cfg)
        // but: nothing to pass to p so that p itself may read one file and no more.
        // p.load() either is called inside this region (gets ALL of #fs) or outside
        // it (gets none). No middle ground, no per-call lending.
    }
}
```

**Recommendation:** A — a capability *value* is the only form that can be lent to a
callee, checked for escape, and revoked by the RAII rule users already know (S63);
it's the natural generalization of the S58 `#audit`/`#unsafe` gate from "unsafe ops"
to "any guarded power." B is strictly a subset (it's just D-EFF1's region) and can't
express per-call lending. Gated on D-EFF1 landing first.

---

---

## Units as a tag — board card c68

### D-UNIT1 — Units of measure: library newtype vs. first-class tag (rec B)

**User story.** Amara is writing a budget tracker. She has a `price: Float` and
a `tax_rate: Float`. Last week she accidentally added them and got a nonsense
total. She wants the compiler to reject `price + tax_rate` outright, and she
wants to write `9.99.usd` in a literal, not `Usd(9.99)`. She is not a type
theorist; she just wants the bug class to disappear.

**Relationship to D-DIST2 (ratified).** D-DIST2 ratified units of measure as a
*stdlib extension* riding on top of distinct types (D-DIST1: `UserId :: distinct
Int`). Option A below is exactly what D-DIST2 shipped: a `struct Usd(Float)`
distinct-type wrapper, hand-built in stdlib, with explicit constructor and
`.raw()` unwrap (D-DIST3). Option B is the *upgrade* to that ratified baseline:
instead of one hand-written distinct struct per unit, units become a
**parameterised tag** `#unit(usd)` on a numeric type, letting the compiler
generate the wrapper, enforce dimensional algebra, and make the literal syntax
natural. Ratifying B does not undo D-DIST1/D-DIST3; it adds a higher-level
surface that compiles down to the same erased newtype.

| Option | Literal syntax | Arithmetic safety | New language surface | Erase at runtime | Dimensional algebra |
|--------|---------------|-------------------|---------------------|-----------------|---------------------|
| A — library newtype (D-DIST2 as-shipped) | `Usd(9.99)` | yes — distinct types block `Usd + Eur` | none (stdlib only) | yes | manual via impl |
| B — `#unit(usd)` tag (RECOMMENDED) | `9.99.usd` | yes — tag mismatch is E0xxx | parameterised tag on numerics | yes — erases to `f64` | derived by compiler |

**How other languages do this.**

- **F# units of measure** — `[<Measure>] type usd` then `9.99<usd>`; `9.99<usd> + 8.00<eur>` is a type error; fully erased at runtime. The gold standard for compile-time, zero-cost units; Jet's erased-tag model mirrors this exactly.
- **Frink** — a runtime units language where every number carries its dimension; handles unit conversion automatically (`3 feet + 1 meter`). Jet takeaway: conversion rules should be explicit; silent conversion is the bug class we're killing.
- **Rust `uom` crate** — phantom-type dimensional system; `Length<meter, f64>`; safe but verbose; the type names leak into every signature. Jet takeaway: units should be invisible in erased code; user sees `Float`, not `Quantity<f64, UnitDef<…>>`.
- **Haskell `dimensional`** — similar to `uom`; `Quantity d a` where `d` is a type-level dimension vector. Jet takeaway: the type-level machinery should be hidden behind the tag; users declare `#unit(kg)`, the compiler does the algebra.
- **Ada dimensioned types** — `type Meters is new Float`; dimensional algebra via explicit subtype declarations; verbose but well-understood in safety-critical domains. Jet takeaway: safe-by-default is worth a little ceremony; the ceremony should be one `#unit` declaration, not a full newtype per unit.

**Options.**

- **Option A — library newtype (D-DIST2 as-shipped).** One distinct struct per unit, manually declared in stdlib or user code. Constructor is `Usd(expr)`, unwrap is `.raw()` (D-DIST3). Arithmetic only between two `Usd` values via the `#Numeric` marker (D-DIST3). No literal method. This is fully working today.

```jet
// stdlib or user declares:
Usd :: distinct Float

// user code
price: Usd :: Usd(9.99)
tax:   Float :: 0.08

total :: price + Usd(tax * price.raw())   // ok: Usd + Usd

// error case
bad :: price + tax
// error[E0127]: cannot add `Usd` and `Float`
//  --> budget.jet:7:16
//   |
// 7 |     bad :: price + tax
//   |                  ^ type mismatch: `Usd` vs `Float`
//   = note: `Usd` is a distinct type; arithmetic requires both sides to be `Usd`
//   help: wrap `tax` → `Usd(tax)`, or unwrap price → `price.raw()`

// no natural literal — must write Usd(9.99) everywhere
```

- **Option B — `#unit(usd)` tag (RECOMMENDED).** A parameterised tag attaches a unit to any numeric binding or expression. The compiler derives a distinct wrapper internally (same erasure as A), enforces unit matching in arithmetic, and exposes a method-call literal syntax `9.99.usd`.

```jet
// stdlib declares a unit family (one line per physical dimension):
#unit_family(currency) { usd, eur, gbp }

// user code
price: Float #unit(usd) :: 9.99.usd
tax:   Float            :: 0.08

tip:   Float #unit(usd) :: 1.50.usd

// ok: same unit
total :: price + tip    // Float #unit(usd)

// error case — different units
bad :: price + 8.00.eur
// error[E0128]: unit mismatch: `usd` and `eur`
//  --> budget.jet:10:18
//   |
// 10 |     bad :: price + 8.00.eur
//    |                    ^^^^^^^^ unit `eur`; expected `usd`
//    = note: `price` carries unit `usd`; you cannot add different currency units
//    help: convert explicitly — `8.00.eur.to_usd(rate)` — or use `.raw()` to strip the unit

// error case — unit + bare float
bad2 :: price + tax
// error[E0129]: cannot add `Float #unit(usd)` and bare `Float`
//  --> budget.jet:13:18
//   |
// 13 |     bad2 :: price + tax
//     |                    ^^^ bare `Float` (no unit); `price` has unit `usd`
//     help: attach the unit → `tax.usd` — or strip price → `price.raw()`
```

**Recommendation:** B — the literal syntax (`9.99.usd`), compiler-derived algebra,
and single-declaration units make this a genuine beginner-friendly safety layer
rather than a boilerplate exercise. It is the natural upgrade to D-DIST2, not a
replacement. Gated on D-QUAL2 (parameterised tags).

---

---

## Linear / must-use values — board card c69

### D-LIN1 — Money that can't leak: linear / must-use values (rec A)

**User story.** Kenji writes a payment service. A `Receipt` is proof of payment;
it must be saved to the database or returned to the caller — it must never be
silently discarded. Today `fn charge(…) -> Receipt` compiles fine even if the
caller writes `charge(order)` on a line by itself and throws the receipt away.
Kenji wants the compiler to catch that, with a clear message naming the dropped
value, every time, on every code path.

Jet already has the substrate: `take` moves a value into the callee (S10), so
values are already consumed at most once. Linear = consumed *exactly* once — the
tag adds the "at least once" half.

| Option | Prevents silent drop | Prevents copy | Cost to author | Failure mode |
|--------|---------------------|---------------|---------------|--------------|
| A — `#linear` tag (RECOMMENDED) | yes — every path must consume | yes — `#linear` implies `#no_copy` | one tag on the type; good errors | compiler error naming the unconsumed binding |
| B — `#must_use` only | warn/err on ignored *call result* | no | zero — just a tag | only catches the ignored-return case; drop in a variable is silent |

**How other languages do this.**

- **Rust (affine types / move semantics)** — values are moved, not copied, unless they implement `Copy`; a moved value cannot be used again. Affine = used *at most* once. Jet already has this via `take`. Linear = at most once *and* at least once; Rust does not enforce the "at least once" half without the `#[must_use]` attribute.
- **Austral** — true linear types: every linear value must be consumed on every code path; the compiler names the un-consumed binding at the branch where it escapes. The gold standard for "exactly-once" enforcement; Jet's `#linear` is this model.
- **Linear Haskell (`%1` multiplicity)** — function arrows annotated with a multiplicity: `f :: a %1 -> b` means `f` consumes its argument exactly once. Jet takeaway: the constraint lives on the *type*, not the *function arrow* — a `Receipt` is always linear, not "linear only when passed to certain functions."
- **Clean (uniqueness types)** — a `*a` is a *unique* value; only one reference may exist at a time; the compiler tracks aliasing. Jet takeaway: uniqueness and linearity overlap; Jet's tag is closer to linearity (must consume) than uniqueness (no aliasing), which is the weaker, more teachable property.
- **Granule / quantitative type theory** — each variable has a *grade* (how many times it may be used); `0` = erased, `1` = linear, `ω` = unrestricted. Jet takeaway: we only need grade `1`; shipping the whole grade lattice would be I8-violating over-engineering.
- **Rust `#[must_use]`** — an attribute on a type or function; the compiler *warns* (not errors) when a `must_use` value is ignored at a call site, but not when the value is bound to a variable and then the variable is dropped. Option B is exactly this; it catches 80% of the cases with zero new machinery.

**Options.**

- **Option A — `#linear` tag (RECOMMENDED).** The tag on a type means: every binding of that type must be consumed (passed to a `take` parameter, returned, or explicitly dropped via `drop(x)`) on every reachable code path. The compiler tracks consumption through branches; any path that lets a `#linear` value go out of scope silently is a compile error.

```jet
// type declaration
#[linear]
struct Receipt {
    order_id: OrderId
    amount:   Float #unit(usd)
}

// fn that produces one
fn charge(take order: Order) -> Receipt ? {
    // ... payment logic ...
    ok(Receipt { order_id: order.id, amount: order.total })
}

// correct usage — consumed via return
fn process(take order: Order) -> String ? {
    receipt :: charge(order)?
    save_to_db(take receipt)?    // take = consume
    ok("saved")
}

// error — binding created but never consumed
fn bad_process(take order: Order) -> String ? {
    receipt :: charge(order)?
    // ... forgot to save ...
    ok("done")
    // error[E0140]: linear value `receipt` is not consumed before it goes out of scope
    //  --> payment.jet:18:5
    //   |
    // 14 |     receipt :: charge(order)?
    //    |     ------- linear value bound here
    // 18 |     ok("done")
    //    |     ^^^^^^^^^^ `receipt` escapes scope unconsumed on this path
    //    = note: `Receipt` is `#linear`; it must be consumed on every path
    //    help: pass it somewhere → `save_to_db(take receipt)?`
    //          or explicitly discard → `drop(receipt)` (requires an `#audit`)
}

// error — silent drop in a branch
fn conditional(take order: Order, save: Bool) -> String ? {
    receipt :: charge(order)?
    if save {
        save_to_db(take receipt)?
    }
    // error[E0141]: linear value `receipt` not consumed on the `else` branch
    //  --> payment.jet:28:5
    //   |
    // 24 |     receipt :: charge(order)?
    //    |     ------- linear value bound here
    // 28 |     }   ← else branch here
    //    |     ^ `receipt` is consumed in the `if` arm but not the `else` arm
    //    help: add `drop(receipt)` in the else arm, or consume it unconditionally before the branch
    ok("done")
}
```

- **Option B — `#must_use` only (weaker stepping stone).** The tag causes the compiler to error when the return value of a call is immediately discarded (not bound). It does *not* enforce consumption of a bound variable.

```jet
#[must_use]
struct Receipt {
    order_id: OrderId
    amount:   Float
}

// error — ignored call result
fn bad1(take order: Order) -> String ? {
    charge(order)?       // result not bound
    // error[E0142]: value of type `Receipt` (marked `#must_use`) is ignored
    //  --> payment.jet:6:5
    //   |
    // 6 |     charge(order)?
    //   |     ^^^^^^^^^^^^^^ return value discarded
    //   help: bind it → `receipt :: charge(order)?`
    ok("done")
}

// NOT caught — variable bound then silently dropped
fn bad2(take order: Order) -> String ? {
    receipt :: charge(order)?
    // receipt never used — but #must_use only checks the call site, not the binding
    ok("done")   // compiles. bug ships.
}
```

**Recommendation:** A — the "at least once" guarantee is the entire point; Option B
only catches the most obvious case and lets the subtler one through. Implement B
first as the simpler stepping stone (it is a strict subset of A), then lift to A.
Both options are gated on D-QUAL2.

---

---

## Taint tracking — board card c70

### D-TAINT1 — Untrusted input can't reach the sink (rec A)

**User story.** Sam owns security for a Jet web service. Last quarter an injection bug
shipped because a request body flowed — through three helper functions and a struct
field — straight into a SQL string with no escaping. Code review missed it; the
dangerous path was four hops long. Sam wants the *compiler* to know that anything
derived from `req.body()` is untrusted, to keep that mark riding along through every
assignment and helper call, and to refuse to let it reach a query unless it has passed
through a blessed sanitizer first. He wants the 80% "don't let user input hit the
sink" win — not a PhD information-flow lattice.

| Option | Tracks | User writes | Spread / strip | Defer to later? |
|--------|--------|-------------|----------------|-----------------|
| A — `#tainted` tag + sanitizers (rec) | one bit: tainted or not | a `#tainted` source + blessed sanitizer fns | derive-from-tainted ⇒ tainted; sanitizer strips it | no — ship it on D-EFF1 |
| B — full information-flow control | a security lattice + declassification | levels, labels, declassify rules | lattice join on every flow | yes — IFC ballot (#30/#33) |

#### How other languages do this

- **Perl taint mode (`-T`)** — the original: data from outside the program is
  *tainted*; tainted data can't be used in `system`, `exec`, SQL, etc.; a regex
  capture is the blessed untaint. Runtime, one bit. Takeaway: the exact 80% model
  Option A copies — but Jet does it *statically*, not at runtime.
- **Ruby `$SAFE` / taint** — had a similar object-taint flag; **deprecated in 2.7 and
  removed in 3.0 with no replacement** ([Feature #16131](https://bugs.ruby-lang.org/issues/16131)).
  It was a global, runtime, mutable safe-level that proved too coarse and too easy to
  bypass. Takeaway: a runtime, global taint flag is a known failure; Jet makes taint a
  static, per-value, erased fact instead.
- **Java Checker Framework** — `@Tainted` / `@Untainted` type qualifiers checked by a
  pluggable type system at compile time; sanitizers return `@Untainted`. Takeaway: the
  static, type-qualifier version of taint — the closest precedent for A, and proof it
  works as a compile-time check.
- **Meta Pysa (Python) / Hack** — taint as static *taint analysis*: sources, sinks,
  and sanitizers declared in config; the analyzer reports any source→sink flow.
  Takeaway: at scale, taint is run as source/sink/sanitizer triples — Jet bakes that
  triple into the language (`#tainted` source, sink = effect, sanitizer fn).
- **JIF / Jif & FlowCaml** — full **information-flow control**: a security-label
  lattice, principals, and explicit *declassification* to lower a label. Takeaway:
  the heavyweight end — expressive but research-grade ceremony; this is Option B, and
  Jet defers it to a dedicated IFC ballot.

Built on D-EFF1's propagation: a "sink" is just an effect (`#db`, `#exec`, `#net`), so
"tainted value reaches a sink" is checked by the same engine that propagates effects.
The taint is a static, per-value tag, erased in codegen (I3).

- **Option A — `#tainted` tag + sanitizer functions (rec).** A value can carry a
  `#tainted` tag (attached at an untrusted source, inline per S82/D-ATTR1). The tag
  **spreads**: anything derived from a tainted value — assignment, interpolation,
  field store, function return — is tainted. A function marked a **sanitizer** is the
  one blessed way to strip it: its return is `#untainted` regardless of input. A
  tainted value reaching a sink effect is a compile error naming the sink.

```jet
fn handle(req: Request) #(db) -> Response ? {
    raw  :: req.body() #tainted         // source: untrusted, tagged inline
    name :: "user_" + raw               // derived ⇒ still #tainted (spreads)

    save(name)?                         // error[E0721]: tainted value reaches a `#db` sink
                                        //  --> handler.jet:4:10
                                        //   |
                                        // 4 |     save(name)?
                                        //   |          ^^^^ `name` is derived from `req.body()`
                                        //   |               (untrusted) and has not been sanitized
                                        //   = note: `save` performs `#db`, a taint sink
                                        //   help: pass it through a sanitizer first —
                                        //         `save(escape_sql(name))?`
    Response.ok()
}

// A blessed sanitizer strips the tag:
sanitizer fn escape_sql(s: String #tainted) -> String {   // takes tainted, returns clean
    s.replace("'", "''")                                  // return is #untainted by contract
}

fn ok(req: Request) #(db) -> Response ? {
    raw   :: req.body() #tainted
    clean :: escape_sql(raw)            // #untainted — tag stripped
    save(clean)?                        // ok — sink sees only sanitized data
    Response.ok()
}
```

- **Option B — full information-flow control.** A security-label lattice (levels,
  principals), label-join on every flow, and explicit declassification to lower a
  label — JIF/FlowCaml-grade. Strictly more powerful (handles confidentiality *and*
  integrity, multiple trust levels, implicit-flow leaks through control flow) but
  research-grade ceremony for the common case, and a large type-system commitment.

```jet
fn handle(req: Request #label(Untrusted)) -> Response ? {
    raw :: req.body()                   // label: Untrusted
    // every binding carries a lattice label; branching on a secret taints the
    // branch (implicit flow); lowering requires an explicit, audited declassify:
    clean :: declassify(escape_sql(raw), to: Trusted, because: "SQL-escaped")
    save(clean)?
    Response.ok()
}
// Powerful, but: lattices, principals, implicit-flow tracking, and declassify
// ceremony on the 80% case that one #tainted bit already covers.
```

**Recommendation:** A — one bit (tainted / not) plus blessed sanitizers covers the
injection-class bug Sam actually ships, rides D-EFF1's existing propagation for free,
and stays beginner-legible. Full IFC (B) is real and worth its own future ballot
(#30/#33), but it is the wrong altitude for v1 (I8 simplicity ratchet). Gated on
D-EFF1 landing first.

---

**Sources** (prior-art confirmation):
- Ruby $SAFE/taint removal — [Feature #16131: Remove $SAFE, taint and trust](https://bugs.ruby-lang.org/issues/16131), [Ruby 3.0 changes](https://rubyreferences.github.io/rubychanges/3.0.html)
- WASI preview 2 capability model — [Capabilities-Based Security with WASI](https://marcokuoni.ch/blog/15_capabilities_based_security/), [Bytecode Alliance — WASI 0.2 Launched](https://bytecodealliance.org/articles/WASI-0.2)

<!-- value-tags cluster: D-UNIT1, D-LIN1, D-STATE1 -->

# Value-tag cluster — draft ballot cards

Three cards. All assume tags are first-class (gated on D-QUAL2 from the c62
qualifier-taxonomy work). UNIT1 and LIN1 are lower complexity; STATE1 is
mid-pack. None of these cards should be ratified before D-QUAL2 settles the
tag-vs-effect-vs-trait routing rule.

**Dependency note (applies to all three cards):** D-QUAL1 (c62) proposed the
taxonomy: a *tag* is a label without methods, written `#[Tag]` on a declaration
or `#Tag` inline on a value. D-QUAL2 is the ballot that ratifies whether tags
are first-class in the language at all and what surface they live on. D-UNIT1,
D-LIN1, and D-STATE1 are built on top of that; treat them as "ratify D-QUAL2
first, then decide these in any order."

---

---

## Typestate — board card c71

### D-STATE1 — Order-of-events types: typestate (rec A)

**User story.** Fatima writes an e-commerce checkout. The invariant is: an `Order`
must be *charged* before it can be *shipped*. Today she enforces this with a
`require(order.is_charged)` at the top of `ship()` — a runtime check that fires in
production, not at compile time. She wants `ship(order)` to be a compile error
unless `order` has passed through `charge()`. She does not want to read a research
paper to achieve this.

Typestate = a tag that changes as a value moves through its lifecycle. A function
consumes one tag-state and returns the next. The tag lives only in sema; it erases
completely at runtime (no vtable, no enum discriminant, no overhead).

| Option | Compile-time guarantee | Runtime cost | Author ceremony | Failure error |
|--------|----------------------|-------------|-----------------|---------------|
| A — transitioning tags (RECOMMENDED) | yes — wrong-state call is a compile error | zero — tags erase | declare states + transitions; write `#[State]` on return type | clear: "expected `#charged`, found `#pending`" |
| B — runtime `require(…)` only | no — wrong-state call panics at runtime | `require` overhead | none | panic in production; message is a string |

**How other languages do this.**

- **Plaid** — the typestate research language; methods carry pre/post state annotations; the type checker verifies transitions; objects live in exactly one state at any time. The academic source for most of what Jet's D-STATE1 proposes; Jet simplifies by routing state through tags rather than separate type declarations.
- **Rust typestate pattern (phantom types)** — a common Rust idiom: `struct Connection<S>(PhantomData<S>)`; `fn open(c: Connection<Closed>) -> Connection<Open>`; state changes force a new type. Correct, but phantom types are invisible boilerplate and the pattern requires careful hand-threading. Jet's tag approach achieves the same guarantee without any phantom-type machinery.
- **Austral** — linear types enforce state protocols: a `Connection` value must be explicitly transitioned; the old state is consumed, the new one produced. Jet takeaway: consuming the old tag-state (`take`) and returning the new one is the right model; it maps directly onto Jet's `take` ownership keyword (S10).
- **Session types (process calculi / Haskell `session-types`)** — encode communication protocols in the type system; a channel has a type that steps with each send/receive. Jet takeaway: the session-types insight is that protocols are sequences of typed operations — typestate is exactly that idea applied to values, not channels.
- **TypeScript discriminated-union state machines** — `type Order = { status: "pending" } | { status: "charged" } | { status: "shipped" }`; a `ship` function takes only the `charged` variant. Works, but the state is a runtime enum discriminant (nonzero cost); the narrowing is done by the type checker reading the `status` field. Jet takeaway: the tag-based model achieves the same narrowing with zero runtime cost because the tag erases.
- **Ada/SPARK (pre/post conditions)** — `Pre => Order.Status = Charged`; verified statically by SPARK's prover. Jet takeaway: SPARK proves preconditions but the precondition is still a runtime value (`Status`); Jet's tag is stronger because the state is the type itself — there is no runtime field to check.

**Options.**

- **Option A — typestate via transitioning tags (RECOMMENDED).** States are tags. A function that *transitions* a value from one state to another takes the old state (consuming the value via `take`) and returns the new state. The tag on a binding tracks which state it is currently in; a call that requires a different state is a compile error naming the mismatch.

```jet
// state tags — plain tags, no methods
// (declared as tag constants; exact declaration syntax gated on D-QUAL2)
#tag Pending
#tag Charged
#tag Shipped

struct Order {
    id:    OrderId
    total: Float #unit(usd)
}

// transition: Pending → Charged
fn charge(take order: Order #[Pending]) -> Order #[Charged] ? {
    // ... call payment processor ...
    ok(order)          // order is returned with the Charged tag
}

// transition: Charged → Shipped
fn ship(take order: Order #[Charged]) -> Order #[Shipped] ? {
    // ... dispatch courier ...
    ok(order)
}

// correct lifecycle
fn checkout(take order: Order #[Pending]) -> String ? {
    charged  :: charge(order)?    // order: Order #[Charged]
    shipped  :: ship(charged)?    // charged: Order #[Shipped]
    ok("shipped: {shipped.id}")
}

// error — skipping charge
fn bad_checkout(take order: Order #[Pending]) -> String ? {
    ship(order)?
    // error[E0150]: state mismatch
    //  --> checkout.jet:23:5
    //   |
    // 23 |     ship(order)?
    //    |          ^^^^^ expected `Order #[Charged]`, found `Order #[Pending]`
    //    = note: `ship` requires the order to be in state `#[Charged]`
    //    = note: `order` is currently in state `#[Pending]`
    //    help: call `charge(order)` first to transition to `#[Charged]`
    ok("done")
}

// error — using the old binding after transition
fn stale(take order: Order #[Pending]) -> String ? {
    charged :: charge(order)?
    ship(order)?         // `order` was moved into `charge`; this is the old binding
    // error[E0031]: use of moved value `order`
    //  --> checkout.jet:35:10
    //   |
    // 33 |     charged :: charge(order)?
    //    |                       ----- `order` moved here
    // 35 |     ship(order)?
    //    |          ^^^^^ value used after move
    //    help: use `charged` (the transitioned value) instead
    ok("done")
}
```

- **Option B — runtime `require(…)` only.** No language change. The author adds a precondition check at the top of `ship`; the compiler does not enforce it.

```jet
struct Order {
    id:      OrderId
    total:   Float
    charged: Bool     // runtime flag — the thing typestate replaces
}

fn ship(view order: Order) -> String ? {
    require(order.charged, "order must be charged before shipping")
    // ... dispatch courier ...
    ok("shipped")
}

// compiles fine — crashes at runtime
fn bad_checkout(take order: Order) -> String ? {
    ship(view order)?    // order.charged is false — panic at runtime:
    // thread 'main' panicked at checkout.jet:10:
    // order must be charged before shipping
    ok("done")
}

// the bug class is alive; the compiler never sees it
```

**Recommendation:** A — the whole value of typestate is moving the bug class from
runtime to compile time; Option B is the status quo that typestate exists to
replace. Option B is listed only to make explicit what "no decision" means in
practice. Complexity sequencing: implement `#linear` (D-LIN1 Option A) first since
it exercises the same "track a tag on a value across branches" machinery; typestate
then adds the "tag changes on transition" layer on top. Both are gated on D-QUAL2.

<!-- foundation+misc: D-QUAL2, D-TXN1, D-MIGRATE1 + deferred -->

# Draft ballot cards — D-QUAL2, D-TXN1, D-MIGRATE1 + deferred stubs

> Status: draft — not yet promoted to `decision-ballots.md`.
> Date: 2026-06-20
>
> **Read order for owner:** D-QUAL2 first (foundational taxonomy); D-QUAL1
> (already in the open queue, board card c62) builds on whatever D-QUAL2
> ratifies and should be re-read in that light. D-TXN1 and D-MIGRATE1 are
> independent.

---

---

## Scoped transactions — board card c72

### D-TXN1 — Rollback semantics for `#transact { }` (rec A)

**User story.** Kai is writing a game action system. A single `use_ability`
call must spend stamina, apply a cooldown, and damage the target — or do none
of those things if any step fails. Today he writes a ladder of manual rollback
calls after each `?`. He misses one. The bug ships. He wants the compiler to
guarantee that a failed sequence is cleanly unwound without him hand-writing
the ladder.

> **Note on syntax.** The `#transact { }` scoped-region syntax is **already
> ratified** (S82 / D-ATTR1: `#Marker { }` is the scoped-effect form). This
> decision is about **rollback semantics** — what `#transact { }` actually
> does when a `?` propagates — not syntax. Do not re-open the surface.

| Option | What rolls back | Who writes rollback logic | Honest about limits? | After D-EFF1? |
|--------|----------------|--------------------------|----------------------|---------------|
| A — trait-declared rollback | types that impl `Rollback` | the type author, once | yes — only types that know how | natural sequencing (after D-EFF1) |
| B — library-only compensation | nothing (caller hand-writes) | every caller | technically honest; no language help | independent; always available |

- **Option A — `#transact { }` over types that declare `Rollback`. (RECOMMENDED)**
  A type opts into the transaction protocol by implementing the `Rollback`
  trait. Inside a `#transact { }` block, every `?`-failure triggers the
  reverse sequence: each step's `rollback` method is called in reverse order
  on the values that were mutated. On clean exit (no `?` propagation), the
  transaction commits — no rollback needed. The compiler tracks which values
  were mutated inside the block and synthesizes the reverse-call chain.

  This is honest: only operations on types that declare a rollback are
  covered. If you use a type that doesn't implement `Rollback` inside the
  block, sema tells you.

```jet
trait Rollback {
    fn rollback(mut self)
}

struct Stamina { current: Int, reserved: Int }

impl Stamina: Rollback {
    fn rollback(mut self) {
        self.current += self.reserved
        self.reserved = 0
    }
}

struct Cooldown { active: Bool }

impl Cooldown: Rollback {
    fn rollback(mut self) {
        self.active = false
    }
}

fn use_ability(player: mut Player, target: mut Enemy) -> Unit ? {
    #transact {
        player.stamina.spend(10)?   // if this fails: nothing to roll back yet
        player.cooldown.apply()?    // if this fails: rolls back stamina.spend
        target.hp.damage(25)?       // if this fails: rolls back cooldown + stamina
    }
    // all three succeeded — committed, no rollback
}
```

```jet
// Error: using a non-Rollback type inside #transact
struct Logger { entries: [String] }
// Logger does not impl Rollback

fn risky(logger: mut Logger) -> Unit ? {
    #transact {
        logger.entries.push("started")?
        //             ^^^^
        // error[E0801]: `Logger` does not implement `Rollback`; mutations
        //               inside `#transact` must be reversible.
        //   fix: impl Logger: Rollback { fn rollback(mut self) { … } }
        //        or move `logger.entries.push` outside the `#transact` block.
    }
}
```

  Natural sequencing note: `#transact` is an effect region (S82). After
  D-EFF1 ratifies the full effects model, rollback becomes a named effect
  that propagates through call sites like any other. Ratify A now as the
  semantic contract; the effect-system wiring follows D-EFF1.

- **Option B — Library-only manual compensation.** No language change. Every
  caller hand-writes the rollback ladder using `??` fallback arms. The `#transact`
  syntax is not used for rollback; it could still be used for other region
  semantics (locking, tracing), but rollback is purely caller responsibility.

```jet
fn use_ability(player: mut Player, target: mut Enemy) -> Unit ? {
    // hand-written compensation ladder — no language help
    player.stamina.spend(10) ?? {
        return err(Error.message("stamina failed"))
    }
    player.cooldown.apply() ?? {
        player.stamina.rollback()         // caller must remember this
        return err(Error.message("cooldown failed"))
    }
    target.hp.damage(25) ?? {
        player.cooldown.rollback()        // and this
        player.stamina.rollback()         // and this
        return err(Error.message("damage failed"))
    }
    return ok(())
}

// A new teammate adds a fourth step and forgets the rollback:
fn use_ability_v2(player: mut Player, target: mut Enemy) -> Unit ? {
    player.stamina.spend(10) ?? { return err(Error.message("stamina")) }
    player.cooldown.apply()  ?? { player.stamina.rollback(); return err(…) }
    target.hp.damage(25)     ?? { player.cooldown.rollback(); player.stamina.rollback(); return err(…) }
    emit_sound(player.sfx)?
    // no rollback for emit_sound — partial success shipped silently
}
```

  Zero language change. Leak-by-omission is the exact failure mode Option B
  accepts: every new step is a rollback the caller might forget.

**How other languages do this.**

| Language | Mechanism | Jet takeaway |
|----------|-----------|-------------|
| Haskell STM (`stm`) | `atomically` block over `TVar`s; the runtime retries on conflict; no partial state ever visible | Jet doesn't have shared mutable state across tasks (S53 deferred); STM's retry loop doesn't apply, but the "all-or-nothing block" idea does |
| Clojure `dosync` / refs | Software transactional memory; `alter`/`ref-set` inside a `dosync` block; retries on conflict | Same as STM — the retry model is for concurrent shared state; Jet's `#transact` is single-threaded sequential undo |
| Database ACID transactions | BEGIN / COMMIT / ROLLBACK; the DB engine tracks the undo log automatically | Jet's Option A is the same contract at the language level: each type declares its own undo; the block synthesizes the ROLLBACK call sequence |
| Saga pattern (microservices) | Each step publishes a compensating action; a saga orchestrator calls compensations in reverse on failure | Option A is a local, synchronous Saga: the `Rollback` trait *is* the compensating action; `#transact` *is* the orchestrator |
| Temporal Workflows | Compensations written as separate activities; the framework calls them on failure | More infrastructure, same idea; Jet's version is zero-framework, compiler-synthesized |

Jet's Option A is unusually explicit: types opt in, the rollback logic is
type-authored and auditable, and the compiler synthesizes the call sequence.
There is no hidden retry, no global undo log, and no runtime overhead outside
the `Rollback` calls themselves.

**Recommendation:** A — `#transact { }` over `Rollback`-implementing types.
The honesty is a feature: only operations whose authors have declared a
rollback are covered; everything else is a compile error telling you what to
fix. Option B is the status quo and the source of the bug Kai hit.

---

---

## Safe schema changes — board card c73

### D-MIGRATE1 — Compile-time enforcement of breaking data-shape changes (rec A)

**User story.** Dev team at a Jet shop ships a library with a public `UserRecord`
struct. Three months later, someone renames a field. Every consumer silently
recompiles, gets default-zero for the missing field, and ships corrupted data to
production before anyone notices. Sam, the library author, wants the compiler to
refuse the rename until he writes an explicit migration — the same guarantee a
database gives when you try to drop a column.

| Option | When is the break caught? | Who writes conversion? | Ignorable? | Needs recorded shape? |
|--------|--------------------------|----------------------|------------|----------------------|
| A — compile-time enforcement + conversion library | at compile time of the library change | the library author | no — it's a compile error | yes — a published shape must be snapshotted |
| B — lint/warn only | at compile time, advisory | nobody required to | yes — warnings are ignorable | no |

- **Option A — Compile-time enforcement: the CHECK is core; conversion is the
  Build-tier versioning library (#11). (RECOMMENDED)** When a type is marked
  `#[PublishedSchema]` (or equivalent), the compiler snapshots its field layout
  at release time and stores it alongside the package (in `.jet/cache/` or
  embedded in the artifact). On the next build, if the shape has changed in a
  breaking way (field removed, type changed, field renamed without migration),
  sema emits **E0901** naming the field and the published version. The author
  must either write a migration (using the Build-tier versioning library) or
  explicitly bump the major version with a breaking-change marker.

```jet
// pkg.jet — published type, shape is snapshotted at release
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    name: String,
}

// Later: rename `name` → `display_name` without a migration:
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,   // renamed from `name`
}
// error[E0901]: breaking change to published schema `UserRecord` (v0.4.0)
//   field `name: String` removed; `display_name: String` added
//   Consumers reading v0.4.0 data will get a missing-field error at runtime.
//   Options:
//     1. Write a migration: `migration UserRecord { rename name -> display_name }`
//     2. Bump the major version and mark this as a breaking release.
//     3. Keep the old field and deprecate it.
```

```jet
// With a migration — the compiler accepts the change:
migration UserRecord {
    rename name -> display_name
}

#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,
}
// compiles: the migration tells consumers how to upgrade v0.4.0 → v0.5.0 data.
```

```jet
// Consumer side — reading old data with the new shape:
record :: UserRecord.from_v040(raw_bytes)?
// the versioning library generates `from_v040` from the migration chain;
// up/down conversion is the Build-tier library's job, not the compiler's.
```

  The compiler's job is the **check** — refuse a breaking shape change without
  a declared migration. The conversion functions (`from_v040`, `to_v040`) are
  generated by the Build-tier versioning library (#11), not by `sema` or
  `codegen` directly. This keeps codegen dumb (I3) and gives the library room
  to handle complex cases (field reorder, type coercion, default injection)
  without adding new compiler machinery for each.

- **Option B — Lint/warn only.** The compiler notices the structural change
  and emits a warning, but the build does not fail. A `jet fix` or `--allow`
  suppresses it.

```jet
// Same rename as above, no migration:
#PublishedSchema
struct UserRecord {
    id: Int,
    email: String,
    display_name: String,
}
// warning[W0901]: breaking change to published schema `UserRecord` (v0.4.0)
//   field `name: String` removed; `display_name: String` added
//   (use --allow schema-break to suppress)
```

  Warnings are suppressible. The one time you most want an unbreakable
  guarantee — public wire formats — is the one time a warning is ignored under
  release deadline pressure. Option B is the database world's equivalent of a
  migration framework you can opt out of: it exists, and the bugs still ship.

**How other languages do this.**

| Language | Mechanism | Jet takeaway |
|----------|-----------|-------------|
| Protocol Buffers | Field numbers + `reserved` keyword; removing a field number is a protocol error at decode time | Runtime check, not compile-time; Jet catches it earlier. The "number-is-identity, name-is-docs" rule is worth considering for the migration syntax |
| Apache Avro | Reader/writer schema resolution at decode time; missing optional fields get defaults | Runtime resolution, not compile-time; Jet's check is stronger. Avro's reader/writer schema pair is the direct analog of Jet's published-vs-current shape diff |
| Rust + serde | No built-in schema versioning; authors use `#[serde(rename = "…")]` and hope; `serde_versioning` crates exist but are optional | Jet enforces what Rust leaves as convention; the `migration` block is the `#[serde(rename)]` that the compiler requires |
| Elm records | The compiler checks record type compatibility structurally; a renamed field is a type error at every call site, found immediately | Elm catches breaks locally (within a codebase); Jet's `#PublishedSchema` catches breaks at the library boundary, where Elm's type system stops |
| Flyway / Alembic (database migration frameworks) | Migration scripts versioned and applied in order; the framework refuses to run if migrations are missing | The exact model Jet's option A adopts at the language level: migration = required, ordered, tracked. Jet makes it a compile error; Flyway makes it a deploy error |
| Ecto migrations (Elixir) | Migrations are first-class modules; `mix ecto.migrate` fails if the schema is ahead of migrations | Same as Flyway; Jet's version is language-native, not a deploy-time CLI |

Jet's Option A is the strongest guarantee in this table: it is a **compile
error**, not a runtime decode error (Avro), a type error within a codebase
(Elm), or a deploy-time failure (Flyway/Ecto). That strength comes at a cost:
a published shape must be snapshotted and stored so the compiler can diff it.
The `.jet/cache/` store is the natural home.

**Recommendation:** A — compile-time enforcement is the only form of this
guarantee that cannot be silenced by a deadline. The conversion library (#11)
handles up/down migration logic without burdening the compiler; sema's job is
exactly the check (I3). Option B is a lint, and lints get suppressed.

---

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* Generalizes D-TAINT1 (taint tracking) and requires the full
effect/tag propagation model from D-EFF1 and D-QUAL1 to be ratified first;
the compliance dimension (what counts as a legal sink) is a policy question
that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## Smart Context — board card c74

### D-CTX1 — Smart Context: an implicit allocator+logger bundle threaded through every call (rec A2 + Cβ)

There are **two coupled questions** here. The owner must answer both:

- **Q1 (the S58 question, dominant):** does an implicit context *replace*, *complement*,
  or get *rejected* against the ratified explicit-allocator stance?
- **Q2 (syntax):** if it ships, how is a per-block swap spelled?

**User story.** Mia, four weeks into programming, writes a program that builds a big list
of records and prints a report. She never types the word "allocator" — she has never
heard it. Her code runs, is memory-safe, and frees everything at scope end. Later, Dev, an
embedded engineer on the same team, needs that exact report-building function to run
against a fixed 64 KB arena with no heap and a silent logger — *without editing the
function*. He wraps the call in one block that swaps the context, and every allocation and
log inside (including in library code he didn't write) reroutes. Mia's source is
untouched and still reads like nothing happened. That "swap once at the top, everything
downstream follows, restores on exit" is the whole feature — and it is exactly the power
S58 deliberately made *explicit and visible* instead.

#### The S58 tension (read this first)

S58 ratified, verbatim: *"explicit Zig-style allocators — allocating APIs take an
allocator parameter; a fixed arena works on embedded."* D-ALLOC1 then ratified the
spelling `arena :: mem.Arena.new()` / `node :: arena.alloc(value)`. The whole point of
that line was **the allocator is a visible parameter you pass**.

Smart Context is the opposite move: the allocator becomes an **implicit, invisible**
value threaded through the call graph, so `alloc` finds it without anyone passing it. That
is genuinely useful (it is *why* beginners never see memory), but it **partly reverses
S58's explicit stance**. The two designs answer the same question — "where does an
allocating function get its allocator?" — with opposite answers. They cannot both be the
default. So Q1 is not a nicety; it is the gate.

| | Where `alloc` gets its allocator | Beginner sees | Expert control | S58 status |
|---|---|---|---|---|
| **S58 today (explicit)** | a parameter the caller passes | passes/sees the allocator (or a defaulted one) | total, local, visible | as ratified |
| **Context replace** | the implicit context, always | nothing | swap a block | **reverses S58** |
| **Context complement** | context **unless** an explicit allocator is passed | nothing | pass param *or* swap block | **extends S58, keeps it valid** |
| **Reject** | a parameter the caller passes | the allocator | total, local, visible | unchanged |

#### How other languages do this

- **Jai — `context` + `push_context`.** A hidden `context` (allocator, logger, …) is
  passed into every call; `push_context new_ctx { … }` swaps it for a block and restores
  on exit; library code transparently picks up the new allocator. This is the direct
  ancestor of the proposal. *Jet takeaway:* this is exactly the ergonomic we want — adopt
  the block-scoped swap-and-restore shape.
- **Odin — implicit `context`.** Every scope has an implicit `context` passed by pointer
  on each Odin-convention call; `new(T)` uses `context.allocator` unless overridden;
  **copy-on-write** so a callee can't back-propagate a bad context to the caller. Built for
  *intercepting third-party code's* allocation/logging. *Jet takeaway:* steal the
  copy-on-write / per-scope-local guarantee — a swap inside a block must never leak
  outward, which also gives us the auto-restore for free.
- **Go — `context.Context` (explicit, the contrast case).** Go threads context as an
  *explicit first parameter* (`func F(ctx context.Context, …)`) and the community treats
  invisible/implicit context as an anti-pattern. *Jet takeaway:* this is the cautionary
  twin — Go chose visibility and ceremony on purpose; it shows the cost of *not* hiding it
  (every signature grows a param) and the benefit (no magic). It is essentially "the
  Reject option, productized."
- **Scheme / Racket — `parameterize`.** Dynamic parameters (`make-parameter`) hold values
  looked up dynamically; `(parameterize ([p v]) body)` rebinds for the dynamic extent of
  `body` and restores after. *Jet takeaway:* the precise semantics we want are
  *dynamic-extent* rebinding, not lexical — proves the swap-restore model is a 40-year-old,
  well-understood construct, not a novelty.
- **Thread-locals (C/C++/Rust `thread_local!`).** A per-thread global the callee reads
  without a parameter. *Jet takeaway:* the likely **codegen** substrate for the implicit
  value — but a leaky mental model for users, so it stays a backend detail and is *never*
  surfaced (mirrors S58's "onboarding never mentions any of it").

#### Q1 options — REPLACE vs COMPLEMENT vs REJECT (the S58 interaction)

- **Option A1 — Replace.** Context becomes *the* way allocators are found; S58's
  explicit-parameter line is superseded. Allocating APIs no longer take an allocator
  parameter — they read the context.

  ```jet
  // Library function — note: NO allocator parameter anymore.
  fn build_report(rows: [Row]) -> [Line] {
      out :: []                 // allocates from the implicit context
      loop r in rows { out.push(format(r)) }
      out
  }
  ```

  Pro: maximally beginner-clean, one mechanism. Con: **directly reverses S58 and
  D-ALLOC1's "alloc is a visible method on a named arena"** — the embedded story
  "a fixed arena works because you pass it" evaporates into invisible threading; experts
  lose the local, visible control S58 promised. Violates the simplicity ratchet by
  *removing* an already-shipped explicit path.

- **Option A2 — Complement (recommended).** Explicit S58/D-ALLOC1 allocator-passing stays
  exactly as ratified and **wins when present**; the implicit context is only the
  **default used when no allocator is passed explicitly**. Nothing about S58 is reversed —
  context fills the hole S58 already had (beginners weren't passing allocators anyway; the
  default heap allocator simply *becomes nameable and swappable*).

  ```jet
  arena :: mem.Arena.new(capacity: 65536)   // S58 / D-ALLOC1, unchanged

  // Explicit wins — exactly S58 today:
  node :: arena.alloc(value)

  // Implicit default — beginner path, fed by the context:
  list :: []                                // uses context.allocator

  // Expert swaps the *default* for a block; explicit calls still override locally:
  using context.allocator = arena {
      report :: build_report(rows)          // build_report's internal allocs -> arena
  }                                         // context restored here
  ```

  Pro: **add-only** — S58 and D-ALLOC1 keep their exact meaning; the explicit parameter is
  still the override and still the embedded story. Beginners get the magic; experts get
  *both* knobs (pass a param for one call, swap the block for a subtree). Con: two ways to
  pick an allocator coexist (mitigated: explicit always wins, one precedence rule, easy to
  teach — "passed beats ambient").

- **Option A3 — Reject.** No implicit context. Allocators stay strictly explicit (S58 as
  is). Loggers, if wanted, are an ordinary passed value or a plain module-level function.

  ```jet
  fn build_report(rows: [Row], in: mem.Allocator) -> [Line] { … }   // S58 forever
  ```

  Pro: zero new magic, S58 fully intact, simplicity ratchet satisfied by *not* adding a
  feature. Con: the beginner "never see an allocator" story still leans on a single hidden
  default that nobody can swap; no clean seam to later carry capabilities/effects (c06 /
  D-EFF1) — we'd reinvent this carrier when effects land.

#### Q2 options — per-block swap syntax (only if A1 or A2 wins)

- **Option Cα — `using context.allocator = arena { … }`.** Jai/`using`-flavored; reads as
  prose, names the exact field being swapped.

  ```jet
  using context.allocator = arena {
      report :: build_report(rows)          // arena is the ambient allocator in here
      log.info("built {report.len()} lines") // still the outer logger
  }                                          // allocator auto-restored on exit
  ```

  Con: `using` collides conceptually with S62's rejected "Jai-style `using` member
  injection" — reusing the word the owner already declined elsewhere is a trap.

- **Option Cβ — `#context(allocator = arena) { … }` (recommended).** A `#` marker block
  (consistent with D-ATTR1's `#unsafe`/`#audit`), naming swapped fields as `field = value`;
  multiple fields comma-separated (D-ATTR2 list feel). Auto-restores on block exit.

  ```jet
  silent :: log.Silent.new()
  #context(allocator = arena, logger = silent) {
      report :: build_report(rows)          // arena + silent logger flow downstream
  }                                          // BOTH fields restored here
  ```

  Pro: rides the **already-ratified `#` marker grammar** — no new top-level keyword, no
  collision with `using` (S62) or `use` (S16); the marker form signals "compiler-managed,
  scoped" exactly like `#unsafe`. Con: a `#(…)` block is a slightly heavier read than bare
  `using`.

- **Option Cγ — `push_context my_ctx { … }`.** Jai-literal: build a whole context value,
  push it.

  ```jet
  my_ctx :: context.with(allocator = arena, logger = silent)
  push_context my_ctx { report :: build_report(rows) }
  ```

  Pro: closest to the prior art, swaps the whole bundle at once. Con: a new top-level
  keyword (`push_context`) for a niche expert op — fails the keyword-budget bar; forces
  users to name a context value even for a one-field swap.

#### Recommendation

**A2 (Complement) + Cβ (`#context(…) { … }`).** A2 is the only option that **does not
reverse a ratified call**: S58 and D-ALLOC1 keep their exact meaning, the explicit
allocator parameter stays the override and the embedded story, and the implicit context
merely makes the *already-hidden default* nameable and swappable — pure add. Precedence is
one sentence: **a passed allocator always beats the ambient one.** Cβ reuses the ratified
`#` marker grammar, dodges the `using` (S62) and `push_context` (keyword-budget) traps, and
the scoped marker form reads as "compiler-managed, restores on exit." v1 holds the bundle
to **allocator + logger only**; the context is the natural future carrier for c06
capabilities and D-EFF1 effects, but that expansion is explicitly out of scope here and
must come back as its own card. **Reject A1** — replacing S58 trades a shipped, visible,
teachable expert path for invisible threading. If the owner wants zero new magic, **A3** is
the clean no; everything beginner-facing still works, we just never get the swap seam.

**Stop-work:** Smart Context implementation is blocked until D-CTX1 (Q1 at minimum) is
decided.

---

Sources (prior-art verification):
- Jai context / `push_context`: [The Way to Jai — Context](https://github.com/Ivo-Balbaert/The_Way_to_Jai/blob/main/book/25A_Context.md), [Jai Community wiki — Context](https://jai.community/t/context/163)
- Odin implicit `context`: [gingerBill — Odin's Most Misunderstood Feature: context](https://www.gingerbill.org/article/2025/12/15/odins-most-misunderstood-feature-context/), [Odin overview](https://odin-lang.org/docs/overview/)
- Go `context.Context`, Racket `parameterize`, thread-locals: standard language docs.

---

## Build-time I/O at comptime — board card c75

### D-CTIO1 — Gated build-time I/O at comptime (rec B)

Jet's comptime engine is already ratified and partly shipped: S26 (the comptime law — value-only, no macros, no comptime types), S57 (`comptime x = …` bindings), S60 (Layer 2 — compile-time pure evaluation + data embedding), D-PURE2 (no ambient I/O; `embed_file` the one named exception), D-WHEN1/2 (`comptime if`, shipped). So this ballot does **not** re-decide comptime. The one unresolved question: **should Jet permit build-time I/O beyond `embed_file`?** Jai's `#run` allows full filesystem access at compile time — a supply-chain risk Jet's S26 law was written to refuse. This card settles the policy boundary.

**User story.** Dana is shipping a graphics tool. She wants a WGSL shader (and a root cert, a JSON schema) baked into the binary as a constant at compile time — without a separate build script, and without opening `jet build` to arbitrary code execution from a dependency.

| | A — pure-only forever | B — ratify `embed_file`/`embed_bytes` | C — broad gated build I/O |
|---|---|---|---|
| Supply-chain risk | none | minimal (read-only, path-checked) | high un-audited; moderate gated |
| Power | lowest | covers ~90% of embed needs | full (env, network, codegen) |
| Consistency w/ S26 law | perfect | good (S26 already names `embed_file`) | strained |
| Ratchet (I8) cost | none | small (two builtins) | high (new gate, lockfile, sandbox) |
| Prior-art twin | — | Zig `@embedFile`, Rust `include_str!` | Jai `#run`, Nim `staticExec` |

**How other languages do this.**
- **Zig** — `@embedFile("path")` bakes a file's bytes in; no general comptime I/O (a compile error). The cleanest precedent for B — takeaway: a dedicated embed builtin, not an I/O grant.
- **Jai** — `#run fn()` runs *anything* at compile time, including filesystem/network/process spawn. A buggy dep can read `~/.ssh` during `jai build`. Takeaway: this is exactly the model S26 refuses.
- **Rust** — `include_str!`/`include_bytes!` are embed-only; arbitrary build execution is isolated to a separate `build.rs`. Takeaway: safe embed built-in, dangerous execution quarantined to a distinct, visible mechanism.
- **D** — CTFE over a pure subset; `import("file")` is the sole file-read intrinsic. Takeaway: even a powerful comptime keeps I/O to one named read.
- **Nim** — `staticRead` (safe embed) *and* `staticExec` (shell at compile time). Takeaway: `staticExec` is the footgun that spreads through packages once it exists — the cautionary tale against C.

- **Option A — keep pure-only forever.** No build-time I/O at all; `embed_file` stays unimplemented. Assets embed via a separate codegen step or are read at runtime.

  ```jet
  comptime shader :: read_file("shaders/main.wgsl")  // error: I/O not allowed in comptime; use a build step or core.fs at runtime
  ```
  Safest and simplest (I8 favors it), but forfeits an ergonomic win the spec already blessed and forces a separate build step for every embedded asset.

- **Option B — ratify `embed_file` / `embed_bytes` (recommended).** Ship the read-only builtins S26/D-PURE2 already name: `embed_file(path) -> String`, `embed_bytes(path) -> [U8]`. Path must be a string literal, resolved relative to the source file, no `..`-escape past the project root. Not new I/O capability — it implements the blessed exception.

  ```jet
  comptime shader_src :: embed_file("shaders/main.wgsl")   // String, baked into the binary
  comptime cert_der   :: embed_bytes("certs/root.der")     // [U8]

  comptime bad :: embed_file(build_path())          // error: path must be a string literal
  comptime esc :: embed_file("../../etc/passwd")    // error: path escapes the project root
  ```

- **Option C — broad gated build-time I/O.** Allow arbitrary comptime functions to do I/O when explicitly gated with a visible audit marker (mirroring the S58 `#audit`/`#unsafe` model).

  ```jet
  #audit("reads the local package list at build time — no network, no secrets")
  comptime pkgs :: #run(io) {
      core.fs.read("local-packages.txt").lines().filter((l) => l.len() > 0)
  }
  ```
  Sandboxed subprocess, an auditable `.jet/build-io.lock` of accessed paths, cache-invalidation on change. Powerful, but a new marker + lockfile + sandbox — heavy against the ratchet, and the Nim/Jai evidence shows un-auditable spread once shipped.

**Recommendation:** **B** — it's the answer Zig and Rust already prove safe at scale, it's what S26/D-PURE2/S60 already committed to (so this is an implementation/surface ratification, not a policy change), and it closes the door on C's supply-chain class. Owner sign-off questions: (1) `embed_bytes` in scope or embed-as-`String` only? (2) does the path restriction get its own diagnostic code? (3) does `embed_file` ride S60 Layer 2's milestone or get its own slot?

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Visible uninitialization — board card c76

### D-UNINIT1 — Visible uninitialization (rec B)

**User story.** Priya is writing a hot networking path that fills a 4 KB stack
buffer from a socket read. Auto-zeroing the buffer every call is measurably
wasteful — she's profiled it. She adds `use core.mem` to opt in to the
low-level tier (S58) and wants to tell the compiler "I'll fill every byte
before I read it; skip the zero-fill." She expects a compile error, not
undefined behavior, if she ever forgets.

| Option | Spelling | Beginner legibility | Tokens introduced | Gate required | Failure mode |
|--------|----------|--------------------|--------------------|---------------|--------------|
| A | `buffer: [4096]U8 := ---` | poor — `---` reads as punctuation noise | none (sigil) | `use core.mem` | opaque; cryptic to grep for |
| B | `buffer: [4096]U8 := uninit` | high — reads as English | `uninit` keyword | `use core.mem` | clearest in diagnostics, quotable in fix-it |
| C | `#uninit buffer: [4096]U8` | medium — attribute-style matches `#unsafe` idiom | none (reuses `#`) | `use core.mem` | separates the "no init" signal from the binding site |

**How other languages do this**

- **Jai (`---`):** `buf: [4096] u8 = ---;` — the three-dash sigil is Jai-specific  
  vocabulary with no prior-art meaning. Terse but opaque. Jet takeaway: don't borrow
  the sigil; borrow the idea that the programmer makes an explicit, visible choice.
- **Zig (`= undefined`):** `var buf: [4096]u8 = undefined;` — a keyword that looks
  like a value. Zig's safety mode traps reads in debug builds; release mode is silent
  UB. Jet takeaway: a keyword is the right shape; the compile-time proof (not a
  runtime trap) is the right safety rail.
- **C (implicit):** `char buf[4096];` — no annotation needed; the buffer is uninit
  by default. The footgun Jet explicitly avoids: silence where UB lurks. Jet
  takeaway: the absence of syntax is the wrong default; opt-in, not opt-out.
- **Rust (`MaybeUninit`):** `let mut buf = MaybeUninit::<[u8; 4096]>::uninit();` —
  correct and safe, but the user must manually call `.assume_init()` after writing,
  with no compiler enforcement that writes actually occurred. Jet wraps this and adds
  the write-before-read proof in sema. Jet takeaway: `MaybeUninit` is the right
  lowering target; the ergonomic surface above it should be a keyword, not a type.
- **C# (`stackalloc` / `SkipLocalsInit`):** `Span<byte> buf = stackalloc byte[4096];`
  zeroes by default; `[SkipLocalsInit]` on the method skips zeroing for the whole
  frame — coarse-grained and invisible at the variable. Jet takeaway: per-binding
  opt-out is finer-grained and safer than per-function attributes.

- **Option A — `= ---` (Jai marker).**

```jet
use core.mem

fn fill(sock: Socket) {
    buffer: [4096]U8 := ---   // skip zero-fill; Jai-style

    sock.read(mut buffer)?

    // Compile error if buffer is read before a full write:
    // E0420  read of possibly-uninitialized value `buffer`
    //        hint: every path through this function must write
    //              `buffer` before reading it
    //        note: declared `:= ---` at line 4
    process(buffer)
}
```

Grep for `:= ---` in a 50-file codebase: noisy, ambiguous with subtraction chains.

- **Option B — `:= uninit` (keyword).**

```jet
use core.mem

fn fill(sock: Socket) {
    buffer: [4096]U8 := uninit   // skip zero-fill; explicit opt-out

    sock.read(mut buffer)?

    // If the read is removed and buffer is used before write:
    // E0420  read of possibly-uninitialized value `buffer`
    //        declared `:= uninit` at line 4; a write to every
    //        element must precede this read on all paths
    //        fix: write to `buffer` first, or remove `:= uninit`
    process(buffer)
}
```

`uninit` is a reserved word gated behind `use core.mem`; outside that gate the compiler
teaches: `:= uninit` is an expert-tier construct — add `use core.mem` at the top of this file.

- **Option C — `#uninit` attribute.**

```jet
use core.mem

fn fill(sock: Socket) {
    #uninit
    buffer: [4096]U8

    sock.read(mut buffer)?

    // E0420  read of possibly-uninitialized value `buffer`
    //        declared with `#uninit` at line 4
    process(buffer)
}
```

The `#` sigil is consistent with `#unsafe` / `#audit` (D-ATTR1, ratified). However,
placing the annotation on a *separate line* splits the meaning from the binding; it is
easy to insert a blank line between them and forget the relationship.

**Recommendation:** B — `:= uninit` is a value-position keyword that sits exactly where
the initial value would be, making the opt-out structurally visible at the binding site.
It is quotable verbatim in diagnostics ("declared `:= uninit`"), greppable, and reads as
plain English. The compile-time write-before-read proof (not a runtime trap) is the
non-negotiable safety rail that separates this from Zig's `= undefined` and C's silence.

**Implementation notes (for agent after ratification):**
- Gated by `use core.mem` (S58); outside that gate emit a teaching error pointing at
  the gate requirement.
- Sema tracks `MaybeUninit` state per binding; performs a dataflow analysis on all paths
  to ensure the binding is written before any read. Read-before-write on any path is
  E0420 (snapshot required, I4).
- Codegen lowers to `MaybeUninit::<T>::uninit()` in generated Rust; after the sema proof
  passes, uses `.assume_init()` — no generated `unsafe` leaks outside the `#unsafe` gate
  (I1).
- `uninit` is not a valid default parameter value, struct field default, or const context;
  sema rejects each with a specific sub-code of E0420.

---

---

## Three-mode execution & JIT dev runtime — board card c77

### D-JIT1 — JIT backend (rec D)

**User story.** Sam runs `jet serve api.jet` for a 40-endpoint service and edits a
handler. He wants the change live in well under a second, with the new code running at
something close to native speed — and he never wants to see a Rust or LLVM error,
because Jet promised him the front end owns every message. The question is what
machine actually turns his saved handler into running code inside the live process.

| Option | Latency to live | Peak throughput | New compiler dep (I6) | I2/I3 risk | Ratification cost |
|--------|-----------------|-----------------|------------------------|-----------|-------------------|
| A Cranelift JIT | very low (ms) | good, below LLVM | yes — Cranelift in the runtime crate | low (sema gates before emit) | high (new backend) |
| B incremental rustc | high (rustc per swap) | best (LLVM) | no new dep, but rustc in the hot loop | **high** — rustc errors in the live path | medium |
| C hybrid (Cranelift dev, rustc release) | very low dev / best release | best release | yes — both | medium — two backends to keep consistent | highest |
| D stay-interpreter-for-v1 | low (no compile) | interpreter-speed | **none** | lowest | lowest |

**How other languages do this.**
- **Cranelift JIT** (wasmtime, rustc's `-Zcodegen-backend=cranelift`): fast machine-code
  emit, designed for low-latency compile, weaker optimizer than LLVM. *Jet takeaway:*
  the natural "fast swap, decent speed" backend, but it's an external crate in the
  runtime — needs an I6 stance even though it's runtime-side, not in the `Source/`
  compiler.
- **JVM HotSpot / tiered JIT**: interpret first, JIT the hot methods on a background
  thread. *Jet takeaway:* tiering (interpret cold, JIT hot — plan item 4b) is the right
  shape regardless of which backend wins; v1 can be tier-0 only.
- **incremental rustc**: real LLVM codegen but seconds-scale per change, and the tool
  doing the compile is the one Jet has sworn never lets speak to users (I2). *Jet
  takeaway:* fine as the *release* backend (already shipped as `jet build`), wrong as
  the *interactive* backend.
- **Cranelift-as-rustc-backend**: rustc itself can emit via Cranelift for faster debug
  builds. *Jet takeaway:* shows the hybrid is real prior art, not a fantasy — but it's
  two backends to keep output-identical (see D-DEVMODE1 / 4e).

- **Option A — Cranelift JIT in the live process.** sema fully checks the unit, then a
  Cranelift backend emits native code in-process. rustc stays the release backend only.

```shell
$ jet serve api.jet
serving on :8080 (JIT: cranelift, tier-0)
# edit handlers/checkout.jet, save
[checkout.jet] checked ok → JIT-compiled in 31ms → swapped live
```

- **Option B — incremental rustc per swap.** Reuse the shipped rustc path for every
  reload. One backend, but rustc runs in the interactive loop.

```shell
$ jet serve api.jet
# edit + save
[checkout.jet] checked ok → rustc rebuild… 4.2s → swapped live
# and if rustc ever rejected the generated crate, that is an I2 ICE, never Sam's error
```

- **Option C — hybrid: Cranelift for dev/serve, rustc for release.** Fast swap in the
  live process, LLVM-grade binary on `jet build`. Pays for two backends and must prove
  they agree (4e).

```shell
$ jet serve api.jet      # cranelift, ms-scale swaps
$ jet build api.jet      # rustc/LLVM, optimized binary
# CI runs every example through both and diffs (D-DEVMODE1 / 4e)
```

- **Option D — stay interpreter for v1, design the JIT seam now.** `jet serve` ships
  on the comptime interpreter (D-DEV3) with hot-swap; the JIT backend lands behind a
  stable seam in a later Epoch-3 milestone. Zero new deps, lowest risk, ships the
  *experience* (live hot-reload server) before the *speed*.

```shell
$ jet serve api.jet
serving on :8080 (interpreter; JIT backend: planned)
# edit + save → re-checked + hot-swapped at interpreter speed, sub-200ms
```

**Recommendation: D.** Ship the hot-reload *experience* on the already-proven
interpreter first — it's the part users feel — and keep the JIT behind a seam so the
backend choice (A vs C) can be made on real workloads without blocking the pillar.
Cranelift (A) is the likely successor; rustc-in-the-loop (B) is rejected outright as an
I2 hazard.

---

### D-HOTSWAP1 — hot-reload semantics (rec: module boundary + type-stable state preservation)

**User story.** Priya's `jet serve` process holds an in-memory session cache and 200
open websocket connections. She fixes a typo in one handler and saves. She expects the
fix live with the cache and the sockets intact. Next she changes the *shape* of the
session record. She does **not** expect the server to keep reinterpreting old bytes as
the new type — that's exactly the memory-unsafety Jet exists to prevent. She'd rather
be told "this change needs a restart" and have it happen cleanly.

Two coupled questions: **(Q1) swap boundary** — what unit gets replaced; **(Q2) state
policy** — what happens to live state across the swap.

| Option | Swap unit | State on type-compatible edit | State on type-changing edit | Safety story |
|--------|-----------|-------------------------------|------------------------------|--------------|
| A function | single fn | preserved | n/a (fn body only) | tight blast radius; can't swap a type at all |
| B module | module | **preserved** (code swapped, data kept) | **announced clean restart** | matches Erlang; clear safety line |
| C whole-program | process | always restart | always restart | trivially safe, loses the live-state win |

- **Option A — function-granularity swap, state untouched.** Only function bodies hot-
  swap; any signature/type/struct change forces a restart. Smallest blast radius,
  simplest invalidation — but most real edits touch more than one body.

```jet
# edit the body only → swapped in place, all state preserved
fn price(c: Cart) -> Money {
    c.lines.sum((l) => l.qty * l.unit) - c.discount?   # was: ... (no discount)
}
```

- **Option B — module-granularity swap with type-stable state preservation.** The
  reload unit is a module. If the module's **public type surface is unchanged**, swap
  the code and **keep the module's live state** (the session cache, the sockets). If a
  reload **changes a type/layout** that live state depends on, Jet does **not**
  reinterpret old data — it performs a **clean, announced restart** of that module (or
  the process, if the change crosses module walls), draining connections first.

```jet
module sessions {
    cache :: SessionCache.new()   # live state

    fn touch(id: SessionId) { cache.bump(id) }   # type-stable edit → hot-swap, cache kept
}
```

```shell
# type-stable edit:
[sessions] checked ok → hot-swapped; module state preserved
# type-changing edit (Session gains a field):
[sessions] type surface changed (Session: +field `region`)
  → live state of `sessions` is no longer well-typed; announced restart in 2s
  → draining 200 connections… restarted clean
```

- **Option C — whole-program swap, always restart.** Every save restarts the process.
  Trivially safe, zero stale-state risk — but throws away the live-cache/live-socket
  benefit that justifies a long-lived JIT process at all.

```shell
[any change] → full restart (state not preserved)
```

**Recommendation: B (module boundary + type-stable preservation).** This is the
Erlang/Elixir gold-standard model adapted to a no-GC, statically-typed setting: keep
state across code-only swaps, refuse to reinterpret state across type changes, and make
the restart *announced and clean* rather than silent. The type-surface check is a sema
job (I3) — the runtime never guesses. Function-only (A) is too coarse a win for too
many restarts; whole-program (C) defeats the pillar's purpose.

---

### D-DEVMODE1 — hot-reload home + dev↔release consistency guarantee (rec: B for home, ratify the guarantee)

**User story.** Theo just wants "edit, see it instantly." He already knows `jet dev`
(the shipped watch-and-rerun loop) and `jet run`/`jet build`. The open question isn't
the verbs — D-DEV4 settled those — it's whether *instant hot-reload* is `jet dev`
growing a hot-swap upgrade, or a separate `jet serve` for long-lived processes. And
separately: Theo must be able to trust that what he sees in dev is exactly what ships.

**Q1 — where does hot-reload live?** **Q2 — ratify the consistency guarantee (4e) as a
hard rule?** (Verb naming is NOT on this ballot.)

| Option (Q1) | Home of hot-swap | Mental model | Cost |
|-------------|------------------|--------------|------|
| A | extend `jet dev` | "dev got faster — same verb, now swaps instead of reruns" | one verb does both short scripts and long servers |
| B | new `jet serve` (D-DEV2) | "`dev` = rerun my script; `serve` = long-lived process I hot-swap into" | two verbs, but each matches its prior art |

- **Option A — hot-reload is an upgrade to the shipped `jet dev` loop.** The watch loop
  (4a, already shipped) keeps its verb; in Epoch 3 it gains hot-swap instead of full
  re-run. One verb for all reload.

```shell
$ jet dev script.jet     # short script: watch + rerun (today) → watch + hot-swap (E3)
$ jet dev api.jet        # also the long-lived server? one verb, two lifetimes
```

- **Option B — `jet dev` stays the rerun loop; `jet serve` is the long-lived hot-swap
  process.** `jet dev` keeps the shipped semantics (re-run my entry on save). The
  long-lived JIT/hot-swap process is `jet serve` (the D-DEV2 surface). The watcher and
  debounce (4a) are shared machinery; the difference is rerun-vs-swap-into-a-living-
  process.

```shell
$ jet dev script.jet     # unchanged shipped behavior: re-run entry on save
$ jet serve api.jet      # long-lived; modules hot-swapped in place (D-HOTSWAP1)
```

**How other languages do this.**
- **Bun `--watch` / Vite dev (HMR)**: `--watch` restarts the process; Vite HMR swaps
  modules into a *running* app — and they are deliberately *different* tools/flags.
  *Jet takeaway:* the ecosystem itself separates "rerun" from "swap into a live app" —
  supports B's two-verb split.
- **nodemon**: pure restart-on-change, no state preservation. *Jet takeaway:* that's
  today's `jet dev` exactly; the hot-swap upgrade is a genuinely different capability,
  worth its own home.
- **Erlang/Elixir hot code swapping**: the gold standard — versioned modules swapped
  into a running node with state carried via `code_change`. *Jet takeaway:* this is a
  `serve`-shaped, long-lived-process feature, not a "re-run my script" feature.
- **JVM HotSwap / JRebel**: HotSwap allows method-body changes only; JRebel extends to
  structural changes. *Jet takeaway:* mirrors D-HOTSWAP1's "type-stable swap vs
  restart" line; reinforces that hot-swap belongs to a persistent process.
- **Wasm component reload**: swap a component instance, explicitly hand off state across
  the boundary. *Jet takeaway:* state hand-off is an explicit, typed operation, never
  an implicit byte-reinterpret — exactly the I1 line D-HOTSWAP1 draws.

**Q2 — the consistency guarantee (4e).** Regardless of Q1: a program must behave
**identically** under the dev runtime (interpreter / JIT) and the release build (rustc
binary). Ratify as a **hard rule**: a `tests/` mode runs every golden example through
**both** paths and **diffs output**; any mismatch is a **release blocker**. This is the
I5 guard across two backends and the standard JIT/AOT-divergence defense.

```shell
$ jet test --consistency
running 142 examples through {interpreter, rustc-release}…
  141 identical
  ✗ 03_floats.jet: dev=0.30000000000000004  release=0.3
  → RELEASE BLOCKED: dev/release output diverged (4e)
```

**Recommendation: B for Q1, and ratify Q2 as a hard rule.** Keep the shipped `jet dev`
rerun loop exactly as users learned it; give long-lived hot-swap its own `jet serve`
verb (already the D-DEV2 surface) — matching how Bun/Vite/Erlang separate the two
lifetimes. And make the dev↔release diff a release blocker, not a warning, so Jet never
ships the "works in dev, breaks in prod" bug class.

---

## Cache-friendly layout (SOA, deferred) — board card c78

### D-SOA1 — Cache-friendly data layout (SOA) (rec A, deferred to Later)

**Tier: Later / deferred.** This decision is ballot-ready but implementation is
deferred until after v1. The owner's vote locks in the syntax now so the feature can
be planned against a fixed spelling.

**User story.** Dev is writing a particle system that updates 100 000 `Particle`
records per frame. Profiling shows cache misses dominate: the default array-of-structs
(AOS) layout loads the `x`, `y`, `z`, and `color` fields of one particle into a cache
line even when the update loop only touches `x`, `y`, `z`. He wants
structure-of-arrays (SOA) layout — one contiguous array per field — without rewriting
every access as `particles_x[i]`, `particles_y[i]`, `particles_z[i]`.

| Option | Spelling | Annotation site | Field-access change? | Ceremony | Composability |
|--------|----------|-----------------|----------------------|----------|----------------|
| A | `#layout(soa) struct Particle { … }` | type definition | none — `p.x` still works | low | layout is part of the type; composable with `#Serialize` etc. |
| B | `particles: soa [Particle]` | variable declaration | none — `p.x` still works | low | layout is per-container; same type can be AOS in one place, SOA in another |

**How other languages do this**

- **Jai (`#place` / `using`):** Jai lets you embed one struct inside another with
  `#place` to force field co-location; SOA is built into the language's array
  primitives. No single annotation; requires structural knowledge of the layout
  system. Jet takeaway: a single annotation is the right UX; the compiler does the
  structural transformation, not the user.
- **Zig (`MultiArrayList`):** `std.MultiArrayList(T)` is a stdlib type that stores
  fields in separate arrays; access is via `.items(.field_name)`, breaking normal
  field syntax. Jet takeaway: field syntax must stay identical; a compile-time
  transform that preserves `p.x` is the goal.
- **Rust (`soa-derive` / `slotmap`):** The `soa-derive` crate generates a parallel
  struct via a procedural macro; `slotmap` provides SOA slots. Both require
  importing a crate and annotating the struct. Jet takeaway: the annotation-on-type
  shape is familiar from Rust macros; a built-in transform avoids external crate
  dependency (I6).
- **ISPC / data-oriented design (manual):** ISPC's `soa<N> T` type declaration
  generates SOA layout for SIMD; elsewhere, data-oriented design achieves SOA by
  hand — splitting one `struct Particle` into multiple parallel arrays. Jet
  takeaway: a compiler-managed transform is superior to manual splitting; the ISPC
  `soa<N>` shape (annotation on the type) confirms the Option A position.
- **Unity DOTS (`[StructLayout]` / `IComponentData`):** Unity's ECS requires
  implementing `IComponentData` and relies on the runtime's archetype system;
  the developer does not choose AOS vs SOA directly. Jet takeaway: the decision
  should be explicit and developer-controlled, not hidden in a runtime framework.

- **Option A — `#layout(soa)` on the struct.**

```jet
#layout(soa)
struct Particle {
    x: Float
    y: Float
    z: Float
    color: U32
}

fn update(particles: mut [Particle]) {
    loop p in particles {
        p.x += p.velocity_x   // field access unchanged
        p.y += p.velocity_y
    }
}
```

The type carries its layout. Any `[Particle]` collection is automatically SOA; the
caller does not need to know. Mixing AOS and SOA `Particle` values in the same
collection is a type error (they are the same nominal type, so sema must track the
layout tag).

*Partial-SOA variant (open question — recommend deferring):*

```jet
#layout(soa: x, y, z)   // only hot fields go SOA; color stays interleaved
struct Particle { … }
// Complexity: the field-access lowering for the cold fields differs.
// Recommend: whole-struct only for v1.
```

- **Option B — `soa` keyword on the container.**

```jet
struct Particle {
    x: Float
    y: Float
    z: Float
    color: U32
}

fn update(particles: mut soa [Particle]) {
    loop p in particles {
        p.x += p.velocity_x   // field access unchanged
        p.y += p.velocity_y
    }
}
```

The layout is per-collection. A `[Particle]` passed to a non-`soa` function is a
type mismatch; the caller must decide the layout. This is more flexible (same type,
two layouts) but surfaces the layout decision to every call site.

**Recommendation:** A — `#layout(soa)` on the type is consistent with the `#`
attribute system (D-ATTR1, ratified) and keeps the layout decision at the definition,
not scattered across every call site. The tradeoff (one layout per type in v1) is
acceptable for the common case; partial SOA and per-container layout are open
questions for a later revision. Defer implementation until after v1; ratify the
syntax now so plans can be written against a fixed spelling.

**Open questions for the owner:**
1. Whole-struct SOA only in v1 (recommended), or support `#layout(soa: field, …)`
   partial annotation?
2. Should `soa` [Particle] (Option B) be a future-reserved spelling even if A is
   chosen, to enable per-container overrides later?
3. Interaction with `#Serialize` and reflection: does SOA layout affect the
   serialized representation?

---

## jet.regex (persona #1 gap) — board card c79

### D-REGEX1 — How does `jet.regex` ship? (rec B)

The 2026-06-20 persona brief found a missing `jet.regex` is the **#1 gap** — it blocks 4 of 9 personas (CLI text-processing, ETL filtering, HTTP route matching, library validation). But regex can't just be built: **I6** says the compiler carries zero external crates, stdlib sub-libraries may bootstrap with crates only **until the end of Epoch 3**, and **any new stdlib external dep requires owner approval**. So this is your call, not a silent build.

**User story.** Mara writes a CLI that greps logs; Elena filters an ETL feed; Tariq matches HTTP routes. All three reach for `regex.match(pattern, text)` on day one and find nothing. Whatever ships must be memory-safe and must not ReDoS (catastrophic backtracking) on hostile input — Jet's whole promise.

| Option | Time to unblock personas | I6 posture | ReDoS-safe | Maintenance |
|---|---|---|---|---|
| A — native engine now | slow (weeks) | clean (no dep ever) | yes (build it linear-time) | ours forever |
| B — bootstrap `regex` crate, native-ize before Epoch 3 ends | fast (days) | I6-sanctioned bootstrap; needs your dep approval | yes (Rust `regex` is linear-time) | temporary dep, then ours |
| C — defer regex | none (gap persists) | n/a | n/a | none |

**How other languages do this.**
- **Rust `regex`** — DFA/NFA hybrid, **guaranteed linear time, no backtracking** (no ReDoS). The crate B would bootstrap on. Takeaway: the safe, fast reference implementation.
- **Google RE2** — the linear-time automaton engine Rust's `regex` descends from; built precisely to kill ReDoS on untrusted input. Takeaway: the design target for a native Jet engine (Option A).
- **PCRE / Perl / Python `re`** — backtracking; powerful (backreferences) but **ReDoS-prone** on adversarial patterns. Takeaway: the model Jet must NOT copy by default.
- **Go `regexp`** — RE2-based, linear-time, deliberately omits backreferences for safety. Takeaway: precedent that dropping backtracking-only features is an acceptable, safety-positive trade.

- **Option A — native engine now.** Hand-write an RE2-style linear-time matcher in Jet/Rust under `jet.regex`, no external crate.

  ```jet
  use jet.regex as re
  m :: re.match("(\\d+)-(\\d+)", "8080-9090")?   // native engine, no dependency
  print(m.group(1))   // 8080
  ```
  Cleanest I6 story (never a dep), full control, but weeks of work and a real risk of subtle bugs in a from-scratch engine.

- **Option B — bootstrap the `regex` crate now, native-ize before Epoch 3 ends (recommended).** Ship `jet.regex` backed by Rust's `regex` crate immediately to unblock the 4 personas; replace it with a native engine before the I6 Epoch-3 deadline. Requires your approval of the dep.

  ```jet
  use jet.regex as re
  m :: re.match("(\\d+)-(\\d+)", "8080-9090")?   // same surface; backed by `regex` crate for now
  print(m.group(1))   // 8080
  ```
  Unblocks users in days and is ReDoS-safe (the crate is linear-time); the cost is a temporary sanctioned dependency and a scheduled native-ization before Epoch 3 ends.

- **Option C — defer regex.** Ship nothing; the 4 personas keep hand-rolling string scans.

  ```jet
  // no jet.regex — users write manual scanners
  fn has_port(s: String) -> Bool { s.contains("-") && s.split("-").all((p) => p.is_digits()) }
  ```
  Zero work and zero dep, but leaves the single biggest adoption gap open.

**Recommendation:** **B** — I6 explicitly sanctions bootstrap crates through Epoch 3, the `regex` crate is the memory-safe linear-time gold standard, and it unblocks the most personas fastest; schedule the native-ization (Option A's engine) as the pre-Epoch-3-close replacement so the end state is still dependency-free. Pick A only if you want native-first now and accept the slower timeline. This card exists because B needs your dep approval (I6).

---

## Testing ergonomics — board card c51

Source plan: `tools/Tower/docs/plans/epoch-3/testing-docs-ergonomics.md`.

The Epoch 2 test core shipped: `#test fn name { … }` blocks (S43/S82), `require`
/ `require_eq` assertions (S36), snapshot `expect(…).snapshot()` with
`--update-snapshots`, `todo` typed holes, and `jet bench`. These three items are
**ergonomics layer**, not core language, and each is gated on a syntax decision Jet
has not yet made.

**What is add-only (not re-deciding):**
- `#test fn name { … }` — the unit-test surface (S43, S82). Ratified. Untouched.
- `require` / `require_eq` — assertion builtins (S36). Ratified. Untouched.
- `///` doc-comment marker — the *existence* of `///` is ratified (S49): "summary
  lines immediately above items; plain text in v1; shown by hover/docs tooling."
  D-TEST4 decides only how code examples *inside* those comments are delimited and
  executed as tests. S49's `///` marker stays.

D-TEST1 gates property testing + shrinking. D-TEST4 gates doc-example execution.
Coverage (D-COV1 below) needs no syntax decision; it is noted as deferred.

---

### D-TEST1 — property-test surface + shrinking (rec B)

**User story.**

Mia writes a `reverse` function for lists. She wants to say "for any list of
integers, reversing twice returns the original" — and when her implementation is
wrong she wants the test runner to hand her the *smallest* list that breaks it, not
just the first random one it found. She has never heard of QuickCheck. She types
something that looks like a normal test and discovers property testing by accident.

**How other languages do this.**

| Language | Spelling | Shrinking |
|----------|----------|-----------|
| **Haskell QuickCheck** | `prop_reverse xs = reverse (reverse xs) == xs`; `quickCheck prop_reverse` — an ordinary function whose args are `Arbitrary` | Automatic (typeclass-driven); built into the library |
| **Python Hypothesis** | `@given(st.lists(st.integers())) def test_rev(xs): assert rev(rev(xs)) == xs` — decorator + strategy objects | Automatic shrinking; strategies carry shrinkers |
| **Rust proptest** | `proptest!(|(xs: Vec<i32>)| { assert_eq!(rev(rev(&xs)), xs); });` macro; strategy is the type | Automatic; macro-driven |
| **JavaScript fast-check** | `fc.assert(fc.property(fc.array(fc.integer()), xs => reverseReverse(xs)))` — imperative | Automatic; arbitraries carry shrinkers |

Jet takeaway from all four: the surface that feels lowest-ceremony is one where the
test annotation and a parameter list together communicate "generate inputs for me."
The shrinking behavior is automatic and invisible in every respected property-test
library; the user never writes a shrinker.

**Current state (add-only).**

S82 shows `#test fn reversing_twice(xs: [Int]) { require_eq(reverse(reverse(xs)), xs) }` as a worked example in the attribute-syntax ratification. This is the *aspirational* property-test surface hinted there; it is **not yet executable** — the plan explicitly states property testing is blocked on D-TEST1. That example establishes the Jet aesthetic the owner already prefers (looks like a normal test, parameter list carries the generated type), so options below are ordered by how closely they follow it.

**Tradeoff comparison.**

| Option | Surface | New keyword/sigil? | Shrinking surface | Ceremony |
|--------|---------|-------------------|-------------------|----------|
| A — `#property fn` attribute | `#property fn name(x: T) { … }` | new attribute `#property` | implicit, always on | low — one new marker |
| B — `#test fn` with parameters | `#test fn name(x: T) { … }` | none (extends existing `#test`) | implicit | zero — same marker, params signal property |
| C — `forall` expression inside test | `#test fn name { forall n: Int { … } }` | new keyword `forall` | implicit | medium — nested blocks |
| D — generator call as a parameter default | `#test fn name(x: Int = Gen.int()) { … }` | none | explicit shrinker optional | medium — call-site annotation noise |


- **Option A — `#property fn` attribute.** A distinct attribute signals "this is a
  property test, not a unit test." The runner generates inputs from the parameter
  types.

```jet
#property
fn reversing_twice(xs: [Int]) {
    require_eq(reverse(reverse(xs)), xs)
}

#property
fn addition_commutes(a: Int, b: Int) {
    require_eq(a + b, b + a)
}
```

Shrinking failure output (automatic — user never writes a shrinker):

```
FAIL property reversing_twice
  failed after 47 examples
  counterexample: xs = [3, 1]
  shrunk to:      xs = [1, 0]
  error: require_eq failed
    left:  [0, 1]
    right: [1, 0]
```

**Tradeoff:** two test markers (`#test` and `#property`) is two concepts to teach
and two words to remember. The distinction is real but adds cognitive overhead on
first contact.

- **Option B — `#test fn` with parameters (recommended).** An `#test fn` with
  parameters is a property test; one with no parameters is a unit test. The runner
  generates inputs from the parameter types. Zero new syntax; the parameter list
  already tells the reader something interesting is happening.

```jet
// unit test — no params, exactly as before
#test
fn empty_list_reverses_to_empty() {
    require_eq(reverse([]: [Int]), []: [Int])
}

// property test — params present → generate and shrink
#test
fn reversing_twice(xs: [Int]) {
    require_eq(reverse(reverse(xs)), xs)
}

#test
fn addition_commutes(a: Int, b: Int) {
    require_eq(a + b, b + a)
}
```

Same failure output as Option A (automatic shrinking; zero user effort). The rule
is a single sentence: "a test function with parameters is a property test."

**Tradeoff:** slightly less obvious from the marker alone that a function is a
property test. The parameter list is the signal, not the attribute. For experts
wanting to pin generated ranges or enumerate cases, a future `#[test, cases(…)]`
multi-marker form (S82) is compatible without breaking this surface.

- **Option C — `forall` expression inside test.** A `forall` keyword inside a test
  block introduces generated variables. Closer to mathematical notation.

```jet
#test
fn prop_reverse_is_involution() {
    forall xs: [Int] {
        require_eq(reverse(reverse(xs)), xs)
    }
}

#test
fn prop_commutes() {
    forall a: Int, b: Int {
        require_eq(a + b, b + a)
    }
}
```

**Tradeoff:** `forall` is a new keyword (I7 demands a slot in `Source/Syntax.rs`
and a decision ID). The nested block adds indentation. The mathematical flavour
may feel out-of-place next to Jet's plain-English style. Benefit: visually
unambiguous — property tests look different from unit tests inside the body.

- **Option D — generator call as parameter default.** Users annotate parameters
  with explicit generator calls; the runner recognizes parameters with generator
  defaults.

```jet
#test
fn prop_reverse(xs: [Int] = Gen.list(Gen.int())) {
    require_eq(reverse(reverse(xs)), xs)
}

// with range constraint:
#test
fn prop_bounded(n: Int = Gen.int(0..100)) {
    require(n >= 0 && n <= 100)
}
```

**Tradeoff:** explicit generators are more powerful (users can constrain ranges
day one) but more verbose. `Gen.int()` is a stdlib API call, not syntax — the
line between library and language blurs. Property tests look like ordinary tests
with defaults, which may cause confusion about which functions actually run
generators.

**Recommendation:** B. It adds zero syntax: an `#test fn` with parameters is a
property test; without parameters it is a unit test. This matches the S82 worked
example the owner already ratified, removes a cognitive split between two
attributes, and follows the simplicity ratchet (I8). Shrinking is always automatic
and invisible. The rule teaches in one sentence. If the owner wants an explicit
generator-constraint story, that is a follow-on decision layered on top of B
without breaking it (e.g. `#test fn prop(n: Int) where n in 0..100 { … }` or a
future `#[test, config(runs: 500)]` multi-marker).

---

### D-TEST4 — doctest convention (rec A)

**User story.**

Lena writes a `parse_int` function and adds a `///` doc comment explaining what it
does (S49). She wants the code example in that comment to run as part of `jet test`
so it never goes stale. She does not know the word "doctest" — she just wants her
example to be checked.

**How other languages do this.**

| Language | Doc marker | Example delimiter | Expected-output convention | Jet takeaway |
|----------|-----------|-------------------|---------------------------|--------------|
| **Rust** | `///` or `//!` | fenced ` ```rust ``` ` or bare ` ``` ` inside doc comment | `// ` comment after expression is *not* checked; `assert_eq!` used instead | Jet can't require `assert_eq!` calls (that's two concepts); expected output must be a simpler convention |
| **Python doctest** | triple-quoted docstring | `>>>` prompt prefix; expected output on the next line(s) | Visually distinct from prose; REPL-style | The `>>>` prompt is universally understood but adds a new sigil |
| **Elixir ExUnit** | `#doc """…"""` | fenced ` ```elixir ``` ` block; `iex>` prompt | `iex>` lines run as doctests; return value on next line is checked | Prompt-style is readable inline; `iex>` is language-specific branding |
| **Julia** | `"""…"""` docstring | ` ```jldoctest ``` ` block with `julia>` prompt | Checked against output | Language-specific fenced language tag works but requires tooling recognition |

Jet takeaway: fenced code blocks inside `///` comments are the cross-language
convention; the question is whether the expected output is a trailing comment, a
following plain line, or embedded via a prompt style. Jet has no REPL yet (c55
deferred), so a prompt style implies a surface that doesn't exist. A trailing
comment convention (`// => value`) is lightweight and already reads naturally in
Jet comments.

**Current state (add-only).**

S49 ratifies `///` as the doc-comment marker and defers example running to M13. E2901
("doctest output mismatch") is reserved in `diagnostics.md`. This decision picks the
delimiter and expected-output convention so D-TEST4 can be implemented.

**Tradeoff comparison.**

| Option | Example delimiter | Expected output | New syntax? | Reads naturally? |
|--------|------------------|-----------------|-------------|-----------------|
| A — fenced ` ```jet ``` ` + `// =>` trailing comment | ` ```jet…``` ` block inside `///` | `// => value` comment on the last expression line | none — reuses `//` comment (S5) and fenced block convention | yes — comment is Jet syntax |
| B — fenced ` ```jet ``` ` + plain following line | ` ```jet…``` ` block; output as a bare second block or following plain text | prose line after the code block | none | ambiguous — prose vs expected output |
| C — `>>>` prompt prefix | `/// >>> parse_int("42")` with `/// 42` on the next line | plain line after `>>>` line | new inline prompt convention | familiar to Python users; unfamiliar to others |
| D — `#doctest` attribute on function | separate attribute triggers example extraction | no in-comment convention | new `#doctest` attribute | separates docs and tests; poor discoverability |


- **Option A — fenced ` ```jet ``` ` block + `// =>` trailing comment (recommended).**
  Examples are delimited by a standard fenced code block inside `///` lines. The
  expected output is a `// =>` comment on the line where a value is produced. The
  runner extracts the block, compiles it, and checks the printed/returned value
  against the `// =>` annotation.

```jet
/// Parse a decimal integer from a string.
///
/// Returns an error if the string contains non-digit characters.
///
/// ```jet
/// parse_int("42")  // => 42
/// parse_int("-7")  // => -7
/// parse_int("hi")  // => err(ParseError { … })
/// ```
pub fn parse_int(s: String) -> Int ? ParseError {
    …
}
```

A mismatch fires E2901:

```
error[E2901]: doctest output mismatch
  --> src/math.jet:6
   |
 6 |   parse_int("42")  // => 99
   |                         ^^
   |   expected: 99
   |   actual:   42
   |   note: update the `// =>` comment to match, or fix the implementation
```

Multiple statements, no expected output for intermediate lines:

```jet
/// ```jet
/// x :: parse_int("10")?
/// y :: parse_int("20")?
/// x + y  // => 30
/// ```
```

**Tradeoff:** the `// =>` convention adds no new tokens (S5 ratified `//` as the
line-comment marker); the runner just looks for that specific comment prefix on the
last expression of a block. The fenced block is the universal doc-example
convention. Downside: the `// =>` idiom is not self-describing on first encounter
(though it reads naturally: "this produces 42").

- **Option B — fenced ` ```jet ``` ` + separate plain-text output block.** A second
  fenced block (or a plain indented block) after the code block holds expected
  output. Rust's standard approach for prose output (not for expression values).

```jet
/// ```jet
/// print(parse_int("42"))
/// ```
///
/// Output:
///
/// ```
/// 42
/// ```
```

**Tradeoff:** two blocks per example doubles the visual weight. The separator label
("Output:") is prose that the runner must parse. Works well when expected output is
multi-line `print` output; awkward for simple expression values.

- **Option C — `>>>` prompt prefix.** REPL-style inline convention, each line
  prefixed by `>>>` inside `///` comments.

```jet
/// >>> parse_int("42")
/// 42
/// >>> parse_int("hi")
/// err(ParseError { … })
pub fn parse_int(s: String) -> Int ? ParseError { … }
```

**Tradeoff:** `>>>` is a new inline convention inside `///` comments — not a token
the lexer sees (it lives in comment text), but a convention the doctest runner must
parse. Familiar to Python users. Jet has no interactive REPL today (c55 deferred),
so `>>>` implies a mode that doesn't exist. The prompt may confuse beginners who
try to type `>>>` at a terminal.

- **Option D — `#doctest` attribute on the function, examples in a separate file.**
  A marker attribute on the function points the runner at examples stored elsewhere.

```jet
#doctest("examples/parse_int.jet")
pub fn parse_int(s: String) -> Int ? ParseError { … }
```

**Tradeoff:** discoverability is poor — the example lives in a different file from
the doc comment. Breaks the "docs and examples colocate" ergonomic goal. Not
recommended.

**Recommendation:** A. The `// =>` trailing-comment convention reuses existing
comment syntax (S5), requires zero new tokens, and reads naturally in Jet — the
`// =>` prefix is already idiomatically used in prose code snippets to show "this
evaluates to." The fenced ` ```jet ``` ` delimiter matches how examples already
appear in this codebase's docs. The diagnostic E2901 slots into the reserved
position cleanly.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---
