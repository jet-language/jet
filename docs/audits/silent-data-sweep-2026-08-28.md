# Silent-data sweep — all stdlib and language semantics, native tiers (2026-08-28)

Owner directive: find everything in Jet that is broken or that looks fine but silently
produces wrong data, across the whole language and stdlib, the way the datetime
differential run did. Sweep-only session: every finding is carded with a verified
repro and a permanent fixture obligation; fixes flow through the board.

## Method

22 Luna probe lanes in three waves (computation; text/data/effects; semantics),
each diffing Jet against an independent oracle (Python, GNU tools, RFC/NIST
vectors) or algebraic laws where no oracle exists. Every probe ran on both the
default tier and `--release`; any stdout difference is itself a finding. Probes
consume results (no bind-and-discard). The orchestrator re-ran every load-bearing
finding first-hand before carding. Three read-only investigators mapped the
source seams onto the cards so the fix burndown starts with exact owned paths.

Roughly 5,000 oracle-diffed cases. Lane reports and raw protocols:
`~/.cache/jet-luna/sweep-2026-08-28/artifacts/`, scratch repros under
`~/.cache/jet-test-scratch/SWEEP-*` (plus `TASKCONC/`, `HASH/`, `FSPATH/`).

## Severity law (ratified for this campaign)

Silent wrong data = P0 regardless of input obscurity; loud breakage/ICE = P1;
diagnostics/ergonomics = P2. Any default-vs-release divergence is a finding.

## P0 — silent wrong data (verified first-hand)

| Card | Defect | Tiers |
|---|---|---|
| #2290 | Nested-list `==` returns false; `x == .None` never matches; `assert_eq` rejects equal lists | default |
| #2291 | `m[k].push(v)` silently discarded; `outer[0].push(9)` no-ops on release; element-field writes vanish | mixed |
| #2292 | Int-keyed map loses values at i64 extremes (fresh arena id per pack — insert and get never match) | release |
| #2293 | JSON: i64::MIN encodes off-by-5 (default) / off-by-1 (release); decode clamps 2^63 to MAX; huge ints silently float | both |
| #2294 | `Fixed(n)` renders 1e21 as 0; silently caps at 9 fractional digits | both |
| #2295 | `i64::MAX/3` panics on release (raw Rust panic, I2); `1e10`/`1E10`/`1e-5` literals and `Decimal("1e-30")` compile then panic | both |
| #2302 | `Rng.int` collapses near-i64-width ranges to the lower bound on release; backwards interval silently returns lower arg | release |
| #2303 | zip: default rejects unequal lengths (per ratified D-ZIPLEN1=D), release silently truncates — one program, two meanings | fork |
| #2304 | `Regex.replace` all-matches on default vs first-only on release; zero-width splits lose characters; byte spans vs codepoint strings; ReDoS hang | both |
| #2305 | `Int.from_u64(U64.MAX)` silently wraps to -1 on default (release ICEs); bound itself computed via `u64 as i64` | default |
| #2311 | Views interpolate as internal `__JetViewMut { … }` records on default (12/25 cases) | default |
| #2313 | Typed Codable: decoded i64::MAX re-encodes as i64::MIN on release; default stringifies MIN then rejects its own output; `{"nan":NaN}` emitted | both |
| #2315 | Bool arm-match: `false` selects the `true` arm — first arm always wins | both |
| #2316 | Closure captures: default snapshots, release shares; one nested case loses all output | fork |
| #2317 | False E3012 "stack overflow" at depth 10 for pure recursion in foldable return arithmetic; a `print()` masks it | both |
| #2319 | Default tier silently swallows piped stdin (`input()`, `read_all_input()` return empty) | default |

## P1 — loud breakage

- #2296 release codegen trait-bound ICEs: Float sort with NaN, tuple-list display, `[] == []`, `:Unit` selector.
- #2307 release task/freeze ICEs: `task.all` returning maps, frozen captures, 8-way join with one failure.
- #2308 release iterator/collection ICEs: `group_by`, `drop_last`, lazy chains, and the root cause for Int-key map assignment — typed empty map literal loses its head type and emits **reversed constructor generics** (`JetMap<i64,String> = JetMap::<String,i64>::new()`); the String-key fallback hides it from the suite.
- #2318 defer is dead on release: LIFO, early-return, and panic-path programs all ICE (default correct).
- #2306 `String.from_bytes` ICEs on every tier (deleted emitter branch; helper still exists).
- #2314 Codable derive: two optional fields emit a duplicate binding; unit-enum decoder rejects its own encoder's wire.
- #2297 lossy UTF-8/hex decode must become policy (ballot D-BYTESDECODE1).
- #2309 `step_by(0)` silently returns `[]`.
- #2320 env-sourced strings measure bytes on release vs scalars on default; `core.sys.set` ICEs on release.

## P2 — conventions, surface, diagnostics

#2298 (+path semantics), #2299 (`x | y` unparseable while `x |= y` works — ballot
D-BITOREXPR1), #2310 byte-codec surface pack, #2312 exact-number completeness
(Decimal can't divide/round/convert; Duration can't scale), #2321 fn-type
parameter names poison assignability, plus E0956 totality rows logged on #2252
(files, crypto, regex, bytes, generics, readline).

## Clean bills (accurate against oracle)

Strings/Unicode (344 cases, consistent codepoint semantics), float arithmetic and
printing (400 ops bit-exact, 1192 math-function cases ≤1 ULP, round-trip exact),
bigint promotion (219 exact), string↔number parsing (307 parity lines, loud
failure rails), crypto digests (65/65 known-answer vectors), base64, seeded RNG
determinism cross-tier on sane ranges, argv fidelity (33 cases), exit codes,
stdout/stderr separation, FS byte fidelity incl. unicode filenames, iterator
laws + safety (use-after-consume and mutation-during-iteration loudly rejected),
task result association and safety guards, view/freeze guards (E0212/E1113,
deep freeze held), struct/enum literals and match bindings (except Bool arms),
Decimal/Duration arithmetic where defined (174 + 23 exact, all mixed-type
combinations loudly rejected), typed Codable strings/nesting/error paths.

## Ballots raised (owner queue)

- **D-MEM-INDEXMUT1** (#2291) — what a mutating method call on an indexed element means.
- **D-BYTESDECODE1** (#2297) — strict-vs-lossy default for bytes→text.
- **D-REGEXREPL1** (#2304) — what regex `replace` means.
- **D-BITOREXPR1** (#2299) — expression-position bitwise OR.
- **D-STDLIB-OPTPARAM1** (#2322) — option parameters vs sibling methods, the owner's
  API-shape proposal; study at `docs/audits/optional-args-stdlib-2026-08-28.html`.
  D-REGEXREPL1 and D-BYTESDECODE1 carry matching options and resolve with it.

No ballot for zip: ratified D-ZIPLEN1=D already decides it; release violates it (#2303).

## Meta-verdict

The compute core (floats, bigints, strings, parsing, digests) is genuinely
accurate — the datetime-style differential method found zero silent errors
there in ~2,500 cases. The silent-data risk concentrates in four seams:
interpreter semantic equality, indexed-place lowering, the packed-Int
(`JET_INT_BIG_TAG`) representation leaking through maps/JSON/formatting, and
release emission totality (trait bounds, defer, tasks). Each seam is now a
carded root cause with exact file:line ownership, and every carded defect
ships its repro as a permanent fixture with the fix.

## Non-goals (explicit)

Web/DOM target, jetpack network paths, and TUI rendering were excluded and
each needs its own future sweep.
