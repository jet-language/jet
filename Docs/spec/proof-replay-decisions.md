# Proof and replay decisions

This record preserves the rationale and ratified contracts for `jet prove`,
`.jetproof` evidence, `.jetproof-replay` capture, the opt-in solver, and proof
lenses. It is for compiler, CLI, artifact, and test authors. The executable
spellings and receipt boundaries are in [`Source/CmdProve.rs`](../../Source/CmdProve.rs),
[`Source/ProveReplay.rs`](../../Source/ProveReplay.rs), and
[`Source/ProveSolver.rs`](../../Source/ProveSolver.rs); exercised command
contracts are in [`tests/prove.rs`](../../tests/prove.rs), and diagnostic copy
is in [`Diagnostics.jet`](../../crates/jet-codegen/src/Prelude/Diagnostics.jet).
Use the [Jet vocabulary](vocabulary.md) for domain terms.

Each section states a rule, the reason for it, and its consequence. Decision
IDs are citations, not alternate names for commands or formats.

## Command surface

**Rule.** `jet prove TARGET` accepts exactly one file, package, or workspace
target. `TARGET` is the only positional argument. The command may select a
lens with repeated `--lens VALUE` or `--lens=VALUE`, print machine output with
`--json`, consume a validated replay with `--replay ARTIFACT` or
`--replay=ARTIFACT`, or capture with `--capture`, `--capture=ARTIFACT`,
`--capture-sensitive`, or `--capture-sensitive=ARTIFACT`. Capture and replay
are mutually exclusive. The proof surface does not create `jet replay`, alter
`jet run`, or add `out`, `encrypt`, `key`, or `case` flags. (D-PROVE-REPLAY1=A)

**Reason.** One command keeps target selection, front-end checking, evidence
production, and replay authority on one path. A closed grammar prevents a
second command or a spaced capture argument from silently selecting a different
producer.

**Consequence.** These are canonical forms:

```sh
jet prove src/payments.jet --json
jet prove src/payments.jet --capture=traces/run.jetproof-replay
jet prove src/payments.jet --replay=traces/run.jetproof-replay --json
jet prove src/payments.jet --lens solver --json
```

The executable parser also accepts a spaced replay path (`--replay
ARTIFACT`). Capture paths use the `=` form. An invalid flag or an incorrect
number of positional arguments is usage error 2 and produces no proof report.

**Rule.** A file target must be a `.jet` file and neither it nor an ancestor
may be a symlink. A directory is a package when it contains `package.jet` and
is otherwise a workspace. Jet gathers the `.jet` members, sorts normalized
paths by their UTF-8 bytes, and adds the relevant package, lock, build, import,
and generated-input closure to target identity.

**Reason.** The proof must describe one deterministic input closure rather than
whatever directory traversal happens to return.

**Consequence.** The report target is
`{kind:file|package|workspace,root,members:[{path,sha256}],inputSha256}`. A
capture or replay operation additionally requires exactly one runnable member;
zero or multiple members is E3624 before build or program effects. The source
resolver and its package marker are the authority for this selection.

## Proof execution

**Rule.** Every valid invocation uses one semantic producer order: front-end
checking, doctests in source order, the compiled `#Test` harness in declaration
order, deterministic budgets, and statistical budgets. A plain `#Test` is unit
evidence. A parameterized `#Test fn` is property evidence produced by the
seeded generator and shrinker. Front-end failure records execution evidence as
skipped with `prior_frontend_failure`; independent later members continue.

**Reason.** A single order makes counts, diagnostics, and replay outcomes
comparable. Continuing independent members preserves useful evidence without
pretending that a failed front end was checked.

**Consequence.** `jet prove` invokes the existing test producer with the same
member ordering and does not measure a budget. Compatible budget reports are
projected as evidence; malformed or rejected reports are unavailable rather
than invented. The `E2940` contract is:

- What `required proof evidence is unavailable`;
- Why `the complete_required policy needs {producer}, but {reason}`;
- Fix `{producer-specific imperative action}`.

For example, the ratified render remains:

```text
Error [E2940]: required proof evidence is unavailable
Why: the complete_required policy needs statistical budget checkout-p95, but no matching baseline exists
Fix: run jet test --measure src/payments.jet on the pinned profile to record the baseline
```

**Rule.** An `assert` or `assert_eq` failure inside the test harness is a
caught assertion. That test records `failed`, later harness tests continue, and
the final result has exit 1. `panic(...)`, bounds or key stops, and failed
`#Pre` or `#Post` clauses are uncaught runtime stops. They retain E3001 or
E3005, terminate that child with exit 70, mark declarations trapped in that
child as skipped with `prior_runtime_panic`, and do not stop independent later
members or budgets.

