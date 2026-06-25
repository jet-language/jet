# Idea cards — from the stdlib blueprint + bug-prevention field guide

Mined from `docs/research/ideal-stdlib-blueprint.md` and
`docs/research/jet-bug-prevention-field-guide.md`. Each card is one distinct
idea, deduped against ratified decisions (`docs/spec/syntax-decisions.md`),
the ballot (`tools/Tower/docs/ballots/decision-ballots.md`), the board
(`tools/Tower/board.json`), sidequests, the roadmap, and what Core already
ships (`docs/reference/core-library.md`).

**Headline finding:** the great majority of both research files is *already
shipped or already decided*. Jet has shipped a first-party ring (http, regex,
csv, toml, log, time, crypto, archive, db) and ratified the hard safety calls
(no-null, distinct types/units, taint, single-use/must-use, typestate,
capabilities, effects, scoped transactions, schema-migration). The genuinely
NEW items are a short list at the end of each section — flagged clearly so the
CEO isn't shown dupes.

Legend: `NEW` · `ALREADY RATIFIED (id)` · `ALREADY IN BALLOT (id)` ·
`ALREADY A CARD/PLAN (cNN)` · `ALREADY IMPLEMENTED (ref)`.

---

# A. From the standard-library blueprint

## A2 — Tiny composable interfaces (Reader/Writer/Iterator) underpin everything
**What it is:** Define a handful of small protocols (read bytes, write bytes,
iterate, close) once; every file, socket, compressor, and encoder implements
them, so one helper works on all of them. This is the "any pipe fits any pipe"
plumbing idea.
**Source:** blueprint, Principle 2; modules `io`, `iter`.
**Status:** ALREADY IMPLEMENTED — `Reader`/`Writer` + RAII cleanup shipped in
E2-M7 (streaming I/O); iterator pipelines exist.
**CEO note:** Skip — foundation is in place.

## A3 — No function "colors": sync-looking code that runs concurrently
**What it is:** No `async`/`await` keyword splitting the world in two; you write
plain blocking-looking code and a runtime multiplexes lightweight tasks. Avoids
the "every library written twice" tax.
**Source:** blueprint, Principle 3; module `concurrency`.
**Status:** ALREADY IMPLEMENTED — `core.tasks`: `spawn`, `join`, typed channels,
no async keyword (E2-M1).
**CEO note:** Skip — already a shipped, ratified design choice.

## A4 — Errors are values that carry context (cause chain + `?`)
**What it is:** Failures returned as values (not thrown), with `?` to propagate
and the ability to attach a human breadcrumb and the underlying cause.
**Source:** blueprint, Principle 4; module `error`.
**Status:** ALREADY RATIFIED/IMPLEMENTED — `T ? E`, `??`, structured `Error`
(message + code + source chain), `impl Source -> Target` conversion for `?`
(D-ERR-CONV, implemented).
**CEO note:** Skip — Jet's error model already matches this.

## A5 — Safe-by-default, sharp-on-request defaults (TLS verified, linear regex, etc.)
**What it is:** The correct/secure choice is the default; the fast/dangerous one
exists but you must reach for it deliberately.
**Source:** blueprint, Principle 5.
**Status:** ALREADY IMPLEMENTED/RATIFIED — TLS verification on (E2-M10, rustls);
linear-time ReDoS-safe regex is the only engine (jet.regex). Money-as-decimal is
the one gap (see A24).
**CEO note:** Skip the principle; the one missing piece (decimal) is carded below.

## A6 — One line for the common case, layers underneath for the 20%
**Source:** blueprint, Principle 6. **Status:** ALREADY IMPLEMENTED — Core's
one-liner helpers (`fs.read`, `json.parse`) with streaming added in E2-M7.
**CEO note:** Skip — design principle Jet follows.

## A7 — Observability in the box: structured logging, tracing, metrics
**Source:** blueprint, Principle 8 / module H. **Status:** ALREADY IMPLEMENTED —
`jet.log` (structured logging/tracing/metrics, E2-M12). Human-readable output
format already carded (D-LOGFMT1 / c92).
**CEO note:** Skip — shipped.

