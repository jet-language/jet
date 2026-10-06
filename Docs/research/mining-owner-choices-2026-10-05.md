# Owner choices hidden in the 2026-10-05 mining claims

Date: 2026-10-05. Author: BallotHarvest (claims-level re-read). Companion to
[mining-playlist-2026-10-05.md](mining-playlist-2026-10-05.md).

## Why this exists

The 2026-10-05 mining run (50 videos, about 37 hours) produced 2,275 claims.
The distillers filed 1,707 as no action, 448 as notes on existing cards, 94 as
new cards and 26 as ballot evidence, which became 7 ballots. The owner
expected more genuine choices. This pass re-read every claim and asked one
question of each: does it imply a choice only the owner may make (new or
changed public API, syntax, command, default, product behavior or scope) that
no ratified or open decision already answers?

## Method

- Inputs: `FINAL-LEDGER.tsv` (2,275 rows), the claim text in
  `DistillSystems/all.tsv`, `DistillLang/all.jsonl` and
  `v3/P2-python-b/claims.json` (the only batch missing from both tables), the
  v3 shard `STATUS.md` and `report.md` files, and the batch reports' "owner
  questions" sections.
- All 626 rows whose contribution is not `confirms-choice` were read in full.
  The 1,649 `confirms-choice` rows were read as claim text plus the ledger
  reason.
- Every candidate was checked against the live Tower decision list
  (`tower decision list --json`, 1,573 decisions, read 2026-10-05) and the
  spec. A candidate is excluded when a ratified or open decision already
  answers it, or when it is implementation under existing law.
- Duplicates across batches (the same Unreal or HTTP point mined two or three
  times) are grouped into one candidate.

## Counts

| Bucket | Count |
|---|---|
| Claims read | 2,275 |
| Distinct owner-choice candidates (claims pass C01-C22, API comparisons C23-C33) | 33 |
| Candidates with a validated ballot in `~/.cache/jet-dev/ballots/READY/` | 15 |
| Candidates excluded because a decision already answers them | 19 |
| Card or card-note leads from the claims pass (implementation under existing law) | 21 |
| API comparison rows (7 miners, all 50 sources) | 400 |
| New cards, card notes and reopen flags in `~/.cache/jet-dev/ballots/CARDS-TO-POST.json` | 36 / 22 / 2 |

## Owner-choice candidates

Size: S = one rule or one function; M = a small API family or default with
migration; L = a subsystem or product-scope decision.

