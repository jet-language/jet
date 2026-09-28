# Performance budget decisions

Vocabulary: [Jet vocabulary](vocabulary.md).

This record explains the durable performance-budget contracts: the declaration
surface, typed metrics, evidence and baseline math, command behavior, provider
boundary, integration points, prototype retirement, and cost transparency. It
is for language, tooling, and benchmark authors. The executable vocabulary is
in [`jetpack_config.rs`](../../crates/jet-foundation/src/Syntax/jetpack_config.rs);
report math is in [`PerformanceBudget.rs`](../../crates/jet-foundation/src/PerformanceBudget.rs);
the command, provider, and store boundaries are in
[`CmdBudget.rs`](../../Source/CmdBudget.rs),
[`BudgetProviders.rs`](../../Source/BudgetProviders.rs), and
[`BudgetStore.rs`](../../Source/BudgetStore.rs). The performance matrix and
measurement manifests are [`Tools/gauntlet/matrix.json`](../../Tools/gauntlet/matrix.json)
and [`Tools/gauntlet/measurement-manifest.json`](../../Tools/gauntlet/measurement-manifest.json).
The checked-in declaration examples are
[`Examples/performance/receipts/hello/src/run.jet`](../../Examples/performance/receipts/hello/src/run.jet)
and
[`Examples/performance/receipts/http_ready/src/run.jet`](../../Examples/performance/receipts/http_ready/src/run.jet).

Provider protocol coverage is exercised in
[`tests/performance_budget_providers.rs`](../../tests/performance_budget_providers.rs).

The ten budget decisions retain these ratified outcomes. Each ID is a
citation; the outcome is the selected option and its durable rule.

| Decision | Outcome | Durable choice |
|---|---:|---|
| `D-PERFBUDGET-SURFACE1` | A | Typed role modules are the one declaration surface. |
| `D-PERFBUDGET-BASELINE1` | A | Statistical evidence uses pinned named baseline artifacts. |
| `D-PERFBUDGET-GRAMMAR1` | A | Budgets use a typed list, closed vocabulary, and exact semantics. |
| `D-PERFBUDGET-REPORT1` | A | Reports and baseline updates use canonical bytes and atomic manifests. |
| `D-PERFBUDGET-OUTPUT1` | A | Commands expose source diagnostics, quiet summaries, JSON, and stable exits. |
| `D-PERFBUDGET-BENCHMIGRATE1` | B | The legacy helper is deleted without source transformation. |
| `D-PERFBUDGET-GAMEMIGRATE1` | A | Game fields map exactly to typed scene budgets and probes. |
| `D-PERFBUDGET-PROVIDER1` | A | Measurement uses an in-process typed provider registry. |
| `D-PERFBUDGET-INTEGRATION1` | A | Each owning command refreshes evidence and all readers share one report. |
| `D-PERFBUDGET-COMPILE1` | C | Compile latency uses typed project workloads and compiler probes. |

`D-COSTLAW1=A` is the related cost-transparency decision. The shared typed
budget requirement is `D-WD14=B`; the plan-first CLI mutation law is
`D-FE-CLI1`. The diagnostic copy and fixture law is identified as `I4`, and
prototype retirement is the `I8` migration gate. Rejected alternatives are
illustrative only; one evaluator, one report model, and one canonical fact
remain the consequence of these choices.

## Performance gate

