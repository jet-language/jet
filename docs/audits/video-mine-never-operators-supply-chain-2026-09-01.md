# Video mine: Condvar, Rust `!`, Python `NotImplemented`, the `arrayref` attack — 2026-09-01

Four new videos from the owner's "Jet Research Queue" playlist, mined in full (auto-caption transcripts, audiences, linked primary sources), cross-checked against the live Jet binary with 30 probe programs across AOT, `jet run`, and `jet eval`, and confirmed by a fresh-context second reader. The five other playlist entries were mined on 2026-08-28 and were not rerun (owner decision).

## Verdict

Every video is about the same thing once the narration is stripped away: **the language's default path decides the outcome, and a name for a fact is not the fact.** Rust spent ten years giving `!` a name; Jet has the name (`!Never`) and the ratified fact (D-NEVER1=C) but its own ratified sample does not compile. Python's `NotImplemented` answers "who owns this operand pair" at run time; Jet answers it statically, then returns zero on the default tier. Cargo runs `build.rs` by default; Jet's law forbids that, and the hidden `extern rust` bridge does it anyway. The Condvar video teaches a primitive Jet already ships; the actual gap is that the 100%-CPU loop compiles in silence.

**Seven verified defects, three of them P0 on the default tier. Two owner ballots filed. Nine Tower cards minted (#2430–#2438).**

## Sources and capture quality

| # | Video | Channel | Length | Captions | Comments | Linked sources | Ledger |
|---|---|---|---|---|---|---|---|
| 1 | Rust Condvar Explained: Stop Wasting 100% CPU | Semicolon | 10:29 | auto | 13/13 | video code repo (retrieved) | 23 claims |
| 2 | Rust just introduced a new "never" type | Let's Get Rusty | 5:35 | auto | 184/184 | RFC 1216, rust-lang/rust PR #155499, Edition Guide, rustc lints, 1.92 post (all verified) | 26 claims |
| 3 | "NotImplemented" is Awesome in Python | Indently | 6:39 | auto | 66/66 | Python data model and exceptions docs (verified) | 32 claims |
| 4 | a lot of people are upset | Low Level | 12:16 | auto | 282/~416 | Rust Foundation post 2026-08-20, RUSTSEC-2026-0260, Wiz analysis (all verified) | 22 claims |

Limitations, stated plainly:

- No video has creator subtitles. Narrated code is caption-reconstructed, not frame-verified; two caption errors were caught ("uninhibited", "from stir").
- Video 4's comment capture stopped at 282 of about 416 after an "incomplete data" retry.
- The probed binary was `target/debug/jet` built 2026-09-01 20:17 from commit 8b9933668 plus a 688-path dirty tree owned by a sibling session. Every defect below was therefore confirmed against **committed** source (`git show HEAD:…`) before it became a card. The binary disappeared mid-verification (sibling rebuild), so the second reader confirmed defect D2 from committed source only; the first reader's live outputs stand.
- The exact-key topic matrix across the four ledgers (103 claims, all enum-valid) found zero repeated keys; the cross-video repeats below were established by semantic review, per the skill.

Captures: `/tmp/jet-mine/jet-mine-<id>.transcript.txt`, `.info.json`, `<id>.claims.json`, `<id>.findings.md`, `<id>.manifest.json`; probes under `~/.cache/jet-test-scratch/mine-<id>/` and `mine-main/`.

## Reframes — where the popular reading is wrong

1. **Condvar video.** The lesson is not "learn `Condvar`". Jet already ships the mechanism (`Shared<T>`, `SharedGuard`, `Condition.new()`, `guard.wait(condition, predicate)`, `notify_one/all`) with predicate re-check, deadline failure (E3003), and AOT/`jet run` parity, all proven live. The real finding is about defaults: the busy loop the video warns against compiles in Jet with zero diagnostics and burns a core, while E0041 tells users Jet "does not share memory".
2. **Never video.** Rust did not add a type; it gave a name to a fact the compiler always had, and the ten years went to paying down two defaults it could not change (`()` fallback, the `Infallible` stand-in type). PR #155499 is merged for 1.100, not yet in stable docs. Jet already has both the name and the fact, but the fact does not flow through calls and the name ICEs in return position.
3. **NotImplemented video.** The sentinel is a run-time answer to a static question. Jet's static answer (D-OPDEF1 hooks, E0360/E0109/E2511) is coherent. The shipped implementation of that answer prints `Money { cents: 0 }` for `2 + 5` on the default tier and ICEs on AOT, including in the repo's own executable spec example.
4. **arrayref video.** "Would Rust have fixed this?" is the wrong question and the video says so; the right one is who holds build-time authority. Jet's ratified answer (D-JPK-SANDBOX2: sandbox or refuse, never host Cargo) is the strongest in the field on paper. The `extern rust` bridge calls `Command::new("cargo")` directly at HEAD, so the exact `arrayref` vector is open in Jet today.

## Verified defects — live contrasts

Each row is one program on the tiers named, with committed-source confirmation. Programs are kept verbatim under the scratch paths above.

### D1 — User operator hook: silent zero on the default tier, ICE on AOT, unsupported on eval (P0) → #2430

```jet
struct Money { cents: Int }
impl Money.Add { fn add(self, rhs: Money) Money -> { return Money{cents: self.cents + rhs.cents} } }
fn run() { a :: Money{cents: 2}  b :: Money{cents: 5}  print(a + b) }
```

| Tier | Output | Exit |
|---|---|---|
| `jet run` | `Money { cents: 0 }` | 0 |
| `jet build` | `internal compiler error: the generated Rust did not compile` | 101 |
| `jet eval` | E0956 `this operation isn't supported by the current evaluator yet` | 1 |
| control `print(a.add(b))` on `jet run` | `Money { cents: 7 }` | 0 |

The repo's own `examples/features/operators/user_defined.jet` prints `0,0 1,2 true true false` on `jet run` (expected `4,6 4,6 …`) and ICEs on AOT. HEAD: `Codegen/mod.rs:2399-2419` (traits emit `-> Self`), `TIR/lower/functions.rs:963-991` (arithmetic traits missing from the raw-protocol list), `jit/lower_ctx.rs:19822-19862`. Also on eval only: `1 + 2.5` and `#Comparable` `<` give E0956 while AOT/JIT print `3.5` and `true`.

### D2 — `Never` in a success return: ICE on AOT, runs on `jet run` (P0, I2 + I9) → #2431

```jet
fn impossible() Never -> panic("stop")
fn run() { print(1) }
```

| Tier | Output | Exit |
|---|---|---|
| `jet build` | ICE `codegen reached a construct the typed IR does not cover: the return type Never` at `Codegen/Items.rs:3391` | 101 |
| `jet run` (uncalled) | `1` plus L0104 | 0 |
| `jet run` (called) | E0956 | 1 |
| control `fn no_fail() Int !Never -> 9` | `9` on both tiers | 0 |

HEAD: `core_surface.rs:208-210`, `Sema/mod.rs:356-370`, `TIR/subset/types.rs:115-120`, `Codegen/Items.rs:3378-3395`.

### D3 — Divergence does not flow through calls; D-NEVER1's own sample fails (P0) → #2431

```jet
fn stop(message: String) { panic(message) }
fn load_name(found: Bool) String -> { return if found -> { "Ada" } else -> { stop("name missing") } }
```

`jet check`: `E0124: this if's branches produce different types: String (text) and Unit`. The same shape with `process.exit(3)` (typed `Never` in `fixed_sigs.rs:791-794`) gives `E0124: … Never and Int` with the fix line "make both branches produce `Never`". Works: `panic(…)` and braced `return` in the same branch position (`7`, `6` on both tiers); `process.exit` as a statement (`before`, exit 3). HEAD: `CheckerInfer/expr.rs:2816-2834` joins equal, numeric, and Todo only.

### D4 — `jet new` writes an unparseable manifest (P1, introduced by HEAD commit) → #2433

```text
$ jet new sample && cd sample && jet run run.jet
Error [E1206]: `package.jet` has a shape error
 Why: `deps:` needs a record value
```

The emitted file ends in `deps: {` with no closing brace; `deps: {}` makes both tiers print `hello, world`. HEAD `Package/Convert.rs:124-160`; commit 8b9933668 changed `deps: .{{\n}}` to `deps: {{\n`. `tests/pkg.rs:2607-2618` and `tests/jet_test.rs:1120-1161` exercise this template and should be red on master.

### D5 — `extern rust` bridge runs host cargo outside the ratified sandbox (P0 risk) → #2432

```jet
extern rust "base64@0.22" { fn b64encode(s: String) String = "base64::encode" }
fn run() { print(b64encode("hi")) }
```

Prints `aGk=` on `jet run` and AOT: the path is live. HEAD `FFI.rs:2209-2229` builds `Command::new("cargo") build`; `:2703-2725` accepts any `name@version`; `:3439-3457` writes plain registry `[dependencies]` lines; no `bwrap`/sandbox reference in the file; `which cargo` under jet-env is the plain toolchain binary. `docs/spec/spec.md:3020-3035`: "Host Cargo is never an unsandboxed fallback." A transitive `build.rs` therefore runs on the host before Jet can object, which is exactly RUSTSEC-2026-0260. Two copy defects ride along: E0704 says build scripts "aren't supported yet" (cargo runs them first), and `jet run` reports an unresolvable crate as E0956 "report this as a compiler bug" while `jet build` reports E0704. The ordinary imported `build.jet` hook was probed and is inert on both tiers (`Driver/mod.rs:2796-2799`, `:3592-3598`): a genuine structural win.

### D6 — Busy-wait on `Shared` state compiles in silence (P2) → #2435

```jet
loop flag.ready == false {}     // consumer; producer sets flag after 500 ms
```

`jet check`: 0 diagnostics. AOT witness: `user 0.38 sys 0.11` of a `0.50` s interval. The parked forms (`guard.wait(condition, predicate)`, channel receive) complete the same program idle. HEAD `diagnostic-rows.md:195-199` lists L0202/L0206 and no hot-loop rule.

### D7 — Copy that contradicts law (P2) → #2434

| Code | What it says | What is true |
|---|---|---|
| E0041 (`mutex()`) | "Jet avoids shared mutable state … not sharing memory" | `Shared<T>`/`Condition` are shipped (`spec.md:587-640`) |
| E2511 (`Vec3 * Float`) | "Operator overloading is blessed on the closed built-in math family ONLY" | D-OPDEF1=A user hooks ship; `syntax-decisions.md:2950-2952` repeats the stale text |
| E0301 (`impl Int.Mul`) | "`Int` names a type that doesn't exist … define `struct Int` first" | `Int` exists; built-ins do not take user impls |
| E0907 (`fn mul(self, rhs: Int)`) | "must match the trait signature exactly" | never shows the expected signature |
| E0109 (`3 * money`) | "make both sides the same type" | not actionable for a user type |

## Cross-video synthesis

| Theme | Videos | Jet evidence | Systemic answer |
|---|---|---|---|
| Sema admits what a tier cannot lower | 2, 3 (and 1 by contrast: condition waits are parity-green) | D1, D2: three different outcomes for one program across AOT, `jet run`, `jet eval` | Seed fixtures into the hardening differential corpus (#2337); no new mechanism |
| Copy that lies about the surface | 1, 3, 4 | D7 plus E0704 and the JIT E0956 copy | One copy card; the diagnostics ledger should cross-check decision ids it cites |
| Defaults decide the outcome | 1, 4 | busy loop silent; bridge cargo unsandboxed | Lint (#2435); route the bridge through the ratified action path (#2432) |
| A mature language paying for a default it cannot change | 2, 4 | Rust `()` fallback and `Infallible`; cargo `build.rs` | Jet is greenfield: fix the position rules and the bridge now, before either becomes a compatibility obligation |

## Corrections and disputed claims

| Claim | Source | Status |
|---|---|---|
| Rust `!` is "stabilized" | video 2 | Merged for milestone 1.100 (PR #155499, 2026-08-24); stable docs still say nightly-only at capture. Both true; the report pins the release target |
| "uninhibited" type | auto-caption, video 2 | caption error; "uninhabited" (three audience corrections) |
| `break` "coerces to U32" example | video 2 | shown without its enclosing loop; audience confusion is about the cropped demo, not the mechanism |
| `proc-macro1` is a typosquat of `proc-macro2` | video 4 | verified (Wiz); one audience correction rightly notes `proc-macro2` is legitimate |
| Cargo credentials are per-repository | podcast claim quoted in video 4 | corrected by the creator: one `$CARGO_HOME/credentials.toml` |
| Rust sees fewer attacks because the ecosystem is smaller | video 4 | hypothesis; audience offers deployment-pattern explanations; no denominator measured by anyone |
| Condvar wake latency is "free" | video 1 implies | audience correction: sleep/wake latency exceeds spinning; not measured here either |
| Python `NotImplemented` demo reached the sentinel | video 3 | one audience comment says the Number demo did not; captions cannot settle it |

## Jet alignment — shipped versus ratified-and-unbuilt

| Surface | Status | Proof |
|---|---|---|
| `Shared<T>`, `SharedGuard`, `Condition`, `guard.wait`, `notify_one/all`, deadline E3003 | shipped, AOT and `jet run` parity | `condition_basic.jet`, `condition_spurious.jet`, `condition_deadline.jet` |
| Blocked-at-exit report E3013, child panic identity in E3001 | shipped (#2277); the 2026-08-28 D5 finding is closed | `task_blocked.jet`, `task_panic.jet` |
| L0206 long guard scope lint | shipped | `guard_scope.jet` |
| `Int !Never` no-failure contract | shipped | `P2_never_domain.jet` |
| `panic`/braced `return` as diverging branches | shipped | `P1_panic_branch.jet`, `P6_return_branch.jet` |
| Bottom fact through calls (D-NEVER1=C sample) | ratified, unbuilt | D3 |
| `Never` return position | ratified as a later ballot by D-NEVER1; ballot D-NEVER2 filed | D2 |
| D-OPDEF1 user hooks `impl T.Add` | ratified, shipped surface, broken ABI | D1 |
| Mixed-type operands (`money * 3`, `2.0 * v`) | undecided; ballot D-OPMIX1 filed | `op_hetero.jet`, `op_reflected.jet`, `lane_scale.jet` |
| Cross-type `==`, `Int + String` rejected (E0109) | shipped on all tiers | `p3_cross_type_eq.jet`, `p2_numeric_mismatch_left.jet` |
| Struct equality derived; `#!Equatable` opt-out E0312 | shipped on all tiers | `p8_pair_*.jet` |
| Imported `build.jet` hook inert at runtime | shipped | `inert-clean` project, marker never written |
| `jet inspect audit` fails closed without lock or signed feed (E2611) | shipped | `inert-clean` before and after `jet fetch` |
| Sandboxed Cargo actions for the FFI bridge | ratified (D-JPK-SANDBOX2), unbuilt on the bridge path | D5 |
| Release maturity window (D-JPK-FRESHNESS1=D), lookalike names (#1912), curated tier (#1911) | ratified; not re-probed against a registry here | decisions and cards read back |
| Registry-backed `jet fetch` provisioning the signed advisory feed | unknown; measurement step on #2432 | E2611 after a local-path fetch |

## Avoid list

| Mistake | Evidence | Jet exposure |
|---|---|---|
| Poll a shared flag in a loop | video 1; D6 | exposed: legal and silent; lint card #2435 |
| Treat a notification as proof of readiness | video 1 [06:30–09:24]; `condition_spurious.jet` | immune: `guard.wait` re-checks the predicate on every tier |
| Say the language "does not share memory" while shipping locks | E0041 vs `spec.md:587-640` | copy defect, #2434 |
| Give a fact a name ten years late (`!`), or a stand-in type (`Infallible`) | video 2; PR #155499 | Jet has the name today; D-NEVER2 decides its return position while the surface is greenfield |
| Let a narrow type name leak into positions the backend cannot lower | D2 | exposed until #2431 |
| Answer "who owns this operand pair" at run time with a sentinel | video 3; audience recursion and order-dependence reports | immune by construction (static hooks); the ABI must be fixed (#2430) |
| Return `False` when an operator is unsupported | video 3 [03:03–04:15] | immune: E0360/E0109/E2511 |
| Treat memory safety as supply-chain safety | video 4; RUSTSEC-2026-0260 | law is right; bridge path exposed (#2432) |
| Run fetched code with ambient host authority | `build.rs` payload chain (Wiz) | imported hooks immune; FFI bridge exposed |
| Publish a scaffold that cannot parse | D4 | exposed since 8b9933668; #2433 |
| Trust sema admission as tier proof | D1, D2 | the general lesson of this mine; hardening corpus is the structural fix |

## Beat vectors — ranked, shipped versus unbuilt

| # | Vector | Evidence | Status | What a competitor must give up to match |
|---|---|---|---|---|
| 1 | Fetched code never executes with host authority: sandbox-or-refuse build actions, inert imported hooks, fail-closed audit, typosquat and maturity policy | D-JPK-SANDBOX2, E1275/L0205, E2611, #1911–#1913, D-JPK-FRESHNESS1 | law ratified; imported hooks and audit gate shipped; **bridge path unbuilt (#2432)** | Cargo's host-process build model |
| 2 | Predicate-safe condition waits with one protocol embedded on every native tier | `SharedProtocol.rs` reached from AOT, JIT, and eval | shipped | duplicated wait semantics across runtimes |
| 3 | Static operator ownership with definite diagnostics, no reflected protocol | E0360/E0109/E2511 on all tiers | shipped surface; **ABI unbuilt (#2430)**; mixed operands undecided (D-OPMIX1) | Python's dynamic sentinel |
| 4 | A named no-failure contract (`Int !Never`) with no stand-in type | `P2_never_domain.jet` | shipped | Rust's `Infallible` history |
| 5 | Compiler-inferred divergence for beginners plus a checkable declared contract for libraries | D-NEVER2 option B | ballot | TypeScript's declaration/expression inference split |
| 6 | Bounded waits through one context deadline instead of per-API timeouts | E3003 on both tiers | shipped | per-primitive `wait_timeout` APIs |
| 7 | Tier parity as a product guarantee | I9 | **currently a loss**: D1 and D2 prove three outcomes for one program | nothing; Jet must fix its own gap first |

## Agent-optimality — the five quantities

| Q | This mine's evidence | Jet position |
|---|---|---|
| a Verdict fidelity | D1 is a wrong verdict (0 for 7) on the default tier; D6 is a missing verdict; D5 is a verdict the law promised and the bridge skips | weakest quantity today; all three are carded |
| b Verdict latency | not measured; every probe finished under 60 s on AOT and `jet run` | no finding |
| c Verdict actionability | E0124's "produce `Never`", E0907's hidden signature, E0301's "define `struct Int`", E0704's false "not supported" all send the agent the wrong way | #2431, #2434, #2432 |
| d Context economy | one 8-line program isolated each defect; Jet's `Condition` program has no `Arc`/`Mutex`/`Condvar` names | strong once the copy stops lying |
| e Repair determinism | every defect maps to one committed seam with line numbers | strong; the cards carry the seams |

This mine moves **a** and **c**. Jet is weakest on **a** for the default tier and on **c** for diagnostic copy that contradicts ratified decisions.

## Surface coverage

**Covered with proof** (ran live): `Shared<T>`, `shared`, `SharedGuard<T>`, `guard_edit`, `Condition.new()`, `guard.wait(condition, predicate)`, `notify_one()`, `notify_all()`, `#Context(deadline:)`, `channel<T>(capacity:)`, `Sender<T>.send`, `Receiver<T>.receive`, `task.group`, `.join()`, `.detach()`, E3003, E3013, E3001, E0041, L0206, L0104; `Never` (error side), `Int !Never`, `core.process.exit`, `panic`, value `if`, braced `return`, `loop {}`/E0073, E0124; `impl T.Add`/`Equatable`/`Comparable`, `#Comparable`, `#!Equatable`/E0312, `Ordering`, `Mat3 * Vec3`, `Vec3(…)`, `Vec3{…}`, E2511, E0360, E0109, E0907, E0301; `extern rust "crate@version"`, `jet new`, `jet run`, `jet build`, `jet check`, `jet eval --pure`, `jet fetch`, `jet inspect audit`, `jet trust list`, `authority.holds`, `BuildContext`, `BuildPlan`, `b.plan()`, E1206, E2611, E0704, E0956.

**Worth checking**: `Shared.guard_read()`, `notify_one` fairness with several waiters, cancellation during `guard.wait`, condition behavior on the web tier; generic callbacks carrying `Never`, `core.sys.stop`, exhaustive pattern elimination on uninhabited arms; `Sub`/`Mul`/`Div` user hooks (same ABI seam as D1), `Vec3.splat`, a Core `scale` method; `jet trust grant/explain/revoke`, `jet inspect provenance`, `jet inspect sbom`, `jet registry vendor`, `policy.exceptions`, D-JPK-FRESHNESS1 against a live registry, whether registry-backed `jet fetch` provisions `.jet/advisories.db`; fixed-size array views as the native replacement for `arrayref`'s whole purpose.

**Missing**: a hot-loop lint (#2435); `Never` in return position (D-NEVER2); typed right operands and reverse hooks (D-OPMIX1); a bridge-specific Cargo action receipt and sandbox (#2432); a hostile transitive-`build.rs` fixture proving deny-before-launch; a parseable default manifest (#2433); Rust's `wait_timeout`/`wait_while` names (absent by design; the deadline context is the one mechanism).

## Owner gates — ballots filed

| Decision | Card | Question | Recommendation | Dissent recorded |
|---|---|---|---|---|
| D-NEVER2 | #2437 | May a function declare `Never` as its return type? | **B**: declared `Never` with inference kept; `fn serve() Never !E` for servers; generics out of scope | GPT-5.6 adversarial pass preferred **C** (explicit `Never` required across package edges) because inferred divergence is not yet in the published API record; kept B because Jet already publishes inferred effects the same way |
| D-OPMIX1 | #2438 | One rule for `money * 3`, `v * 2.0`, `2.0 * v` across user hooks and lane types | **D**: typed right side `impl Money.Mul<Int>` plus a reverse hook on the built-in side owned by the user type's package (Rust's orphan rule; Jet already enforces E0902) | The author's base draft recommended **C** (`#Commutative` marker); the adversarial pass showed a mis-marked hook gives silently wrong answers, reverses evaluation order, and cannot spell third-type results, so the recommendation moved to D. Both beginner readers preferred C's ergonomics |

Both ballots ran all six passes: base, boil-the-ocean, hybrid, cooperative, fresh rli5 beginner (`NeverBeginner`, `OpmixBeginner`), and rival-family adversarial (Sol). No decision was ratified by the agent.

## Tower

| Card | P | Title |
|---|---|---|
| #2430 | P0 | User operator hooks give the wrong value on jet run, an ICE on AOT, and E0956 on jet eval |
| #2431 | P0 | Divergence does not flow through calls; `Never` in a success position is an ICE (D-NEVER1=C incomplete) |
| #2432 | P0 | extern rust bridge builds crates with host cargo outside the ratified sandbox (the arrayref vector) |
| #2433 | P1 | jet new writes an unparseable package.jet (`deps: {` never closed) since 8b9933668 |
| #2434 | P2 | Diagnostic copy contradicts ratified law: E0041, E2511, E0301, E0907, E0109, and the core-library concurrency page |
| #2435 | P2 | Lint: unbounded loop polling Shared state (busy-wait), with a fix to Condition or channel receive |
| #2436 | P3 | Core surface ledger: bind Condition.notify_one and notify_all rows to fixtures |
| #2437 | P1 | Design: named non-returning contract, ballot D-NEVER2 (lane: decide) |
| #2438 | P1 | Design: mixed-type operands under one rule, ballot D-OPMIX1 (lane: decide) |

Prior cards reused rather than duplicated: #496 (D-OPDEF1), #2252 (E0956 batch), #427 and #398 (build hooks, sandbox), #2277 (task observability; its findings are now verified closed), #1291 and #1842 (never-path work). `tower lint` after the writes reports nothing owned by this run except a shared-ref `duplicate-suspect` heuristic between #2432, #2434, and #2435.

## Prioritized actions

| P | Action | Card |
|---|---|---|
| 1 | Fix the operator hook ABI and the evaluator paths; run the repo example on three tiers | #2430 |
| 2 | Contain the `extern rust` bridge; hostile fixture; fix E0704 and the JIT copy | #2432 |
| 3 | Make the bottom fact flow through calls; turn the `Never` ICE into a diagnostic | #2431 |
| 4 | Close the `deps: {}` template regression and its red tests | #2433 |
| 5 | Owner decides D-NEVER2 and D-OPMIX1 | #2437, #2438 |
| 6 | Copy card, busy-wait lint, ledger fixtures | #2434, #2435, #2436 |

## Files and links

- Ledgers and findings: `/tmp/jet-mine/kHpEolpE3pU.*`, `/tmp/jet-mine/wpqiH56ITZo.*`, `/tmp/jet-mine/xUBIbhPC_rQ.*`, `/tmp/jet-mine/uQV6hYwyjMY.*`, `/tmp/jet-mine/manifest.json`
- Probes: `~/.cache/jet-test-scratch/mine-kHpEolpE3pU/`, `mine-wpqiH56ITZo/`, `mine-xUBIbhPC_rQ/`, `mine-uQV6hYwyjMY/`, `mine-main/`, `mine-verify/`
- Ballot JSON as filed: `~/.cache/jet-luna/ballot-D-NEVER2.json`, `~/.cache/jet-luna/ballot-D-OPMIX1.json`
- Videos: https://www.youtube.com/watch?v=kHpEolpE3pU · https://www.youtube.com/watch?v=wpqiH56ITZo · https://www.youtube.com/watch?v=xUBIbhPC_rQ · https://www.youtube.com/watch?v=uQV6hYwyjMY
- Primary sources: https://rust-lang.github.io/rfcs/1216-bang-type.html · https://github.com/rust-lang/rust/pull/155499 · https://doc.rust-lang.org/edition-guide/rust-2024/never-type-fallback.html · https://blog.rust-lang.org/2026/08/20/supply-chain-attack-on-arrayref/ · https://rustsec.org/advisories/RUSTSEC-2026-0260 · https://www.wiz.io/blog/rust-supply-chain-attack-on-arrayref-significant-overlap-with-dprk-campaigns · https://github.com/suryanox/dump/tree/main/cdvar

## Strongest unverified assumption

That the `extern rust` bridge's `Command::new("cargo")` path is reachable for an arbitrary transitive `build.rs` exactly as the committed source reads. This mine deliberately ran only a benign crate and never executed a hostile package; #2432's first criterion is the local hostile fixture that turns this reading into a proof.
