# Map entry syntax after bare-colon bindings: adversarial review

Date: 2026-11-24. Ballot: D-MAP-ENTRY1. Compiler: tip22, built from dev
e271dd7f56. Probes are short programs run with `jet run`; each is quoted with
its output.

The ballot asks what separates a map's key from its value. Today it is `:`, in
the type `[String: Int]`, the literal `["apple": 3]`, the map-yielding loop
item `n: n * n` and the pattern `["apple": a, ..]`. Since D-BIND-BARE1,
`name: value` is also every immutable binding. The recommendation (A) moves
every map separator to `=>`. This review attacks each claim A makes, and states
the strongest existing designs fairly.

## The strongest existing designs

- **Swift** writes `[String: Int]` and `["apple": 3]`, Jet's current spelling.
  Its best case: one character, no new token, and the spelling most readers
  of modern typed languages have seen. Swift avoids the clash only because it
  binds with `let x = 1`; its colon never means a binding. Swift writes bounds
  in `<T: P>` or `where`, never inside a collection type
  ([Swift book, Collection Types](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/collectiontypes/)).
- **Go** is the hardest counterexample. It binds with `x := v`, declares labels
  with `name:` and writes map entries `map[string]int{"apple": 3}`, and Go
  programmers do not report confusion between them. But Go never writes a
  binding as bare `name: value`, so the entry `k: v` and a binding never share
  a shape. Jet's immutable binding is exactly that shape.
