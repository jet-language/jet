# M5 — Collections & one string story

**Blocked on decisions:** S33 (generic brackets), S37 (list literal),
S38 (map literal), S39 (indexing & out-of-bounds), S40 (slicing),
S41 (string model & `Char`), S42 (numeric types & conversions).
Depends on M3 (Option) and M4 (`or`, runtime report).
**Error codes:** E0501+ / L0501+.

## Goal

`List[T]` and `Map[K, V]` as built-in generic types bridging `Vec` /
`HashMap` (users never see those names), literals, iteration, indexing
with great failure behavior, copy-based slicing (no exposed references —
tier 1), and exactly one string type with a real API. After M5, programs
like wordcount, grep-lite, and CSV munging are pleasant.

## Surface (uses ballot recommendations — substitute ratified choices)

```jet
fn main() {
    var nums: List[Int] = [3, 1, 2];
    nums.push(4);
    nums.sort();
    print(nums[0]);                  // 1 ; out of bounds = runtime report
    print(nums.get(99) or -1);       // safe access returns Int?
    val mid = nums[1..2];            // inclusive slice (S22), copies

    var counts: Map[String, Int] = [:];
    for word in "the quick the".split(" ") {
        counts[word] = (counts.get(word) or 0) + 1;
    };
    for key, count in counts {
        print("{key}: {count}");
    };

    for c in "héllo".chars() { print(c); };
}
```

### Type & literal rules

- Built-in generic types: `List[T]`, `Map[K, V]` (S33 brackets). These
  are compiler-known; user generics arrive in M9.
- List literal `[a, b, c]`; empty `[]` needs a context type (mirrors M3
  `none`, code E0501). Map literal `["k": v, …]`; empty map `[:]` (S38).
- Map keys: `Int`, `String`, `Bool`, `Char`, and payload-free enums in
  v1 (E0502 otherwise — "this type can't be a map key yet").
- `Char` (S41): what `"x".chars()` yields and what single-quoted `'x'`
  literals denote. Printable, comparable, usable in switch conditions.