**Reason.** Caught assertions are test outcomes; an uncaught stop is a runtime
boundary. Treating them differently keeps the exit status and evidence ledger
honest.

**Consequence.** After a valid invocation, exit precedence is ICE 101, runtime
stop 70, user/static/caught-assertion/budget/policy failure 1, then success 0.
Malformed CLI remains 2. No view may expose raw rustc text, generated-Rust
paths, a backtrace, or an implementation-only type name. A reached contract is
attached to the producer invocation that called it; an unreached clause is
`declared` and `not_observed`.

**Rule.** The report uses `allow_incomplete`. Every applicable producer runs.
Complete available evidence is `pass`/0. Passing evidence combined with
unavailable, skipped, or merely declared evidence is `pass_incomplete`/0;
`complete_required` may reject that typed result. A caught assertion is
`fail`/1, and an uncaught E3001 or E3005 takes precedence with 70.

**Reason.** Missing evidence is different from failed evidence. A typed
incomplete result lets a caller choose whether to require every producer
without changing what Jet observed.

**Consequence.** `zero` means successful discovery with count 0. `unavailable`
means an applicable producer could not start or lacked input, with one of
`missing_baseline`, `missing_tool`, `unsupported_target`, or
`producer_start_failed`. `skipped` means deliberate non-execution, with one of
`prior_frontend_failure`, `prior_runtime_panic`, `not_executed_by_mode`, or
`fail_fast_policy`. Existing E0613, E0965, E2901, E3001, and E3005 copy through
unchanged. E2940 requires its registry entry, UI snapshot, `jet explain`,
generated reference, and I4 coverage before use. (D-PROVE-SEM1=A)

## Report shape and diagnostics

**Rule.** One in-memory `ProofReport` is the semantic object. Its required core
members are `schemaVersion`, `evidencePolicy`, `target`, `tool`, `result`,
`exitCode`, `summaries`, `evidence`, and `diagnostics`. `tool` is
`{jet,proofProducer,targetTriple}` and `result` is `pass`, `pass_incomplete`, or
`fail`. Evidence kinds are `front_end`, `doctest`, `unit`, `property`,
`contract`, `deterministic_budget`, and `statistical_budget`; states are
`declared`, `checked`, `executed`, `zero`, `unavailable`, and `skipped`.

**Reason.** Human output, JSON output, and persisted artifacts must project the
same facts. A second report schema would permit counts or diagnostics to drift.

**Consequence.** Evidence outcomes are `proved`, `passed`, `failed`,
`observed`, `not_observed`, `met`, `unavailable`, or `not_run`. Required
nullable payloads include:

- property: `{effectiveSeed,caseIndex,generatedCases,shrinkTrace,source,toolchain}`;
- contract: `{marker:Pre|Post,site,observation}`;
- budget: `{profile,kind,observations,baselineId,limit,actual,unit}`.

Every evidence item carries `id`, `producer`, `kind`, `state`, `outcome`,
`count`, `source`, `attachment`, `reason`, its typed payloads, and
`diagnosticIndexes`. A source-owned report may also retain `grade`,
`candidateIdentity`, `mir`, and `derivations`; those fields do not replace the
required semantic members.

**Rule.** Summaries derive only from evidence. For front-end, doctest, unit,
and property evidence, selected equals the appropriate passed or proved count
plus failed plus skipped. Property `generatedCases` is the sum of its evidence
payloads and `shrunkFailures` counts failed properties with a nonempty
`shrinkTrace`. Contract counts satisfy
`selected=declared=passed+failed+notObserved+skipped` and
`observed=passed+failed`. Each budget satisfies
`selected=met+failed+skipped+unavailable`.

**Reason.** A report must be internally checkable; independently maintained
counters are an easy source of false proof.

**Consequence.** A report that violates an equation is invalid. `--json` carries
the complete report, never a lens-filtered report. The command transport wraps
that payload in the standard `jet.status/v1` object; the wrapper is transport
metadata, not a second evidence model. Human output projects the same report.
(D-PROVE-SEM1=A)

**Rule.** Diagnostics are typed values, not bare codes. Each has
`{type,code,severity,message,origin,span,caret,notes,frames,context,safeLocals}`.
Front-end and producer diagnostics preserve What, Why, Fix, source span, source
line, and caret. Runtime E3001 and E3005 preserve the Jet message, Jet span and
caret, ordered Jet frames, source context, and D-OBS2 safe locals
`{name,type,value,redacted}`. No diagnostic contains rustc material.