## A8 — Tested documentation (doc examples run in CI)
**Source:** blueprint, Principle 9. **Status:** ALREADY IN BALLOT / CARD —
doctests are D-TEST4 (ballot) and c51; doctest milestone E2-M11.
**CEO note:** Skip — already in the testing-ergonomics card.

## A9 — Editions for evolution (fix mistakes without breaking old code)
**Source:** blueprint, Principle 10. **Status:** ALREADY IMPLEMENTED —
editions/epochs ratified and shipped (E2-M2, `edition:` in `pkg.jet`).
**CEO note:** Skip.

## A10 — The ten "Ergonomic Laws" (pit of success, named args, types hold guardrails, …)
**What it is:** A named, testable checklist for making an API *feel* good: the
easy path is the safe path; name your boolean arguments; make illegal states
not compile; symmetric naming (encode/decode); errors that teach the fix;
dangerous ops get long scary names; zero-config first call.
**Source:** blueprint, Part 2½ (new in v2).
**Status:** Mostly ALREADY RATIFIED piecemeal — named args + defaults (S61/c02),
parameterized-query SQL (D-DB-style), distinct `Url`-style types (D-DIST1),
what/why/fix errors (diagnostics.md), did-you-mean (shipped). As an *explicit
written API-design rubric for the stdlib*, NEW.
**CEO note:** Worth a tiny doc: most laws are already Jet policy, but writing
them down as a "stdlib API review checklist" would keep new modules consistent.
Cheap, no syntax. Candidate for a short style note, not a ballot.

## A11 — `collections` — one obvious data structure per shape
**Source:** blueprint, module A `collections`. **Status:** ALREADY IMPLEMENTED —
list/map/set built in.
**CEO note:** Skip.

## A12 — `iter` — lazy pipelines (map/filter/fold over a stream, O(1) memory)
**Source:** blueprint, module A `iter`. **Status:** Partially shipped (iterator
adapters exist). The full adapter set (window/chunk/group_by) may have gaps.
**CEO note:** Mostly done; if any adapters are missing it's a small stdlib
fill-in, not a decision. Low priority.

## A13 — `text`/`string` — Unicode by grapheme cluster (so "👩‍👩‍👧".len == 1)
**What it is:** Iterate strings the way a human counts characters (grapheme
clusters), not by bytes or code units, to kill emoji/accent length bugs.
**Source:** blueprint, module A `text`.
**Status:** PARTIAL — Jet strings iterate by Unicode scalar (`s.chars()`, S-level
ratified), which fixes the UTF-16 surrogate bug but is *not* grapheme-cluster
aware (so a family emoji still counts >1).
**CEO note:** NEW-ish gap. Grapheme iteration is a real correctness nicety but
niche; a `graphemes()` method is a stdlib addition, no syntax. Park it.

## A14 — `fmt` — type-safe interpolation + Display/Debug split
**Source:** blueprint, module A `fmt`. **Status:** ALREADY IMPLEMENTED — `{name}`
interpolation (S8); print/format exist.
**CEO note:** Skip. (A Debug-vs-Display distinction may be a minor future refinement.)

## A15 — `time` — separate Instant/Duration/Date/ZonedDateTime + injectable Clock + IANA tz
**What it is:** Keep "a point on the timeline," "an amount of time," and
"wall-clock calendar date in a zone" as distinct types so DST and tz-rule
changes don't break date math.
**Source:** blueprint, module A `time` (flagship).
**Status:** ALREADY IMPLEMENTED — `jet.time` (calendar dates, zones, formatting,
E2-M9); `core.time` for monotonic ms. Injectable clock as a *capability* is the
bug-guide angle (see B7).
**CEO note:** Skip the module; the injectable-clock framing is covered in B7.

## A16 — `math`/`num` — arbitrary-precision integers
**Source:** blueprint, module A `math`. **Status:** PARTIAL — `core.math` ships
float/int math; bigint not documented as present.
**CEO note:** NEW-ish small gap. Bigint is a stdlib type addition (no syntax),
useful for crypto/finance. Low priority unless a user needs it.

