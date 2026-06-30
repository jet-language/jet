# External inherent methods (`fn Type<sep>method`) + the `extend` question

**Card:** Tower #92 · **Epoch 3** · **Status:** planning → *deciding* (one open ballot)
**Depends on (ratified):** S28, D-IMPLDOT1 (`impl Type.Trait`), D-BIND1/D-BIND2, S83 (external-definition connector `~~`, ratified but never implemented).
**Open ballot:** **D-EXTMETH1** — spelling of an external inherent method definition. No code lands until D-EXTMETH1 is ratified.

## 1. What this feature is

An **external inherent method** is a method defined on a type *outside* its definition block. Distinct from implementing a trait (`impl Type.Trait`). This is the second entry point of the ratified inline-or-external principle in `philosophy.md`.

S83 ratified `fn Point~~dist(self)` as the spelling. D-IMPLDOT1 then retired `~~` for the trait-impl form (`impl Point.Trait`) but did not rule on the inherent form. This card resolves the inherent external method spelling.

**Orphan rule (identical across all options):** type must be defined in the current module; foreign types require a trait impl (`impl ForeignType.MyTrait`).

## 2. DECISION BALLOT — D-EXTMETH1

Rank only on: safety, beginner experience, consistency, one mechanical path (I8). Difficulty is not a factor.

### Option A — keep S83's `~~`: `fn Point~~dist(self)`
```jet
fn Point~~dist(self, other: Point) -> Float { … }
```
One connector for inherent only. Revives a token D-IMPLDOT1 deliberately retired. Two external connectors (`~~` inherent, `.` traits) — weakest one-path. Unusual token for beginners.

### Option B — mirror D-IMPLDOT1 with `.`: `fn Point.dist(self)` — RECOMMENDED
```jet
fn Point.dist(self, other: Point) -> Float { … }
fn Point.origin() -> Point { return Point.{ x: 0.0, y: 0.0 }; }
let d = p.dist(q);
let o = Point.origin();
```
One connector for impl + inherent + forwarding. `fn` keyword and PascalCase type name remove definition/call ambiguity. Completes the unification D-IMPLDOT1 started.

Errors:
```jet
fn Vec.shuffle(self) { … }
# E-EXTMETH-ORPHAN: cannot add inherent method to `Vec` — not defined in this module.
fn Point~~dist(self) { … }
# E-EXTMETH-SEP: `~~` retired; external methods attach with `.`. Write `fn Point.dist(self)`.
fn Point.x(self) -> Float { … }
# E-EXTMETH-FIELD-COLLIDE: method `x` collides with field `x` on Point.
```

### Option C — `extend Type { … }` block
```jet
extend Point {
    fn dist(self, other: Point) -> Float { … }
    fn origin() -> Point { … }
}
```
Groups methods cleanly. Adds a second block keyword beside `impl` and a third external shape. Two near-identical block forms (`extend`/`impl`) weaken one-path.

### Option D — no external inherent methods
Conflicts with ratified `philosophy.md` inline/external principle. Not recommended.

**Rec: B.** Wins or ties every ranked axis.

## 3. Build order (after D-EXTMETH1 ratified)

1. `crates/jet-foundation/src/Syntax.rs` — record ratified connector
2. Lexer — no change for B; add `~~` for A; add `extend` keyword for C
3. Parser — recognize `fn <Type> . <name> (…)` as external inherent method; produce same AST node as inline method
4. Sema — enforce local-only orphan rule (E-EXTMETH-ORPHAN); merge into type's method table; detect duplicates (E-EXTMETH-DUP) and field collisions (E-EXTMETH-FIELD-COLLIDE)
5. Codegen — lower to same Rust `impl Type { fn … }` as inline methods (parity test)
6. Diagnostics — E-EXTMETH-ORPHAN, E-EXTMETH-DUP, E-EXTMETH-FIELD-COLLIDE, E-EXTMETH-SEP (teaching error for retired spelling)
7. Tests — ui fixtures for each error; parity test (inline == external in generated code)
8. Example — one example showing inline + external method on same type with golden output
9. Spec — add to `docs/spec/spec.md`; amend S28/S83 entries; update ledger

## 4. Acceptance

- D-EXTMETH1 ratified and recorded
- External method on local type callable like inline method
- Parity: inline vs external → identical generated code
- Orphan rule enforced with E-EXTMETH-ORPHAN
- Field/dup collisions caught with messages pointing at first definition
- Retired spelling → teaching error
- `cargo test` green; every diagnostic has snapshot + `diagnostics.md` entry