**Reason.** A proof receipt must remain useful after the producer process exits,
and a user must be able to act on the Jet source location rather than an
internal backend.

**Consequence.** Diagnostic indexes are zero-based references into the report's
ordered diagnostic array. `jet explain` and generated references use the same
registry rows and snapshots as the command output. (D-OBS2)

## Persisted proof evidence

**Rule.** `.jetproof` persists the exact semantic `ProofReport` under
`proofReport`. The ratified logical version-1 envelope has
`schema:"jet.jproof"`, `version:1`, a lowercase 64-hex `report_id`,
`artifact:{path}`, `privacy:{absolute_paths,argv,environment,full_source,producer_transcripts,safe_locals}`, and
that complete `proofReport`. Artifact metadata is a sibling of the report, not
part of it. `report_id` is SHA-256 of the A-canonical ProofReport bytes plus
one LF.

**Reason.** Identity must follow facts, not destination, privacy declaration,
serialization choice, or a later signature. Consumers can recompute the hash
before trusting a receipt.

**Consequence.** The canonical path is
`.jet/proofs/<kind>/<name>/<first-16-report-id>.jetproof`. Identical existing
bytes remain unchanged; different bytes refuse overwrite. Privacy omits
absolute paths, argv, environment, full source, and producer transcripts;
D-OBS2 governs safe-local redaction. Project-relative paths, spans, semantic
messages, property counterexamples and shrink values, budget values, and any
unredacted safe local in the report are disclosed. `.jetproof` is sensitive
evidence. No unratified include-source, encryption, destination, overwrite, or
inspection flag exists.

The source writer's concrete envelope uses `schema:"jet.jproof"`,
`version:2`, `consumed`, `produced`, and `links_version:1` beside the
`proofReport`. Those links are delivery metadata and do not change report
identity. Proof links use `RecordKind::Proof` in
`.jet/records/index.jsonl`. Test evidence is written as a typed report under
`.jet/evidence/<report-id>.json`.

**Rule.** A-canonical JSON is UTF-8 without a BOM or insignificant whitespace,
with one trailing LF, recursively Unicode-scalar-sorted object keys, preserved
array order, shortest decimal integers, no floats or negative zero, NFC schema
and extension keys, exact user/source Unicode scalars, and the specified
shortest JSON escapes. Duplicate keys after normalization fail. Version-1
readers preserve unknown envelope and ProofReport members as complete JSON
values; unknown enums are unsupported and an unknown major version is E3613.

**Reason.** Hashes and artifact identity must be independent of map iteration,
platform JSON libraries, and harmless formatting choices.

**Consequence.** Readers recompute `target.inputSha256` and `report_id`. The
separate artifact hash covers all canonical artifact bytes; an optional future
`signatures` member is outside that semantic identity. This decision adds no
signing or trust command.

**Rule.** Jet canonicalizes the project, takes the exclusive mode-0600 proof
lock, creates new artifact directories as mode 0700, resolves parents and the
final component without following links, writes a same-directory exclusive
mode-0600 temporary file, fsyncs it, commits without replacement, and fsyncs
the parent directory. Absolute or `..` escape, symlink, non-directory parent,
or non-regular final path is E3614. Failure removes only the owned temporary.

**Reason.** A proof artifact is evidence, so a crash or a concurrent writer must
not expose a partial or substituted report.

**Consequence.** E3611–E3614 remain disjoint proof-artifact diagnostics:
corrupt/hash/canonical failure, wrong target, unsupported artifact version, and
unsafe path. They exit 1; malformed CLI is 2, runtime stop is 70, and ICE is
101. The registered copy, UI snapshots, `jet explain`, generated references,
and I4 coverage are part of the artifact contract. (D-JPROOF1=A)

The reserved artifact copy is:

| Code | What | Why | Fix |
|---|---|---|---|
| E3611 | `proof artifact is corrupt` | report hash or canonical bytes do not match | move the corrupt artifact aside and run `jet prove TARGET` again |
| E3612 | `proof artifact names a different target` | the embedded root differs from the resolved target | move the other artifact aside |
| E3613 | `proof artifact version {version} is not supported` | this reader does not support that `jet.jproof` version | use the writing Jet version or generate the supported version |
| E3614 | `proof artifact path is unsafe` | a parent or final path is a symlink or otherwise unsafe | replace it with a regular project directory or file |