## A17 — `random` — split fast-PRNG vs cryptographically-secure RNG
**What it is:** Two clearly-named generators so nobody seeds password tokens
with the predictable game-dice RNG.
**Source:** blueprint, module A `random`.
**Status:** ALREADY IMPLEMENTED — `core.random` (fast/seedable) + `jet.crypto`
ships "vetted random primitives" (secure RNG).
**CEO note:** Skip — both halves exist; could verify the naming makes the split
obvious, but no decision needed.

## A18 — `io` / `fs` — path objects with `/` joining, atomic write, dir-walk iterator
**Source:** blueprint, modules B `io`/`fs`. **Status:** ALREADY IMPLEMENTED /
CARDED — `Path` + streaming I/O (E2-M7); `fs.list_dir` full-paths + path join is
D-LSDIR1 / c88 (in ballot).
**CEO note:** Skip — shipped or carded.

## A19 — `os`/`process` — safe subprocess (arg list, never a shell string)
**Source:** blueprint, module B `os/process`. **Status:** ALREADY IMPLEMENTED —
`core.process.run(["git", …])` takes an arg list; no `shell=True` happy path.
**CEO note:** Skip — Jet already shipped the safe-by-default version.

## A20 — Structured concurrency: `scope`/nurseries that can't exit until children finish
**What it is:** Concurrent tasks live in a lexical scope that blocks until all
its children complete, so leaked/orphaned tasks are impossible — plus a
`Context` for deadlines/cancellation that propagates.
**Source:** blueprint, Principle 3 + module C.
**Status:** PARTIAL/NEW — Jet has `spawn`/`join`/channels and warns on dropped
task handles (L1101), but does **not** have a structured `scope {}` nursery or a
cancellation `Context`. Task-detach idiom is carded (D-DETACH1/c84) but that's
the opposite concern.
**CEO note:** **NEW and interesting.** A structured-concurrency `scope` (auto-join,
deadline-cancel-all) would be a real safety upgrade over today's manual join and
the L1101 warning. Worth a decision card (needs syntax). Medium effort.

## A21 — `serialize` — one derive, many formats (serde-style data-model / wire-format split)
**What it is:** Annotate a type once; read/write it as JSON, CSV, MessagePack,
TOML, binary — through one interface, no per-format hand-written parser.
**Source:** blueprint, module D `serialize` (called "highest-leverage").
**Status:** PARTIAL — Jet ships per-format modules (json/csv/toml) and typed
CSV-row + typed-JSON-output are carded (D-CSVROW1/c89, D-JSONOUT1/c90). A
*unified derivable Serialize/Deserialize across all formats* does **not** exist;
user-defined derives (S56) are explicitly deferred to Epoch 3.
**CEO note:** **NEW (big).** This is the serde architecture — the single
highest-leverage idea in the blueprint. Blocked on user-derives (S56, Epoch 3),
so not actionable now, but worth flagging as a north-star once derives land. It
would unify the c89/c90 typed-row work under one mechanism.

## A22 — `json` / `csv` / `toml` / `compress` modules
**Source:** blueprint, module D. **Status:** ALREADY IMPLEMENTED — `core.json`,
`jet.csv`, `jet.toml`, `jet.archive` (zip/tar/gzip). Streaming/strict-vs-lenient
JSON modes and surfacing lenient coercions are carded (c10).
**CEO note:** Skip — all shipped. (zstd/brotli/msgpack codecs are minor additions.)

## A23 — `regex` — RE2-style linear-time engine as the default
**Source:** blueprint, module E. **Status:** ALREADY IMPLEMENTED — `jet.regex` is
linear-time, no backtracking/backreferences by design (D-REGEX1); native
in-house engine carded (c79).
**CEO note:** Skip — this is exactly what Jet shipped.