- `String` API counts **characters** (Unicode scalar values), not bytes:
  `s.len()`, `s.chars()`, `s.contains(sub)`, `s.starts_with(p)`,
  `s.ends_with(p)`, `s.trim()`, `s.split(sep) -> List[String]`,
  `s.replace(a, b)`, `s.to_upper()`, `s.to_lower()`, `s.repeat(n)`,
  `s.slice(a..b) -> String` (char positions). No `s[i]` indexing
  (E0503 teaches `.chars()` / `.slice(…)` — strings aren't arrays).
- Conversions (S42): `Int.parse(s) -> Int or ParseError`,
  `Float.parse(s) -> Float or ParseError`, `x.to_string()`,
  `n.to_float()`, `f.to_int()` (truncates — say so in docs). No `as`
  keyword (recognized only for a teaching error, E0026).

### Core methods (exact v1 set — implement all, nothing more)

`List[T]`: `len`, `push`, `pop -> T?`, `insert(i, v)`, `remove(i) -> T`,
`get(i) -> T?`, `first -> T?`, `last -> T?`, `contains(v)`,
`index_of(v) -> Int?`, `reverse`, `sort` (T comparable), `join(sep)`
(T printable), `clear`, `is_empty`. Mutating methods require a `var`
receiver (reuses M2 E0202 machinery via `mut self`).

`Map[K, V]`: `len`, `insert(k, v)`, `get(k) -> V?`, `remove(k) -> V?`,
`contains_key(k)`, `keys -> List[K]`, `values -> List[V]`, `clear`,
`is_empty`.

## Sema rules

1. Literal element types must agree; E0504 names the first mismatching
   element ("this list started as `Int` but item 3 is a `String`").
2. Indexing: `xs[i]` needs `Int` index (E0505); `m[k]` key type must
   match (E0505). Read `m[k]` on a missing key is a **runtime** report;
   write `m[k] = v` inserts. (S39 — if owner picks Option-returning
   indexing instead, `xs[i]` typechecks as `T?` and E0506 ensures
   handling; the plan's tests cover both shapes, pick one.)
3. Slicing `xs[a..b]` is **inclusive** (S22) and **copies** (no exposed
   references, C1). Cloning cost is real: lint L0501 fires on slices in
   loops (mirrors L0202's voice). Negative or reversed bounds are
   runtime reports.
4. **Iteration safety:** `for x in xs` takes a shared borrow of `xs` for
   the loop body; calling a mutating method or assigning into `xs`
   inside → E0507, worded with the M2 human framing ("while the loop is
   reading `nums`, nothing may change it"; fix: collect changes into a
   second list, or loop over `0..xs.len()-1` indices). This is the
   milestone's crown diagnostic — invest in it.
5. `for key, value in map` iterates entries (sorted by key for
   determinism — document it; lowering uses a BTreeMap-backed iteration
   or collected+sorted pairs so golden outputs are stable).
6. Loop variables are immutable bindings; element type inference flows
   from the collection.
7. `List`/`Map`/`Char` are built-in type names: redefining → E0106.
8. Ownership: lists/maps of non-clonable types are themselves
   non-clonable; `take`/moves compose through (extend M2 tables).
   `xs[i]` on a list of non-copy `T` yields a clone with lint L0201
   (consistent with M2) — `.get` + `is` borrows nothing in tier 1, it
   also clones; document honestly in docs/01.

## Codegen lowering

| Jet                  | Rust                                            |
|----------------------|--------------------------------------------------|
| `List[T]` / `[a, b]` | `Vec<T>` / `vec![a, b]`                          |
| `Map[K, V]` / `[:]`  | `std::collections::BTreeMap<K, V>` / `BTreeMap::new()` (BTree for deterministic iteration) |
| `xs[i]`              | runtime-checked helper `jet_index(&xs, i, file, line)` → friendly report, exit 70 |
| `xs[a..b]` slice     | helper that bounds-checks then `xs[a..=b].to_vec()` |
| `s.chars()`          | `s.chars()` adapter; `Char` → `char`             |
| `s.len()`            | `s.chars().count()` (chars, per S41)             |
| method calls         | thin runtime-helper functions in a generated prelude module — codegen stays dumb (R1), all signatures fixed here |

Runtime report for bounds (same shape as M4's):

```
The program stopped: the list has 3 items, so position 99 doesn't exist
  --> file.jet:7
```

## Diagnostics to register

E0501 empty literal needs a type · E0502 type can't be a map key ·
E0503 strings aren't indexable (teach `.chars()`/`.slice`) · E0504
mixed-type literal · E0505 wrong index/key type · E0506 (reserved for
Option-indexing variant of S39) · E0507 collection changed while a loop
reads it · L0501 slice copy inside a loop.
Teaching: E0026 `as` casts → `.to_float()` etc. · E0027 `append`/`add`
→ `push` · E0028 `dict`/`HashMap`/`Vec` → `Map`/`List` · E0029
`s[i]` on strings handled by E0503.

## Examples & tests

- `examples/15_lists.jet` — build/sort/slice/join.
- `examples/16_wordcount.jet` — THE exit-criteria example: split, count
  into a map, print sorted results.
- `examples/17_strings.jet` — chars, unicode (`"héllo"`), trim/split/
  replace, parse with `or` defaults.
- Golden stderr tests for out-of-bounds and missing-key reports.
- ui fixtures for all E05xx/L0501 + teaching errors, with `.fixed.jet`.
- An ownership fixture: mutating a list inside its own `for` loop, plus
  the fixed version using an index loop.

## Out of scope

`map`/`filter`/`reduce` (need closures — M8 adds them to these same
types), sets, deques, sorting with custom comparators (M8), lazy
iterators (never — eager copies until profiling demands otherwise),
string formatting beyond interpolation, byte-level string APIs (`.bytes()`
deferred to M10 alongside file I/O), user-defined generic types (M9).

## Suggested implementation order

1. syntax.rs: `List`, `Map`, `Char`, literal sigils per ratified ballots.
2. Parser: generic type brackets, list/map literals, slice expressions,
   `for k, v in` (fixtures first).
3. Sema: built-in generic type representation (this is the template M9
   generalizes — keep it a clean `Type::Generic(name, args)` shape),
   rules 1–8.
4. Codegen prelude module of runtime helpers + lowering table.
5. Iteration-safety borrow rule wired into the M2 checker.
6. Examples, stderr goldens, teaching errors, docs, snapshots.