The pinned refinements proof fixture retains report ID
`fc529e50a4e44a97559abbd6f5cbef458008adb1b0401ad7befc7dc5bfbc0b83` and
artifact SHA-256
`c1c4f75e6e018c4a68c9374cc0be8df61e2cb553f90b6b92c2d6d0e1a2b823ea`.
Its evidence IDs and the solver fixture IDs are cited in the solver section;
the writer and tests, rather than a duplicated generated JSON blob, own the
full byte sequence.

## Replay invocation and authority

**Rule.** Replay consumption remains
`jet prove TARGET --replay ARTIFACT`; it reads a validated
`.jetproof-replay`, grants no ambient effect, and never captures or mutates the
artifact. Capture is a complete invocation using the safe flag form
`jet prove TARGET --capture[=ARTIFACT]` or the sensitive counterpart
`--capture-sensitive[=ARTIFACT]`. Relative paths are project-relative,
forward-slash paths ending in `.jetproof-replay`; explicit paths must be
nonexistent. The flag form is the executable closed surface; `jet prove capture`
is not an alias.

**Reason.** Capture and replay have opposite authority directions. Keeping them
on the normal `jet prove` producer prevents a replay from becoming an
unreviewed execution shortcut.

**Consequence.** One invocation produces one run and at most one artifact.
Capture resolves the target, checks source and identity, computes the reachable
effect/native graph, checks ordinary lexical and invocation authority, checks
privacy, cardinality, target support, and destination, and only then builds.
Capture never grants an effect. Unsupported FFI, native callbacks, dynamic
loading, opaque calls, tasks or concurrency, wall-driven scheduling, and
unsupported effect boundaries fail during preflight. Replay opens no clock,
RNG, stdin, terminal, DNS, socket, file, environment, subprocess, FFI, task, or
GPU boundary; an unrecorded request is E3623.

**Rule.** A package or workspace capture or replay must resolve exactly one
runnable member. Zero or multiple members fail E3624 before build or program
effects and identify the selected paths. A `.jet` file is accepted only when
normal target resolution accepts it.

**Reason.** A replay artifact records one authority stream and one outcome; a
multi-member selection has no unambiguous stream owner.

**Consequence.** The preflight refusal leaves no final replay artifact. The
focused proof tests cover path escape, symlink targets, front-end failure,
reachable I/O, opaque calls, unsupported `Time.sleep`, multiple time sites, and
multi-member cardinality. (D-JREPLAY1=A)

## Replay wire format

**Rule.** Version 1 bytes are the ASCII magic `JREPLAY\0`, little-endian u16
major and minor, little-endian u32 canonical-header length, canonical header,
zero or more frames, and a footer. The header is UTF-8 JSON without a BOM,
whitespace, duplicate keys, or floats; field names are ASCII and byte-sorted,
required integers are minimal decimal, strings use the shortest valid JSON
escape, and user text is never normalized. Required fields are `schema`,
`version`, `artifact_id`, `producer`, `identity`, `capture`, `privacy_salt`,
`limits`, and `run`. Optional fields live under sorted `extensions` maps.

**Reason.** The binary prefix makes version rejection unambiguous, while the
canonical header makes identity and byte hashes reproducible across hosts.

**Consequence.** A prefix/header version disagreement, invalid UTF-8, invalid
path, unknown required field or record kind, or unsupported major version is
rejected as E3620 or E3622 according to the failing layer. Paths are
project-relative UTF-8 forward-slash paths with no empty, `.`, `..`, absolute,
NUL, or symlink component. Binary values use unpadded base64url; out-of-range
64-bit integers use lowercase fixed-width hexadecimal strings; floats use
lowercase IEEE-754 bit-pattern hex and preserve NaN payload and signed zero.

**Rule.** A frame contains u8 flags, u16 kind, u64 sequence, u32 payload length,
payload bytes, and a raw SHA-256 over flags through payload. Numbers are
little-endian, sequences start at zero and are contiguous, and the footer is
`JEND`, u64 frame count, u64 payload-byte count, and a SHA-256 over every byte
before `JEND`. The reader verifies frame hashes, footer, lengths, ordering, and
the zeroed-field content ID.

**Reason.** Framing lets replay reject corruption before injecting any value;
sequence and request identities prevent a reordered record from appearing
valid.

**Consequence.** The v1 registry includes these record shapes:

