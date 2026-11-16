# Mine for Jet: Roc and Skip/Hack (2026-09-30)

## Verdict

Both talks back Jet's direction and sharpen it. Roc shows how much "magic" a
friendly language can infer: failure sets, effects, and in-place updates, with
one short mark (`?`) where control flow leaves. Hack and Skip show how a
checker stays interactive on tens of millions of lines: fine-grained reverse
dependencies, immutable shared facts, saved state, and a proof that
incremental equals clean. Jet already has most of Roc's safety surface and a
query engine to build Hack's architecture on. The gaps are a concurrent,
declaration-granular query engine, persisted compiler state, exact inferred
failure unions, and a clear, sigil-marked compile-time family.

## Sources and capture limits

| ID | Talk | Channel, date | Length |
|---|---|---|---|
| `12yVcgQHAK0` | [What makes Roc my forever language (Niclas Åhdén)](https://www.youtube.com/watch?v=12yVcgQHAK0) | Impure Pics, 2026-09-28 | 54:31 |
| `J31LlAUtoos` | [How Would You Type-Check 30 Million Lines of PHP? (Julien Verlaguet)](https://www.youtube.com/watch?v=J31LlAUtoos) | Developer Voices, 2026-09-25 | 2:21:56 |

- Both sources were new to the prior-art registry (checker status `new`).
- Only auto-captions exist. They garble names ("rock", "Okamel", "wasome").
  Technical claims were checked against primary sources: roc-lang.org docs and
  FAQ, the Roc repository mini-tutorial, the Joy repository, Hack/HHVM source
  (`server_type_check.ml`, `server_lazy_init.ml`,
  `hh_saved_state_verifier.ml`), the Facebook Hack launch post, and the Skip
  blog posts on mutability, macros, parallelism, and memoization.
- Numbers from the speakers (Roc 16 min to 16 s; Hack about 1M lines/s on 64
  cores) have no published benchmark and are low confidence.
- Richard Feldman's linked Roc 0.1 preview had no captions; only its
  description was used. Audience comments were out of scope.
- The claim ledger (30 Jet-relevant topics) was validated; raw per-talk
  ledgers had 68 and 80 claims.

## Roc in brief

Roc is "fast, friendly, functional". Pure by default, with `var $x` locals and
`for` loops for clarity. Each app runs on one platform that owns I/O and
memory. Key mechanisms:

- **Errors:** `Try(ok, err)` (`Ok`, `Err`). Postfix `?` returns early on
  `Err`; the guest calls it Roc's biggest advantage over Rust (35:12). `??`
  supplies a fallback. `crash` is only for impossible or unrecoverable cases.
- **Type magic:** principal inference everywhere. Error types are open tag
  unions the compiler infers, so a function that reads a file and calls HTTP
  returns `[FileReadErr(..), HttpErr(..), ..]` with no declared enum (36:18).
- **No Option:** Roc has no `Maybe`/`Option`; absence is a named `Err` tag or
  a domain union (FAQ).
- **Effects:** `->` pure, `=>` effectful, effectful names end in `!`; purity is
  inferred and checked.
- **Performance:** refcount uniqueness enables in-place updates of functional
  values (Perceus-style, 27:23).
- **Tooling:** `roc check`, `dbg`, `expect`, hot reload, hashed URL packages.

## Skip and Hack in brief

Hack kept Facebook's edit-save-refresh loop on a huge PHP codebase by making
the checker resident and incremental. Skip turned those lessons into a
language and runtime where mutability is visible in types, so memoization,
invalidation, and parallelism are safe. Ranked compile-speed mechanisms:

1. Reverse dependencies at declaration/function granularity; recheck only
   fanout (22:21).
2. Resident process with file watching (10:25).
3. Saved state loaded at startup, including from CI (74:35).
4. Immutable shared facts across workers (12:57).
5. Old/new declaration diff: body-only edits recheck one body (25:18).
6. Eager name index, lazy bodies, open files first (83:36).
7. Memory before cores: heap size limited the checker first (16:50).
8. Deterministic dumps and random revision replay: incremental must equal
   clean (27:48).
9. Affected-test selection by reachability (138:15).

Skip language ideas: `mutable` types, `freeze`, `.!field` updates, `~>` pure
closures, narrow derive-like macros with `#` names.

## Jet alignment and gaps

| Topic | Jet today | Class | Owner |
|---|---|---|---|
| `?` propagation mark | Implicit (S7) | owner gate | D-FAIL-VISIBLE1 |
| Inferred error union | Default `Err`; `err.as<T>()` recovers | owner gate | D-FAIL-INFER-UNION1 |
| `??` fallback, `?(text)` context | Shipped | implemented | — |
| No Option type | `T?` kept; three states ratified | rejected (conflict) | D-OUTCOME-SHAPE1 |
| `crash` boundary | Bugs stop, failures are values | implemented | — |
| Effect marks (`=>`, `name!`) | Inferred effect rows | owner gate | D-HONEST-SIG2 (option C added) |
| Local mutation | `:=` | implemented | — |
| In-place updates | Copy model ratified | in progress | #3837 |
| Inferred signatures | Signatures are written (S4) | rejected (conflict) | — |
| Exhaustive match | E0307 | implemented | #3716 |
| `dbg` | `print("{x=}")` only | owner gate | D-DBG1 |
| Reverse-dependency graph | Single-threaded in-memory memo (`crates/jet-queries/src/lib.rs:94-208`) | real gap | #3852 |
| Saved state | Run/build caches only | real gap | #3853 |
| Resident process | Opt-in session within D-JPK-NODAEMON1 | in progress | #2529 |
| Incremental equals clean | Hostile invalidation matrix | in progress | #2531 |
| Affected tests | Dependency paths recorded | owner gate | D-TEST-AFFECTED1, #3851 |
| Mutability in types, `freeze`, pure closures | `&`, `freeze(x)`, `-[]>` | implemented | — |
| Compile-time names with a sigil | Being redesigned | owner gate | #3662 ballots |

## What changed on the board

New cards: #3852 (concurrent declaration-granular query engine), #3853 (saved
compiler state, blocked by #3852), #3851 (affected tests, with ballot
D-TEST-AFFECTED1), and map card #3859, which holds the unified plan.

Logged evidence: #3837 (Perceus reuse), #2531 (random revision replay),
#2529 (latency, cancellation, open-file-first), #3740 (overlap with #3708).

Ballots updated with this evidence:

- **D-FAIL-VISIBLE1, D-FAIL-PUBSIG1:** Roc comparisons added.
- **D-HONEST-SIG2:** new option C, Roc-style `!` names on dishonest functions.
- **D-COMPILER-NS1:** Roc's `$` locals added as a loss for `$`; Skip's `#`
  macro names added as a precedent for `#`.
- **D-META-ROOT3, D-NAME-SPLICE1, D-DECL-META1:** Skip macro comparisons.

A duplicate inferred-error ballot was withdrawn in favor of the existing
D-FAIL-INFER-UNION1. The existing D-DBG1 already cites Roc.

The compile-time family on #3662 was cleaned so that each ballot holds one
decision, and every fact now carries a sigil:

| Ballot | Decides |
|---|---|
| D-COMPILER-NS1 | The glyph: `$` (recommended), lowercase `#`, or `%` |
| D-META-ROOT3 | The path: `T.$fields` members, `$type(T)` records, or `.reflect()` |
| D-BUILD-FACT3 | The roots: `$build`/`$package` or `$compiler.build`; amends D-BUILD-FACT2 |
| D-NAME-SPLICE1 | Computed names: `fn {name}` or `fn $name` |
| D-DECL-META1 | Declaration metadata: `$sites:` or reserved plain names |

Family map, repeated in each ballot: `prep` marks code that runs before the
program, the sigil marks facts the compiler knows, and `@` marks links.

## Unified plan (map card #3859)

1. Performance foundation: copy model (#3836, #3837), infallible lowering
   (#3740, #3708), then the query engine, saved state, session, verifier, and
   affected tests (#3852, #3853, #2529, #2531, #3851).
2. Failure and absence: three states (#3838), failure path reports (#3713),
   and the open failure ballots.
3. Reading surface: fact sigil family, `++`/`--` removal, `&&`/`||` grouping,
   `Type{expr}`, and the retired binding form.
4. Beginner loop: scaffold, quiet check, help order, glyph explain, authority,
   brackets, diagnostic sweep.
5. Measure: dogfood rerun (#2393).

## Avoid list

- Inferring parameter types on signatures: it hurts local reasoning, and S4
  keeps signatures written.
- A background daemon: D-JPK-NODAEMON1 forbids it. Saved state gives cold-start
  speed instead.
- Dropping `T?` for Roc's Option-less style: D-OUTCOME-SHAPE1 is ratified.
  Prefer domain enums in teaching.
- JSON shape inferred from later use (caption-only, no primary source).

## Beat vectors

- **Roc:** Jet adds typed effect rows, authority, and three tiers of one
  meaning. It can match Roc's inferred error unions and add a runtime failure
  path report Roc lacks.
- **Hack and Skip:** Jet can build decl-granular incrementality into the
  compiler from the start rather than retrofitting it. Jet sema already has
  immutability and effect facts that make caching safe.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| error-propagation-mark | decision | D-FAIL-VISIBLE1 |
| inferred-error-union | decision | D-FAIL-INFER-UNION1 |
| effect-marking | decision | D-HONEST-SIG2 |
| debug-print | decision | D-DBG1 |
| affected-tests | card | #3851 |
| reverse-dependency-graph | card | #3852 |
| signature-diff | card | #3852 |
| immutable-shared-ir | card | #3852 |
| eager-index-lazy-bodies | card | #3852 |
| memory-first | card | #3852 |
| compile-speed-target | card | #3852 |
| saved-state | card | #3853 |
| resident-server | card | #2529 |
| incremental-equals-clean | card | #2531 |
| in-place-mutation | card | #3837 |
| sigil-metaprogramming | decision | D-COMPILER-NS1 |
| no-option-type | decision | D-OUTCOME-SHAPE1=A |
| principal-inference | no-action | rejected: conflicts with S4 written signatures |
<!-- /audit-dispositions -->

Strongest unverified assumption: that Hack's speed came mainly from
declaration-granular fanout rather than from hardware scale; the talk gives no
reproducible benchmark.

## Adversarial pass (Opus 5.5, 2026-09-30)

Two read-only Claude Opus 5.5 reviewers attacked the compile-speed program
and the error-model ballots.

**Compile speed.**
- #3852: accept, with required changes.
- #3853: accept local saved state; reject remote verdicts.
- #2529: accept, with required changes.
- #2531: accept, and move it first as a split-out core oracle (#3860).
- #3851: reject its first wording; the reworded version is now option A.

Code facts found by the review:

- The query engine has no early cutoff: an equal value still bumps its
  generation (`crates/jet-queries/src/lib.rs:164`).
- Each edit prunes every memo (`:274`).
- Function cache keys include spans (`Validation.rs:571`), so inserting a
  line rechecks the rest of the file.
- Whole-bundle effect passes run on every check.
- Fingerprints use Rust's `DefaultHasher`, which is not stable.
- Only module paths are recorded, not function reachability.

Blockers designed away:

- **Body-dependent inferred facts.** Public inferred failure, effect, and
  ownership facts become interface facts, with value-hash early cutoff.
- **Remote state as a safety bypass.** CI state carries names, signatures,
  and edges only; it is signed and sample-verified.

The added criteria are on the cards. Build-time peers are decided in
D-BUILD-PEERS1, because Jai is in closed beta.

**Errors.**
- **D-FAIL-INFER-UNION1:** accept, with changes. The inferred union is a
  checked view over the one `Err` carrier, so the ABI stays stable. Other
  changes cover `Err` overlap, higher-order attribution, trait-bound calls,
  warnings for unreachable member arms, and an SCC fixpoint with a cap. The
  ballot was redrafted: option B was a straw man, because D-ERR-TRAIT1
  already removed E2402.
- **D-ERR-CASES1:** accept, with flat case patterns, a lint for shadowed
  case names, and a closure ownership rule.
- **D-FAIL-PUBSIG1:** redrafted to the owner's choice. A written contract
  is an optional ceiling that callers see. It is never required. `Never!`
  denies failure, and publishing classifies failure-set changes as semver
  changes.
- **Errors as effects (D-FAIL-EFFECT1):** keep the `T E!` spelling and share
  the effect-row law. Putting failures in the row would make `-[]>` mean
  "cannot fail", break pure but fallible functions, and add a case that
  changes the return shape to an erased row.
