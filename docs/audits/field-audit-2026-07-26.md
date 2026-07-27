# Rust access modes and Jet memory safety

Date: 2026-07-26

## Answer

Jet has two confirmed implementation gaps in one safety rule.
Three small programs pass Jet sema but fail native Rust compilation.
The failures violate I2 because rustc finds user-facing borrow errors after sema accepts the programs.

Jet should keep read as the default access.
The Rust corpus does not give enough evidence for a change to move-by-default.
It strongly supports read as the default receiver access.

## Rust corpus result

The main pass counted 200,135 parameter and receiver sites in 18 production Rust repositories.
The selected files contain 3,285,513 physical source lines.
The parser accepted all 8,711 selected files.

### Ordinary parameters

This table excludes `self` receivers.

| Access form | Count | Share |
| --- | ---: | ---: |
| By value, `T` | 75,944 | 57.4% |
| Shared read, `&T` | 40,735 | 30.8% |
| Exclusive write, `&mut T` | 15,602 | 11.8% |
| Total | 132,281 | 100.0% |

The median project uses 51.1% by-value, 36.9% shared-read, and 10.6% write parameters.
This median reduces the effect of large repositories.

### Method receivers

| Access form | Count | Share |
| --- | ---: | ---: |
| Owned receiver, `self` | 7,383 | 10.9% |
| Shared receiver, `&self` | 39,447 | 58.1% |
| Mutable receiver, `&mut self` | 21,024 | 31.0% |
| Total | 67,854 | 100.0% |

Rust methods borrow their receiver in 89.1% of measured receiver sites.
Jet's bare read receiver matches the most common Rust receiver form.

### All sites

| Access form | Count | Share |
| --- | ---: | ---: |
| By value or owned receiver | 83,327 | 41.6% |
| Shared read | 80,182 | 40.1% |
| Exclusive write | 36,626 | 18.3% |
| Total | 200,135 | 100.0% |

All borrow forms together account for 58.4% of the measured sites.
By-value and shared-read access differ by only 1.5 percentage points.

### By-value is not the same as move

Rust does not move every `T` parameter argument.
Rust copies a value when its type implements `Copy`.
Rust can move the value in the other cases.

The syntax pass split the 75,944 by-value parameters as follows:

| By-value shape | Count | Share of by-value | Share of all ordinary parameters |
| --- | ---: | ---: | ---: |
| Obvious `Copy` shape | 19,315 | 25.4% | 14.6% |
| Known owned standard shape | 5,819 | 7.7% | 4.4% |
| User, generic, or unresolved shape | 50,810 | 66.9% | 38.4% |

The known owned group includes `String`, `Vec`, `Box`, `Arc`, `PathBuf`, maps, sets, and handles.
The unresolved group can contain either `Copy` or non-`Copy` types.

Therefore, source syntax gives a 4.4% lower bound and a 42.8% upper bound for definite non-`Copy` takes.
It cannot give one honest true-move percentage without type resolution.

Runtime call frequency needs production telemetry.
Source code can measure signature frequency, but it cannot measure how often each function runs.

### Sensitivity pass

A second pass removed methods from trait implementations.
This pass avoids repeated copies of each trait signature.

| Site group | By value | Shared read | Exclusive write |
| --- | ---: | ---: | ---: |
| Ordinary parameters | 58.4% | 32.1% | 9.5% |
| Receivers | 11.0% | 55.5% | 33.5% |
| All sites | 44.7% | 38.9% | 16.5% |

The result stays stable.
Ordinary parameters favor by-value syntax.
Receivers favor borrowed access by a large margin.

## Per-project results

Percentages in this table apply to ordinary parameters.
The receiver columns use each receiver form as a share of all receivers.