| Kind | Payload |
|---|---|
| `0x0001` `TimeWall` | `{site_id,call_id,unix_ns}` |
| `0x0002` `TimeMonotonic` | `{site_id,call_id,ticks_ns}` |
| `0x0003` `TimeSleep` | `{site_id,call_id,requested_ns,outcome}` |
| `0x0010` `RandBits` | `{site_id,call_id,requested_bits,bits}` |
| `0x0020`–`0x0022` I/O | `IORead`, `IOChunk`, `IOEnd` with stream, chunks, bytes, and EOF/error |
| `0x0030`–`0x0034` network | request, DNS, response, chunks, and EOF/error |
| `0x00f0` `Outcome` | normal/explicit exit or `runtime_stop` with exact diagnostic code and output hashes |
| `0x4000` `GameEvent` | `game.v1.action`, `fixed_step`, `backend`, or `budget` |
| `0x1001` `RunAct` extension | `{function,line,locals}` causal act snapshot |

The source codec writes `TimeWall` and, when a recorded run is supplied,
optional `RunAct` frames; its header marks that extension as `recorded_run`.

`TimeSleep` permits only `interrupted`; I/O and network terminal records use
the portable error registry
`interrupted|timed_out|would_block|broken_pipe|connection_aborted|connection_refused|connection_reset|not_connected|address_in_use|address_unavailable|permission_denied|not_found|unexpected_eof|invalid_data`.
An unmapped or ambiguous OS error fails closed with E3625. Missing, extra,
reordered, wrong-site, wrong-shape, or unconsumed records are E3623.

Canonical Jet values are tagged objects: unit is `{"t":"unit"}`; booleans,
integers, floats, text, bytes, lists, and records use the `t`/`v` or
`fields` shapes specified by the wire contract. Record fields sort by ASCII
name. Missing, extra, duplicate, unsupported, or noncanonical values are
E3622. Limits are 100,000 frames, 1 MiB per payload, and 256 MiB per artifact;
crossing a limit deletes the temporary and emits E3628 without a valid artifact.

The pinned one-frame vector is 1054 bytes with `TimeWall` kind `0x0001` at
sequence 0. Its test salt is
`AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA`, content ID
`c10aee50b2a70ae2bfc02072`, and full-file SHA-256
`72bf4a0448f895b84993950ee59061d04ea3fe4378fdd05c4c679d717dae7090`.
Its normative path is
`.jet/replays/fixture-c10aee50b2a7.jetproof-replay`; production names use the
random-salt artifact ID and never this predetermined ID.

## Replay privacy, identity, and delivery

**Rule.** Safe capture records Time only and refuses reachable raw Rand, I/O,
terminal input, or network data with E3627. Sensitive capture is the sole path
for storing those raw values; it requires the exact TTY phrase
`capture sensitive`, prints authorities, hosts, byte limits, residual
disclosures, and destination, and refuses in a non-interactive process. The
artifact is mode 0600 and not encrypted. (D-JREPLAY1=A)

**Reason.** Untainted input can still contain secrets. Explicit consent and a
visible disclosure boundary are safer than inferring privacy from taint alone.

**Consequence.** A sensitive artifact may disclose stdin, response bodies, DNS
answers, paths, host names, panic text, and run metadata to anyone who can read
it. Secret or tainted data in URL userinfo, query values, header names or
values, or a body fails E3627 before the request. A 32-byte OS-random
`privacy_salt` is encoded as 43-character unpadded base64url. Header and body
digests use the exact `jet.replay.v1/net-header` and
`jet.replay.v1/net-body` NUL-separated, length-framed preimages; replay
recomputes them and reports divergence as E3623. Stdout and stderr are never
stored as raw bytes: outcome stores lengths and SHA-256 hashes, while the
terminal receives live output. No performance sample, span, allocation,
scheduler, lock, duration-profile, source-map, flamegraph, or OTel field is a
replay record; those belong to card #441 and `.jettrace`.

The non-interactive sensitive boundary is observable as:

```text
Error [E3627]: replay capture refused sensitive data
 Why: non-interactive `--capture-sensitive` cannot collect TTY consent for raw Rand/IO/Net values
 Fix: run from a TTY and type `capture sensitive`, or use safe `--capture` for Time-only roots
More: jet-lang.dev/e/E3627
```

**Rule.** Replay identity binds schema, Jet build, Core ABI, MIR/TIR schema and
hashes, target triple/backend/profile, producer, entry, source hashes,
package/workspace/lock/build inputs, and the authority/capture manifest. A
reader requires exact semantic identity (E3621), then runs the recorded adapter
and any ratified counterpart. `dev-tir-v1` and `aot-native-v1` are the only
ratified counterpart pair for a matching native target, ABI, profile, Core ABI,
and checked TIR hash.

**Reason.** Reproducing a value is not equivalent to proving that two different
program identities or adapters produced the same result.

