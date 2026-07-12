# Surface condensation — 2026-07-11 sweep

Full-surface audit of ratified law (`syntax-decisions.md`), `Syntax.rs`
(~200 user-typeable entries), the CLI (60+ verbs), and the `core.*` tree
(70+ modules), hunting I8 violations: two spellings for one semantic job,
contradictions between ratified decisions, and missing magic defaults.
Every action item below is a Tower ballot (one decision per ballot) or a
no-decision consistency card. Nothing here removes a feature; merges are
clean breaks with no deprecation residue.

## A. Contradictions inside ratified law (ballots)

### A1. `jet store` vs `jet hangar` vs `jet clean` — D-CLI-STORE2

D-CLI-SURFACE1=B (2026-07-10) creates a `jet store` group holding
verify/rollback/generations **plus gc**, fetch, lock. D-JPK-STORECLI1=D
(same day) gives the physical verbs to `jet hangar` (verify, repair, copy,
import, export, dump/restore, sign) and re-affirms `jet clean` as the
**sole** GC+optimize intent (owner amendment 2026-07-03: "there is no
`jet store gc`"). The shipped CLI has all three spellings plus a bare `gc`
alias. One noun must own the store.

### A2. `jet serve` ≡ `jet dev --swap` — D-CLI-DEVSERVE1

`Source/main.rs:1395`: `jet serve <entry>` == `jet dev <entry> --swap`,
a literal alias. S14: no aliases, ever. Either `serve` gets distinct
semantics or it dies.

### A3. `#Wasm` / `#Js` vs `#Target(Wasm)` / `#Target(Js)` — D-MARK-TARGET1

`Syntax.rs` ships both marker families; D-OSTARGET1 already ratifies
`#Target(…)` taking `Web/Browser/Wasm/Js`. Two spellings, one job
(item-level target partition). `#WasmExport` is a distinct job (export
surface) and is not part of the merge question.

### A4. `#Suppress(MustUse) { … }` vs `.drop("reason")` — D-MARK-DISCARD1

D-IGNORERET1/2's recorded law says the shipped discard spelling is
`.drop("reason")`, yet the parser/sema also ship a region form
`#Suppress(MustUse) { … }` under the same decision ID. Two spellings for
"I mean to discard this result."

### A5. `@Debug` opt-in vs Debug auto-derive — D-MARK-DEBUG1

S55 lists `@Debug` among **explicit opt-in** markers; D-DISPLAYDBG1 says
Debug "is dev-facing and **auto-derived**". Both are current law; they
disagree. Pick one derivation law.

### A6. `core.archive` vs `core.compress.{gzip,zstd}` — D-CORE-COMPRESS1

Both are registered core modules; both own gzip (`core.archive` via
D-DEP-ARCHIVE1, `core.compress.gzip` via D-CODECS1). One byte-stream
codec should have one home; container formats (zip/tar) are a separate
job from stream codecs (gzip/zstd).

### A7. CLI verbs unassigned to the ratified rings — D-CLI-SURFACE3

D-CLI-SURFACE1=B fixed a ~20-verb flat ring + four groups
(registry/inspect/store/self), with moved verbs becoming teaching errors,
never silent aliases. The shipped CLI still has, outside both ring and
groups: `env, fetch, search, info, logs, outdated, clean, push, trust,
bridge, services, image, os, config, toolchain, doctor, devtools, gc`.
Three (`doctor`, `devtools`, `gc`) are silent aliases of grouped verbs —
a direct S14/D-CLI-SURFACE1 violation. Every verb needs a ratified home.

## B. Missing magic defaults (ballots)

### B1. Bare project verbs — D-CLI-BARE1

`jet` → REPL (D-REPL4) and `jet file.jet` → run (D-CLI-SUGAR) already
ship. But inside a package, `jet run` / `jet dev` / `jet debug` /
`jet bench` still demand a file argument even though `pkg.jet` +
D-ILE1 target inference already know the entry (`jet test` and `jet fmt`
are already bare-capable). Beginner pass: `cd app && jet run` should just
work. Expert pass: explicit file/`-p member` always wins.

## C. Ratified-kept overlaps — observations only, no ballot

- `++`/`--` beside `+=`: owner-chosen I8 exception (D-INCR1). Standing.
- `core.term` vs `io.terminal`: D-COREIO1=A explicitly keeps `core.term`
  as the raw-key bridge. Standing.
- `print`/`input` prelude vs `core.io` handles: one mechanism, two
  entrypoints (beginner/expert). I8-compatible by design.
- Method-in-body vs `fn Type.method` vs `impl Type { }`: structural
  flexibility, one feature (philosophy "one mechanical path, flexible
  structure").

## D. Consistency debt — cards, no decision needed

- **Data → DataTree sweep**: D-SERDE13=B renamed the dynamic value to
  `DataTree`, but later ratified texts (D-ENCSTREAM-SURFACE1, D-ENCXML1,
  D-ENC-DYN1 index entries) still speak `Data`. Sweep docs + surface to
  the ratified name.
- **Maturity metadata check**: D-MARK-META1=B makes
  `#Meta(maturity: .Experimental | .Tested | .Hardened)` the sole maturity
  surface. The closed values stay out of standalone marker registries.
- **D-CLI-SURFACE1 implementation drift**: `doctor`, `devtools`, `gc`,
  `toolchain` still dispatch flat (silent aliases); law requires grouped
  spelling + teaching error. Implementation card (gated on A7 for the
  unassigned verbs).

## E. v2 census wave (2026-07-11, second pass — card #509)

Full census: 58 registered markers (20 `@` / 38 `#`), derived from
`Syntax::CONTRACT_MARKERS` and `Syntax::DIRECTIVE_MARKERS`; 111 keyword
entries, ~70 core modules, 587 diagnostic codes. Three ballots fall out;
drift fixes are cards (see `architecture-infra.md`).

### E1. Marker growth law + maturity field — D-MARK-META1

Doc-only metadata uses `#Meta`: `category` and `tunable` come from
D-CANVASMETA1, while
`#Meta(maturity: .Experimental | .Tested | .Hardened)` comes from
D-MARK-META1=B. Every future doc-only annotation is a ratified `#Meta` field,
preserving one tool-facing metadata mechanism.

### E2. Three secret-adjacent modules — D-CORE-SECRETS1

`core.vault` (encrypted secret store), `core.secrets`
(rotating/expiring secrets), `core.crypto` (primitives + envelopes).
The vault/secrets boundary is not teachable; TTL wrapping already lives
in `core.time.expiring`. Ballot: merge `core.secrets` into `core.vault`
(one secrets home: store + rotation), crypto stays primitives.

### E3. Core namespace admission law — D-CORENS2

47 top-level `core.*` entries and growing. A new top-level name should
require a *domain*, not a feature. Ballot: the admission rule plus the
two moves the rule implies today — `core.devserver` →
`core.web.devserver`, `core.async.loadable` → `core.reactive.loadable`
(the `async` prefix names a namespace that doesn't otherwise exist).

## F. Go-simplicity pass (2026-07-12, third pass — card #512)

Lens: Go's discipline — few words, one obvious way, orthogonal families —
applied to every ratified surface, feature-preserving. Go holds 25
keywords by *excluding* effects, comptime, patterns, and a low-level
tier; Jet carries those, so raw keyword count is the wrong target. The
right target: **every construct belongs to a family with one rule.**
This pass hunts family outliers. Every finding was checked against
ratified law; each ballot names the rule it completes.

### F1. Expert regions are `#` blocks — except three — D-BLOCKPLANE1

The scoped-region family is `#Unsafe { }`, `#Transact { }`, `#Shield { }`,
`#Caps { }`, `#Grant { }`, `#Reactive { }`, `#Impure { }` — directive
plane, exactly where D-MARKER-FAMILY1 puts "changes what's legal in a
region". Three regions are bare keywords instead: `region r { }`
(D-REGION1), `live { }` (D-TERM1), `assume_deterministic { }` (D-DET1).
Three keywords die; one rule survives: *an expert region is a `#` block*.
Bonus: `assume_deterministic` becomes reason-gated like its siblings
(`#Nondeterministic("reason")`).

### F2. One meaning for "policy" — D-POLICY-WORD1

`policy no_alloc` is a bare in-source module item (D-NOALLOC-SEM1);
`policy:` is the manifest governance namespace (D-JPK-POLICYSURFACE1,
trust/lints/providers/replacements). Same word, two unrelated surfaces.
The module floor is a directive ("changes what's legal") →
`#Policy(no_alloc)`, freeing the bare word for the manifest namespace
alone.

### F3. One meaning for "drop" — D-DROP-WORD1

`.drop("reason")` discards a must-use result (D-IGNORERET, just
re-ratified as the sole discard). `drop(x)` inside `#Unsafe` *consumes a
linear value* (D-LIN1-DROP) — a different semantic under the same word.
Rename the linear one `consume(x)`: it literally satisfies "must be
consumed exactly once", and the expert-tier surface is tiny.

### F4. One spelling for "show me the Rust" — D-CLI-EMIT1

`jet emit --rust <file>` (verb) and the global `--emit-rust` flag
("also print the generated Rust") both exist. Two spellings, one
output. Keep the verb; the flag dies.

### F5. One math home — D-CORE-NUMERIC1

`core.numeric` holds two types (BigInt, Decimal); `core.math` holds the
math functions and is already width-generic (D-FLOATW1). Go's
`math/big` precedent: fold numeric into math. One import for
everything numeric.

### F6. Considered and kept (the negative space, with reasons)

- `::`/`:=`/`=` bindings — owner-frozen core (D-BIND4).
- `++`/`--` beside `+=` — owner-chosen I8 exception (D-INCR1).
- `?` family (`?`, `??`, `?.`, `T?`, `T ? E`) — one coherent axis.
- `if x == { }` dispatch — ratified branching core; no confusion
  evidence on file.
- `#` triple mnemonic (marker / pinned number / version pin) — ratified
  "a pinned number" story; each position unambiguous.
- Visibility quartet (`pub`, `priv`, `pub(package)`, `#PubFile`) —
  `#PubFile`+`priv` is the ratified org-flexibility axis (I8 allows
  policy flexibility); dropping it loses a feature.
- Migration contextual verbs (`add`/`remove`/`change`/`rename`/`via`) —
  contextual inside `migration { }` only; zero collision cost.
- `taskgroup g { }` — a control structure like `loop`, not an expert
  region; keyword is right.
- `it` — the ratified dispatch-subject name for expression subjects.
- `jet ?` vs `jet help` — D-FE-HELP1=D makes `?` the interactive app,
  `help` the static text; distinct jobs, watch for drift.
- Effect spellings (`#(Net)`, `#(!Net)`, `#Caps`, `#Grant`, `@Pure`,
  `#(via f)`) — five distinct semantics (declare/prohibit/restrict/
  authorize/publish) on one row grammar; no merge without losing one.
- Three method entry points — philosophy-blessed structural
  flexibility, one feature.
- `fn run`/`fn build`/`fn dev`/`#Task` entry family — reserved-name law
  exists (D-JPK-TASKRUN1).
- Sized-number menu, `step`, unit literals, `Val`/`None`, multiline
  strings — all settled, no outliers found.

Codebase-side: the corelib/ source tree vs the include_str-embedded
prelude (CoreLib.rs) needs a source-of-truth audit — added to card #511.

## G. Exhaustion pass (2026-07-12, fourth pass — API layer + residue)

Layers swept this pass: the method-level Core API surface against the
D-STDRUBRIC1 rubric, spec.md's full heading map, every remaining marker
family, board epochs/milestones. Findings:

### G1. Law 1 vs shipped `len()` — D-API-LEN1 (card #513)

The API rubric's own example ("`length`, not `len`") contradicts S41/S76
ratified surface (`s.len()`, `.len` compile-time constant). One text
must bend; ballot with rec to amend the rubric (closed blessed list).

### G2. Marker-family residue — notes, no ballots

- `#Track` (D-PROVENANCE1=B) shipped narrow: Float-local origins only,
  `.origin() -> String`. Not an I8 violation — an incomplete feature.
  Belongs on its implementation card's criteria, not a new decision.
- `@Comparable` names both a derive marker (D-MARKERMOVE3) and a
  capability bundle on `distinct` types (D-CAPBUNDLE1). One meaning —
  "this type is comparable" — two mechanisms behind one word, which is
  the hybrid-pass-compatible shape; no confusion evidence on file.
- The serde field-marker family (8 `#[...]` markers) is coherent and
  matches its ratified wire law; no action.
- Everything else in both marker planes maps 1:1 to a ratified decision
  with no family outlier remaining after D-BLOCKPLANE1/D-POLICY-WORD1.

### G3. Structures verified healthy

- Board: 9 epochs (e2 arrived → e9 planned), 2 open milestones, lanes
  lint-clean. No reorganization warranted.
- spec.md heading map aligns 1:1 with ratified law; it closes with a
  "Deliberately absent" negative-space record — the discipline this
  document now mirrors for surface decisions.
- stdlib-api-laws.md drift table references pre-migration card ids
  (`c44-follow-*`) — re-mint as real Tower cards (rides card #513).

## H. Fifth pass (2026-07-12) — lexical core, prelude, artifact names

Layers swept: the M1 lexical/grammar core (EBNF, precedence table,
terminator rules), the ambient prelude surface, the artifact-extension
namespace, the env-var namespace.

### H1. Artifact-extension law + `.jreplay` collision — D-ARTIFACT-EXT1 (card #514)

Two extension families ship (`.jetmap`/`.jetnb` vs
`.jproof`/`.jtrace`/`.jreplay`) and `.jreplay` names two different
ratified formats: game input replays (D-GAME-REPLAY1) and proof replays
(D-JREPLAY1). One family law + collision fix.

### H2. Ambient-surface registry — D-PRELUDE-LAW1 (card #514)

The no-prefix list spans D-PRELUDE1 (`print`, `input`), S36 (`panic`,
`require`), and the comptime gates (`embed_file`, `embed_bytes`, `find`,
`fetch`). One closed registry + ballot-gated admission, the same shape
as the namespace/metadata/constructor laws.

### H3. Drift found in the lexical core — criteria on card #500

- spec.md EBNF still shows the retired `~` param sigil (D-MEM1).
- `input()` documented as `Result(String, IoError)` — non-Jet spelling
  and an S66 violation (`IOError`).
- JET_* env vars have no registry page; added as a reference-docs item.

### H4. Verified clean this pass

Precedence table (no surprises; `|` vs `&&` split matches pattern law),
terminator/continuation rules (S6-R self-consistent, `ends_statement`
extension rule recorded), string escapes (closed set, E0001 on the
rest), number literals, extension-optional path resolution, `if`-arm
grammar (range arms with their three porting-hazard teaching errors),
label grammar, destructure grammar. The M1 core is coherent; its only
defects were documentation drift (H3).

## I. Sixth pass (2026-07-12) — release policy, error reference, cross-doc drift

Layers swept: release-policy.md (first full read), docs/reference/errors/
(per-code explain sources — healthy), roadmap↔board epoch numbering,
Syntax module naming. **Zero decision-bearing defects found** — the
convergence point. Drift recorded as criteria:

- roadmap.md still numbers Bootstrapping as Epoch 8; the board has
  e8 = CI & Documentation, e9 = Bootstrapping (#500).
- release-policy.md's enforcement section cites pre-seam paths
  (`Source/Jetpack/…`, `Source/Syntax.rs`, `Source/Loader.rs`) (#500).
- `Syntax/effects_tests.rs` holds keyword constants under a *_tests
  name (#511).

Owner-question revisions the same day: D-API-STORE1 rewritten to the
add/add_new collision pair (upsert returning displaced `T?`; race-safe
`add_new -> Bool`); D-VALIDATE1 rewritten to the three-layer chain
(type-level constraints + `Validate.over` accumulation chain +
`decode().validate()` entry, `@Pre` out of the story).

Analysis of the polyglot/replace-every-language track lives in
[`polyglot.md`](polyglot.md).