| Repository and commit | Lines | Params | `T` | `&T` | `&mut T` | Receivers | `self` | `&self` | `&mut self` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [Alacritty `852e971`](https://github.com/alacritty/alacritty/tree/852e971cddfabe222d2d5bcda466e130f53af207) | 33,287 | 1,369 | 67.2% | 24.3% | 8.5% | 939 | 6.0% | 40.5% | 53.6% |
| [bat `7895139`](https://github.com/sharkdp/bat/tree/78951393e29bfd2f2a45f4326b9d2bb5e737dd2a) | 11,975 | 424 | 48.6% | 40.1% | 11.3% | 196 | 3.6% | 49.0% | 47.4% |
| [Cargo `e22c5be`](https://github.com/rust-lang/cargo/tree/e22c5be31b208baa8912aea960d3e73346041a75) | 130,293 | 6,262 | 33.8% | 55.4% | 10.8% | 2,504 | 10.9% | 69.0% | 20.1% |
| [fd `ca51233`](https://github.com/sharkdp/fd/tree/ca51233d277e5c0601ddb216948bcbcf0a56c80e) | 5,051 | 166 | 42.8% | 50.6% | 6.6% | 65 | 10.8% | 72.3% | 16.9% |
| [Firecracker `c1490c7`](https://github.com/firecracker-microvm/firecracker/tree/c1490c7983644f68facfc267c20156546e81bd4f) | 116,738 | 3,075 | 63.3% | 24.6% | 12.1% | 1,819 | 2.2% | 55.6% | 42.2% |
| [Helix `079a789`](https://github.com/helix-editor/helix/tree/079a789e8cb08ead67f19e1971a1b7438b37354b) | 98,175 | 4,545 | 59.7% | 22.3% | 18.0% | 1,569 | 11.9% | 48.9% | 39.2% |
| [Meilisearch `a765263`](https://github.com/meilisearch/meilisearch/tree/a765263e32424145adc18fa44462fd82c5666c56) | 176,795 | 7,760 | 50.4% | 38.0% | 11.6% | 3,369 | 12.4% | 70.6% | 16.9% |
| [Nushell `677af84`](https://github.com/nushell/nushell/tree/677af8423131f6a36d24230aa902bd4acb08df04) | 330,750 | 15,880 | 46.5% | 42.0% | 11.5% | 8,358 | 8.1% | 80.1% | 11.8% |
| [ripgrep `f9c05a9`](https://github.com/BurntSushi/ripgrep/tree/f9c05a949d1a0dc8e16dee28ca9605d38611faeb) | 35,748 | 1,152 | 51.8% | 40.2% | 8.0% | 1,027 | 4.3% | 55.7% | 40.0% |
| [rustls `bd9f7f5`](https://github.com/rustls/rustls/tree/bd9f7f59aa790da07010961188209e68384febe5) | 61,391 | 2,855 | 42.9% | 35.8% | 21.3% | 1,878 | 12.3% | 65.9% | 21.8% |
| [Serde `747814f`](https://github.com/serde-rs/serde/tree/747814f7d5fbab872df3b02f070c165b91bde062) | 26,254 | 1,429 | 48.4% | 43.5% | 8.1% | 928 | 64.1% | 23.9% | 12.0% |
| [Starship `d2db83a`](https://github.com/starship/starship/tree/d2db83a75de893d4f4efc8221a9bb9283bbcb4cf) | 50,770 | 818 | 25.8% | 72.7% | 1.5% | 170 | 13.5% | 80.6% | 5.9% |
| [TiKV `91ccfb2`](https://github.com/tikv/tikv/tree/91ccfb212677a43fd5255183ccf2afa4e3cec23e) | 572,318 | 23,023 | 62.7% | 26.9% | 10.4% | 11,794 | 7.0% | 57.9% | 35.1% |
| [Tokio `818e2dd`](https://github.com/tokio-rs/tokio/tree/818e2dd866e0d6b0e25ebad8508722efa3a2f8fb) | 122,178 | 3,389 | 56.7% | 14.5% | 28.8% | 3,237 | 22.9% | 52.7% | 24.4% |
| [uv `9e4d20c`](https://github.com/astral-sh/uv/tree/9e4d20c1b1eeb1c38b51c2d462ff3a32b8dda9a4) | 279,390 | 10,033 | 47.9% | 44.7% | 7.4% | 4,109 | 17.2% | 75.1% | 7.7% |
| [Vector `0d1e921`](https://github.com/vectordotdev/vector/tree/0d1e921e4527dbbcaa1ff114d35ea18c8f8939d6) | 374,825 | 10,377 | 59.8% | 30.8% | 9.4% | 5,838 | 17.3% | 62.1% | 20.6% |
| [Wasmtime `4993061`](https://github.com/bytecodealliance/wasmtime/tree/499306131f8d306ac29fec2c6da982c456c953d4) | 583,113 | 30,622 | 65.6% | 19.8% | 14.6% | 16,017 | 8.5% | 43.5% | 48.0% |
| [Zellij `812ad86`](https://github.com/zellij-org/zellij/tree/812ad861bc3f4a8ba6f411c1a3b1163bfef43766) | 276,462 | 9,102 | 71.4% | 23.8% | 4.8% | 4,037 | 4.3% | 48.7% | 47.0% |

## Method

The audit cloned each repository with `--depth 1` on 2026-07-26.
Each result uses the exact commit in the table.

The scan selected `.rs` files below a `src` directory.
It excluded test, benchmark, example, fuzz, fixture, snapshot, vendor, and third-party path components.
It also skipped items marked with `#[test]`, `#[bench]`, or `#[cfg(test)]`.

The parser used `syn` 2.0.119 with the full syntax tree and visit features.
It counted free functions, inherent methods, trait declarations, trait implementations, and foreign functions.
It did not expand macros or count closure parameters.

The primary classification uses the top-level parameter type:

- `T` is by value.
- `&T` is shared read.
- `&mut T` is exclusive write.
- `self`, `&self`, and `&mut self` use a separate receiver table.

`Option<&T>` counts as by value because the function takes the `Option` by value.
This rule also applies to wrapper types such as `Pin<&mut T>`.

The weighted total answers how many source sites use each form.
The project median checks whether large repositories control the result.

## Jet safety findings

### F1 — Nested argument access escapes sema

Severity: P0.

This program passes `jet check`:

```jet
fn see(x: Int) => Int { return x }
fn both(a: &Int, b: Int) { a += b }

fn run() {
    x := 1
    both(&x, see(x))
}
```

Jet JIT also runs it.
Native rustc rejects the generated Rust with E0503.
The mutable loan starts in the first argument, but the nested call reads the same place.

The direct-call checker keeps local `HashSet<String>` values for read and write names.
It checks only arguments whose outer expression is `Expr::Ident`.
It does not carry the active write into a nested argument expression.

Evidence:

- `crates/jet-sema/src/Sema/CheckerInfer/calls/direct_calls.rs:1006`
- `crates/jet-sema/src/Sema/CheckerInfer/calls/direct_calls.rs:1010`
- `crates/jet-sema/src/Sema/CheckerInfer/calls/direct_calls.rs:1343`

### F2 — A whole-place read alias has no complete conflict fact

Severity: P0.

This program also passes `jet check` and JIT:

```jet
fn both(a: &[Int], b: [Int]) {
    a[0] = b[0]
}

fn run() {
    xs := [1, 2, 3]
    alias :: xs
    both(&xs, alias)
}
```

Codegen emits `alias` as a Rust reference into `xs`.
Native rustc rejects the later mutable borrow with E0502.

The spec says that a bare place creates a checked read window.
The current binding path records range and returned views.
A plain identifier can still lower as a borrow without a matching live `ViewFact`.

Evidence:

- `docs/spec/spec.md:300`
- `crates/jet-sema/src/Sema/CheckerCore/bindings.rs:611`
- `crates/jet-sema/src/Sema/CheckerCore/bindings.rs:615`
- `crates/jet-sema/src/Sema/CheckerOwnership.rs:273`

### F3 — Imported calls use another incomplete access path

Severity: P0.

This imported call passes `jet check`:

```jet
helper.both(&x, x)
```

The helper has parameters `a: &Int` and `b: Int`.
Native rustc rejects the generated Rust with E0503.

The imported-call checker validates each argument.
It does not use the direct-call read and write sets.
This split lets the same source rule change with the call form.

Evidence:

- `crates/jet-sema/src/Sema/CheckerCoreLib/imports.rs:84`
- `crates/jet-sema/src/Sema/CheckerCoreLib/imports.rs:109`

### F4 — The completion status is too strong

Severity: P1 after the P0 fixes.

`docs/spec/spec.md` calls memory model v5 done.
The ratified decision text still says that the S9 final verification gate remains.
The roadmap also keeps adversarial Rust-level proof as a hard readiness gate.

The three native failures show why the remaining gate matters.
Jet must not state Rust-equivalent enforcement until the gate closes.

Evidence:

- `docs/spec/spec.md:209`
- `docs/spec/syntax-decisions.md:1417`
- `docs/spec/roadmap.md:237`

## What already works

The targeted ownership suite passed 98 tests.
The concurrency boundary suite passed 9 tests.

The current checker has strong coverage for these cases:

- A plain parameter cannot gain write access.
- A named move needs `^`.
- A hidden clone is an error.
- Overlapping range views conflict.
- Known disjoint fields and ranges can coexist.
- A live tracked view blocks owner movement, replacement, and resize.
- Returned views carry owner provenance through named boundaries.
- Tasks and channels reject borrowed or mutable captures.
- `Shared<T>` is the explicit synchronized shared-write path.

These rules are sound in design.
The gaps come from incomplete enforcement across lowering and call paths.

## Actual Rust capability gaps

This section uses a strict test.
An actual gap means safe Rust can preserve an essential memory property that safe Jet cannot preserve today.
A copy, a different representation, or a required unsafe call does not close that gap.

### G1 — Provenance-polymorphic views

Rust can return a reference chosen from several input owners.
Its lifetime relation keeps all possible owners live.
The standard `longest` example shows this exact form.

Rust can also put references in lists, tuples, options, results, enums, closures, and trait APIs.
This supports borrowing parsers, lending iterators, and zero-copy decoded trees.

Jet requires one stable source for each returned view slot.
It rejects source choice, list or tuple storage, view-returning function values, and open trait dispatch.
E2305 and E2307 report these cases.

This is one root gap in Jet's public provenance algebra.
Tower card #1197 records it at P0.
Ballot D-MEMPROVENANCE2 asks how the user surface should express it.

### G2 — Runtime-proven disjoint writes

Rust safe methods can prove disjointness at runtime.
`slice::split_at_mut` returns two mutable slices after one bounds check.
`HashMap::get_disjoint_mut` rejects duplicate keys before it returns mutable values.

Jet proves different fields and constant ranges.
Dynamic indices and ranges always overlap.
Jet also cannot store the resulting write views in a tuple or list today.

This blocks general safe multi-index edits and lending mutable iterators.
Tower card #1198 records it at P0.
Ballot D-MEMDISJOINT1 asks where the runtime proof should appear.

### G3 — Scoped borrowed concurrency

Rust scoped threads can borrow stack data.
The scope joins all children before borrowed data can leave.
Rust permits read sharing and disjoint mutable work under that bound.

Jet taskgroups also join children at scope exit.
Jet still applies the same owned-capture rule as `tasks.spawn`.
It rejects borrowed views and mutable captures with E1102 or E1101.

This leaves the taskgroup lifetime guarantee unused for memory access.
Tower card #1199 records it at P0.
Ballot D-TASKBORROW1 asks which scoped borrows `g.task` should admit.

### G4 — Reusable address-stability contracts

Rust `Pin` lets unsafe internals expose a safe no-move contract.
This supports self-referential state, intrusive collections, and address-sensitive futures.
Safe callers cannot move the pinned value.

Jet has raw pointers, arenas, Fixed storage, Pool IDs, and safe cleanup.
It has no reusable contract that stops safe code from moving an address-sensitive caller-owned place.
Changing the representation to a Pool ID does not preserve the same property.

Tower card #1200 records this gap at P0.
Ballot D-PIN1 asks whether Jet should use a borrowed pin, stable handle, or type marker.

### G5 — Sema does not enforce all ratified conflicts

F1, F2, and F3 are implementation gaps rather than missing language power.
They still create a real safety and compiler-boundary failure.
Jet accepts programs that native rustc rejects.

Tower card #1196 records the unified checker fix at P0.
It needs no ballot because it implements the current ratified rule.

## Polish and ergonomic gaps

These cases have a safe Jet rewrite.
The rewrite adds overhead, changes structure, or hides the direct memory intent.

### P1 — Local interior mutability

Rust `Cell` and `RefCell` can change local private state through a shared reference.
They avoid `Arc` and operating-system lock overhead.
`RefCell` guards check the same reader-writer rule at runtime.

Jet can use `Shared<T>` for the same result.
That solution adds synchronized shared ownership even when one thread owns the value.

Tower card #1201 records this polish gap at P1.
Ballot D-LOCALCELL1 asks whether Jet should add one local cell family or keep `Shared<T>` only.

### P2 — Long-lived guards and condition waits

Rust lock guards can span helper calls.
Mapped guards can expose one field.
A condition wait atomically releases and reacquires a lock.

Jet can rewrite many cases with one `Shared.read` or `Shared.edit` closure.
It can also use channels or a task loop.
Those rewrites are less direct for guarded protocols.

Tower card #1202 records this polish gap at P1.
Ballot D-SHAREDGUARD1 keeps short closures as the default and asks about an expert guard tier.

### P3 — Borrow-heavy library shape and teaching

Jet needs real ports that test the intended view model before it claims parity.
The existing Tower cards now have these priorities:

- #745: zero-copy parser audit, P0.
- #1162: indexed simulation update audit, P0.
- #1163: owner-backed collection view audit, P0.
- #1164: checker precision, diagnostics, and teaching, P1.

The first three are capability proofs.
The last is polish after the model works.

## Checked non-gaps

Jet already supports these Rust-class operations:

- Partial moves from struct fields.
- Last-use loan endings.
- Disjoint field and constant-range views.
- Returned read and write views with one stable owner.
- Automatic cleanup, explicit `close`, and deferred close.
- Explicit copies, moves, and write access.
- Raw pointer work inside a reason-gated `#Unsafe` region.
- Shared mutation through `Shared<T>`.
- Stable identity through `Pool<T>` and `Id<T>`.

The audit compiled and ran a partial struct move before classifying it as a non-gap.

## Default recommendation

Keep bare `T` as read in Jet.
Do not change the syntax decision from this corpus.

The reasons are:

1. Rust ordinary parameters favor by-value syntax, at 57.4%.
2. At least 14.6% of all ordinary parameters have obvious `Copy` shapes.
3. Rust receivers favor borrowed access, at 89.1%.
4. All source sites are almost tied between by-value and shared read.
5. Jet already passes scalar reads by value, so many Rust `T` sites do not argue for Jet `^T`.
6. The cost of a wrong move default is ownership loss at the caller.
7. The cost of a wrong read default is an explicit compiler request for `^`.

Jet's safety and beginner goals favor the second error.
The current data does not overcome that product rule.

If Jet optimized only ordinary parameter punctuation, move would win this corpus.
Jet does not optimize only punctuation.
It also makes ownership transfer visible at each call site.

## Ranked Tower backlog

### P0 — Safety and Rust capability parity

1. #1196: unify place access across every call and generated borrow.
2. #1197: support provenance-polymorphic returns and aggregates.
3. #1198: support runtime-proven disjoint write views.
4. #1199: support safe scoped taskgroup borrows.
5. #1200: support reusable address-stability contracts.
6. #745, #1162, and #1163: prove the model with borrow-heavy ports.

### P1 — Memory polish

1. #1201: add local interior mutability if D-LOCALCELL1 selects it.
2. #1202: add Shared guards and waits if D-SHAREDGUARD1 selects them.
3. #1164: improve checker precision, diagnostics, and teaching.

### P2 — Default telemetry

1. Add opt-in compiler telemetry for Jet parameter and receiver capability sites.
2. Run it on the Jet dogfood portfolio.
3. Reopen the read default only if Jet-native data shows a clear product loss.

## Footguns Jet already avoids

- A named ownership transfer is visible as `^` at the call site.
- A hidden clone cannot make a program compile.
- A bare parameter cannot gain write access from its body.
- Raw references do not form an untracked public surface.
- Shared mutation uses named synchronized types.
- Dynamic projections use conservative overlap when sema cannot prove separation.

Keep these properties while the P0 logic changes.

## Sources

- [Rust Reference: moved and copied types](https://doc.rust-lang.org/stable/reference/expressions.html#move-and-copy-semantics)
- [Rust Book: references and borrowing](https://doc.rust-lang.org/stable/book/ch04-02-references-and-borrowing.html)
- [Rust Book: lifetime-linked returned references](https://doc.rust-lang.org/book/ch10-03-lifetime-syntax.html)
- [Rust Reference: function parameters](https://doc.rust-lang.org/reference/items/functions.html#function-parameters)
- [Rust standard library: scoped threads](https://doc.rust-lang.org/std/thread/fn.scope.html)
- [Rust standard library: slice methods](https://doc.rust-lang.org/std/primitive.slice.html)
- [Rust standard library: interior mutability](https://doc.rust-lang.org/std/cell/)
- [Rust standard library: pinning](https://doc.rust-lang.org/std/pin/)
- [Rust Reference: higher-ranked trait bounds](https://doc.rust-lang.org/reference/trait-bounds.html#higher-ranked-trait-bounds)
- The 18 pinned repository links in the per-project table.