**Consequence.** Each adapter consumes all records and matches the outcome. A
target without a counterpart may report `parity: not available` and may
reproduce, but may not claim dev/AOT equivalence. Browser/WASM and other
platforms do not substitute a host backend. The executable `jet prove` receipt
uses `execution_adapter:"mir-v1"` and `mir_schema:"mir-v1"`; tests assert both
fields, so that source spelling must not be silently replaced by the ratified
counterpart labels. `D-ARTIFACT-EXT1` keeps this proof format separate from
card #238 game input recordings: `game.Replay.record("path.jetreplay")` writes
only a game replay, while `jet prove --replay` consumes only
`.jetproof-replay`; neither consumer accepts the other's artifact.

**Rule.** Capture writes through a same-directory mode-0600 temporary, fsyncs
content and footer, closes and verifies canonical bytes, then commits without
replacement and fsyncs the directory. An identical final path is accepted as
`already captured`; differing bytes are E3629 and never overwrite. Default
names are `.jet/replays/<sanitized-entry>-<artifact-id[0..12]>.jetproof-replay`.
Explicit paths must stay project-relative, use safe parents, and not already
exist. A crash temporary is not evidence.

**Reason.** The artifact must be durable and content-addressed without allowing
a concurrent or untrusted writer to replace evidence.

**Consequence.** Normal exit, explicit nonzero exit, and captured runtime stop
may finalize an artifact. Authority, cardinality, privacy, unsupported/build,
signal/kill, limit, recorder, finalization, or ICE failure emits no valid
artifact. Captured or replayed panic returns 70; an explicit nonzero or replay
or capture diagnostic returns 1; malformed CLI is 2; ICE is 101. Runtime-stop
identity preserves E3001, E3005, or another existing exit-70 code rather than
relabeling it. (D-JREPLAY1=A)

The replay code allocation keeps these What/Why/Fix meanings:

| Code | What | Why | Fix |
|---|---|---|---|
| E3620 | `replay schema version is incompatible` | the required reader vocabulary is unsupported | use a supported reader or recapture |
| E3621 | `replay semantic identity does not match` | the first source, build, toolchain, or adapter identity differs | replay the exact recorded revision |
| E3622 | `replay artifact is corrupt` | a byte, frame, canonical, length, or footer check failed | pass an intact `.jetproof-replay` |
| E3623 | `replay diverged at record {sequence}` | the request, order, site, shape, or outcome differs | recapture and replay the matching target |
| E3624 | `replay target cardinality is not one` | target selection produced zero or multiple candidates | select one runnable target |
| E3625 | `replay capture cannot model {operation}` | an effect, native, task, opaque, or adapter boundary is unsupported | change the source or target to a supported boundary |
| E3626 | `replay capture lacks {effect} authority` | an existing lexical or invocation grant is missing | add that existing grant or change the target |
| E3627 | `replay capture refused sensitive data` | raw or tainted data would be stored without the required consent | provide sanitized input or use the approved sensitive path |
| E3628 | `replay capture exceeded its artifact limit` | actual frames or bytes exceed the bound | reduce the captured input |
| E3629 | `replay artifact could not be finalized` | the exact destination, race, or filesystem durability check failed | fix the path and retry |

E3620–E3629 exit 1; usage is 2, captured or replayed E3001 is 70, and ICE is
101. The registry and snapshots use the same meanings (for example, the
rendered What for E3623 is `Replay diverged from captured authority`).

The replay receipt index is `.jet/records/index.jsonl`, keyed by target input
hash, tool version, and execution engine. Saved replay bytes use
`.jet/records/saved/<artifact_id>`; ordinary artifacts retain the
`.jetproof-replay` suffix. These paths and the `RecordKind::Replay` links are
source-owned receipt mechanisms, not a second replay format.

## Solver proof boundary

**Rule.** The solver is an opt-in `--lens solver` producer. Bare `jet prove
TARGET` never runs it. Sema remains authoritative for compilation, user
rejections, and diagnostics; the solver consumes immutable typed obligations
exported after sema. It cannot parse source as an authority, add syntax, relax a
rejection, alter TIR or codegen, or ask rustc whether code works. It is a
standard-library-only path producer with no external crate, daemon, network
call, executable, or compiler dependency. (D-WD12) (D-OOBPROOF1)

**Reason.** Proof must discharge facts Jet already understands, not create a
second type checker or backend-dependent acceptance path. This preserves I3,
I6, and the one-meaning rule across execution tiers.