- **Odin** declares with colons (`x : int = 1`, `x :: 1`, `x := 1`), like Jet.
  It chose `=` for map and struct entries (`map[string]int{"apple" = 3}`)
  ([Odin overview, Maps](https://odin-lang.org/docs/overview/#maps)). This is
  the strongest evidence that a colon-declaring language should not also use
  `:` for entries. `=` is not open to Jet: it reassigns an existing `:=`
  binding, so `[n = n * n]` would collide with reassignment exactly as
  `n: n * n` collides with binding today.
- **PHP, Ruby, Perl and Elixir** write `"apple" => 3`. Elixir also allows
  `apple: 3` for atom keys, which gives one idea two spellings; A has one.
- **Scala** writes `"apple" -> 3`, and **Kotlin** writes `"apple" to 3`. Both
  are ordinary pair-building functions, not map syntax.

## Claims and attacks

### 1. "After A, `:` always has a name on its left"

Every registered spelling with a colon was scanned
(`Compiler/JetFoundation/Data/Syntax/*.jet`). Map entries were the only
colon-separated form whose left side can be any expression. The `<:`/`:>`
fences are separate tokens.

The scan missed three forms:

- the string format selector `{value:Fixed(2)}` (spec.md, D-FORMAT-TRAIT1=A)
  puts an expression before the colon;
- a destructuring binding puts a pattern of names there:
  `[first, second]: [10, 20]` and `{min, max}: {min: 1, max: 9}` both run;
- braces build a map where one is expected, so `count({"a": 1, "b": 2})`
  prints 2 with a string before the colon.

**Verdict: the original claim fails.** The narrower claim holds: after A,
`:` never separates a key from a value. A's text now states the narrower
claim, and braces that build a map take `=>` too.

### 2. "`[key => 1]` can't be misread as a field"

The probe `key: "fig"` then `print([key: 1])` prints `[fig: 1]`, so the key is
the variable's value. `{key: 1}` is a record with a field named `key`. Today
the two shapes are identical, and only the bracket tells them apart.

**Verdict: holds.** Under A, `[key => 1]` has a shape no record or binding
uses.

### 3. "A map loop item no longer reads as a binding"

`[Int: Int]{loop n in 1..<4 -> n: n * n}` runs and prints
`[1: 1, 2: 4, 3: 9]`. Its item `n: n * n` is character for character a binding
of `n`.

**Verdict: holds.**

### 4. "`[T: Describe]` becomes available for a bound"

Today `[T: Describe]` is a map type from `T` to `Describe` values. A trait in
type position already means dynamic dispatch (`[Shape]`), so it is a valid
map. After A, a map type never has a colon, so the spelling is free.

The claim does not settle D-IMPL-STRUCTBOUND1. It only makes one of that
ballot's candidate spellings unambiguous.

**Verdict: holds, limited to "available".**

### 5. "`=>` means one thing"

Attack: the parser already recognizes `=>`. `xs.map(n => n * 2)` fails today
with "`=>` is not how Jet writes a lambda", fix `fn(n) { … }`
(Parser/Habits.jet:270-283). A reader from JavaScript or C# may read the map
loop item `n => n * n` as a lambda.

Mitigation: `=>` becomes an entry only directly inside a map's brackets,
typed head or pattern. Everywhere else it keeps today's lambda error.

The review found a hole here. Inside brackets, `handlers: [x => x * 2]`
would parse as a map keyed by `x` and report UnknownName. A therefore adds
a rule: an entry whose key is a name not in scope, and is used in its own
value, gets the lambda teaching. With that rule the compiler never accepts
a JavaScript lambda and always teaches the fix.

Residual cost: a JavaScript reader may misread a map-yielding loop. The
surrounding map type `[Int => Int]{…}` is the cue.

**Verdict: holds within that limit.**

### 6. "One typo, `>=` for `=>`, does not silently change meaning"

The probe `flags: [a >= b]` builds `[Bool]`. Passing it where `[Int: Bool]`
is wanted fails with ValueTypeMismatch at the call.

Under A, the swap `[a => b]` builds a map. Wherever an expected type exists,
the checker rejects the wrong one. In an untyped binding, the wrong type
survives until its first typed use. No reported defect class was found in
Rust or PHP, which have both tokens, but that absence is not proof.

**Verdict: holds only where a type is expected.** The ballot names this
cost.

### 7. "The migration is mechanical"

Size: about 6,800 map-type sites and 485 string-key literals in about 1,300
tracked files. A regex census over-counts slightly. 15 UI snapshots and 13
golden outputs show a map.

A text rewrite would be wrong. `[K: V]` must change, but `<N: Int>`, records
and labels must not. The LanguageRewrite row must rewrite parsed map type,
entry, loop-item and pattern nodes.

The first draft's cutover list was incomplete and mis-cited several
sites. The review added these:

- **Display.** Map display is emitted by the IR, not by Prelude: JetIR
  `BuildMaps.jet` `ir_render_map`. The empty map prints `[:]` today.
- **Debugger.** It renders map entries and recognizes a map by a colon in
  its type name, so it would silently show maps as lists after A.
- **Persisted type text.** Type text is hashed into receipt schema
  digests and written into store schema names.
- **Type-text tables.** Compiler tables carry type text that the type
  parser reads.
- **Type text in strings.** No syntax rewrite reaches it.
- **`jet find`.** It refuses every query that contains `=>`.
- **Editors.** Besides tree-sitter: the VS Code and TextMate grammars, the
  prebuilt Zed parser, and their generator.
- **Data files.** Ratchet and schema files that spell map types.
- **Missed patterns.** Two map-pattern spellings, the empty pattern `[:]`
  and the optional key `"pear"?: p`.

The ballot's option A now carries the full list with file and line for
each site. It decides the open points:
- `[]` matches an empty map;
- type names print `[K => V]`;
- receipt digests and store schema names change once, as one schema change;
- internal interning keys stay;
- type text in strings moves by a census of string literals.

Missing any of these is a failed cutover under the greenfield rule.

**Verdict: holds within limits.** The rewrite is mechanical only if it is
parse-aware, and the cutover list above is part of the build card.

### 8. "Swift familiarity is the only real loss"

Attack: Jet's own documentation, examples and every reader of the repository
learned `[K: V]`. Greenfield policy removes the compatibility cost but not the
retraining cost.

Mitigation: E0003 at `[String: Int]` gets a `jet fix` edit to
`[String => Int]`, as D-BIND-TYPE2 teaches the retired binding form.

**Verdict: holds within limits.** The retraining cost is named and cannot be
removed.

## Alternatives

- **B, the word `to`**, reads aloud, but Scala and BASIC readers know
  `1 to 4` as a range. In Kotlin, `to` builds a Pair. `to` would be a context word between two expressions,
  while `to:` stays a label.
- **C, keep `:`**, has Swift's best case and no cost, but keeps claims 2-4 as
  open problems.
- **D, `->`**, adds a third meaning to an arrow that already ends signatures,
  starts arms and yields loop items. `[Int -> Int]` sits one keyword from
  `[fn(Int) -> Int]`.
- **E, `=>` in entries only**, cuts the migration to about 500 sites. It
  leaves `[T: Describe]` a map, and puts two separators in one line,
  `[String: Int]{"fig" => 7}`.
- **F, keep `:` and mark computed keys** (`[(key): 1]`, with a bare
  `[key: 1]` as E0003), fixes the computed-key clash for a small migration.
  It leaves the loop-item and `[T: Describe]` clashes.
- **Odin's `=`** is not offered, because in Jet `=` reassigns a `:=`
  binding.

## Review pass

A fresh reviewer, G1Generation-2, attacked the first draft and sent it back.
Its findings, all answered in the ballot:
- the overbroad colon claim (claim 1);
- the `>=` and JavaScript-reader costs missing from the ballot's losses;
- an unfair comparison against D, since A also puts two arrows in a map
  loop;
- a false dichotomy in the lesson, and the missing Odin comparison;
- the wrong Kotlin claim;
- the lambda teaching inside brackets;
- an undercounted entry census, about 1,000 lines and not 485;
- the two missed map-pattern spellings;
- the incomplete cutover inventory.

It also raised two options: Odin's `=` and computed keys. The first is
recorded as rejected, the second added as F.

## Verdict

A holds within the limits stated above:
- the narrowed colon claim: `:` never separates a key from a value, but
  a destructuring binding and a format selector still put something other
  than a name before it;
- a JavaScript reader's lambda reading;
- an untyped `>=` swap;
- a parse-aware rewrite with the full cutover list.

Each of these limits is written into the ballot. Claim 1 failed as first written and holds as narrowed. No other claim
failed outright.
The open question the review cannot answer is taste: whether `=>` is worth
losing Swift's one-character spelling. That is the owner's choice.
