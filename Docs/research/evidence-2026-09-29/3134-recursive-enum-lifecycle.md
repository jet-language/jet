# #3134: recursive enum ownership and storage lifecycle

Closer11, 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-dev/safe-jet.sh`.
Scratch: `~/.cache/jet-dev/scratch/Closer11/{tree,treebench}/`. Tier runner:
`~/.cache/jet-dev/scratch/Closer11/tiers.sh <file> [run interp aot]`.

## 1. Finite recursive tree (criterion 1, finite half)

`tree/finite_nodefer.jet`:

- `enum Tree { Leaf  Node(left: Tree, value: Int, right: Tree) }` with
  `build(1, 7)` and `build(1, 100000)`.
- It exercises in-order traversal into `&[Int]`, early-return search, and a
  consuming rebuild `scaled(^t, 10)` with `small = scaled(^small, 10)`.

`jet run` output (exit 0):

```
inorder: [1, 2, 3, 4, 5, 6, 7]
size: 7 sum: 28 depth: 3
contains 5: true contains 9: false
scaled: [10, 20, 30, 40, 50, 60, 70]
big sum: 5000050000 depth: 17
body done
```

The values are correct: 100000·100001/2 = 5000050000, and ⌈log2(100001)⌉ = 17.

The other tiers fail:

- **`--interpret`** (`tree/finite_tree.jet`, the same program plus a `Holder`
  resource with `impl Holder.Close` and two `defer close(^...)`):
  `E0956 MIR execution exhausted its fuel`.
- **AOT build**, two separate failures:
  - `tree/finite_tree.jet` fails in rustc with
    `E0405 cannot find trait __jet_Close in this scope` (`impl __jet_Close for ...Holder`).
  - `treebench/tree_jet.jet` (the Tree alone, no Close) fails in rustc with
    `E0308 expected Box<Tree>, found Tree` in the `Tree::__jet_Node { __jet_left: ... }`
    construction. The AOT backend does not box the recursive field of a
    named-field recursive variant; rustc's own help suggests `Box::new`. This is
    an I2 hidden-backend failure. The existing golden
    `Examples/features/types/recursive_enum.jet` uses a single positional
    payload `Wrap(Expr)` and does not exercise this path.
- **`jet run` with the defers**:
  ICE `Cranelift cannot execute MIR function ...::run: MIR captured place ... is not writable (Read)`.
  Removing the `Holder` / `defer` lines (`finite_nodefer.jet`) makes `jet run`
  pass.

## 2. Recursive generic tree (criterion 1, generic half): fails on every route

Repro files are `tree/gen_repro.jet`, `tree/gen_repro2.jet` and
`tree/gen_nonrec.jet`.

- **Positional generic payload.**
  `enum Node<T> { Tip(T)  Branch(Node<T>, Node<T>) }` is rejected by
  `E0003 A positional enum variant can carry only one value`. That is the
  current law: named fields are required.
- **Named literal.** `Node<String>.Branch{left: ..., right: ...}` fails with
  `E0119 There's no type called Branch` and
  `E0113 ... this returns Branch`. This is the same blocker the 2026-09-29
  overnight proof recorded. The target-typed `.Branch{...}` literal is
  accepted.
- **Payload bindings are not substituted.** Matching a `Node<String>` binds
  payloads at the *unsubstituted* type:
  - `.Branch(left, _) -> first(left)` fails with
    `E0112 first wants Node<String>, but this is Node<T>`;
  - `.Tip(item) -> item` fails with
    `E0124 branches produce different types: T and String`.

  The non-recursive `enum Slot<T> { Has(T) Empty }` shows the same thing: `get`
  on `Slot<String>` fails with `E0124`, and `Slot<String>.Has("x")` is typed
  `Slot`, not `Slot<String>` (`E0112`).
- **Generic function route.** With a generic function
  (`fn leaves<T>(n: Node<T>) -> Int` recursing with `leaves<T>(left)`), `jet run`
  fails with
  `ICE checked TIR cannot lower to MIR ... leaves__generic__String: missing checked function target leaves`.
  Without explicit `<T>` at the recursive call it fails with
  `E0904 Can't figure out what T should be here`.
- There is no user-level generic enum anywhere in `Examples/`. The only Core one
  is `Core/reactive/loadable.jet`, and it avoids payload-typed use at concrete
  types.

## 3. Copy / move / borrow / destruction and escaped views (criterion 2)

- **Borrow.** `inorder(t, &out)` and `contains(t, 5)` read the tree without
  consuming it. The binding stays usable afterwards (the next lines use it):
  observed on `jet run`.
- **Move.** `scaled(^small, 10)` consumes the old tree and rebinds the result.
  Using a moved-from `Holder` is rejected:
  `E0121 big was consumed by close, so it can't be used here`, with the note
  `closed resources cannot be copied or reused` (`tree/finite_tree.jet` before
  it was reordered).
