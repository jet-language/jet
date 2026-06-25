# c163 — Typestate `state { }` declaration block + `#Transition` marker

**Status:** Ready (ratified, unimplemented). Decisions D-STATE-DECL=B and
D-STATE-TRANS=A ratified 2026-06-25. Augments the shipped D-STATE1=A core.

## Goal

Replace the *loose-tag* typestate state set (states-as-`tag`s, set inferred from
markers) with an explicit, bounded, typo-checked `state TypeName { … }`
declaration block — a bare-keyword member of the `tag`/`struct`/`enum` family.
States stay pure compile-time facts that erase to zero runtime. `#State(X)` and
`#Transition(A -> B)` keep their shipped spellings; the only marker change is that
their state names must now resolve against the type's declared set.

## Current state (verified, file:line)

Shipped typestate (D-STATE1=A, option A loose-tags), built end-to-end:

- **Example:** `examples/features/113_typestate.jet` — declares states as
  `tag Pending {}` / `tag Confirmed {}` / `tag CheckedIn {}`, then `impl Reservation`
  methods carry `#Transition(_ -> Pending)`, `#Transition(Pending -> Confirmed)`,
  `#State(CheckedIn)`. Expected output `examples/features/expected/113_typestate.out`
  (3 lines). Golden auto-discovers every `examples/features/*.jet`
  (`tests/golden.rs:17-33`, `read_dir`), so extending 113 needs no test wiring.
- **Tests:** `tests/typestate.rs`; ui fixture `tests/ui/typestate_wrong_state.jet`
  + `.stderr` (the E0150 case: `check_in` on a still-`Pending` reservation).
- **Diagnostic E0150** (wrong-state): `docs/spec/diagnostics.md:166`; emitter
  `Source/Sema/State.rs:571-589` (`e0150`), guard logic at `State.rs:450-470`
  (`check_state`).
- **Sema machinery:** `Source/Sema/State.rs` — `StateTable` (`:37`) holds
  `requires` (`Type::method` → state), `transitions` (→ `(from,to)`), free-fn
  variants, and `entry_ctors` (`Type::method` → to-state). `StateTable::build`
  (`:56`) / `add_items` (`:66`) collect markers off `Func.state_requires` /
  `Func.state_transition`. `StateCtx` (`:131`) does intraprocedural forward
  dataflow over locals (same shape as `Taint.rs`): seeds initial state from an
  entry-ctor call, advances on transition calls, threads through `:=` rebinds,
  joins at if/else. `check_items_state` (`:537`) / `check_func_state` (`:518`)
  are the entry points, wired at `Source/Sema/Registration.rs:610-612` and
  `Source/Sema/Bundle.rs:924-930`. **Note `State.rs:26-27`:** "the state set is
  derived from the markers … there is no separate `states { }` declaration in v1"
  — this is exactly what c163 changes.
- **AST:** `Func.state_requires: Option<(String, Span)>` and
  `Func.state_transition: Option<StateTransition>` (`Source/AST.rs:~913-919`);
  `StateTransition { from: Option<String>, to: String, span }`. `Item` enum at
  `AST.rs:498`, `Item::Tag(TagDef)` at `:516`, `TagDef` at `:786`.

How the `tag` keyword (the family `state` joins) is declared/parsed:

- Keyword constant `KW_TAG = "tag"` (`Source/Syntax.rs:571`), registered in the
  keyword list (`Syntax.rs:1361`); lexed in `Source/Lexer/mod.rs:32-54`
  (`keyword()` → `TokKind::KwTag`); token display `Tokens.rs:185`.
- Parsed by `tag_def` (`Source/Parser/Items.rs:1946`): `pub? tag Ident` then either
  a `{ … }` body (tags carry no methods; stray `fn` recovers → E0732) or bare
  `tag Name;`. Sema name-uniqueness registration at `Registration.rs:132`.

How `#Transition(A -> B)` parses today (the `->` glyph):

- `->` is **`TokKind::Arrow`** (`Source/Lexer/Tokens.rs:76`, display `:216`),
  the same return/match-arm arrow (`Syntax::OP_ARM_ARROW = "->"`, `Syntax.rs:411`).
