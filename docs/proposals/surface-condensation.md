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
- **Maturity-marker plane check**: D-MATURITY1 ratifies
  `@Experimental/@Tested/@Hardened` (retired `#` spelling = E0062);
  `Syntax.rs` still carries them in `#`-plane marker lists. Verify plane
  wiring, fix drift, re-bless.
- **D-CLI-SURFACE1 implementation drift**: `doctor`, `devtools`, `gc`,
  `toolchain` still dispatch flat (silent aliases); law requires grouped
  spelling + teaching error. Implementation card (gated on A7 for the
  unassigned verbs).

## E. v2 census wave (2026-07-11, second pass — card #509)

Full census: 63 markers (24 `@` / 39 `#`), 111 keyword entries, ~70 core
modules, 587 diagnostic codes. Three ballots fall out; drift fixes are
cards (see `architecture-infra.md`).

### E1. Marker growth law + maturity trio — D-MARK-META1

Doc-only metadata now spans three surfaces: the maturity trio
(`@Experimental`/`@Tested`/`@Hardened`, D-MATURITY1), `#Meta(category,
tunable)` (D-CANVASMETA1), and `@Doc` (D-CLIFLAG1). Each was ratified
alone; together they are three ways to attach tool-facing metadata.
Ballot: a growth law — every future doc-only annotation is a `#Meta`
field (one ballot per field), and whether the shipped trio folds in or
stays.

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

Analysis of the polyglot/replace-every-language track lives in
[`polyglot.md`](polyglot.md).