- **Early return.** `contains` returns from inside the recursive match on
  `jet run`.
- **Normal teardown.**
  - Jet has no implicit drop hook to observe. Destruction is observed through
    `impl T.Close` + `defer close(^x)`, the D-SHAPE-RESOURCE2 mechanism.
  - That observation is **not achieved** for a resource owning a recursive
    enum: the JIT ICEs, AOT fails with the `__jet_Close` rustc error, and the
    interpreter runs out of fuel (section 1).
  - The teardown *time* is measured below by reassignment (`tree = Tree.Leaf`).
- **Escaped view.** `tree/recursive_enum_escaped_view.jet`: a `View<str>` of a
  subtree's `label`, returned from `fn leftmost_label(tree: Tree, spare: String) -> View<str> from spare`.
  `jet check` output:
  - `E2307 Returned string views need a stable owner relationship` at 9:42
    (`return label.trim()`), which is correct rejection under current law;
  - a **second** `E2307` at 12:11 on `spare.trim()`, which *is* derived from
    the declared `from spare` owner. That looks like a false positive and is
    recorded as a defect, not blessed.
  - This fixture is not blessed; per contract the UI snapshot is for Pip or the
    provers.

## 4. Measurements (criterion 3)

Same balanced tree over 1..N with phases build / traverse (sum) / teardown:

- Jet: `treebench/tree_jet.jet`, where teardown is `tree = Tree.Leaf`.
- Rust: `treebench/tree_rust.rs`, an `enum Tree { Leaf, Node(Box<Tree>, i64, Box<Tree>) }`
  built with `rustc -C opt-level=3`, rustc 1.97.1 (via jet-env).
- C: `treebench/tree_c.c`, malloc'd nodes with `NULL` as the leaf, built with
  `cc -O2` (gcc 15.3.0 wrapper).

Allocation counts and peak live bytes come from the LD_PRELOAD counter
`Tools/perf/alloccount.c`. Peak RSS comes from `getrusage(RUSAGE_CHILDREN)`.
Timing medians are over 5 runs, with phase timers inside the process.

| Representation | N | build median | traverse median | teardown median | allocations | peak live bytes | max RSS |
|---|---|---|---|---|---|---|---|
| Rust `Box` enum | 100000 | 4.09 ms | 0.54 ms | 1.06 ms | 200013 | 4,800,576 | 13,388 KiB |
| Rust `Box` enum | 1000000 | 40.93 ms | 6.08 ms | 15.64 ms | 2000013 | 48,000,576 | 64,248 KiB |
| C malloc nodes | 100000 | 2.01 ms | 0.10 ms | 0.34 ms | 100001 | 2,400,000 | 13,136 KiB |
| C malloc nodes | 1000000 | 21.98 ms | 1.12 ms | 4.00 ms | 1000001 | 24,000,000 | 32,404 KiB |
| Jet recursive enum (AOT) | any | unavailable: AOT does not compile (`E0308 expected Box<Tree>`) | | | | | |
| Jet recursive enum (`jet run`) | 1000000 | unavailable: timed out after 300 s (`exit=124`) | | | | | |
| Jet recursive enum (`jet run`, #CLI N) | 100000 | 1393.1 ms (1 sample) | 32960.2 ms (1 sample) | 0.005 ms (suspect: reassignment may not free) | unavailable: JIT process | unavailable | unavailable |

Rust boxes both children, including `Leaf`, so it makes 2N allocations. C uses
`NULL` leaves and makes N allocations. That is why C peaks at half of Rust's
live bytes.

The Jet `jet run` sample at N=100000 printed the correct `sum=5000050000`. It
then ended with the ICE `typed drop Int field 0 is unavailable` at teardown.
Traversal is about 41,000× the Rust `Box` median on the same N, and build
about 340×; both are single samples on the JIT tier. The JIT timings are
in-process phase timers, so they exclude compile time.

No "enum allocation-free" claim is made. A recursive enum allocates per node in
every peer measured.

## Verdict

**FAIL**, as a proof card.

- **Criterion 1: FAIL.** The finite tree passes only on `jet run`. The generic
  tree fails on every construction and traversal route.
- **Criterion 2: PARTIAL.**
  - Observed: borrow, move and early return (JIT), and escaped-view rejection.
  - Not observed: teardown through `Close`, because of the ICE on every tier.
- **Criterion 3: PARTIAL.** The Rust and C peers are measured; the Jet cells are
  unavailable (listed above with reasons).
- **Criterion 4: not runnable.**
  - `types/recursive_tree_lifecycle` cannot be blessed: no example compiles on
    all tiers.
  - The UI fixture is drafted but not blessed.
  - The `recursive-tree` gauntlet entry was not created, because the Jet arm
    cannot build.
  - No cargo golden, corpus or snapshot test was run.
