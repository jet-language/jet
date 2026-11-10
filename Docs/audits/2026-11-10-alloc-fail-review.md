# D-ALLOC-FAIL1 review: one spelling per allocating operation

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-ALLOC-FAIL1. Earlier pass: BallotFix REPORT.md, section D-ALLOC-FAIL1.

## Question

Should each allocating operation have one spelling, such as `push`, failing as an `AllocError` value only
where a handler sits on the call or the function's contract names `AllocError`? That would retire the
`try_` twins.

## Findings that changed the ballot

1. **A's migration broke a ratified rule.** The detail said "jet fix rewrites each to the plain name", and the
   single-add paragraph said "jet fix writes it from the existing try_insert form". Both are jet fix rewrites
   of retired spellings. D-TEACH-RECONCILE1=A (ratified 2026-11-05) and the owner's rule recorded on
   2026-11-10 (cards #6622 and #6281) bar them until D-LANG-MIGRATE1 is decided. A now rewrites the
   in-tree callers in the deleting commit, with no tool rewrite or teaching. Code outside the tree that
   calls `try_push` then gets a generic unknown-method error. That is a new, named loss.
2. **The census counted Tower JSON.** The earlier `git grep` over `Tools` matched 42 lines in
   `Tools/tower/.tower/*.json`. It also matched comment lines. Restricted to `*.jet` with comments dropped,
   53 lines call a `try_` allocation form:

   | Form on the call | Lines | Under A |
   |---|---|---|
   | `??` on the call | 31 | handled call, same meaning |
   | `?` propagation | 13 | under an AllocError contract, rule two |
   | matched in place (`== {`, `== .Ok(…)`) | 2 | an arm table on the call |
   | bound, returned or ignored | 7 | returned ones sit under a contract (for example `Examples/Memory/Allocation/ProgramAllocator/run.jet:4`). Bound ones become a match. The ignored ones are `tests/ui/fallible_ignored*` rows, which retire with the twins |

   `.add(…) ??` lines: 69 in 51 Jet files. The earlier 72 in 53 included JSON.
3. **The Map.add hazard is now probed.** a08 runs `m.add("k", 1) ?? -1` twice and prints `first -1, second 1`.
   So `??` on add already means "no old value" (`Collections.jet:426`). A keeps that meaning.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| a01 twins | `loaded 3`, `abc` |
| a02 `??` on plain push | E0405 |
| a03 contract names AllocError around plain pushes | `loaded 3` (the pushes still stop on failure today) |
| a04 heap budget | `grew 100` |
| a05 `#Context(allocator: …)` block value | E0003 |
| a06 fixed allocator, `try_alloc` 7 then 9 | `7 -1` |
| a07 user `with_capacity` | E0311 |
| a08 `m.add(k, v) ?? -1` (new) | `first -1, second 1` |

## Peers (sourced)

- Rust: `Vec::try_reserve` is a twin beside `reserve`. <https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve>
- Zig: every allocating call can fail and takes an allocator. <https://ziglang.org/documentation/master/#Choosing-an-Allocator>
- C++: a failed `new` throws `std::bad_alloc`. <https://en.cppreference.com/w/cpp/memory/new/bad_alloc>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| jet fix rewrites the twins | D-TEACH-RECONCILE1=A | **Failed**. Replaced by an in-tree rewrite in the deleting commit |
| 92 try_ calls | Recount on `*.jet` | **Failed**: 53. The rest were JSON and comments |
| Map.add `??` keeps its meaning | a08 | Holds |
| Every in-tree call keeps its meaning | Breakdown by form | Holds for 31 + 2 on-call handlers and for the `?` and returned results under contracts. Bound results are rewritten as a match |
| Fallibility is lexical | Callee context | Holds: decided from the call's own line and its function's signature |

## Remaining losses

- A handler on the call decides whether it can fail. Hover marks it.
- A callee that does not name AllocError still stops inside.
- Out-of-tree callers of `try_` names get a generic error until D-LANG-MIGRATE1 is decided.