- `parse_transition_marker` (`Source/Parser/Items.rs:1177-1196`) already consumes
  `# Transition ( from -> to )` using `expect(TokKind::Arrow, …)` — **the `->`
  parse exists and needs no change.** `from` is `_` (`Syntax::STATE_ENTRY = "_"`,
  `Syntax.rs:713`) → `None` (entry ctor), else an ident. `parse_state_require_marker`
  (`:1165`) parses `#State(Ident)`. Dispatch via `at_state_fn`/`at_transition_fn`
  (`:677`/`:685`), gathered in `parse_fn_with_markers` (`:1147-1161`); module-level
  path mirrored at `Source/Parser/Modules.rs:183`.
- Marker keyword constants exist: `KW_STATE = "State"`, `KW_TRANSITION = "Transition"`
  (`Source/Syntax.rs:698,709`).

**`state` (lowercase) is a FREE keyword.** No `KW_STATE`-lowercase constant, no
`"state"` in the keyword list or lexer; the only `"state"` literal in the tree is a
filesystem path (`Source/Jetpack/Store.rs:85`, `.local/state/jet`). `KW_STATE`
holds the capital `"State"` for the `#State` marker, which is orthogonal.

## Decision (ratified, verbatim intent)

- **D-STATE-DECL=B:** states are declared in a dedicated
  `state TypeName { Pending, Confirmed, CheckedIn }` block — a bare-keyword decl in
  the `tag`/`struct`/`enum` family. One cohesive `state`/`#State`/`#Transition`
  family: `state` *declares* the set, `#State`/`#Transition` *mark* methods (like
  `tag` vs `#Tainted`). The set is bounded, typo-checked, tied to the type by name,
  and **erases** (no runtime discriminant). A **dead-end state** (declared, no
  outgoing transition) is a **warning** (default — a half-built machine still
  compiles). Loose-tags (A) and overloading `enum` (rejected).
- **D-STATE-TRANS=A:** `#Transition(From -> To) fn` using the `->` glyph; `_`
  from-state marks an entry constructor. Matches the shipped spelling — no change.

## Implementation (staged)

### 1. Lexer + Syntax.rs (I7)
- Add `pub const KW_STATE_DECL: &str = "state";` to `Source/Syntax.rs` with a
  D-STATE-DECL doc comment (distinct from the capital `KW_STATE` marker). Add to
  the keyword list near `KW_TAG` (`Syntax.rs:1361`/`1378`).
- `Source/Lexer/mod.rs:32-54`: map `s == Syntax::KW_STATE_DECL => TokKind::KwState`.
  Add `TokKind::KwState` variant (`Tokens.rs`) + display string (`Tokens.rs:185`
  region): "the keyword `state`".
- `#Transition`'s `->` already lexes/parses — no change.

### 2. Parser — `state TypeName { A, B, C }` block
- AST: add `Item::StateDecl(StateDecl)` to the `Item` enum (`AST.rs:498`); 
  `pub struct StateDecl { is_pub: bool, type_name: String, type_name_span: Span,
  states: Vec<(String, Span)>, span: Span }`.
- New `state_decl` parser in `Source/Parser/Items.rs` (modeled on `tag_def`
  `:1946`): `pub? state Ident { Ident (, Ident)* ,? }`. Comma-separated state
  idents; empty block allowed (parses, sema warns/errs per below). Hook into the
  item-dispatch match (where `KwTag`/`KwStruct`/`KwEnum` branch) at both
  top-level and module scope (`Parser/Modules.rs`).
- Parser error: trailing junk / non-ident in the brace list → reuse the standard
  `expect_ident` path; a `:` or `=` inside (struct-field habit) gets a teaching
  error (mirror the `#Context` `E0760` style) — proposed **E0766** parser.

### 3. Sema — bounded set, typo-check, transition graph
- `StateTable` (`State.rs:37`): add `states: HashMap<String, HashMap<String, Span>>`
  (type_name → {state → decl span}) and collect `Item::StateDecl` in `add_items`
  (`:66`). Keep `entry_ctors`/`requires`/`transitions` as-is.
- Registration (`Registration.rs:132` neighborhood): register each `StateDecl`'s
  `type_name` for name-uniqueness; a duplicate state name within one block →
  **E0765** (`defined_twice`-style). A `state` block whose `TypeName` matches no
  declared type → reuse the existing unknown-type diagnostic, or a fresh
  **E0764** "no type named `X` for this `state` block".