## A24 — `Decimal` in Core (exact base-10 money math)
**What it is:** A built-in exact-decimal number type so people stop using floats
for currency (`0.1 + 0.2 != 0.3`). The blueprint calls Core decimal a
"public-health measure for financial code."
**Source:** blueprint, module A `math`; Principle 5; novel-bits #6.
**Status:** NEW (the *type*) — the bug-guide's float-for-money **lint** maps to
"new" too. No `Decimal` type ships today; sized floats F32/F64 are carded (c93)
but that's the opposite (more float, not decimal).
**CEO note:** **NEW and worth it.** A Decimal type + a "you used float for money"
lint is high-value, low-syntax-risk, and prevents a notorious bug class.
Candidate for a card. (Pairs with the bug-guide's money entry — same idea.)

## A25 — `net` / `http` / `url` / `ws` — networking crown jewels (client + routed server, TLS)
**Source:** blueprint, module F. **Status:** ALREADY IMPLEMENTED / CARDED —
`jet.http` client+server+TLS (E2-M10); routing+middleware is D-ROUTE1 / c83 (in
ballot). `url` parsing and `ws` may be partial.
**CEO note:** Skip core; **url** (WHATWG-correct parsing) and **WebSockets** may be
genuine small gaps — verify and, if missing, they're stdlib additions, not
decisions. Low priority unless asked.

## A26 — `crypto` — misuse-resistant high-level API (libsodium/Tink `seal`/`sign`)
**What it is:** The headline crypto call is "encrypt this blob with this key"
returning authenticated ciphertext with nonce handled — raw primitives demoted
to the basement so you can't foot-gun a reused nonce.
**Source:** blueprint, module G; novel-bits #2.
**Status:** PARTIAL — `jet.crypto` ships hash/HMAC/vetted-random *primitives*
(E2-M9). A high-level misuse-resistant `seal`/`sign` envelope API is **not**
documented.
**CEO note:** **NEW.** The misuse-resistant envelope is the blueprint's "strongest
opinion." It's a stdlib API-shape addition (no language syntax) layered over the
existing primitives. Worth a card — and it's the prerequisite for A27.

## A27 — Post-quantum + crypto-agility by default (hybrid X25519+ML-KEM behind the safe API)
**What it is:** Default to hybrid post-quantum crypto (classical + ML-KEM) so
traffic recorded today can't be decrypted later by a future quantum computer;
because callers say `seal.encrypt` not `aes_gcm`, the whole ecosystem upgrades
with zero call-site edits ("crypto-agility").
**Source:** blueprint, module G "Post-Quantum by default" (new in v2); novel-bits #8.
**Status:** NEW — nothing PQ in `jet.crypto` today; depends on A26's high-level
API existing first.
**CEO note:** **NEW, strategically interesting, not urgent.** The "harvest now,
decrypt later" threat and the NIST 2030 deadline are real, but this is a Tier-1
library upgrade, not v1-blocking. Flag as a forward-looking card; sequence it
after the misuse-resistant API (A26).

## A28 — `test` — property-based testing built into the standard box
**Source:** blueprint, module H; novel-bits #3. **Status:** ALREADY IN BALLOT /
CARD — D-TEST1 (property tests + shrinking) and c51; milestone E2-M11.
**CEO note:** Skip — already carded.

## A29 — `cli` — declarative arg parsing with auto `--help` (clap-shaped)
**Source:** blueprint, module I. **Status:** ALREADY IN BALLOT — D-ARGS1 /
c91 (structured flag/argument parsing).
**CEO note:** Skip — in the ballot.

## A30 — `uuid` (v4 + v7 time-sortable) and `encoding` (base64/hex)
**Source:** blueprint, module I. **Status:** PARTIAL/NEW — not in Core's eight
modules and not in the `jet.*` ring list; base64/hex/uuid don't appear to ship.
**CEO note:** NEW but trivial. uuid (esp. v7) and base64/hex are tiny,
no-decision stdlib additions. Bundle as a small "utilities" fill-in card. Low risk.

## A31 — `database/sql` — a driver *interface* with parameterized-only queries
**Source:** blueprint, module I. **Status:** ALREADY IMPLEMENTED (partial) —
`jet.db` ships SQLite (FFI-tier, E2-M9). A general *driver interface* (Go's
`database/sql` shape) over multiple DBs isn't documented; parameterized-only
queries align with the ratified taint model (B6).
**CEO note:** Skip the SQLite piece; a pluggable *driver interface* is a future
ecosystem question, not a v1 need.

## A32 — Embedded / no-runtime: one library, swappable I/O engine (core ⊂ alloc ⊂ std)
**What it is:** Instead of a separate "embedded stdlib," layer the library into
rings (no-heap `core` ⊂ heap `alloc` ⊂ OS `std`) and make "what waiting means" a
swappable engine chosen at link time — so the *same* code runs from a server to a
32 KB microcontroller with no `async` coloring and no second library.
**Source:** blueprint, Part 3½ (new in v2); novel-bits #9.
**Status:** PARTIAL — Jet already ships `--freestanding` cross-compilation
(E2-M15) and `use core.mem` low-level tier (E2-M13), and has no function colors
(A3). The *full* core/alloc/std ring layering + a pluggable colorblind I/O engine
is **not** designed.
**CEO note:** **NEW (large, strategic).** This is the "data center to doorbell"
ambition. Aligns with Jet's no-color design and existing freestanding work, but
it's a major architecture track, not a quick card. Flag as a long-horizon
direction; the ring-layering question is the concrete first decision.

## A33 — Explicit allocation at the boundary (caller-supplied buffer/allocator)
**What it is:** Functions that *can* avoid the heap take a caller-supplied
scratch buffer (`json.parse_into(input, buf)`) so they work in fixed-memory
environments — the convenience auto-allocating form stays the default.
**Source:** blueprint, Part 3½ Move 3.
**Status:** ALREADY A CARD/PLAN — arena/allocator work is c05 / D-ARENA-style
(`stdlib-allocators-arena.md` sidequest); arena inference is c26.
**CEO note:** Skip — the explicit-allocator direction is already carded.

---

# B. From the bug-prevention field guide

## B1 — Make bad states impossible with the type system (sum types, newtypes, typestate, linear)
**What it is:** Push invariants into types so whole bug classes won't compile:
can't add dollars to euros, can't pass an OrderId where a CustomerId is wanted,
can't read a closed file, can't leak a resource.
**Source:** field guide, Play A.
**Status:** ALREADY RATIFIED (bundle) — distinct types/units (D-DIST1/2/3),
units tag (D-UNIT1), typestate (D-STATE1), single-use/linear (D-LIN1 →
`#SingleUse`), RAII cleanup (S63). The "no-null + maybe type" half is its own
card (B2).
**CEO note:** Skip — this whole play is already ratified across c23/c68/c69/c71.

## B2 — No null; a "maybe" type the compiler forces you to handle
**Source:** field guide, Play A / menu row 1. **Status:** ALREADY RATIFIED/
IMPLEMENTED — no null; `T?` optionals with `value(x)`/`null`, forced handling,
`??` fallback, `?.` chaining (S35/S71). Jet's existing "#4 idea."
**CEO note:** Skip — shipped.

## B3 — Out-of-bounds index checked by default, prove-in-range to skip
**Source:** field guide, menu row "Out-of-bounds." **Status:** PARTIAL — bounds
checks exist (safe by default); a *prove-in-range to elide the check* tier is the
expert escape and isn't clearly ratified.
**CEO note:** Mostly done. The "unchecked-with-proof" fast path is a niche
expert-tier optimization; defer unless a perf user asks.

## B4 — Take away ambient superpowers: capability-based security
**What it is:** Code can't secretly read the disk, hit the network, or ask the
time — those powers must be *handed in*. Kills supply-chain surprises, injection,
and tz/flaky-time bugs in one stroke.
**Source:** field guide, Play B.
**Status:** ALREADY RATIFIED — scoped capabilities `#grant(fs){…}` revoked at
scope end (D-SCAP1), the c06 value-capability vocabulary (D-CAP1
`view`/`edit`/`take`/`share`), manifest capability surface (c07). Gated on the
effect system (D-EFF1/c66).
**CEO note:** Skip — this is one of Jet's signature ratified bets.

## B5 — Effect system (functions tagged with the effects they perform)
**Source:** field guide, Play B (effects). **Status:** ALREADY IN BALLOT —
D-EFF1 / c66 (effects as inferred tags); `#(no_net)` prohibition is the deferred
follow-on (D-PROP1).
**CEO note:** Skip — already the centerpiece of the qualifier ballot.

## B6 — Taint tracking: untrusted input can't reach a sink (SQL/exec/net)
**Source:** field guide, Play B / menu "Injection." **Status:** ALREADY RATIFIED
— D-TAINT1 option A: `#tainted` tag spreads, `sanitizer fn` strips it, reaching a
sink is E0721; full information-flow control (option B) deferred post-Epoch-3.
**CEO note:** Skip — ratified exactly as described.

## B7 — Injected clock & RNG (kills tz/DST bugs and flaky tests)
**What it is:** `now()` and randomness are powers passed in, not globals — real
clock in production, fake clock in tests, so tests never flake on time or
entropy.
**Source:** field guide, Play B / menu rows "Timezone/DST" + "Nondeterministic."
**Status:** PARTIAL — `core.time` has a test hook (`LEX_TEST_EPOCH`) and
`core.random.seed()` for determinism, and capabilities (B4) provide the
mechanism. But clock/RNG are **not** yet modeled as injected capability *values*
the way the guide describes; the guide lists this as "new (extends #7)."
**CEO note:** NEW framing on an existing mechanism. Once D-SCAP1 capabilities
land, modeling `Clock`/`Rng` as grantable capabilities is a natural, high-value
follow-on (the guide calls it "highest-leverage for little syntax"). Worth a card
sequenced after the effect/capability engine.

## B8 — Living graph: every value can explain its own origin (`why total`)
**What it is:** Instead of sprinkling print statements, the runtime keeps the
receipts — you can ask any value where it came from; variables keep history; a
failure becomes a typed "hole" that flows on instead of erasing the scene.
**Source:** field guide, Play C (the existing "#1–#4 living graph" track).
**Status:** NEW as a built decision (it's an aspirational track, not yet carded
in the ballot/board I can see). Related observability (log/trace) ships, but
value-provenance / `why?` is its own deep idea.
**CEO note:** **NEW, ambitious, signature.** The guide calls building this into
the runtime "a genuine moat." Large and research-y; flag as its own long-horizon
track, not a near-term card. Distinct from logging.

## B9 — Smell detector: warn on plausible-but-wrong code (dead branches, float `==`, etc.)
**What it is:** Gentle lints for code that looks right but isn't: identical
if/else branches, always-true conditions, comparing floats/decimals with `==`,
an unused result.
**Source:** field guide, Play D (called out as *new*, not in the 42-idea list).
**Status:** NEW — Jet has did-you-mean and what/why/fix errors, but no
"semantic smell" lint family. (Float-`==` overlaps with the decimal/money card A24.)
**CEO note:** **NEW, cheap, high-value.** A small lint pack (identical branches,
constant condition, float-equality) extends Jet's existing diagnostics strength
with no new syntax. Good momentum card — each lint is a diagnostic + snapshot.

## B10 — Confusable-name + did-you-mean lints (`users` vs `user`, `l` vs `1`)
**Source:** field guide, Play D / menu. **Status:** PARTIAL — did-you-mean on
typos ships (edit-distance ≤2 in diagnostics.md). A *confusable-name-in-same-scope*
warning (two near-identical live names) is NEW.
**CEO note:** NEW (small). The same-scope confusable warning is a cheap lint
addition; bundle with B9.

## B11 — Ignored results are errors, not warnings (must-use; opt out with `_ =`)
**Source:** field guide, Play D / menu "Swallowed error." **Status:** ALREADY
RATIFIED — `#MustUse` is the stepping-stone half of D-LIN1 (`#SingleUse`); the
guide's "must opt in with `_ =`" matches.
**CEO note:** Skip — ratified (may ship before full single-use).

## B12 — Ban assignment in conditions (`if x = 5` → error)
**Source:** field guide, Play D / menu; listed in the "suggested first wave."
**Status:** NEW — I found no ratified decision or diagnostic banning `=` in a
condition. (Jet uses `::`/`:=` for binding and `==` for equality, which already
reduces the risk, but a `=`-in-condition guard isn't documented.)
**CEO note:** **NEW, trivial, first-wave.** A single diagnostic; near-zero syntax
risk. Good quick win. (Worth confirming whether Jet's grammar even permits `=` in
a condition — if not, this is a non-issue and can be closed.)

## B13 — Integer overflow checked by default (opt into wrapping/saturating)
**What it is:** `255 + 1` on a byte traps instead of silently wrapping; experts
opt into `wrapping`/`saturating` explicitly.
**Source:** field guide, menu "Integer overflow" (new emphasis).
**Status:** NEW — not found ratified. Jet rejects out-of-range U8 *literals*
(E1003) but checked-arithmetic-by-default with wrapping/saturating escapes isn't
documented.
**CEO note:** **NEW, worth it.** Checked overflow is a classic safety default
(Rust debug-mode shipped it). Needs a small decision on the escape-hatch spelling
(`wrapping_add` vs a `#Wrapping` tag). Medium-low effort, high safety value.

## B14 — Money in floats: decimal type + float-for-money lint
**Source:** field guide, menu "Money in floats." **Status:** NEW — same idea as
blueprint A24 (Decimal). Dedup: **count once.**
**CEO note:** See A24 — merge. NEW, recommended.

## B15 — Schema-drift safety: no breaking data-shape change without a migration
**Source:** field guide, menu "Schema drift." **Status:** ALREADY IN BALLOT —
D-MIGRATE1 / c73 (compile-time migration check).
**CEO note:** Skip — carded.

## B16 — Copy-paste drift / structural-dup lint (updated 3 of 4 copies)
**Source:** field guide, menu "Copy-paste drift" (existing #40/#41). **Status:**
ALREADY a known idea (#40/#41); tooling, not carded in the ballot I can see.
**CEO note:** NEW-ish as a concrete card but lower priority; a structural-dup
lint is a tooling project. Park behind B9/B10.

## B17 — Examples = tests = docs + auto-fuzz (stale-docs / untested-error-path defense)
**Source:** field guide, menu "Stale docs / untested errors." **Status:** ALREADY
IMPLEMENTED/CARDED — golden examples enforce docs (I5); doctests + property/fuzz
testing in c51/D-TEST1/E2-M11.
**CEO note:** Skip — covered.

## B18 — Complexity hints / budgets-as-types (O(n²), N+1 queries)
**Source:** field guide, menu "Accidental slowness." **Status:** ALREADY IN
BALLOT (deferred) — D-BUDGET1 (budgets as types, deferred; needs comptime
cost inference).
**CEO note:** Skip — already deferred in the ballot.

## B19 — The safety ladder (Beginner → Working → Expert rungs)
**Source:** field guide, §4. **Status:** ALREADY IMPLEMENTED (philosophy) — this
is Jet's "safe by default, expert tier opt-in" (I1) made into a picture; matches
the ratified `@unsafe`/`#Audit`/capability tiers.
**CEO note:** Skip — it's the existing philosophy restated; useful framing for docs.

---

# Summary for the CEO

**Total distinct ideas extracted: 51** (A1–A33 = 33 from the blueprint;
B1–B19 = 19 from the bug guide; A24/B14 are the same Decimal idea, deduped to one
→ **50 unique**).

**Already covered (skip — shipped, ratified, or carded): ~36.** Jet has already
done the heavy lifting: two-tier library, no-color concurrency, value-errors,
linear regex, TLS-verified networking, the whole bug-prevention safety stack
(no-null, distinct types/units, taint, capabilities, effects, typestate,
single-use/must-use, schema-migration), the first-party ring, and the testing/
observability cards.

**Genuinely NEW and worth the CEO's eye (~14), ranked by value/effort:**
- **Quick wins, low syntax risk:** ban `=` in conditions (B12), smell lints +
  same-scope confusable lint (B9/B10).
- **High-value, small decision:** `Decimal` type + float-for-money lint (A24/B14);
  checked integer overflow by default (B13); injected Clock/Rng capabilities (B7,
  after the capability engine lands).
- **Medium, real decisions:** structured-concurrency `scope`/nursery + cancellation
  Context (A20); misuse-resistant high-level crypto API (A26).
- **Strategic / long-horizon (flag, don't card yet):** serde-style unified
  Serialize across all formats (A21, blocked on user-derives/Epoch 3); embedded
  "one library, swappable engine" ring layering (A32); the living-graph value-
  provenance engine (B8); post-quantum crypto by default (A27, after A26).
- **Trivial fill-ins (no decision):** uuid v4/v7 + base64/hex (A30); grapheme
  iteration (A13); bigint (A16); url-parse + WebSockets if missing (A25).

Output file: `tools/Tower/docs/ideas/from-stdlib-blueprint-and-bug-guide.md`.