**Consequence.** File, package, and workspace resolution, identity, ordering,
report parity, exits, and `.jetproof` persistence remain the proof laws above.

**Rule.** An obligation is `{id,kind,assumptions,claim,origin}`. Its ID is a
lowercase SHA-256 over length-framed UTF-8 components: target input hash,
obligation kind, project-relative path, source span, and formula hash. The v1
kinds are `fixed_index_bounds`, `call_precondition`, and
`function_postcondition`. Claims cover fixed-list bounds, covered linear
`#Pre` calls, and covered linear `#Post` returns. No solver option proves memory
safety, effects, termination, concurrency, floating point, runtime I/O,
sampled properties, or arbitrary Bool-returning functions.

**Reason.** A bounded obligation vocabulary makes a solver result auditable and
keeps unknown outside the solver's authority rather than disguising it as a
compile result.

**Consequence.** Formula bytes are one A-canonical JSON object plus LF with
normalized affine inequalities. Terms combine and sort by UTF-8 variable ID;
Boolean nodes flatten, deduplicate, and sort by canonical bytes; integer
comparisons normalize to `le`, `eq`, or `ne`. The typed IR permits integer
literals, affine addition/subtraction, literal multiplication, comparisons,
and Boolean `and`/`or`/`not`. Variable multiplication, division, modulo,
powers, bitwise operations, Float, strings and collections except fixed-list
length, uncovered calls, recursion, loops, branch-crossing mutation, effects,
dynamic dispatch, unsafe/native/FFI, and `assume_deterministic` are
`unknown unsupported_formula`, never compile errors.

A function postcondition requires a pure, nonrecursive, branch-free body of
affine immutable bindings followed by one return. A call-precondition site
requires affine arguments and branch-free dominating assumptions. Fixed-index
obligations use the checked fixed-list range and refinement facts.

**Rule.** Obligation order is target-member UTF-8 path, source span, kind, then
ID. Backends are deterministic: no wall clock, host load, thread race, random
seed, portfolio, or map order affects a result. Limits are 10,000 obligations
per invocation, 50,000 normalized terms per obligation, 256 variables, 256-bit
source literals, and 1,000,000 backend steps per obligation. Checked i128
overflow returns `unknown coefficient_overflow`; it never wraps. Structural
overflow returns `unknown structural_limit` with zero steps.

**Reason.** Fixed work budgets make an unknown result reproducible and prevent a
large or adversarial obligation from turning a proof command into an elapsed
-time service.

**Consequence.** Native-Presburger uses canonical FIFO branch search; finite
checking enumerates assignments in variable-name then numeric order. A step is
charged before its action and the limit is checked first. External kill produces
no complete ProofReport or `.jetproof` and retains no partial proof.

**Rule.** Backends return `proved`, `disproved`, or `unknown`. `proved` requires
a certificate in solver evidence and an independent standard-library checker
against the exact normalized obligation. Native-Presburger certificates use
`assumption`, `and_intro`, `linear_contradiction`, and `split` nodes; the
checker validates indexes, arithmetic, branch coverage, and every leaf.
Native-finite certificates carry `domainManifest`, `assignmentCount`, and
`rollingResultSha256`. `certificateSha256` hashes A-canonical certificate JSON
plus LF. `disproved` requires a concrete integer assignment that satisfies all
assumptions and falsifies the claim. An invalid certificate or counterexample
is ICE 101. There is no fallback to execution, property tests, another backend,
or rustc.

**Reason.** A proof claim is stronger than a sampled test. Independent checking
makes the producer's output auditable, while a concrete counterexample keeps a
false claim actionable.

**Consequence.** Unknown reasons are `unsupported_formula`,
`structural_limit`, `step_limit`, `coefficient_overflow`, and
`machine_overflow`. Unavailable is reserved for no selected backend or target,
with `backend_not_shipped` or `unsupported_target`. Unknown or unavailable is
incomplete under `allow_incomplete`; `complete_required` uses E2940. A verified
counterexample adds E2950 and fail/1 unless runtime 70 or ICE 101 takes
precedence. The owner copy remains verbatim:

- What `'solver found a counterexample to {obligation}'`;
- Why `'these values satisfy every assumption but make the claim false: {values}'`;
- Fix `'{origin-specific imperative fix}'`.

The exact ratified render remains:

```text
Error [E2950]: solver found a counterexample to function postcondition add_fee
Why: cents = 1 and result = 1 satisfy the precondition but make result > cents false
Fix: change add_fee so every return is greater than cents, or correct the #Post claim
```