Jet must satisfy the strict comparator in
[AGENTS.md](../../AGENTS.md#strict-performance-gate) for every required cell
and metric. For each cell `c`, metric `m`, and matched peer `p`, compare the
same workload and tier with `Jet/p`:

| Peer | Strict win | Permitted parity | Loss |
|---|---:|---:|---:|
| Rust | `< 1.00` | `<= 1.05` (measurement noise, not a win) | `> 1.05` |
| Any non-Rust peer | `< 1.00` | none | `>= 1.00` |

The comparator is per cell and per metric. Averages cannot hide a losing cell;
a missing, wrong, unavailable, uncovered, mismatched, or inconclusive result
cannot pass. The reason is that a single required regression is a real user
cost. The consequence is that a performance card, milestone, dashboard, or
release gate remains open until every required comparison is valid and passes;
semantics, diagnostics, determinism, safety, and I9 parity are never traded
for a score.

The required foundations are numerics (`numerics.float-kernel`,
`numerics.fft`, `numerics.tensor-map`, `numerics.int-kernel`), text
(`text.kernel`, `text.regex-kernel`, `text.regex-find-all-large`,
`text.report-cli`, `text.script`), files (`files.script`,
`files.orchestration`), concurrency (`concurrency.app`,
`concurrency.service`), networking (`netserv.client`, `netserv.service`),
build time, and run time. The critical-area cells are web
(`webfront.widget`, `webfront.app`), CLI and scripts (`cli.app`,
`text.report-cli`, `text.script`, `formats.csv-cli`, `files.script`), data
analysis (`formats.csv-cli`, `numerics.script`, `numerics.notebook`), backend
services (`concurrency.service`, `netserv.service`), and embedded
(`embedded.kernel`, `embedded.data`). Games, AI/ML applications, and GUI
applications become required only when Jet ships a first-party battery for the
area. A niche without that battery cannot support a niche-win claim.

The gauntlet manifest requires the declared Jet tiers and peer rows for each
mode, and records explicit structural reasons for metrics that do not apply.

The report contract requires Jet `aot` and `run` tiers, permits `dev`, and
uses `aot` and `run` for peer ratios; mode-specific tier sets stay in the
manifest.
The manifest and gate encode this policy; prose is not evidence. Historical
receipts remain immutable evidence under their recorded policy and never
weaken this gate. A performance-motivated spelling also requires a paired
two-program cell against the plain spelling it replaces. The pair must
preserve behavior and output and must show a strict surface/plain win before
ratification; the pair belongs in the canonical manifest, and a loss remains
carded. This prevents a fast-looking surface, an easier workload, or a
favorable average from becoming an unsupported language promise.

## One declaration surface

Declare budgets as typed facts in a role module:

```jet
module perf.<role> {
    budgets: [Budget{ ... }]
}
```

The role is a policy-group identity owned by its containing package. It does
not attach a budget to a service or target. `Budget.scope` attaches the fact
to a package, environment, service, scene, test, or target. Build, test, dev,
prove, dossier, and CI consume the same facts. No manifest field, marker,
external policy file, shorthand parser, alias, or second enforcement engine
may create a parallel policy. This gives beginners one ordinary declaration
while keeping the expert contract in the same `BudgetSpec`
(`D-PERFBUDGET-SURFACE1`, `D-WD14`).

`perf` is a reserved top-level role namespace, and `<role>` is one nonempty
lowercase `snake_case` segment. Every module role belongs to its containing
package. `Package`, `Env(name)`, `Service(name)`, `Scene(name)`, `Test(name)`,
and `Target(name)` are the closed scopes. A named attachment resolves to one
canonical identity in the loaded package graph: an unqualified name searches
only its containing package, and an imported identity uses its canonical
package-qualified name rather than an import alias. The same kind and package
rules apply to environment, service, scene, bench, and target names. Missing,
ambiguous, wrong-kind, wrong-case, or wrong-package attachments are
compile-time errors. The reason is that a role name alone must not silently
change the workload being measured.

A budget identity is containing package, role, and name. Names are unique in
the merged role. The effective collision key is scope identity,
metric/percentile, provider identity, and applicability. Entries with
intersecting applicability and the same key are rejected even when their
names differ; disjoint target/profile partitions are allowed. Module merging
is independent of source order. Every `BudgetSpec` retains source spans for
the module, complete entry, name, scope, metric, provider,
comparison/baseline, limit/value, enforcement, and every target/profile
selector so a diagnostic can point to the violated rule
(`D-PERFBUDGET-GRAMMAR1`).

## Metrics, providers, and values

This grammar choice does not reopen the shared-fact requirement, the role
surface, or pinned-baseline/update ownership from `D-WD14`,
`D-PERFBUDGET-SURFACE1`, and `D-PERFBUDGET-BASELINE1`; it selects only the
user-typeable grammar, unit suffixes, and checked BudgetSpec semantics.

The vocabulary is closed. Providers are `BuildArtifact(target)`,
`CompilerFacts`, `AllocationProbe(name)`, `BenchMeasurement(name)`,
`ServiceProbe(name)`, `SceneProbe(name)`, and
`CompilerProbe(Clean|NoChange|Edit(name))`. A provider is inferred only when
unambiguous: `BinarySize` or `ArtifactSize` with `Target` may infer
`BuildArtifact(target)`; the package scope may infer the containing package's
primary artifact only when exactly one exists; structural compiler metrics
infer `CompilerFacts`; and `BenchTime` with `Test` may infer the same-named
`BenchMeasurement`. Every other combination requires an explicit provider,
and provider identity remains part of each measurement and report fact.

| Metric family | Metrics and constraints |
|---|---|
| Deterministic facts | `BinarySize`, `ArtifactSize` use `Bytes` with `Package`/`Target` and `BuildArtifact`; `GeneratedUnsafe`, `PublicApiItems`, `DependencyCount`, and `EffectCount` use `Count` with `Package`/`Target` and `CompilerFacts`. |
| Allocation facts | `AllocationCount` and `AllocationBytes` use `Count`/`Bytes`, `Test`/`Service`/`Scene`, and an explicit `AllocationProbe`. They remain statistical unless one finite named workload pins exact input bytes, warmups, measured iterations, allocator implementation/version, process isolation, an exact event/byte rule, and identical integer results on repeated runs. Only that complete contract permits `Absolute`. |
| Statistical facts | `StartupTime` uses `Duration` with `Target`/`Service`/`Scene`; `FrameTime(percentile)` uses `Duration` with `Scene`/`Test`; `Latency(percentile)` and `Throughput` use `Duration`/`Rate` with `Service`/`Test`; `MemoryHighWater` uses `Bytes` with `Target`/`Service`/`Scene`/`Test`; `BenchTime(percentile)` uses `Duration` per iteration with `Test`; `ServiceReadiness` uses `Duration`, `Service`, and `ServiceProbe`; `CompileTime(percentile)` uses `Duration`, `Target`, and `CompilerProbe`. |
| Scene facts | `SceneAssetBytes` is lower-is-better; `DrawCalls(percentile)` is lower-is-better and uses `Count`. Both use the scene migration law below. |

`ServiceReadiness` starts when its named probe starts the service and ends at
the probe's declared ready event. Process spawn or an arbitrary log line is not
a readiness event. Percentiles are exactly `P50`, `P90`, `P95`, `P99`, and
`P999`. Invalid metric/scope/provider pairs are compile-time errors.

The value families are `Duration` (`ns`, `us`, `ms`, `s`), `Bytes` (`B`,
`KiB`, `MiB`, `GiB`), `Percent` (`pct`), nonnegative integer `Count`, and
`Rate{ count: Int, per: Duration }`. These suffixes are reserved in
`Syntax.rs`. Source quantities normalize to unsigned integer nanoseconds,
bytes, counts, or exact rate rationals. Overflow, fractional bytes or counts,
negative values, NaN, infinity, and runtime expressions are rejected. The
same typed values feed beginner and expert forms; a metric-key mini-language
or shorthand parser would create a second semantic path
(`D-PERFBUDGET-GRAMMAR1`).

`Absolute` is only for deterministic facts and compares directly.
`AbsoluteFrom(baseline)` is a statistical SLA with an absolute `AtMost` or
`AtLeast` limit whose trials, hardware, and confidence policy come from the
pinned baseline. `RelativeTo(baseline)` uses `RegressionAtMost` or
`ImprovementAtLeast` with a percentage limit. Statistical metrics use
`AbsoluteFrom` or `RelativeTo`; deterministic metrics use `Absolute`. A
baseline-relative limit with `Absolute`/`AbsoluteFrom`, or an absolute limit
with `RelativeTo`, is rejected.

Lower-is-better metrics are `BinarySize`, `ArtifactSize`, `GeneratedUnsafe`,
`PublicApiItems`, `DependencyCount`, `EffectCount`, `AllocationCount`,
`AllocationBytes`, `StartupTime`, `FrameTime`, `Latency`, `MemoryHighWater`,
`BenchTime`, `ServiceReadiness`, `SceneAssetBytes`, `DrawCalls`, and
`CompileTime`. `Throughput` alone is higher-is-better. `AtMost` is legal only
for lower-is-better metrics and `AtLeast` only for higher-is-better metrics;
regression and improvement use the same fixed direction, never a provider's
interpretation. Deterministic budgets must use `Fail`; statistical budgets may
use `Fail` or `Warn`, and `Warn` changes only policy outcome, not measurement.
Defaults are `Fail`, `Package`, and `Current`; comparison and provider defaults
exist only under the unambiguous rules above.

Deterministic gates do not borrow statistical baseline semantics. Missing,
mismatched, stale, zero, or unavailable evidence never silently passes
(`D-PERFBUDGET-BASELINE1`).

Relative arithmetic is exact. `1pct` is 100 basis points, and source permits
at most two fractional decimal places (`0.25pct` is 25 basis points). Integer
quantities and reduced `Rate` rationals are compared without rounding. For a
lower-is-better regression, `bad_delta = max(current - baseline, 0)`; for a
higher-is-better regression, `bad_delta = max(baseline - current, 0)`. The
regression passes when
`bad_delta * 10000 <= baseline * limit_basis_points`. Improvement swaps the
good direction and uses `>=`. Rate rationals are cross-multiplied. A zero
baseline is unavailable, including `0` versus `0`; it never passes. The
implementation uses a standard-library arbitrary-precision unsigned
calculation, and every absolute comparison uses normalized integers or exact
rationals (`D-PERFBUDGET-GRAMMAR1`).

## Applicability and compile workloads

`BudgetApplies{ targets, profiles }` selects `Current`, `All`, or
`Only(nonempty list)` on each axis. `Current` records the one resolved target
class/triple and profile for the invocation; `All` covers every supported
context. Target selectors are `Class(Native|Web|Freestanding|Plugin|OSImage)`
and `Triple("canonical-triple")`. Profile selectors are `.Dev`, `.Release`,
`.Small`, `.Test`, `.Bench`, or `Named(text)`. Named text is nonempty lowercase
`snake_case`, cannot equal a built-in case-insensitively, and resolves uniquely
to a declared containing-package profile. Cross-package profiles use
`<dependency-package>::<profile>` with the resolved dependency name, never an
import alias. Unknown, ambiguous, wrong-case, wrong-package, and reserved
values reject; an empty `Only` is invalid, and it does not mean `All`.
Triples are canonical.

Classes and triples independently contribute to the target union, so a triple
need not belong to a listed class. Profile selectors also form a union, and
the two axes form their Cartesian product. Two budgets overlap only when both
their target sets and profile sets intersect; analysis expands
classes/triples/profiles symbolically, with `Current` intersecting only the
invocation's resolved context. A non-applicable budget remains a checked
declared fact with `notSelected` rather than disappearing. The reason is that
selection must be auditable and collision analysis must not depend on the
order in which a command visits contexts.

Compile latency is opt-in and uses the shared BudgetSpec, provider, baseline,
report, diagnostic, and command path. Its only metric spelling is
`CompileTime(.P50|.P90|.P95|.P99|.P999)` with
`CompilerProbe(.Clean)`, `CompilerProbe(.NoChange)`, or
`CompilerProbe(.Edit("name"))`. The name resolves to one typed
`CompilerWorkload.Edit{ target: "...", patch: "..." }` in the owning role.
The provider applies that exact patch to a copied source tree; it never
infers an edit from a timestamp or ambient cache file.

Each compile candidate records source-tree and patch identities, cache
scenario, compiler/Core digests, target, profile, backend, linker, host, one
fixed warmup, twenty samples, process CPU-time variance, peak RSS, workload
bytes, edit bytes, and resident-compiler phase totals. The `dev` profile uses
the production Cranelift JIT lens; optimized `release` uses the rustc AOT
lens. Missing or changed inputs, unsupported targets, incompatible
identities, partial timing, unsupported profiles, provider crashes, and
deadlines are unavailable evidence or operation failures, never passes. The
resident fixture accepts `dev`, `release`, `debug`, `ci`, and `small`, plus
profiles declared in `package.jet`; unknown profiles reject before
measurement. A project without a compile budget does not acquire compile
measurements (`D-PERFBUDGET-COMPILE1`).

Baseline IDs are nonempty slash-separated lowercase kebab-case segments, each
matching `[a-z0-9]+(?:-[a-z0-9]+)*`. Empty or dot segments, whitespace,
uppercase, leading/trailing slashes, and package qualifiers are invalid. A
well-formed ID is declaration-valid without consulting mutable history. A
missing pinned generation is an unavailable execution result and enters the
bootstrap/update path; it is not a compile error and never passes silently.
Compile-time rejection covers duplicate identity, overlapping effective keys,
bad attachment or provider/metric/scope, unit/direction/comparison mismatch,
invalid applicability, deterministic `Warn`, malformed baseline ID, and
nonconstant or overflowing values.

## Typed declaration example

The checked-in performance receipts use `perf.receipt`, the `hello` and
`http_ready` targets, the `BuildArtifact` and `ServiceProbe` providers, and
the baseline name `card-2142/linux-x86-64-dev`. This excerpt keeps the
beginner deterministic form and the same typed syntax used by statistical
facts:

```jet
module perf.receipt {
    budgets: [Budget]{{name: "binary", scope: .Target("hello"), metric: .BinarySize, provider: .BuildArtifact("hello"), comparison: .Absolute, limit: .AtMost(1MiB), applies: BudgetApplies{
        targets: .Only([.Class(.Native)]),
        profiles: .Only([.Dev])
    }}, {name: "startup", scope: .Target("hello"), metric: .StartupTime, provider: .BuildArtifact("hello"), comparison: .AbsoluteFrom("card-2142/linux-x86-64-dev"), limit: .AtMost(50ms), applies: BudgetApplies{
        targets: .Only([.Class(.Native)]),
        profiles: .Only([.Dev])
    }}}
}
```

A beginner can provide name, metric, and limit when provider and comparison
are unambiguous. An expert can provide attachment, provider, percentile,
absolute SLA or trend, baseline, enforcement, target/triple, and profile.
Omitted fields elaborate to the same BudgetSpec; there is no second truth.

This is the selected typed Budget-list surface with defaults: every field
maps to one BudgetSpec axis, and metric, unit, direction, and provider errors
are rejected before a report exists. Projects without a `perf` module remain
unchanged.

## Evidence and canonical reports

A report must be portable, self-describing, and reproducible from retained
inputs. `D-PERFBUDGET-REPORT1` depends on the grammar's metric, direction,
comparison, enforcement, unit, applicability, provider, and baseline-ID bytes.
Presentation, diagnostics, exits, provider protocols, remote attestation,
and integration have their own laws below; they cannot reinterpret the report
vocabulary.

### Canonical wire objects

`BudgetReport` is exactly
`{schema:Text="jet.budget-report",version:Int=1,report_id:Hex64,content:ReportContent}`.
`AcceptanceAudit` is exactly
`{audit_id:Hex64,accepted_at:RFC3339UTC,kind:"pass"|"bootstrap"|"exception",actor_label:Text="local",reason:?Text,flags:{bootstrap:Bool,accept_regression:Bool},prior_state_id:?Hex64,prior_head_report_id:?Hex64,report_id:Hex64}`.
Plain `jet budget update` writes `kind=pass`, a null reason, and both flags
false. `--bootstrap --reason R` writes `kind=bootstrap`, normalized `R`,
`bootstrap=true`, and `accept_regression=false`.
`--accept-regression --reason R` writes `kind=exception`, normalized `R`,
`bootstrap=false`, and `accept_regression=true`. Every other combination
rejects. `actor_label` is the literal `local`; it never comes from a user,
environment, or CLI value. `audit_id` hashes the canonical audit without
`audit_id` plus LF, and `report_id` hashes ReportContent plus LF.

The selected manifest is exactly
`{schema:"jet.budget-manifest",version:1,manifest_id:Hex64,content:ManifestContent}`;
`manifest_id` hashes content plus LF. Hashes prove integrity and linkage only;
signing, identity, and attestation require separate owner gates.

A-canonical JSON is UTF-8 without BOM with exactly one LF terminator, no
insignificant whitespace, and object keys sorted by unsigned UTF-8 bytes.
Arrays use their semantic order. `"`, `\\`, backspace, form-feed, LF, CR,
and tab use their two-byte JSON escapes; other controls use lowercase
`\\u00xx`, and every other Unicode scalar is emitted directly. Unpaired
surrogates are rejected. Integers are arbitrary-precision signed decimal in
shortest form, with no leading zero or negative zero; booleans and null are
lowercase; floats are forbidden; duplicate, unknown, and missing keys reject.
Hash input is canonical content including its LF, not a wrapper. `Hex64` is
64 lowercase hexadecimal characters. `RFC3339UTC` is
`YYYY-MM-DDTHH:MM:SS.NNNNNNNNNZ`. Readers re-encode and byte-compare before
trusting an artifact.

`ReportContent` is exactly `{subject, toolchain, evidence_id, measurements,
summary, privacy}`. `subject` contains `target_id`, and sorted
`member_sources:[{path:WorkspacePath,sha256:Hex64}]` (path bytes, then hash),
`target_triple`,
`target_class`, `profile`, `artifact:{sha256:Hex64,bytes:Int}|null`, and
`measured_start`/`measured_end`. `toolchain` contains `jet_version`,
`compiler_build_id`, `stdlib_id`, `runner_id`, and a digest over its first
four fields plus LF; compatible reports require every field and digest to
match, not just a major version. `summary` is
`{outcome:"pass"|"warn"|"fail",pass:Int,warn:Int,fail:Int}`.
`privacy` is `{schema:Int=1,workspace_paths_only:Bool=true,retained:[Text],excluded:[Text]}`
with both arrays sorted by UTF-8 bytes. Providers are measurement-local, not
a report-global field, because one invocation can combine compiler facts,
artifacts, probes, and benchmark providers. No other fields exist.

`WorkspacePath` is nonempty, slash-separated, relative UTF-8 text normalized
to NFC. Empty, dot, dot-dot, absolute, backslash, NUL/control, drive, URI,
percent-decoding, and symlink-resolved host paths are illegal. Reports exclude
hostnames, usernames, absolute paths, environment values, repository remotes,
IP/MAC/serial identifiers, raw source, command lines, actor labels, and
acceptance reasons. Plain-pass audit reason is null; bootstrap and exception
reasons are explicit NFC UTF-8, trimmed, free of controls/newlines, and 1–512
scalars. The UI warns that the text is checked in. `actor_label` enters no
report, context, fingerprint, or evidence ID.

### Measurement and evidence model

A `Rational` is exactly `{num:Int,den:Int}`, gcd-reduced with `den>0` and
zero `{0,1}`. A `Quantity` is a nonnegative Rational. Durations, bytes, and
counts have denominator one; Rate is a reduced count-per-base-duration.
`Metric` is `{name:Text,percentile:"p50"|"p90"|"p95"|"p99"|"p999"|null}`.
`Provider` is `{kind,identity,version,isolation,cpu_arch,cpu_model,logical_cpus,memory_bytes,os,kernel,power_governor,hardware_fingerprint}`;
identity is the resolved provider-protocol payload, or empty only for
`CompilerFacts`, and the fingerprint hashes preceding fields plus LF.

`MeasurementPolicy` is
`{min_candidate_samples,min_baseline_samples,baseline_generations,bootstrap_resamples,lower_rank,upper_rank,stale_after_seconds,trend_generations}`.
The v1 seed is `{20,20,5,10000,250,9750,2592000,5}` in that field order.
Statistical bootstrap stores it; later compatible reports copy the policy
byte-for-byte. `AbsoluteFrom` and `RelativeTo` consume stored policy;
deterministic `Absolute` has null policy, statistics, and baseline.

`HistorySelection` is `{state_id:Hex64,report_ids:[Hex64]}` in newest-first
order and carries no statistical policy. `StatisticalBaseline` is
`{history:HistorySelection,pooled_samples:[Quantity],statistics:Statistics,policy:MeasurementPolicy}`.
Measurements sort by budget ID then source and contain
`{budget_id,budget_spec:BudgetSpecCanonical,budget_spec_sha256,source,metric,target_class,unit,direction,provider,comparison,enforcement,context_key,policy,samples,statistics,history,baseline,decision}`.
Deterministic Absolute has null policy/statistics/baseline and optional history
for trend. Statistical measurements have policy/statistics; baseline is null
only for bootstrap or unavailable evidence, and otherwise its history equals
`baseline.history`.

`Statistics` is
`{count,sorted_samples,p50,p90,p95,p99,p999,mean,mad}`. `Trend` is
`{label:"improving"|"stable"|"regressing"|"insufficient",report_ids:[Hex64],estimators:[Quantity],score:?Rational}`
with oldest-first, index-aligned inputs. `Decision` is
`{evidence,reason,point,lower95,upper95,trend,policy_outcome}`. Comparison is
exactly one of `absolute {kind,limit:Quantity,direction}`, `absolute_from
{kind,baseline,limit:Quantity,direction}`, or `relative_to
{kind,baseline,limit_basis_points,goal,direction}`. Unknown, missing, or extra
fields reject.

`BudgetSpecCanonical` is exactly
`{package_id,perf_role,name,scope,metric:Metric,provider:{kind,identity},comparison,enforcement,applies:{targets,profiles}}`.
Defaults are explicit; baseline bytes are unchanged; resolved target/profile
identities are deduplicated and sorted. Spans, comments, source order, import
aliases, and sugar are absent. `budget_spec_sha256` hashes these embedded
canonical bytes plus LF; no other hash input is valid.

`evidence_id` breaks seed circularity. Hash canonical
`{subject,toolchain,measurements}` after replacing history, baseline, and
decision with null while retaining the embedded BudgetSpec/hash, metric and
percentile, provider, comparison, context, policy, samples, and statistics,
plus LF. It precedes selection, resampling, trend, outcomes, summary,
`report_id`, and acceptance. `context_key` hashes domain
`jet-budget-context-v1\0`, u64-BE-length-framed `target_id`, metric name,
percentile-or-empty, target class/triple/profile, every toolchain field, and
every measurement provider field/fingerprint. There is no fallback context.

Deterministic evidence has one Quantity sample, null statistics/policy/baseline,
direct Absolute, and optional history for trend. Statistical counts, windows,
resamples, ranks, staleness, and trend length come from stored policy. Samples
retain acquisition order; sorted samples use exact cross-multiplication.
Nearest-rank `p(q)=sorted[ceil(q*n)-1]`, including p999; `mean=sum/n`; MAD
uses exact deviations from p50. Estimator `E` is the selected percentile or p50
when no percentile is selected. Arbitrary-precision rational operations reduce
every step; no float or rounding decides a result.

For each bootstrap replicate, resample candidate `n` and baseline `m`
independently with replacement. Replicates are `0..N-1`, each starts at block
zero. Block `b` is
`SHA-256(domain || evidence_id || context_key || ordered baseline report ids || u64-be replicate || u64-be b)`.
Consume four consecutive u64 words in digest byte order, candidate draws first
and baseline draws second. For population `k`, reject words at or above
`floor(2^64/k)*k`, consuming rejected words; after four words increment `b`,
and use `word mod k` for an accepted index. `AbsoluteFrom` consumes candidate
samples only; `RelativeTo` consumes candidate then baseline. Each replicate
computes the selected estimator, and one-based stored ranks select bounds;
`point` uses unresampled estimators.

`Absolute` passes `AtMost` exactly when sample ≤ limit and `AtLeast` exactly
when sample ≥ limit. `AbsoluteFrom` bootstraps the candidate while the exact
baseline supplies compatible provider, toolchain, and trial policy. For
`AtMost`, the statistic is `E_candidate - limit`: upper95 ≤ 0 passes, lower95
> 0 regresses, and otherwise the result is inconclusive. `AtLeast` uses
`limit - E_candidate` with the same bounds. `RelativeTo` computes baseline
estimator `B` and candidate estimator `C`; `B=0` is unavailable. For
`RegressionAtMost`, bad delta is `max(C-B,0)` for lower-is-better and
`max(B-C,0)` for higher-is-better, with statistic
`(bad_delta*10000/B)-limit_basis_points`. For `ImprovementAtLeast`, good delta
is `max(B-C,0)` for lower-is-better and `max(C-B,0)` for higher-is-better,
with statistic `limit_basis_points-(good_delta*10000/B)`. Every replicate
recomputes these values as exact reduced rationals; the same bounds classify
pass, regression, or inconclusive. This is the budget-baseline comparison,
not a substitute for the per-cell peer gate above.

Trend is informational and never changes policy. Statistical trend uses
`policy.trend_generations`; deterministic trend uses exactly five. Select
accepted exact-context generations oldest-to-newest. Fewer than three produces
`insufficient`, retaining available IDs and estimators with null score. For
each `i<j`, the direction-normalized slope is `(E_i-E_j)/(j-i)` for
lower-is-better or `(E_j-E_i)/(j-i)` for higher-is-better. Its exact
nearest-rank p50 score is positive for improving, zero for stable, and
negative for regressing. Persist IDs, estimators, and score.

Evidence and enforcement are separate. Evidence pass maps to policy pass.
With `Fail`, regression, inconclusive, or unavailable maps to fail; with
`Warn`, those outcomes map to warn. Deterministic metrics cannot use `Warn`,
and there is no observe level. Report summaries count policy outcomes with
fail > warn > pass precedence.

### Baseline selection and storage

For CLI baseline name `N`, each statistical measurement must have comparison
baseline bytes equal to `N`. A deterministic `Absolute` has no embedded name:
with an existing `N`, it matches only a compatible prior budget ID, BudgetSpec
hash, and context; the first `--bootstrap` explicitly associates it. A
candidate is nonempty, every measurement matches `N`, and budget IDs are
unique. Zero matches, an unmatched or duplicate measurement, or a deterministic
non-bootstrap without a prior match rejects. Multiple matches are accepted
atomically, never by first match.

Compatible history requires exact schema, context, BudgetSpec, metric
percentile, unit, direction, comparison, target, toolchain, provider, and
privacy. Statistical selection uses `policy.baseline_generations`; deterministic
trend uses five. Container order controls newest-first history. Staleness uses
`audit.accepted_at` and policy seconds, or 2,592,000 seconds for deterministic
facts; a future timestamp is unavailable. Plain update requires every item to
pass. Bootstrap requires deterministic direct pass, candidate-only confidence
pass for AbsoluteFrom under the stored v1 policy, and permits a RelativeTo
unavailable seed under that policy. An exception requires at least one
regression/inconclusive and every item to be pass/regression/inconclusive;
unavailable, corrupt, or mismatched evidence rejects the whole update. The
report commits atomically.

A baseline name is the grammar's nonempty slash-separated ASCII lowercase
kebab-case name. It is also used byte-for-byte as path segments: Unicode
normalization, case folding, percent decoding, encoded separators, backslashes,
drives, NULs, and controls are rejected. Measurement never updates a
baseline.

`ManifestContent` is exactly
`{name:BaselineName,head_report_id:Hex64,generations:[Generation]}`.
Generations are oldest-to-newest, and `Generation` is
`{report_id:Hex64,audit:AcceptanceAudit}`. The head equals the last generation;
the audit report ID equals that generation; and its prior state/head point to
the predecessor manifest and head, or both are null for bootstrap. Deletion and
reordering are invalid. Update verifies manifest ID `M0`, creates the report
object, takes the per-name exclusive lock, re-reads and checks `M0` (or absence
for bootstrap), writes a canonical same-directory temporary manifest, fsyncs,
atomically replaces the named manifest, and fsyncs its directory. An existing
object is reusable only when canonical, hash-valid, and byte-identical; any
mismatch aborts without overwrite. A changed head aborts and leaves the
verified object unreferenced. Incomplete temporary manifests are reported and
ignored; the last hash-valid named manifest remains authoritative.

`jet budget gc` is the only collector. Under one global GC/update exclusion
lock it verifies every manifest, marks every referenced report, and removes
only valid unreferenced report objects whose filesystem modification time is
older than 24 hours. Corrupt or unknown files are reported and retained;
startup never deletes evidence.

Readers recompute and require equality for toolchain digests, provider
fingerprints, embedded BudgetSpec hashes, context keys, candidate/pooled
statistics, `evidence_id`, every SHA stream/point/bounds/evidence/outcome,
trend from persisted IDs/estimators, summaries/counts, report and audit IDs,
and container hashes/chains/CAS links. With referenced objects they also
reconstruct pooled samples and trend estimators. Standalone reports verify
math over persisted inputs but claim no unhashed provenance. Missing or
mismatched data is corruption or unavailable evidence, never normalization.

The storage root is `.jet/perf/baselines`, and names cannot escape it. Open
existing components without following symlinks and reject symlink/reparse
traversal. On POSIX, artifact directories/files use 0755/0644 subject to
umask, while locks and temporary state use 0700/0600. Windows artifacts use
inherited repository ACLs and temporary/lock state uses current-user ACLs.
Existing permissions are neither authenticity nor confidentiality evidence and
do not invalidate hash-valid artifacts. Locks are advisory-exclusive across
Jet processes. Update and GC require create-new, no-follow/reparse checks,
same-filesystem atomic replacement, file and directory durability, and reliable
locks. If a platform cannot provide those guarantees, mutation returns
unavailable without changing policy; read-only checks may use only canonical,
hash-valid artifacts.

The selected storage surface is immutable objects plus a named atomic manifest:
canonical reports are create-new objects at
`.jet/perf/baselines/objects/<report_id>.json`, and each baseline name is a
manifest at `.jet/perf/baselines/names/<segments>.json`. Rollback is a new
audited generation selecting an existing object, never a history rewrite. A
single checked-in name keeps the beginner path simple while immutable
measurements, exact samples, contexts, and integrity links preserve expert
reproduction.

```jet
jet budget update --baseline card-2142/linux-x86-64-dev
# writes objects/<report_id>.json create-new
# advances names/card-2142/linux-x86-64-dev.json by CAS
jet budget gc
# removes only verified, unreferenced objects older than 24h under the global lock
```

## Commands, diagnostics, and exits

The grammar owns declarations and BudgetSpec semantics. The report decision
owns report bytes, evidence math, baselines, audits, and storage. The output
decision owns command validation, confirmation, projection, ordering,
diagnostics, fixtures, annotations, and exits. `--bootstrap` is allowed only
when the selected baseline has no usable compatible generation because history
is absent or its newest otherwise-compatible generation is stale. Absent
history creates the first generation with null prior IDs, empty history, and
the new report as head. Stale history does not reset or delete it; the new
report is an audited generation of the same name (`D-PERFBUDGET-OUTPUT1`).

The command surface is:

```text
jet budget check [--json] [--verbose] [--quiet] [--annotations auto|none|github]
jet budget update --baseline <BaselineName> [--bootstrap|--accept-regression]
    [--reason <text>] [-y|--yes] [--json] [--verbose] [--quiet]
    [--annotations auto|none|github]
```

`check` evaluates every applicable BudgetSpec for the resolved target/profile,
reads each statistical baseline from its declaration, writes the canonical
report to `.jet/perf/reports/<report_id>.json`, and never mutates baseline
state. `update` measures/checks once, applies the all-measurements-match law
for its baseline, prints a plan, and advances the selected manifest only after
confirmation. `--bootstrap` and `--accept-regression` are mutually exclusive;
`--reason` is required with either and is rejected otherwise. `--quiet`
suppresses only the trailing pass/fail recap; it does not change evaluation,
diagnostics, report creation, or baseline mutation.

Check and update install canonical report bytes at
`.jet/perf/reports/<report_id>.json`, with the verified lowercase Hex64 as the
filename. Open `.jet`, `perf`, and `reports` component by component without
following symlinks or reparse points; reject escapes, wrong types, and wrong
owners. Create directories with POSIX 0755 subject to umask (inherited
repository ACLs on Windows), create a same-directory random temporary file as
0600/current-user ACL, write all bytes, fsync or `FlushFileBuffers`, install
with an atomic no-replace primitive, and fsync the directory. If a platform
cannot provide no-follow, no-replace, file durability, or directory
durability, return E2908 before claiming a report. Destination EEXIST is
idempotent only after the existing object passes the exact canonical/hash
checks.

Every update prints a plan before mutation. Interactive human mode asks
`Apply? [y/N]`; only `y` or `yes` applies. N/EOF prints
`plan cancelled; no baseline changed` and exits OK. Non-TTY without `-y` or
`--yes` prints the deterministic plan plus
`plan only; pass -y or --yes to apply in a non-interactive shell`, performs no
write, and exits OK. JSON never prompts: without `-y`/`--yes` it returns the
plan with `applied:false` and exits OK; with either flag it applies. Rejected
evidence prints no apply prompt because there is no valid plan.

Check may create or reuse a content-addressed report without confirmation;
that additive artifact is not described as a zero-write query. Short work
prints no transient progress and names the persisted report ID in its final
summary. Long provider/build/measurement work uses the shared dependency-chain
live region and promotes completed rows into the ledger. `--verbose` adds
`+ report <id> <path>` for creation or `~ report <id> <path> (verified reuse)`.
Update uses the `+`/`~`/`-` plan renderer: `+ report <id>` for a new object or
`~ report <id> (verified reuse)`, followed by
`~ baseline <name> <old-head-or-none> -> <report-id>`. Applied rows correspond
one-for-one with plan rows. Verbose output adds applicable pass rows, report
path/ID, context/toolchain/provider fingerprints, selected baseline report IDs,
point/bounds/trend, and create/reuse information without changing evaluation.
TTY redraw ends before diagnostics; non-TTY and `NO_COLOR` output is
append-only and deterministic.

### Results and human output

Results sort by class and UTF-8 budget ID: policy failure from
regression/inconclusive, policy failure from non-stale unavailable evidence,
policy failure from stale evidence, policy warning, then pass. Overall
`status` precedence is fail > unavailable > stale > warn > pass. Under `Warn`,
unavailable and stale remain warning policy outcomes. `failure_kind` is
`budget` for the first class, `evidence` for the next two, and null for warn
or pass. A compiler-front-end stop is `fail/compiler`; provider protocol or
execution, report/container, write, CAS, or permission failure is `fail/tool`.
Provider unavailability before measurement is ordinary evidence. Tool/compiler
failure never prints a budget-result summary as if measurement completed.

The terminal copy is exact. Define
`count(n,"budget","budgets")` as `1 budget` for one and `<n> budgets`
otherwise; apply the same rule to warning/warnings, result/results, and
baseline/baselines. Omit zero-valued segments:

```text
budgets: <count(P,budget,budgets)> passed · report <12-char-id>
budgets: [<count(P,budget,budgets)> passed · ]<count(W,warning,warnings)> · report <id>
budgets failed: <count(F,budget,budgets)> failed[ · <count(W,warning,warnings)>] · report <id>
budgets unavailable: <count(U,result,results)> unavailable[ · <count(W,warning,warnings)>] · report <id>
budgets stale: <count(S,baseline,baselines)> stale[ · <count(W,warning,warnings)>] · report <id>
```

The bracketed segment appears only when positive. E2907's headline is
`performance budget <name> regressed` or
`performance budget <name> is inconclusive`; E2906 is
`performance budget <name> has no usable evidence`; E2908 is the operation-
neutral `performance budget operation failed`. A tool failure ends
`budget command failed before a valid report was produced` when the report is
null, or `budget command failed · report <id> was not accepted` otherwise.
Compiler failure prints only canonical compiler diagnostics. A source
diagnostic uses the canonical Jet frame and exact What/Why/Fix. Human output
is stderr only, stdout is empty, and color carries no meaning.

### JSON, diagnostics, and CI annotations

JSON mode begins only after the whole command line validates. Every usage
failure, including an unknown command/flag, missing or invalid value, repeated
flag, or illegal combination with `--json` before or after it, uses the
canonical human usage diagnostic on stderr, leaves stdout empty, and exits 2.
Usage never emits the budget JSON schema. Once JSON mode is established,
stdout is exactly one A-canonical object plus LF, stderr is empty, and no ANSI,
progress, human diagnostic, or annotation is emitted.

The command object is exactly

```text
{schema:"jet.budget-command",version:1,command:"check"|"update",
 status:"pass"|"warn"|"stale"|"unavailable"|"fail",
 failure_kind:null|"budget"|"evidence"|"compiler"|"tool"|"ice",
 exit_code:Int,applied:Bool,report:BudgetReport|null,
 report_path:WorkspacePath|null,plan:Plan|null,results:[Result],
 diagnostics:[Diagnostic]}
```

`BudgetReport`, `BudgetSpecCanonical`, `Metric`, `Quantity`, `Rational`,
`Trend`, `BaselineName`, `Hex64`, and `WorkspacePath` are the REPORT1 types,
not extension points. `Plan` is exactly
`{baseline:BaselineName,rows:[PlanRow],requires_confirmation:Bool}`. Check has
null plan; update has one after valid evidence and null before it.
`PlanRow` is exactly
`{operation:"create"|"reuse"|"advance",artifact:"report"|"baseline",path:WorkspacePath,id:Hex64,from_id:Hex64|null,to_id:Hex64}`.
Rows are report then baseline; report rows use `id=to_id=report_id` and null
`from_id`, and baseline advance uses the prior head or null as `from_id`.
`requires_confirmation` is true only for remaining interactive human
confirmation, and false for `-y`/`--yes`, JSON, and non-TTY plan-only mode.

A `Result` is exactly
`{budget_id:Text,status:"pass"|"warn"|"stale"|"unavailable"|"fail",evidence:"pass"|"regression"|"inconclusive"|"unavailable",stale:Bool,enforcement:"warn"|"fail",source:{path:WorkspacePath,line:Int,column:Int},metric:Metric,unit:Text,direction:"lower_is_better"|"higher_is_better",comparison:{kind:"absolute",limit:Quantity,direction:"AtMost"|"AtLeast"}|{kind:"absolute_from",baseline:BaselineName,limit:Quantity,direction:"AtMost"|"AtLeast"}|{kind:"relative_to",baseline:BaselineName,limit_basis_points:Int,goal:"RegressionAtMost"|"ImprovementAtLeast",direction:"AtMost"|"AtLeast"},point:Rational|null,lower95:Rational|null,upper95:Rational|null,trend:{label:"improving"|"stable"|"regressing"|"insufficient",report_ids:[Hex64],estimators:[Quantity],score:Rational|null}|null,baseline_report_ids:[Hex64],reason:Text,diagnostic_code:null|"E2906"|"E2907"}`.
`direction` is the REPORT1 measurement direction and describes estimator
orientation only; nested comparison direction remains `AtMost`/`AtLeast`.
`stale` is true exactly when REPORT1 finds otherwise-compatible evidence older
than its staleness window, and that result retains evidence-unavailable.
`Fail` enforcement maps it to stale; `Warn` maps it to warn while retaining
`stale=true`. Non-stale results set false. `baseline_report_ids` preserve
REPORT1 newest-first history order. Point, bounds, trend, and reason are
persisted decision values, never renderer calculations.

The diagnostic code is null, E2906, or E2907 as shown above. Result arrays use
the common ordering. `Diagnostic` is exactly
`{severity:"warning"|"error",phase:"compiler"|"tool",code:Text,message:Text,why:Text,fix:Text,source:{path:WorkspacePath,line:Int,column:Int,end_line:Int,end_column:Int}|null}`.

The compiler translator maps every canonical front-end diagnostic field for
field, not only E2903–E2905; absent range ends equal starts, related notes
append to `why` with LF in compiler order, and the outcome is
`fail/compiler`, exit 1, null report/plan, and empty results. Tool diagnostics
map directly. Unknown, missing, and extra fields reject. No diagnostic text is
parsed back from rendered stderr.

### Exit law

The exit mapping reuses [`crates/jet-foundation/src/ExitCodes.rs`](../../crates/jet-foundation/src/ExitCodes.rs): OK=0 covers pass,
warning-only output, cancelled plans, non-TTY/JSON plan-only output, and
successful or idempotent mutation. USER_ERROR=1 covers budget, evidence,
compiler, and tool failure. USAGE=2 covers command, flag, argument, and
combination errors. RUNTIME_PANIC=70 never comes from budget tooling. ICE=101
is reserved for an impossible invariant or rustc exposure; malformed source,
bad data, unavailable providers, and statistical outcomes are not ICEs.
JSON `failure_kind` distinguishes the shared exit 1; human copy distinguishes
the diagnostic or summary. Signals retain OS behavior.

Compiler diagnostics are E2903 for invalid declarations/values/units/
directions/comparisons/applicability, E2904 for duplicate or overlapping
effective keys, and E2905 for unresolved scope/profile/target/provider before
measurement. E2906 covers missing, mismatched, zero, stale, or unavailable
evidence; E2907 covers regression or inconclusive evidence after a valid
BudgetSpec; E2907 severity follows Warn/Fail. E2908 covers corrupt or
non-canonical artifacts, provider protocol/execution failures,
CAS/permission/filesystem/report-write refusal, and disallowed updates. Its
What is always `performance budget operation failed`, never a
baseline-specific message. Tool diagnostics never come from rustc.

The `I4` copy is:

| Code | What | Why | Fix |
|---|---|---|---|
| E2903 | `performance budget <name> is not valid` | One violated grammar rule or typed value | One legal form |
| E2904 | `performance budgets <a> and <b> overlap` | Effective key and applicability intersection | Remove the overlap or make applicability disjoint |
| E2905 | `performance budget <name> cannot resolve <attachment>` | Zero or multiple canonical matches | Qualify the identity or provider |
| E2906 | `performance budget <name> has no usable evidence` | Exact missing/mismatch/zero/provider-unavailable/stale reason | Correct the provider or bootstrap only when absent/stale evidence is eligible |
| E2907 | Regression or inconclusive headline | Estimator, bound, limit, confidence, direction, and baseline IDs | Improve, inspect, or record an explicit exception |
| E2908 | `performance budget operation failed` | Named operation and canonical/provider/CAS/permission/durability refusal | Correct the named cause and retry; never force |

The six codes require summary and detailed What/Why/Fix rows in `diagnostics.md`
and reviewed snapshots
`tests/ui/perf_budget_e2903_invalid.stderr`,
`perf_budget_e2904_overlap.stderr`, `perf_budget_e2905_unresolved.stderr`,
`perf_budget_e2906_unavailable.stderr`, `perf_budget_e2907_regression.stderr`,
and `perf_budget_e2908_operation.stderr`, paired with `.jet` fixtures or
tool-fixture drivers. E2906 and E2907 also require warning snapshots, E2906
stale has a separate golden transcript, and human/JSON/GitHub forms receive
golden tests. No required snapshot means that code does not ship.

`--annotations` defaults to `auto`; JSON forces `none`. Human `auto` selects
GitHub only when `GITHUB_ACTIONS` bytes equal lowercase `true`; explicit
`github` or `none` overrides. After each human source diagnostic, emit this
single stderr line:

```text
::<level> file=<file>,line=<line>,col=<col>,title=<title>::<message>
```

Property order is file,line,col,title. `level` is error for policy fail/tool
error and warning for Warn; title is raw `Jet <code>`. The raw message is
`<message>\nWhy: <why>\nFix: <fix>`. Encode UTF-8, then escape property values
in order `%` → `%25`, CR → `%0D`, LF → `%0A`, `:` → `%3A`, `,` → `%2C`.
Escape message `%`, CR, and LF in that order; do not escape its colon/comma.
Other UTF-8 bytes remain unchanged. Do not annotate rows without source, pass
rows, or compiler diagnostics already owned by the compiler CI bridge.

The beginner path is quiet green output or one source-linked What/Why/Fix and a
next command. The expert path exposes verbose reports, exact JSON,
annotations, plans, reasons, fingerprints, confidence, trend, and stable
exits. Every surface renders the same ordered report; JSON, annotations, and
human text never independently evaluate policy. This is the selected
source-native-diagnostics-plus-quiet-summary surface (`D-FE-CLI1`,
`D-PERFBUDGET-OUTPUT1`).

For example, the checked-in `http_ready` receipt can identify the named
`readiness` budget and its `card-2142/linux-x86-64-dev` baseline without
changing the diagnostic contract:

```text
jet budget check
# Error [E2907]: performance budget readiness regressed
#   --> <workspace>/run.jet:<line>:<column>
#  Why: the selected estimator exceeds its AtMost limit using the pinned baseline.
#  Fix: improve readiness, inspect `jet budget check --verbose`, or accept explicitly with
#       `jet budget update --baseline card-2142/linux-x86-64-dev --accept-regression --reason "approved tradeoff"
# budgets failed: 1 budget failed · report <12-char-id>
```

## Prototype retirement

The `I8` gate keeps one typed evaluator. `D-PERFBUDGET-BENCHMIGRATE1=B`
removes every first-party `bench_budget` use with a retirement ledger that
maps its name, body, and `max_ns` intent to BudgetSpec. Fixed warmups/trials,
floating mean and standard deviation, stderr/environment rendering, and Bool
return behavior are retired rather than copied. The helper, parser, facts,
output, and evaluator are deleted; external calls receive the ordinary
unresolved-member diagnostic. No retired benchmark claim marker, migration
command, compatibility alias, teaching parser, adapter, dormant parser, or
second evaluator survives.

The cutover deletes the helper without source transformation.

`D-PERFBUDGET-GAMEMIGRATE1=A` maps game fields exactly. The game migration
uses the closed metrics `SceneAssetBytes` and `DrawCalls(percentile)`:
`frame_ms` maps to `FrameTime(.P99)` in nanoseconds, `memory_mb` maps to
`MemoryHighWater` in MiB, `asset_kb` maps to `SceneAssetBytes` in KiB, and
`draw_calls` maps to `DrawCalls(.P99)` as Count. Each uses `AbsoluteFrom` and
`AtMost`. One
`SceneProbe` pins backend build, target, device, replay/input, scene-ready
event, 120 warmup frames, 600 measured frames, viewport, and settings; it
defines the sample stream and max/percentile estimators. The former
`Game.Budgets` declaration, display, and evaluator path are deleted; no alias
or second engine survives. Old uses receive ordinary unresolved diagnostics.

| Former fact | Canonical replacement | Retired behavior |
|---|---|---|
| `bench_budget("parse", 5_000_000, body)` name and body | `#Test("parse") { .measure { body } }` plus a `.Test("parse")` / `.BenchMeasurement("parse")` Budget | Helper Bool return, fixed 3/10 sampling, floating mean/deviation, environment-controlled stderr |
| `bench_budget` `max_ns` | `.BenchTime(percentile)` with `.AtMost(5ms)` and explicit baseline policy | Implicit unpinned wall-clock hard gate |
| `Game.Budgets.frame_ms` | `.Scene(scene)` / `.SceneProbe(scene)`, `.FrameTime(.P99)`, nanosecond-normalized `.AtMost` | Transcript display and runtime setter |
| `Game.Budgets.memory_mb` | `.MemoryHighWater`, `.AtMost(value MiB)` | Transcript display and runtime setter |
| `Game.Budgets.asset_kb` | `.SceneAssetBytes`, `.AtMost(value KiB)` | Transcript display and runtime setter |
| `Game.Budgets.draw_calls` | `.DrawCalls(.P99)`, `.AtMost(value)` | Transcript display and runtime setter |

Every migrated statistical fact uses `.AbsoluteFrom(baseline)`. Scene probes
retain backend build, target, device, replay/input, ready event, warmups,
measured frames, viewport, and settings in provider/context identity.
Unmappable semantics remain owner-gated rather than gaining a private alias or
parallel enforcement path.

## Provider boundary

`D-PERFBUDGET-PROVIDER1=A` resolves providers deterministically from the
compiler-owned registry, never from `PATH`. A `ProviderRequest` fixes
schema/version, request ID, provider/context/budget hashes, ordered metrics,
workload, and policy; canonical sorting and hashing use REPORT1 bytes.
Providers collect evidence only, while the shared evaluator owns policy and
outcomes. The stream is contiguous and ordered as typed `Sample` or
`Unavailable` events followed by one final `Complete`; bounded metadata carries
compile-workload provenance through the same protocol. Limits are 1,000,000
samples, 16 MiB total bytes, 4,096 specs, and 512 detail scalars. Valid
unavailability or too few samples is E2906; malformed streams, panic, or
timeout is E2908; unsupported pairs are E2903; unresolved providers are
E2905. Provider identity and context remain part of every fact.

`BuildArtifact(target)` measures `BinarySize` and `ArtifactSize` as the byte
count of one selected artifact. For target-scoped `StartupTime` and
`MemoryHighWater`, it runs twenty fresh child processes. Startup is elapsed
nanoseconds from spawn to the target program's first stdout readiness line.
Memory is Linux `ru_maxrss` in bytes with live `/proc/<pid>/status` `VmHWM` as
the same-process observation. A request containing both statistical metrics
shares one twenty-trial process family. Provider version and isolation fields
pin this rule in baseline context; the provider does not imply throughput
measurement.

## Integration ownership

`D-PERFBUDGET-INTEGRATION1=A` assigns refresh to the command that owns the
intent. Every build runs deterministic `Fail` gates. `jet test --measure` owns
`BenchMeasurement`; dev owns explicitly requested startup, service, and scene
probes. These commands refresh evidence when a relevant digest changes.
Read-only dossier, Canvas, and LSP views never measure. CI runs
`jet budget check`. Prove never measures and adds no parallel flags or report
types; it translates compatible budget results into the existing proof
Evidence model.

An exact compatible slice may satisfy a matching fact but never stand in for
the whole policy. Missing, stale, mismatched, unavailable, inconclusive,
warning, and failure evidence remain visible under the report/output laws;
failures fail. Every surface reads the same verified BudgetReport and shared
evaluator. A budget baseline does not weaken the strict peer gate: the
per-cell/per-metric comparator still evaluates every required peer and metric.

## Cost transparency

`D-COSTLAW1=A` keeps optimizer excellence and cost transparency together. A
cost the optimizer proves it removed appears in `jet explain --cost` but does
not emit a lint. A semantic cost that remains visible in lowered code remains
reportable. The reason is to expose meaningful costs without warning about
work that the optimizer eliminates.

The five reportable cost rows are:

1. **View materialization:** a read-only view crosses an owning boundary and
   must copy.
2. **Map copy-on-write:** a shared map spine is copied before mutation.
3. **Exact-Int spill:** an operation leaves packed `Int` range and uses the
   exact big-number representation.
4. **Outcome construction:** a `Result` or `Option` carrier is built before an
   immediate consumer fast path can remove it.
5. **Generic representation fallback:** a collection uses its generic
   representation because no shape proof selected a direct representation.

The sema checker supplies view-copy `L2510` rows. Typed TIR supplies the other
four rows and the complete explain projection. Before projection, the shared
cost seam requires every sema-checked reachable callable to have a matching
TIR body. Type-parameterized, foreign, and otherwise uncovered reachable
surfaces are explicit completeness failures; they are never silently omitted.
`jet check` and `jet lint --cost` merge these sources and deduplicate identical
source-site diagnostics. They retain semantic remainders inside loops.
`jet explain --cost` also retains optimizer-proven removals. Backends do not
reconstruct cost from emitted Rust; every explain row uses the honest
`tier=shared-tir` label.

```jet
loop item in items {
    out.push(item.view())        // view materialization: L2510 when semantic
    counts[item.key] += 1        // map copy-on-write: L2510 when semantic
    total += item                // exact-Int spill: L2510 when semantic
    result :: read(item)         // outcome construction: L2510 when semantic
    value := items[index]        // generic fallback: L2510 when semantic
}
```

`jet check` keeps ordinary code quiet unless a semantic remainder repeats in a
loop. `jet lint --cost file.jet` reports those rows directly.
`jet explain --cost file.jet` reports semantic remainders and
optimizer-proven removals, so a missing lint row is explainable rather than
silent.

## Precedence

The grammar decision owns declarations, closed vocabulary, defaults, inference,
constant normalization, exact arithmetic, applicability, collision detection,
source spans, and prototype retirement. The report decision owns canonical
report/baseline bytes, identifiers, evidence and context, statistical
decisions, storage, retention, migration, path security, and CAS. The output
decision owns command validation, confirmation, projection, ordering,
diagnostics, fixtures, annotations, and exits. Provider and integration
decisions own measurement transport and intent-owned refresh. The cost decision
owns semantic cost rows and their projection.

When an older surface or baseline example conflicts with a specialized law,
the specialized law controls. When this record's performance explanation
conflicts with AGENTS.md, the strict per-cell, per-metric gate controls. No
section authorizes a second evaluator, report format, provider lookup path,
compatibility alias, or silent evidence fallback.

Each command path must satisfy its parser, sema, provider, migration,
diagnostic, and test contracts end to end. This requirement preserves one
auditable evaluator and report model; it does not authorize a partial or
parallel implementation.