| ID | The choice in one sentence | Evidence (claim ids; source) | Current Jet state | Why it is an owner choice | Size | Disposition |
|---|---|---|---|---|---|---|
| C01 | What a backward inclusive range (`10..1`) and a negative constant stride mean in a loop and a slice. | v3-P2:PYB-3, PYB-61, PYB-2; learning-compilers:PY81 (4M87qBgpafk) | Both `loop i in 10..1` and `loop j in 0..1, -1` print nothing and check clean (dev-05ea86f65 probe p17). S19 only states positive strides; the spec never says what a backward inclusive range does. Card #4629 plans "one advisory lint and one written stride law". | Language semantics of a core construct; the card would pick the meaning without a vote. | S | Ballot D-RANGE-BACKWARD1 on #4629 |
| C02 | Whether list slices get open-ended bounds (`xs[2..]`) and a step (`xs[0..8, 2]`), or keep named methods with teaching errors. | v3-P2:PYB-48, PYB-49, PYB-50; learning-compilers:PY95, PY101 (4M87qBgpafk) | `xs[2..]` and `xs[0..4, 2]` are generic E0003 parse errors (probes p7, p7b). The jobs exist as `skip`, `take`, `drop_last`, `step_by`. Loops already accept the comma stride. | New syntax; the synthesis deferred it as "only the teaching target is in scope" without a vote. | M | Ballot D-SLICE-OPEN1 on #4022 |
| C03 | Whether `+` joins two lists. | v3-P2:PYB-51; learning-compilers:PY96 (4M87qBgpafk) | `a + b` on lists is E0109 with a generic fix. `a.concat(b)` and the spread literal `[...a, ...b]` exist; `+` on text is refused by S8. | New operator meaning, or a deliberate refusal that needs a stated rule. | S | Ballot D-LIST-PLUS1 on #4022 |
| C04 | The default buffering of stdout and stderr when they are a file or pipe versus a terminal. | cpp-craft:I02, R02 (i_wDa2AS_8w); report F3b | Every non-empty write flushes, even into files and pipes; `stdout.flush()` is dead (Term.rs:152-177). Card #4634 says "measure, then fix". | A default every program observes (interleaving, crash output, speed). | S | Ballot D-STDOUT-BUFFER1 on #4634 |
| C05 | Whether the HTTP server sends a `Date` header by default. | v3-H2:FknTw9bJsXM-B33; web-data-net:C76 (FknTw9bJsXM) | The serializer never writes `Date`; an IMF-fixdate formatter exists for `Last-Modified` only. RFC 9110 6.6.1 requires `Date` from an origin server with a clock. | A server default that trades protocol conformance against byte-identical test output. | S | Ballot D-HTTP-DATE1 on #4635 |
| C06 | Whether `http.serve` gets a per-request memory budget that fails one request instead of the process. | languages-a:R12, R13 (DzhIprQan68, Roc NIA); v3 A2-langs note 2 | `serve`/`bind` take `(addr, mux, tls, deadline)`; time has deadlines (D-FOUND-LIFECYCLE1), memory has only the static `Mem.Alloc(above: N)` denial (D-AUTHORITY-MEM2). | New public server option and failure behavior. | M | Ballot D-HTTP-REQMEM1 on #4635 |
| C07 | Whether a log or profiling span closes at scope exit like other owned resources. | systems-perf:SP2-DaJWWePhRsM-46; v3-S1:DAJ-12; systems-perf-deep:SD-DA-06, SD-KX-12; systems-perf:SP2-KxREK-DYu70-15 | `log.span(name)` returns a `LogSpan` closed only by explicit `close(span)` (core-library.md:2587-2588). Readers, writers and locks already release at lexical scope exit (core-library.md:756-757). Card #4632 lists "scoped spans". | Changes a public Core API's lifetime rule. | S | Ballot D-LOG-SPAN-SCOPE1 on #4632 |
| C08 | Whether ordered containers (`PriorityQueue`, sorted collections) accept a per-instance order without a wrapper type. | languages-a:Q28 (DuGy1tmKP-w, Gren); v3 A2-langs note 1 | `PriorityQueue.new()`/`.from(xs)` take no order (core-library.md:284). Order is per type through D-KEY-TRAITS1, so a max-heap needs a wrapper type. Lists already have `sort_by`/`sort_by_desc`. | New public constructor parameter on Core containers. | S | Ballot D-ORDER-BY1 on #4499 |
| C09 | What `headers_set`/`headers_append`/`with_header` do with an invalid name or a control byte in the value. | v3-H2:FknTw9bJsXM-B30; web-data-net:C59 | They return the headers unchanged, so a typo like `"Content Type"` vanishes silently. | Public API error behavior. | S | Ballot D-HTTP-HEADER-BAD1 on #3665 |
| C10 | What Jet 1.0 promises about formatter output, machine-readable CLI output and the editor protocol across toolchain updates. | languages-a:G56 (RntfkL8lUY4); learning-compilers:OZ1; systems-perf:SP2-DaJWWePhRsM-07; v3-S1:DAJ-07 | release-policy.md covers compiler SemVer and source editions; it is silent on tool-output stability. D-ADOPT-LTS1 sets the LTS calendar only. | Release policy is owner law. | M | Ballot D-TOOL-STABLE1 on #1349 |
| C11 | Whether `jet learn` gets a zero-install browser lane. | v3-P1:PYA-068; languages-a:R43; learning-compilers:AH2 (4M87qBgpafk, Pyodide; Roc site) | D-LEARN1 made `jet learn` in-toolchain and offline. Jet has a web target (D-WASM1) and source-defined playgrounds (#2464) but no browser lane for Learn. | Product scope and distribution. | M | Ballot D-LEARN-WEB1 on #4630 |
| C12 | Whether game time and frame hooks stop while the window is minimized. | cpp-craft:G02, G06, G25, G26 (eopps3YF6aE); cpp-craft report owner question 2 | D-GAME-LOOP1 loops "until the window closes"; nothing states minimized behavior. The raylib bridge strands a minimized window. | Product default for every windowed game. | S | Ballot D-GAME-HIDDEN1 on #238 |
| C13 | Whether a game must declare an action enum before binding input, or may use an implicit per-scene action set. | cpp-craft:G17 (eopps3YF6aE); cpp-craft report owner question 3 | D-GAME-INPUT1 ratified "Both A & C"; the shipped input and raylib APIs still take strings that return `false` when misspelled. Whether `.jump` resolves without a declared enum is unstated. | Ambiguity inside a ratified public API. | S | Ballot D-GAME-ACTIONSET1 on #238 |
| C14 | Whether the binary cursor `Reader.over(bytes)` keeps the name `Reader` beside the `core.io.Reader` byte-stream trait. | v3-H1:FknTw9bJsXM-A09; web-data-net:C07 | Two public things are called `Reader` (D-NETIO-CONTRACT1/2 trait; D-SHIFT1 / D-BINREAD-LEN1 binary cursor). | Public naming of a Core type. | S | Candidate; not drafted (needs a naming probe across Core and the ratified D-BINREAD-LEN1 text) |
| C15 | How a vendored C static library declares the system libraries it needs (`-lm`, `-lX11`). | v3-S1:DAJ-10, DAJN-01 (DaJWWePhRsM) | `c@"vendor/path"` link deps fall back to pkg-config; a vendored archive has no stated way to declare its own link closure. D-FOUND-HANDLE1 covers generated bindings, not vendored archives. | New package-manifest field. | S | Candidate; not drafted (needs the exact `package.jet` link-dependency grammar) |
| C16 | Whether web builds get a default payload-size budget. | languages-a:Q25, R18; v3 A2-langs note 7 | Typed budgets exist (D-PERFBUDGET-GRAMMAR1) and native `--small` is documented in binary-size.md; no web payload budget or default exists. | A default that fails builds. | S | Candidate; not drafted (single source) |
| C17 | Whether `core.game` owns networked state replication. | systems-perf:SP-07, SP2-KxREK-DYu70-42..46, SP2-eP3DFpsWABc-11; systems-perf-deep:SD-KX-33..37, SD-E3-13 | No owner. Synthesis rejected a research card as premature because the ECS (#238 criterion 10, D-GAME-ENTITY1) is unbuilt. | Product scope. | L | Candidate; deferred until D-GAME-ENTITY1 ships |
| C18 | Whether game systems get per-system update rates and budget-driven degradation. | systems-perf:SP-04, SP2-KxREK-DYu70-25..28; systems-perf-deep:SD-KX-20..22 | D-GAME-BUDGET1 only reports budgets; D-SYSSCHED1 has no rate. | New public scheduling API. | M | Candidate; deferred until the ECS exists |
| C19 | Whether a test assertion stops the test or records and continues. | v3-H1:FknTw9bJsXM-A17; web-data-net:C18 | D-CLAIM-WORD1 chose one word (`assert`); a stop-versus-continue rule was not found in S43. | Test semantics. | S | Candidate; not drafted (needs the current `jet test` assertion behavior read from source) |
| C20 | Whether a division by a constant zero is a check error or stays a run-time stop. | learning-compilers:RC5; v3-P2:PYB-124 | Not caught at check time (probe p04); exact `/` by zero also reports line 0 (card NC-STOPS #4627). | Error-versus-stop rule for a language operator. | S | Candidate; not drafted (low stakes; may be decided as a lint under D-LINTPOLICY1) |
| C21 | Whether the HTTP server answers unknown status codes with an invented reason phrase. | web-data-net:C57; v3-H2:FknTw9bJsXM-B24, B25 | Four divergent reason tables; the serializer falls back to "OK" (503 goes out as "503 OK"). | Wire behavior; mostly a bug, but the empty-reason policy is a choice. | S | Note on #4635 (empty reason for unknown codes is the RFC answer, so not balloted) |
| C22 | Whether `jet build` reports compiler performance from a flag rather than `JET_TIMING=1`. | systems-perf:SP2-A2U_GMWl9l4-02; systems-perf-deep:SD-A2-02 | `JET_TIMING=1` prints phase timings (environment.md:47); `jet inspect explain-build` exists. | CLI surface. | S | Note on #3961 (D-CLI-ONE1 inspect tree already governs where it lives) |
| C23 | One word for membership on every collection (`has` or `contains`). | v3-P2:PYB-33, PYB-34, PYB-84; APICompPython u06, v01 | List and Queue use `contains`, Set/Rank/Tally/Bits `has`, Map `has_key` and `contains_value`. E0384 says write `contains` on a Set, then E0311 calls `contains` foreign and suggests `has`. D-ONCE-VERB1 requires one verb but never chose it. | Ratified law demands one word; the word is the owner's. | S | Ballot D-HAS-VERB1 on #4022 |
| C24 | Whether `a ?? b ?? c` groups from the right. | learning-compilers:AP1 (ts38mSIUPSg); APICompLearn e09, f02 | Associativity of `??` is unwritten (spec.md:826-830); left grouping rejects a fallback chain with E0405 and leaks the backup's failure. | Operator rule. | S | Ballot D-FALLBACK-CHAIN1 on #4628 |
| C25 | Whether a fact read such as `T.$layout.size` may appear inline in ordinary code. | APICompLearn e06, g02 (7_o-YRxf_cc) | Inline reads are E0302 "compile-time only"; a module-level `prep` binding works. | Language rule under D-LAYOUT-FACTS1/D-PLACE1. | S | Candidate; not drafted (fix the proven E0107 defect first, card in CARDS-TO-POST) |
| C26 | What reversed or invalid bounds do in `random.int`, `randint` and `rng.int`. | APICompLearn e11; APICompLangB q15 | `random.int(10, 1)` returns 10; `rng.int(10, 1)` stops E3010; core-library.md:1879 documents "returns lo". | Public API error behavior with conflicting doc and runtime. | S | Candidate; not drafted |
| C27 | A field-level codec override (`#Codec(encode:, decode:)`) so an app can encode a foreign type's field without a wrapper. | APICompLangA (IGmwiyines0 21:11-26:13) | Encode/Decode derive per type; the orphan rule blocks adding Encode for a foreign type. | New marker. | S | Candidate; not drafted (single source) |
| C28 | A `git` dependency source in `package.jet` (URL plus locked commit). | APICompLangA (DuGy1tmKP-w 42:20-43:26); languages-a:Q39 | APICompLangA found no git source spelling in Docs/spec; the ledger filed Q39 against done card #532. | Package manifest surface. | M | Candidate; needs reconciliation with Jetpack sources (D-VERDICT-2190-1) before a ballot |
| C29 | Whether D-MEMBER-LIVE1 also reports parameter defaults that every call overrides or no call overrides. | APICompLangB p01_default_always (yKl2fSdnw7w 53:47) | Defaults exist (spec.md:314); D-MEMBER-LIVE1 covers fields and variants only. Corrects claim languages-b:R48. | Amendment to a ratified lint scope. | S | Candidate; amendment to D-MEMBER-LIVE1 |
| C30 | Whether `math.to_bits` returns U64 instead of a signed Int. | APICompCraft p03, p11 | Signed result breaks binary packing of negative doubles. | Public signature change. | S | Owner nod inside the binary-codec card |
| C31 | Whether `compare_exchange` returns the observed value on failure. | APICompSystems b_atomic_time (hD0fyLtWIIE) | Returns Bool only. | Public signature change. | S | Note on #2887 |
| C32 | Whether `print(a, b)` joins arguments on one line. | APICompPython t01 | Prints one argument per line. | Already decided. | S | Excluded: D-VERDICT-1321-1 (one line per argument); doc gap only |
| C33 | Whether `[T].average()` returns an optional with exact division. | APICompPython r02, r03, u05 | Returns 0.0 for empty and a binary float. | Implied by D-STATS-CANONICAL1 (no answer gives None) and D-INTDIV1. | S | Card in CARDS-TO-POST |

## Excluded: already answered by a decision

| Claim group | Answered by |
|---|---|
| Order-preserving dedupe (`unique()`; v3-P2:PYB-98, learning-compilers:PY131) | D-ITER-UNIQUE1 (ratified: distinct keys in encounter order) |
| In-place range removal (v3-P2:PYB-53) | D-ITER-DRAIN1, D-ITER-SPLICE1 |
| Localization and message catalogs (languages-a:E80; learning-compilers:AP8, AP9) | D-CORE-CATALOG1, D-VERDICT-3307-1 |
| Core colour type (v3-S1:DAJ-34; systems-perf-deep:SD-DA-15) | D-VERDICT-3288-1 |
| Multi-name declaration from a tuple (v3-P1:PYA-030) | D-TUPLE-DESTRUCT1 |
| Busy loops and scheduler fairness (languages-b:O11; languages-a:G15, G16) | D-TASK-SLICE1 |
| Unused-variable warnings and liveness across configurations | D-MEMBER-LIVE1, D-STRUCT-LIVE1 |
| Duplicate map-literal keys | D-MAPLIT-DUP1 |
| CRLF string escape | D-STR-ESCAPE1 (open) |
| Byte layout visibility | D-LAYOUT-SHOW1 |
| Screens and asset lifetime | D-GAME-SCREENS1 |
| Format into an existing buffer (v3-H2:FknTw9bJsXM-B28; web-data-net:C75) | D-EFF-FORMAT-WRITER1 |
| Regex replace-all versus first; capture spans | D-REGEXREPL1, D-REGEXENGINE1 |
| Request-scoped memoization of pure functions (cpp-craft:W13, W14) | D-MEMO1 for pure caching; the request-scoped single-flight variant has one experimental source and stays a lead |
| Browser source maps (languages-a:Q22, E31) | D-OBS1/D-OBS3 (`.jetmap`); emitting browser-readable maps is implementation |
| Lint settings are order independent (systems-perf:SP2-DaJWWePhRsM-13) | D-ECO-COMPOSE2 |
| Core functions returning `-1` for "absent" (v3-H1:FknTw9bJsXM-A06; web-data-net:C04, C47; v3-H2:FknTw9bJsXM-B04) | D-OUTCOME-SHAPE1 (absence is an optional) and D-CHOOSE-FIND1; the repair is implementation |
| `print(a, b)` layout (APICompPython t01) | D-VERDICT-1321-1 |
| `average()` on empty input (APICompPython r02) | D-STATS-CANONICAL1, D-INTDIV1 (card filed) |

## Card and card-note leads (implementation under existing law)

These need work, not a vote. Each names its owner.

| Lead | Claims | Owner |
|---|---|---|
| Teaching errors for Python slice, `len(xs)` and list `+` habits | v3-P2:PYB-14, 48, 49, 51 | #4022 (D-CORE-ONE-NAME1) |
| Delete alias rows (`append`, `discard`, `take`, `index`) | v3-P2:PYB-20, PYB-99 | #4022 |
| Fragmenting reader double and framing-computing fixture builder for parser tests | v3-H1:FknTw9bJsXM-A25; v3-H2:FknTw9bJsXM-B02, B11; web-data-net:C27 | #3665 under D-TESTKIT1 (ratified, partly built) |
| `lines()`/`read_line()` on TCP streams | v3-H1:FknTw9bJsXM-A05; web-data-net:C03 | #3665 under D-NETIO-CONTRACT1/2 |
| Streaming handler response from a Reader | v3-H2:FknTw9bJsXM-B39 | #3665 |
| HTTP method case, version default, status-line and reason-phrase conformance | web-data-net:C22, C23, C56, C57; v3-H1 A19, A51; v3-H2 B24, B25 | #3665, #4635 |
| Lambda `http.serve` path: unbounded threads, no panic isolation, accept-error loop | v3-H2:FknTw9bJsXM-B20, B21, B43 | #4635 |
| Incremental `Hasher.update` that does not buffer the whole input | v3-H2:FknTw9bJsXM-B53 | #3667 |
| MIME table gaps (mp4, webm, mp3, pdf) | v3-H2:FknTw9bJsXM-B31 | #3665 |
| Binary response bodies in `parse_response` | web-data-net:C69; v3-H2:FknTw9bJsXM-B56 | #4428 |
| Case-only identifier look-alikes | learning-compilers:VA10 | #4613 / D-IDENT-CHARS1 |
| Compound assignment that repeats its target (`x -= x - d`) | v3-P1:PYA-079 | #3919 lint port |
| Constant-condition lint parity | v3-P1:PYA-098 | #3919 |
| Sub-condition values in failed test reports | v3-P1:PYA-105 | D-REPORT-TEST1 owner |
| Learner-visible branch coverage and hidden boundary cases in Learn | v3-P1:PYA-059, PYA-099; v3-P2:PYB-59 | #4630 |
| Window lifecycle: pump events while minimized, resize as a frame fact, close as an event | cpp-craft:G02, G06, G25, G26, G28 | #238 criteria 13/14 |
| Nearest-neighbour default for pixel art, integer scaling | systems-perf-deep:SD-9H-05; cpp-craft:G04 | #3027 |
| Wait attribution, inclusive time, recursion in `jet perf` | systems-perf:SP-05, SP2-KxREK-DYu70-13/14; v3-S1:DAJL-01 | #4632 |
| Stdout flush measurement | cpp-craft:I02, R02 | #4634 (the default itself is C04) |
| Reversed-range advisory lint | v3-P2:PYB-3 | #4629 (meaning is C01) |
| Restart rate visible in `tree.show()` | languages-a:G13 | #1153 / D-SERVICE1 |

## Owner-named examples not found in the claims

Main relayed these owner examples. None of them appears in the 2,275 claims or
in the batch reports; they look like port-work findings. Their current Jet
state, read from the spec:

| Example | Current Jet state | Status |
|---|---|---|
| Regex callback replacement and per-group offsets | Capture spans are zero-based character positions (D-REGEXENGINE1, syntax-decisions.md:4185-4192); `replace` and `replace_first` take text. No callback form is specified. | Real API gap; needs its own evidence source before a ballot. |
| Map `get_mut` / Entry | Keyed storage is `add`/`add_new` (D-API-STORE1); `m[k].push(v)` is governed by D-MEM-INDEXMUT1. No entry or in-place update form is specified. | Real API gap; needs evidence. |
| Swap/take/replace through `&`; mutable iteration | Not specified in core-library.md; write access is marked with `&` at calls (stdlib law 91-93). | Needs a source read of the port friction first. |
| Unused-use rules for untaken prep branches | D-STRUCT-LIVE1 reports unused imports; `#Known if` branches are compile-time. D-LINT-UNUSED1 is an open spec-only import. | Real rule question; needs a probe. |
| Fragmenting test reader, Date header, per-request memory budgets, log spans, ordered containers, open-ended and stepped slices, list concatenation, vendored C system libraries, localization, Wasm size budgets, source maps, networked game state, zero-install `jet learn` | Found in the claims; see C01-C17 and the exclusions above. | Covered above. |

## API comparisons

Seven API-comparison miners (APICompSystems, APICompLangA, APICompLangB,
APICompPython, APICompLearn, APICompCraft, APICompWeb) compared every API named
in the 50 sources with Jet's counterpart. The full 400 rows, one per API, with
source locator, Jet counterpart, verdict, concrete action and probe evidence,
are in `/mnt/jetscratch/mine/apicomp/<Name>.tsv`, with probes under
`/mnt/jetscratch/mine/apicomp/probes/`.

| Verdict | Rows |
|---|---|
| Jet avoids the source's mistake | 93 |
| Jet is improvable | 93 |
| Equal | 78 |
| Jet is better | 63 |
| Jet repeats the mistake | 44 |
| Jet is missing the API | 29 |

Every improvable, repeated or missing row became a ballot (C23-C33 above), a
new card or a card note in `~/.cache/jet-dev/ballots/CARDS-TO-POST.json`, or a
proven-defect row in `~/.cache/jet-dev/ballots/FIX-WAVES.md`. Two done cards
still reproduce and are flagged for reopening: #4374 (two `Mul` hooks on one
type) and #4285 (about 25 alias spellings still ship). The headline rows:

| Source API | Jet counterpart | Verdict | Action |
|---|---|---|---|
| testify/Go regex inputs; RE2 linear time | `core.regex` recursion overflows at 3,000 chars | repeats a mistake | card (FIX-WAVES wave 1) |
| Elixir `String.upcase("José")` | `core.text.parse.upper` gives "JOSé" | repeats the Latin-1 mistake | card |
| Go `v, ok := <-ch` | `tasks.recv` returns -1 for closed | repeats the sentinel mistake | card |
| QueryPerformanceCounter to CLOCK_MONOTONIC | Stopwatch reads the wall clock | repeats the wrong-clock mistake | card |
| V8 `JSON.parse` | `json.parse` about 460x slower | improvable | card with perf budget |
| Go `conn.Read` partial UTF-8 | `tcp_read_text` loses split scalars | repeats a mistake | card |
| C++ `std::bit_cast` | `to_bits` signed; negative doubles fail to pack | repeats a mistake | card plus owner nod (C30) |
| Python `2 ** 3` | E0208 suggests `#Unsafe` | improvable | card |
| Python `list.pop(0)` | checks clean, ICE at run | defect | card |
| Rust `Option<Box<T>>`, closures, `[T; N]` | ownership, captures, fixed arrays | better or equal | doc fixes only |
| C++ manual `delete`, `std::move` | ownership and moves | avoids the mistake | none |

The claim-level comparisons made before the miners reported:

| Source API | Jet counterpart | Verdict | Action |
|---|---|---|---|
| Python `range(a, b, -1)` | `loop i in a..b, -1` | improvable: meaning unspecified | C01 ballot |
| Python slices `xs[2:]`, `xs[::2]` | `skip`, `step_by`; no slice syntax | improvable | C02 ballot |
| Python `a + b` on lists | `concat`, spread | improvable: generic error | C03 ballot |
| C++ `std::endl` flush per line | flush per write everywhere | repeats the mistake, worse | C04 ballot, #4634 |
| Go `w.Header().Set` (Date auto-added by net/http) | serializer never sends Date | missing | C05 ballot |
| Roc NIA per-request arena | `serve(addr, mux, tls, deadline)` | missing | C06 ballot |
| C++ scoped profiler zone; Unreal `SCOPE_CYCLE_COUNTER` | `log.span` with explicit close | improvable | C07 ballot |
| Gren `Dict` with a compare module | `PriorityQueue.new()` with no order | improvable | C08 ballot |
| Go `Header.Set` (no validation) | `headers_set` silently ignores bad input | repeats a mistake | C09 ballot |
| Go `bytes.IndexByte` returning -1 | `core.text.find` returns -1; lists return None | repeats the mistake | #4022 / D-OUTCOME-SHAPE1 repair |
| Go `io.Reader` over file and socket | one Reader trait, but `Reader.over` is a different thing | improvable naming | C14 |
| testify `assert` vs `require` | one `assert`; stop rule unstated | unclear | C19 |
| Go `sha256.New().Write` streaming | `Hasher.update` buffers everything | repeats a mistake | #3667 |
| Python `dict` literal duplicate keys | later wins | repeats the mistake | D-MAPLIT-DUP1 |
| Python `set(xs)` dedupe | `to_set`; `unique` ratified | equal / better | D-ITER-UNIQUE1 |
| Go `map[k]` zero value for missing | `headers_get` returns `?String` | better | none |
| Go `strconv.Atoi` default on error | content-length rejected as malformed | better (but `content_length` returns -1) | #3665 |

## Ballots drafted

All 15 pass `node ~/.cache/jet-dev/ballots/READY/validate.mjs <file>` with no
gaps. Recommended option is A in each.

| Ballot file | Card | Candidate |
|---|---|---|
| D-RANGE-BACKWARD1.json | #4629 | C01 |
| D-SLICE-OPEN1.json | #4022 | C02 |
| D-LIST-PLUS1.json | #4022 | C03 |
| D-STDOUT-BUFFER1.json | #4634 | C04 |
| D-HTTP-DATE1.json | #4635 | C05 |
| D-HTTP-REQMEM1.json | #4635 | C06 |
| D-LOG-SPAN-SCOPE1.json | #4632 | C07 |
| D-ORDER-BY1.json | #4499 | C08 |
| D-HTTP-HEADER-BAD1.json | #3665 | C09 |
| D-TOOL-STABLE1.json | #1349 | C10 |
| D-LEARN-WEB1.json | #4630 | C11 |
| D-GAME-HIDDEN1.json | #238 | C12 |
| D-GAME-ACTIONSET1.json | #238 | C13 |
| D-HAS-VERB1.json | #4022 | C23 |
| D-FALLBACK-CHAIN1.json | #4628 | C24 |

Not drafted, with the reason in each row above: C14-C22 and C25-C31.