The diagnostic requires its registry row, exact CLI and UI snapshots,
`jet explain`, generated reference, and I4 coverage. Raw backend text never
reaches users. (D-PROVE-SOLVER1=A)

The refinements fixture keeps formula hash
`fef21cce571b7291308098d3395817087c4e7c7ded116b7fb70eec4d439813ba`,
obligation ID
`349a5f450e99f71374c2ce380fcaad6e9e41f110bca05b41180ac75bc4c6b351`, and
solver evidence ID
`37eb859ca3cb83056113d8a2d2a27624b1d35cd096b54ec469ae132c3f6a4339`.
The native backend is `native-presburger`, version `1`, and the pinned proof
uses `certificateSha256`
`1daaaa1b6476b341d8e5a2682b9a13a632ec828f2a3c94a4596ab59ca8216454`.

```jet
# src/bad_fee.jet
#Pre(cents > 0, "positive input")
#Post(result > cents, "fee grows total")
fn add_fee(cents: Int) -> Int {
    return cents
}

jet prove src/bad_fee.jet --lens solver
```

## Lens projection and evidence facets

**Rule.** The facet enum is exactly `all`, `refinements`, `effects`, `taint`,
`contracts`, `tests`, `budgets`, `replay`, and `solver`. Front-end obligations
use `refinements`, `effects`, or `taint`; contracts use `contracts`; unit,
property, doctest, generated-case, shrink, and caught-assertion evidence uses
`tests`; budgets use `budgets`; replay evidence uses `replay`; solver evidence
uses `solver`; generic parse/type/load evidence uses `all`. Each evidence item
has exactly one facet.

**Reason.** A facet names the kind of fact a user is inspecting without
creating a second execution or report path.

**Consequence.** Facets have this fixed display order:

```text
all, refinements, effects, taint, contracts, tests, budgets, replay, solver
```

`all` evidence is included in every selection. `all` absorbs every non-solver
facet and is identical to no lens except for the human `LENSES   all` line; it
explicitly excludes solver. `--lens all --lens solver` enables solver and shows
the complete view.

**Rule.** Every lens except `solver` is presentation-only. Jet runs all
non-solver producers in normal order. It runs the solver producer only when
`solver` is explicitly named; bare prove and `--lens all` do not run it. A
presentation lens may suppress successful unrelated rows, but it must show the
overall result and every failed, unavailable, skipped, or declared/not-observed
item outside the selection under `OUTSIDE SELECTED LENSES`.

**Reason.** Projection is safe only when it cannot turn a failure into a pass,
hide incomplete evidence, alter work, or change report identity.

**Consequence.** A lens cannot change pass/fail, pass_incomplete, exit 0/1/2/70/
101, or hide E0965, E2901, E3001, E3005, E2940, or E2950. `--json` and
`.jetproof` retain the complete report; selecting a presentation lens leaves
that report and its report ID unchanged. The focused lens tests compare the
full JSON bytes, assert canonical duplicate-union output
`LENSES   tests, budgets`, require `OUTSIDE SELECTED LENSES`, and verify that
`--lens all` does not enable solver.

For E3005 projection, `jet prove` owns the structured diagnostic when the
runtime stop is not already structured. It renders the `diagnostics.md` What,
full source location, Why, and Fix verbatim; it does not paraphrase the runtime
string. The contract explanation remains: `#Pre` is an argument claim checked
at entry, `#Post` is a `result` claim checked before return, and a failed clause
is checked in every build rather than a debug/release split.

**Rule.** Repeated `--lens A --lens B` values form a set union; duplicates are
idempotent and canonical display order is independent of CLI order. Comma
lists, empty values, whitespace variants, aliases, singular/plural guesses,
and case variants are invalid. Unknown values allocate E2941, exit 2, and emit
no ProofReport or `.jetproof`.

**Reason.** Exact vocabulary makes scripts portable and prevents an accidental
new machine schema from growing out of display spelling.

**Consequence.** The ratified E2941 template remains:

- What `unknown proof lens {value}`;
- Why ``jet prove` accepts all, refinements, effects, taint, contracts, tests, budgets, replay, solver`;
- Fix names one valid command.

The executable snapshot is:

```text
Error [E2941]: Unknown proof lens `test`
 Why: `jet prove` accepts all, refinements, effects, taint, contracts, tests, budgets, replay, solver
 Fix: Try `jet prove plain.jet --lens tests`
More: jet-lang.dev/e/E2941
```

The registry, exact UI/CLI snapshot, `jet explain`, generated reference, and I4
coverage are required for E2941. (D-PROVE-LENS1=A)
