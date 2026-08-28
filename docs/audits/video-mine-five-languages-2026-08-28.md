# Video mine: Rust trait solver, Python features, ES2027, Go 1.27 ×2 — 2026-08-28

Five videos from the owner's "Jet Research Queue" playlist, mined in full (transcripts,
audiences, linked primary sources), cross-checked against the live Jet binary with 30+
probe programs. 456 ledger claims across 5 resources; every load-bearing Jet finding below
was re-run first-hand from the main checkout (`target/debug/jet`, built 2026-08-28).

## Verdict

The five videos agree on one meta-lesson without knowing it: **every mature language is
paying down a default it can no longer change** — Rust's old trait solver (4-year dual-solver
rewrite), JS's `Date` (9 years to Temporal), Go's case-insensitive JSON v1 (needed a v2),
Python's silent `dict.get` None. Jet's greenfield defaults already sit on the right side of
all four, and the probes prove most of them live. The mine's bad news is equally crisp: the
**default `jet run` tier is where Jet's story breaks** — four fresh E0956 reproducers — and
**user `Display`/`Debug` is broken on every tier** (silently ignored on run, ICE 101 on
release), violating ratified D-DISPLAYDBG1.

## Sources & capture quality

| # | Video | Channel | Len | Captions | Comments | Ledger |
|---|---|---|---|---|---|---|
| 1 | The biggest change to the Rust compiler is here! (`aPL6y2oJjMw`) | Let's Get Rusty | 5:04 | auto only (low conf) | 100/~101 | 94 claims |
| 2 | 5 Uncommon Python Features I Love (`sQ1Q96-Vhjk`) | Indently | 15:09 | auto only | 300 | 111 claims |
| 3 | JavaScript's Biggest Update in Years — ES2027 (`DLT6n3wCkuc`) | Better Stack | 7:01 | auto only | 139/139 | 81 claims |
| 4 | Everything New in Go 1.27 (`rTgROnXIwnI`) | Coding with Patrik | 18:06 | auto only | 54/54 | 105 claims |
| 5 | Go is becoming shockingly good (`c7eLIsaDL7U`) | Awesome | 8:10 | **creator** captions | 202/202 | 65 claims |

Limitations: four of five had auto-captions only (technical terms OCR-mangled; all
load-bearing names re-verified against primary sources). No video frames beyond low-res
storyboards; on-screen code marked as such. One 429 on a duplicate caption track. The Rust
and JS/GORT/GOLANG lanes needed salvage/correction passes after timeouts; all deliverables
are complete. Durable artifacts (ledgers `jet-mine-<ID>.claims.json`, findings
`jet-mine-<ID>.findings.md`, probes `jet-probe-{RUST,PY,JS,GOLANG,GORT}.md`) live in
`~/.cache/jet-luna/mine-2026-08-28/artifacts/`; JS probes were re-run by the orchestrator.

## Reframes — where the popular reading is wrong

1. **Rust video**: the "compile times" graph is **CPU instruction counts** on
   root-package-clean `cargo check`/`cargo build` (`perf stat -e instructions:u`), not wall
   time, not clean-vs-incremental. "New solver may be faster" is unsupported as a build-time
   claim. The real story: replacing a solver in a shipped language cost ~4 years, a dual-solver
   coexistence period, 208+ issue backlog, and it still ships with immature diagnostics —
   that cost structure, not the perf plot, is the lesson.
2. **ES2027 video**: half the features aren't ES2027 — Decorators is Stage 2.7 (video says 3),
   Signals is Stage 1, and Bun had already shipped Temporal by default two days before upload.
   The durable surfaces are Temporal's five-type split and `using`'s scope-exit protocol.
3. **Go videos**: the "free performance" allocator numbers are qualified upstream (≤30% only
   for <80-byte allocations, ~1% program-level, +60 KB binary, `GOEXPERIMENT` opt-out), and
   generic methods **cannot implement interfaces** — a compatibility-locked seam Go can never
   close. The loudest audience signal (150 likes) is that Go's generics spelling is eroding
   its readability identity.

## Verified defects — live contrasts (all re-run first-hand)

### D1 — User `Display`/`Debug` is broken on every tier ⟶ new card

D-DISPLAYDBG1 ratifies: "interpolation `{}` calls it [`display`]; `{value:Debug}` selects it."
One program, two tiers, two different failures:

```jet
struct Book { pages: Int }
impl Book.Display { fn display(self) String -> "display:{self.pages}" }
impl Book.Debug   { fn debug(self)   String -> "debug:{self.pages}" }
fn run() { book :: Book{pages: 42}; print("{book}"); print("{book:Debug}") }
```

| Tier | Command | Result |
|---|---|---|
| default | `jet run` | exit 0, prints `Book { pages: 42 }` twice — **user impls silently ignored** |
| release | `jet run --release` | **ICE, exit 101** — generated Rust: `E0407 method debug is not a member of trait JetDebug`, `E0119 conflicting implementations of JetDebug`, `E0053 display has incompatible type (String vs Result<String, JetErr>)` |

Sema accepts an impl codegen cannot lower (I3 violation), tiers disagree (I9 violation), and
the ratified contract is unmet on both. Silver lining: the ICE routing itself worked exactly
as designed — branded report, generated file preserved, exit 101, rustc hidden (I2 held).

### D2 — S40 slice spelling drift ⟶ new card

S40 ratifies `s.slice(a..b) -> String`. Live:

| Input | Result |
|---|---|
| `text.slice(0..4)` | **E0311** "`slice` isn't a method on this value" |
| `text.slice(0, 4)` | works, prints `abcd` |

The shipped method table has the two-Int form only; the ratified Range form does not exist.

### D3 — Four fresh E0956 default-tier reproducers ⟶ attach to #2252

| Program shape | Diagnostic |
|---|---|
| call a named fn taking an enum, body is `if x == { .Variant… }` match | ``E0956 `call `describe`` isn't supported`` |
| `impl` method with its own type parameter, called | ``E0956 `method `convert`` isn't supported`` |
| `uuid.v4()` | ``E0956 ``core.crypto.uuid.v4()`` isn't supported`` |
| `core.time.new(...)` (inside zoned-datetime construction) | ``E0956 ``core.time.new()`` isn't supported`` |