- **Typo-check (new E0763):** every state named in a `#State(S)` /
  `#Transition(From -> To)` marker on a method of `Type` must be a member of
  `Type`'s declared set. Unknown name → **E0763** "`Type` has no state `S`" with a
  did-you-mean suggesting the nearest declared state (states are now bounded, so
  this is decidable — the whole point of B over A). `_` entry from-state is exempt.
- **Dead-end warning (new L0802):** build the transition graph per type
  (from `transitions`); any declared state with no outgoing `#Transition(That -> _)`
  is a dead end → **L0802** warning (default-on lint), "state `X` has no outgoing
  transition — the machine can't leave it". A `#State(X)`-only terminal (read
  guard, no transition out) is the intended dead end and still warns by default;
  half-built machines compile (ratified). Entry-only / unreachable states are out
  of scope for v1 unless trivially derivable.
- Dataflow (`StateCtx`, `:131`): unchanged in shape — seeding, advancing, joining
  all still key off the same `requires`/`transitions`/`entry_ctors` maps. Wrong-
  state call stays **E0150** (`State.rs:571`), reused verbatim.
- Drop the loose-tag assumption: states are no longer `tag`s. Markers resolve
  against the `state` set, not the tag namespace. Update the `State.rs:26-27`
  module note.

### 4. Codegen — erasure (I3)
- `Item::StateDecl` emits **nothing** (like `Item::Tag` today). Confirm the
  codegen item match has an empty arm for it; markers already erase. Golden on
  113 verifies generated Rust is identical to the untagged program.

### 5. Diagnostics (I4 — codes + snapshots)
- `docs/spec/diagnostics.md`: add **E0763** (unknown state in a typestate marker),
  **E0764** (`state` block for an unknown type), **E0765** (duplicate state in a
  block), **E0766** (parser: `:`/`=` inside a `state` block), **L0802** (dead-end
  state warning). Next free sema code after E0762; next lint after L0801.
- New ui fixtures under `tests/ui/`: `typestate_unknown_state.{jet,stderr}` (E0763),
  `typestate_dead_end.{jet,warn}` (L0802). Keep `typestate_wrong_state` (E0150)
  but **migrate it to the `state {}` form** (replace its `tag` decls).

### 6. Example + golden (I5)
- Rewrite `examples/features/113_typestate.jet`: replace the three
  `tag Pending {}` / `tag Confirmed {}` / `tag CheckedIn {}` lines with a single
  `state Reservation { Pending, Confirmed, CheckedIn }` block; update the header
  comment to describe the declaration block. Method markers and `main()` unchanged.
  Expected output `expected/113_typestate.out` stays identical (states erase).

### 7. Formatter (round-trip required for new syntax)
- Teach `Source/Formatter` to emit `Item::StateDecl` (`state TypeName { A, B, C }`,
  one-line if short, else one state per line) and add a fmt STABILITY test — per
  the standing rule that new syntax needs formatter emission + a stability test,
  not just idempotence.

### 8. Tests
- Extend `tests/typestate.rs`: `state` block parses; E0763 unknown-state;
  E0765 duplicate state; L0802 dead-end; entry `_` still works; erasure unchanged.
- `nix develop -c cargo test` (filter the dev-shell banner); bless ui snapshots
  with `UPDATE_EXPECT=1` only after hand-checking against diagnostics.md format.

### 9. Docs
- `docs/spec/spec.md`: add a Typestate section (it currently has none) describing
  the `state` block + `#State`/`#Transition` markers, bounded set, erasure.
- `docs/spec/syntax-decisions.md`: under D-STATE1 (`:2042`) and the D-STATE-DECL /
  D-STATE-TRANS rows (`:2934-2936`), mark **implemented** (date), note the loose-tag
  set was replaced by the `state {}` block; the D-STATE-TRANS spelling was already
  shipped (this card only formalizes it as ratified, no syntax change).

## Sequencing / gates

No upstream gate — both decisions ratified, the `->` glyph and marker parsers
already ship. Order: Syntax/lexer → parser + AST `StateDecl` → sema (table + E0763
typo-check + L0802 graph) → codegen empty arm → formatter → diagnostics + ui
snapshots → migrate 113 + wrong_state fixture → tests → docs. The E0150 dataflow
core is untouched; risk is concentrated in the new bounded-set wiring and the
graph analysis for dead ends.