The same programs succeed via `--release`/AOT. The beginner-facing tier is currently Jet's
weakest tier; that inverts the mission ordering. (#2251/#2252 own this; these are new rows.)

### D4 — Typed lambda does not drive two-param generic inference ⟶ new card

`apply(f: fn(T) U, x: T) U` with an annotated lambda still demands explicit type arguments
(E0904); `apply<Int, String>(…)` works. Go 1.27 just shipped exactly this inference class.
Bonus actionability bugs from the same probes: the E0904 fix text suggests an annotation even
when `Int` and `String` can never share one `T`, and the E0904/E0112 group is emitted twice.

### D5 — Blocked tasks die silent; task failures lose identity ⟶ new card

A task blocked forever on a full channel hangs until external timeout with **no Jet report**;
a panicking child's `panic` text and any task identity are absent from the parent's
`TaskFailure` diagnostic. Go 1.27 ships `goroutineleak` profiles + labeled tracebacks;
`docs/spec/architecture.md:200-204` explicitly documents non-detection. Jet has `--observe`
(bounded live snapshot, no names) — the seam exists, the report doesn't.

## Beat vectors — ranked, shipped vs unbuilt

| # | Vector | Evidence | Status |
|---|---|---|---|
| 1 | **One trait solver, from day one.** Rust's strongest lesson is a cost Jet structurally cannot incur: probes confirm a single sema resolution path (`TraitRegistry`, one `infer_subst_inner`), product diagnostics (E0904/E0905/E0112 with what/why/fix), and a depth guard (E0909 at 64 levels; 65-deep probe rejected cleanly in <2s where Rust hit E0275 hangs/regressions) | probe RUST Q1–Q3 | **shipped** |
| 2 | **Dates: compile-time rejection + zoned arithmetic live.** `DateTime{"0"}` → E0155 at *compile time* (JS: `new Date("0")` silently = 2000-01-01; Temporal took 9 years and only fixes it at runtime). The video's own NY→London DST flight demo runs today: `20:00 -04:00` + 7h → `07:00 +00:00`, offsets shown, 3600→0 transition observed | probe JS Q1, first-hand | **shipped** (E0956 asterisk on one constructor path, D3) |
| 3 | **Ownership-checked cleanup beats disposal protocols.** `defer close(^r)` only; double-close is a *compile-time* E0121 move error; general `defer print(…)` is E0003 with the design stance in the Why text. JS `using` (runtime `SuppressedError`) and Go `defer` cannot catch double-dispose before running | probe JS Q2, first-hand | **shipped** |
| 4 | **Structural immunities, each taught by a diagnostic.** Import-time execution impossible (E0620 — makes JS `import defer` unnecessary); no user mutex/atomics surface (E0041: "share data through channels" — makes `Atomics.pause` unnecessary); map `[k]` is strict + `get` returns `?V` forcing `??` (kills Python's silent-None class) | probes JS Q5/Q7, PY Q6 | **shipped** |
| 5 | **JSON defaults Go needed a v2 to reach.** Case-sensitive exact field matching, unknown fields ignored + `#DenyUnknownFields` (E2412), `[]` vs absent distinguished, typed `FieldError` decode, observed deterministic map key order. Go must run v1-backed-by-v2 with `GOEXPERIMENT=nojsonv2` escape hatches forever | probe GOLANG Q4 | **shipped** (pin map-order determinism with a test) |
| 6 | **Reactivity as one general mechanism.** `reactive.effect`/signals ran live in a *terminal* program (200→300→450 propagation). JS Signals is Stage 1, framework-fragmented, with no built-in effect | probe JS Q6, first-hand | **shipped** (native tier) |
| 7 | **Zip with policy family.** short / pad(None) / pad-fill / strict(panic "zip length mismatch") all live — the exact ES2027 `Iterator.zip` mode surface | probe JS Q3, first-hand | **shipped** (no zipKeyed analog) |
| 8 | **UUID v4/v5/v7 with injected Clock** (deterministic v7!) — Go got stdlib UUID in 2026; Jet's takes `Clock`, so time-sortable IDs are testable | probe GORT/GOLANG Q5 | shipped on release tier; **E0956 on default (D3)** |
| 9 | **SIMD default-on direction.** D-SIMD1/2/3 ratified; base + wide lanes and `#Scalar` executed live in probes; Go's is `GOEXPERIMENT=simd`, arch-split, opt-in | probe GORT Q3, card #2261 | **ratified, in progress** (AOT proof criteria open) |

## Avoid list

| Mistake | Evidence | Jet exposure |
|---|---|---|
| Shipping a second solver/mechanism beside the first "temporarily" | Rust: 4 years, 208+ issues, still-immature diagnostics | Structurally averted (I8, greenfield law); keep the test-only `CallBinder` seam from ever becoming a second production path |
| Freezing a wrong default so hard a v2 namespace is needed | Go `encoding/json/v2`; JS `Date`→`Temporal` | Immune today, **but only while pre-release**: every default ratified now is this decision |
| Generic spelling that erodes the language's readability identity | Go audience: 150-like "losing readability", "froze at the method signature" | Jet's `fn f<T>(…)`/`call<T>(…)` reads conventionally; watch every new sigil against this |
| Fixing generics but fencing them out of interfaces | Go 1.27 generic methods can't satisfy interfaces (compat-locked) | Jet trait methods currently reject own type params *by design* — fine, but record the stance so it never becomes an accidental fence |
| Runtime-only cleanup protocols | JS `using`/`SuppressedError` | Immune: ownership-checked `close(^r)` (E0121) |
| Import-time side effects needing a lazy-import feature to mitigate | JS `import defer` exists because module evaluation runs code | Immune: E0620 |
| Feature videos overstating stages/perf | Decorators "Stage 3" (2.7); "free performance" (workload-qualified); instruction-counts sold as compile times | Process lesson: Jet's own claims must name metric + config (already law in #676's corrections) |
| Uncontrolled format-selector sprawl vs undiscoverable `__format__` | Python audience: custom `__format__` "undiscoverable, looks like magic" | Jet's closed selector registry (E0914 on unknown) is the right shape — D1 must be fixed for user types to join it safely |

## Agent-optimality — the five quantities

| Q | This mine's evidence | Jet position |
|---|---|---|
| a. Verdict fidelity | Go leak detection is runtime-profile-only; JS disposal errors are runtime; Jet moves double-close and date-literal validity to *compile time* | Strong where implemented; **D1/D3 are fidelity holes** (accepted-but-wrong impls; tier-gated verdicts) |
| b. Verdict latency | Rust's solver rewrite was partly about latency ceilings; no wall-time data in any source | Not moved by this mine; #666 owns it (needs a `resolution_us` subphase — see gaps) |
| c. Actionability | Probed Jet diagnostics are genuinely good: E0041/E0620/E0956/E0121/E0155 all carry what/why/fix that an agent can apply mechanically | Two concrete bugs: E0904's impossible-fix suggestion, E0904/E0112 duplication |
| d. Context economy | Walrus probe: Jet compute-once-reuse = 42 tokens vs Python 38 — parity without a second binding spelling | Holding; #2265 owns the remaining LOC gap |
| e. Repair determinism | One mechanism held everywhere probed: no set-operator second spelling, no walrus, one enum/match form | Strongest quantity; D1's "impl accepted, output unchanged" is its worst violation — no error *and* no effect |

Weakest quantity today: **(a) on the default tier** — E0956 turns real verdicts into
"unsupported", and D1 gives no verdict at all.

## Surface coverage

**Covered with proof** (ran live): `Range` values + list `take/skip/step_by/reverse` + bracket
ranges; `Set.union/intersection/difference/symmetric_difference`; `{x:Fixed(2)}` (Float *and*
Int), `{x:Debug}`, E0914 closed registry; map `get → ?V`, `??`, strict `[k]` (exit 70);
closures + capture; `zip` short/pad/fill/strict; `task`, `task.all {…}` positional,
`TaskFailure`, channels; `testing.fake_clock/fake_rng/fake_data` (SplitMix64);
`DateTime`/zoned time/durations/offsets; `defer close(^r)`, E0121; `#Codable`
encode/decode ×6 policies; enums (unit/payload/named) + leading-dot match; `uuid.v4/v5/v7/parse`
(release); `F32x4/F64x2` + wide lanes + `#Scalar`; `reactive.effect`; `core.text.splitn/rsplitn`,
`core.net.url` parse/join; `jet self doctor/toolchain`, `jet fix/fmt/test/doc/version`;
ICE routing exit 101; generic fn inference + explicit `call<T>`; E0909 depth guard.

**Worth checking**: AOT behavior of user `Display` once D1 is fixed (web tier too); whether
map-key JSON order is contract or accident (pin it); `JET_OBSERVE=1` + `jet inspect live` on a
blocked task; release-tier generic `impl` methods end-to-end; `--release` parity for every D3
reproducer after #2252 lands.

**Missing** (exact names): `text.cut_last` (Go `strings.CutLast`); Euclidean div/rem spelling on
arbitrary-precision `Int` (Go `Int.Divide` w/ rounding modes); Unicode **17** tables (Jet pins
16.0.0, `UnicodeTables.rs:1-6`); in-memory HTTP test server (`serve_once` exists, port-free
`httptest.NewTestServer` analog doesn't); scheduler-wide deterministic time bubble (Go
`testing/synctest`; Jet fake time is handle-level only); named `task.all` results
(`Promise.allKeyed` analog — parse error today); zipKeyed named-field zip; generic hasher
surface (`maphash.Hasher` analog); task labels; `sema`-phase `resolution_us` perf column.

## Owner gates

1. **Named `task.all { first: f(), … }` results** — new surface syntax; ballot drafted
   (see Tower). Everything else here is stdlib/bugfix under existing law.
2. **Explicitly declined, no ballot needed (I8):** set operators (`|&-^`) as a second spelling
   for shipped set methods; Python walrus `:=` (statement bindings measured at token parity);
   Go-style struct embedding/promoted fields (named composition is the one mechanism).

## Prioritized actions

| P | Action | Owner |
|---|---|---|
| P0/P1 | Fix D1 Display/Debug across all tiers (sema contract check + codegen lowering + snapshot) | new card |
| P1 | Land the four D3 reproducers as #2252 criteria evidence | #2252 (attached) |
| P1 | D4 lambda-driven generic inference + E0904 fix-text/duplication | new card |
| P2 | D2 S40 `slice(a..b)` drift — implement ratified spelling or re-ratify the two-Int form | new card |
| P2 | D5 blocked-task-at-exit report + task identity in failures (ride `--observe` seam) | new card |
| P2 | rustc exact pin + direct generated-Rust-rejection→101 regression test (branch proven live by D1's ICE) | new card |
| P3 | Go 1.27 stdlib parity pack: `cut_last`, Int Euclidean div/rem, Unicode 17, JSON map-order pin, in-memory test server + scheduler time bubble assessments | new card |
| — | SIMD: evidence attached to #2261; brevity overlaps stay on #2265 | existing |

## Strongest unverified assumption

That the observed deterministic JSON map-key ordering is a stable contract rather than an
artifact of the current map implementation — it was observed once, on one map shape, on one
tier, and nothing in the spec or a test currently pins it.
