# Ladder audit — 2026-10-28

- Auditor: LadderAudit (report-only; no Tower writes)
- Subject: `dev` at `d9bed7c10c1f245a0fec6f37cac5eaf3c7bc3d01` (worktree `Main-dev-collectall`)
- Lens: pillar 4 "Ladders, Not Cliffs", tempered by pillar 2 (easy to reason about) and pragmatism.
  A missing rung counts as a finding only when real programs need it. An extra rung counts only when it hides which rung is in force.
- Evidence rules: code (`Compiler/`, `Core/`, `Jetpack/`), executable examples, probe runs, and ratified rows in `Docs/spec/syntax-decisions.md` (cited as `SD:<line>`). Prose alone proves nothing. Claims that were not executed are marked `[INFERENCE]`.
- Probes: copies under `/mnt/jetscratch/lanes/Audit/ladder/probes/`, run with `ladder/go.sh <dir> <timeout> <jet args>`. The jet binary is `/mnt/jetscratch/discovery/base/latest/jet`, built from the same commit, with an isolated `HOME`. Nothing ran inside the repo.

## Probe ledger

| probe | command | observed |
| --- | --- | --- |
| p01 module + block `#Policy(copies: .Explicit)` | `run`, `explain copies` | prints 3; chain shows `[shadowed] module … 0..26`, `[effective] block … 60..86` (byte spans, not lines) |
| p02 `#Policy(unsafe: .Forbid)` in source | `check` | E0930 "#Policy arguments do not match Policy(PolicySetting...)" |
| p03 package `policy: { unsafe: .Forbid }` | `check` | E1206 "policy.unsafe must be .Deny or .Paths([…])" |
| p04 fn `sentries: .Off` + block `.On`; module `.Off` + block `.On` | `check` | E0355 both; the message does not name the outer site |
| p05 package `explicit_units: true, sentries: .Off` + block `.On` | `check` | E0355 (package→block chain enforced) |
| p06 inline `package { policy … }` / `package { authority … }` without `name` | `check` | E0003 "Package needs a name field" |
| p07 `lib._helper()` across modules; `pub fn` | `run`, `check` | underscore call works silently; `pub` → E0411 |
| p08 copy of `Examples/features/packages/monorepo` | `run -p hello`, `run packages/ranker`, `run hello`, `run repo.hello`, `check`, `test` | first two run; `hello`/`repo.hello` E2105; `check` E2104 ambiguous; `test` E2104 "no package.jet found" |
| p10 lambda / fn / lambda-style fn | `run`, `check` | lambdas and fn values run (6, 8, 10); `fn double(x: Int) -> x * 2` → E0080 with a `jet fmt` fix |
| p11 `#Test("…") {}`, `#Test fn f()`, `#Test fn p(a: Int)` | `test` | block + named pass; property form E2105 "native property-test generation and shrinking are not implemented" |
| p11 | `test --measure` | E2104 "`jet test --measure` is not built yet" |
| p12 `#allow(float_money, …)` on a field / on a fn | `check --verbose` | field ok; fn → **internal compiler error, exit 101**: "the function marker `#allow` is not supported by the self-hosted checker yet" |
| p14 root package `members` + `policy: { sentries: .Off }`; member block `#Policy(sentries: .On)` | `run -p app`, `run` in member, `explain sentries` | runs ("member ran"); chain lists only the block; the workspace rung is not inherited |
| p15 `#Abilities(Env) { … }` | `run` | E0927 "`#Abilities` isn't a known applied rule" |
| p16 doc example in `///` | `test --docs`, `doc --check` | `--docs` E2104 not built; `doc --check` L2201 flags script `fn run` as undocumented public API, exit 1 |
| p17 copy of `Examples/features/loose_files/prototype` | `run`, `run run.jet` | bare `run` E2104 "No file given and no package.jet"; `run run.jet` works |
| p18/p19 the same folder plus a three-line `package.jet` | `run`, `fix --all .` | E0623 on `use "./app/report"`, E0602 on `use "../shared/text"`; `fix --all` "no changes made" |
| p20 the same folder | `init` | E2104 "`jet init` is not built yet" |
| cont copy of `Examples/continuity/script_to_system/run.jet` | `check`, `run` | E0357 "Constant 'defaults' must be ALL_CAPS" at `run.jet:29:1` (`defaults: { run: APP }`) |

## Family table

Action vocabulary: **keep** / **add rung** / **unify names** / **remove rung** / **ballot**.

| # | family | rungs that exist (evidence) | missing rungs real programs need | cliffs | names that differ between rungs | unclear precedence | action |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Code reuse | block → lambda `x -> e` (S46 SD:437, D-LAMBDA-IFACE2=A SD:448) → `fn f(x: T) -> R { }` (D-SIG-AFTER1=A SD:287-312) → methods (S27 SD:414) → traits (S28 SD:2233) → generic modules (D-GENMOD1/2 SD:2127-2224); p10 runs | none | the `->` role changes (lambda: body follows; fn: result type follows), but E0080 teaches it and `jet fmt` fixes it (p10) | `->` meaning (see cliffs) | none | keep (L-21) |
| 2 | Effects / authority | fn `-[E]>`/`-[!E]>`/`-[via f]>` (D-SHAPE8=A SD:3150, D-EFF2 SD:3223); `effect` leaf decl (D-EFFECT-DECL1=A SD:3214); package `authority.{allow,deny,needs,trust}` (`Loader.jet:346-383`, D-AUTH-AMBIENT3=A SD:3179, D-AUTHORITY-DENYROW1=A SD:3191); package path ceiling `policy.effects` (`Blocks/Policy.jet:124-134`); CLI `--allow/--deny/--allow-all/--gate` (`Front.jet:865-898`); `#!` = `--allow-all` | block rung `#Abilities` is ratified (D-EFF1 SD:3148, D-SCAP1 SD:3257) but unregistered (p15 E0927); no module rung | a `#!` line silently jumps to all authority | effects / `#Abilities` / `authority` / `policy.effects` / `--gate` / `JET_ORG_UNSAFE_POLICY`; `grants` advertised (`PackageParse.jet:208`) but rejected (`Loader.jet:346-383`) | `#!` vs `authority` vs `--allow`; which rung granted a right is shown only by `jet inspect rights` | ballot (L-05, L-07, L-08) |
| 3 | Markers / policy | package `policy:` → module/fn/block `#Policy(...)` with tighten checks (`Policy.jet:99-143,251-280`; `Statements.jet:65-120`; D-MARK-SCOPE1=A SD:2540, D-PACKAGE-POLICY-SCOPE1=A SD:2563); p01/p04/p05 enforce; org via env `JET_ORG_UNSAFE_POLICY` (`GatePolicy.jet:41-63`) | workspace rung (p14); source spelling for audited gates (only gc/explicit_units/copies/sentries parse, `Policies.jet:50-66`) | `unsafe` values differ per rung (p02/p03) | `unsafe: .Forbid/.Obligations` (org) vs `.Deny/.Paths` (package); `explicit_units: true` (package) vs `#Policy(explicit_units)` (source) | E0355 omits the outer site; explain uses byte spans; `sentries` On-inside-Off is refused as "widening" | unify names (L-06, L-13, L-14, L-15) |
| 4 | Visibility / lifecycle | everything public; `priv module`, `priv(package) module`, `priv` field (S18 SD:2067-2090, card #4723); `#Deprecated` | a per-item hide (one helper fn) needs a wrapping `priv module`; package privacy "will come through the manifest" (S18) | D-STRUCT-LIFE1=A (SD:8142) rungs `_name` → `pub _name` → `pub` were deleted by S18; `_name` has no access meaning (p07) | `pub` (E0411) vs `priv` | none | ballot (L-12) |
| 5 | Errors / recovery | `fn run()` fallible by default (S12 SD:334) → `!Err` (S80/D-FAIL-ERROR1 SD:1886) → `!MyError` (`#Error`); recovery `??`, `?(ctx)`, `#Transact` | none | typed error needs one hand-written `impl Src -> MyErr` per source (D-FAIL-CONV1/2 SD:1893-1914) | none | none | keep (L-22) |
| 6 | Config / settings | CLI `--set k=v` → package `settings:` (`PackageParse.jet:469`) → `$build.settings.*`; FactLaw layers Declaration…CommandLine with `.Force` (`FactLaw.jet:44-87`), used by settings (`Merge.jet:175`) | workspace and org writers for settings [INFERENCE: `ContributionLayer.Workspace` exists, no writer found] | none observed | `.Force` (FactLaw) vs `b.pin` (BuildOptions) vs "Tighten" (Policy) | three resolvers with different rung sets (L-02); D-CONF-MERGE1 / D-SETTING-ORDER1 exist only as index rows (SD:8684, SD:9661) | unify names (L-02) |
| 7 | Build | flags → package `build: { profile: … }` → `fn build(b: Build)` in package/`@build.jet` (`InlinePackage.jet:12-19`, D-ROLEFILE1=A SD:7225); `build_options_resolve` notes replaced values and refuses pinned conflicts with E2113 (`BuildOptions.jet:371-393`) | none | none | `full`/`off` (manifest, flag) vs `.Full`/`.Off` (code); `--release` / `--profile` / `--optimize` overlap (`jet help`); pillar says `build.jet`, the file is `@build.jet` | flag-over-profile is printed as a note; good | keep layering; unify names (L-16) |
| 8 | Run / dev | `jet run file.jet` → `fn run` → `@run.jet`/`fn dev` → package `defaults`/Outputs (D-CMDOVERRIDE1=A SD:7233, D-CMD-OVERRIDE1=C SD:7215, D-ROLEFILE1=A, `--show-default`) | bare `jet run` in a loose folder with `run.jet` (p17) | continuity witness fails (cont: E0357) | `fn run` vs Output `entry`/`defaults: { run: … }` | entry choice between `fn run` and `defaults` | keep override ladder; ballot (L-04) |
| 9 | Test / bench / docs | `#Test("n") {}` and `#Test fn n()` (S43 SD:3800) pass (p11); `test { entry }` target; `@test.jet`/`fn test(suite: Test)` (D-CMD-OVERRIDE1=C) | property tests (p11 E2105), doctests (`--docs` E2104), bench `--measure` (E2104), `jet doc` beyond `--check` (D-DOC-GEN1=A SD:6440 deferred) | `jet doc --check` holds a one-file script to library rules (p16 L2201) | none | none | add rung (L-11) |
| 10 | Packages / modules | loose file(s) with `use "./x"` → inline `package {}` (D-ECO-INLINEPACKAGE1=A SD:7368) → `package.jet` → `members:` workspace (`PackageParse.jet:211-215`) | `jet init` (E2104); workspace commands (p08) | **adding `package.jet` breaks every file import and changes call names** (p18, E0623/E0602; `fix --all` no-op) | `report.headline(...)` (loose) vs `headline(...)` (package); `workspace.jet` examples vs D-ECO-FILEROOT1=A (SD:7362) | workspace vs member (p14) | ballot (L-01); add rung (L-03) |
| 11 | Comptime | `$` facts (D-COMPILER-NS1/D-BUILD-FACT3 SD:40-44, SD:7539), comptime blocks, `fn build` generation (`Examples/features/comptime/`) | none | none | stale `@build.os`/`T.@layout` in SD:3394, SD:3454, SD:5225-5258, SD:3997 and `Markers.jet:7` | `build_fact_precedence.jet` only prints the law as strings | keep; unify names (L-24) |
| 12 | Concurrency | `task f()` → `task.all/race/any {}` → `task.group g(limit:) {}` (D-CONC-SPAWN1=D SD:3080, D-CONC-GROUP1 SD:3008) → `core.service` | none | group → service: a new stringly API (`Service.jet:549,1196,1378,1900`), contradicting D-SERVICE1=D (SD:6988) typed endpoints; restart policy only at service (`:529`) | `task.group` vs `tree/group/worker/send` strings | none | ballot (L-10) |
| 13 | Memory / ownership | default copies → `copies` / `gc` policy at every scope → `use core.mem` → `#Unsafe("why") {}`/fn (S58 SD:3543, D-UNSAFE-OBLIG1=A SD:3567) → `#Region` (D-REGION1 SD:3644) | none | none | `unsafe` values (see 3) | none | keep |
| 14 | FFI / unsafe | `use "x.h" as x` → `use c.lib` → `#Import module` overlay → `#FFI(lang) fn` (D-FFI-UNIFY1 SD:4149, D-FFI-INLINE1 SD:3973) | none | Rust is off-ladder: `extern rust "crate@v" {}` (S50 SD:3916, `Examples/features/lowlevel/ffi.jet:7`) | `#Extern module` in prose (S59 SD:3929, SD:4153) vs `#Import module` in code (`Spellings.jet:95`; `MARKER_EXTERN_MODULE_RETIRED`) | `#ABI` is per fn with no module rung (SD:3962) | unify names (L-23) |
| 15 | Data / serde | auto-derive (S55 SD:2259) → `#Codable` → field markers → hand codec (`Examples/features/serde/`) | none | none | none | refusal: package lint vs per-type `#!Printable`, no module rung (low need) | keep |
| 16 | CLI apps | `fn run()` + `process.args()` → `fn run(args: T)` + `#CLI` (D-CLIFLAG1, D-SHAPE-CLI1=A SD:388) → `#CLI(Standard)` command struct (D-CLI-GLOBAL1=E SD:398) → `#Job` argv subcommands (D-JOB-SUBCMD1=C SD:7172, SD:7279) | none | none | raw argv: `core.process.argv` (S12 SD:343) vs `core.args` (SD:393) vs `process.args()` (`script_mode.jet:14`; `Process.jet:76-78` has both) | `#Job` words are selected "before ordinary CLI parsing" (SD:7280), so a `#Job` and a `#CLI` command with the same name resolve silently [INFERENCE] | unify names (L-19) |
| 17 | Web / game / GUI | script inline `package {}` → battery app; routes via `.route()`, `#server.Get`, `mux().get()` (`Examples/features/web/battery/run.jet:27-40,70-122,158-169`) | none | none | dep `web`/`games`/`gui` vs `core.web`/`core.game`/`core.ui` (battery `package.jet` files) | three route forms with no stated rung order | unify names (L-17) |
| 18 | Logging | `log.info/…` → `setup("level=…")` / `set_level(String)` (`Core/Log/Log.jet:129,144`), process-global | level by env / flag / package setting / module | none | `warning`/`warn`, `err`/`error` aliases (`Log.jet:49`) | n/a | add rung (L-18) |
| 19 | Lints / formatting | site/field `#allow(lint, "why")` (`Markers.jet:115-123`, `MarkerRules.jet:1404-1406`) → package `policy.lints` (D-LINTPOLICY1=A SD:6541-6553); formatter zero-config (S44 SD:3905, D-FMTPROJECT1 SD:7036) | fn rung crashes (p12 ICE 101); no module rung; no org lints rung (`GatePolicy.jet:41-63` gates only, contra D-LINTPOLICY1) | crash on the fn rung | `#allow` lowercase among PascalCase markers; banned in marker groups (`NamedMarkerGroups.jet:147`) | none | add rung (L-09); keep formatter |

## Ranked findings

Ranked by user impact. Every user who outgrows one file meets the first findings; only specialists meet the later ones.

**L-01 — The prototype → package climb is a rewrite, not a rung.** (high; ballot)
- What happens: a loose folder runs with `jet run run.jet` (p17). Adding a three-line `package.jet` turns:
  - every `use "./x"` into E0623;
  - every `..` path into E0602;
  - and the package then merges all files into one namespace, so duplicate top-level names fail with E0624 (p13).
- Call sites must change from `report.headline(...)` to `headline(...)`.
- Tools don't bridge the gap:
  - `jet fix --all .` reports "no changes made" (p19);
  - `jet init` is not built (p20);
  - E0623 has no structured fix (`DiagnosticRows.jet:5155-5168`, `no_fix_reason: Design`).
- No decision row is cited for the package-namespace rule. S16 (SD:2028-2041) ratifies quoted file imports with no package restriction.
- Pillar text broken: "grow … without rewrites".

**L-02 — Three precedence engines, three rung sets, three words for "force".** (high; unify names)
- `Policy.jet` scopes are Organization / Package / Module / Function / Block and tighten (`Statements.jet:65-120`).
- `BuildOptions.jet:371-393` layers stock < profile < code < flag, with `b.pin` and E2113.
- `FactLaw.jet:10-87` uses Package / File / Module / Function / Block / Item × Declaration / OptimizationBundle / Workspace / Environment / System / Fleet / CommandLine, with `.Force` and E3521. Settings use it (`Merge.jet:175`).
- Authority uses floor + allow, deny wins. CLI fields use input > `#Env` > default.
- The governing rows D-CONF-MERGE1 and D-SETTING-ORDER1 appear only in the index (SD:8684, SD:9661).
- The witness `Examples/features/comptime/build_fact_precedence.jet` only prints the law as strings.
- `Jetpack/PackageModel/Tests/Merge.jet:68` claims "without a second priority table", but BuildOptions is a second one.
- Pillar text broken: "make it obvious which rung is in force".

**L-03 — The workspace rung exists for `run -p` only.** (high; add rung)
- Root `policy: { sentries: .Off }` does not reach a member. Its `#Policy(sentries: .On)` block runs, while the same nesting inside one package is E0355. `jet explain` shows only the block (p14).
- On the monorepo example (p08):
  - `jet check` is E2104 "ambiguous";
  - `jet test` does not see the workspace;
  - `jet run hello` and `jet run repo.hello` are E2105, although the example comments promise them.
- `workspace.jet` examples persist although D-ECO-FILEROOT1=A (SD:7362) folds workspace.jet into package.jet. The `workspace` field is reserved-empty (`PackageParse.jet:213-215`).
- `PolicyScope` has no Workspace. Prior LAY-04 covered the command half.

**L-04 — The flagship script → system witness fails.** (high; ballot)
- `Examples/continuity/script_to_system/run.jet:29` `defaults: { run: APP }` fails `check` and `run` with E0357 (cont).
- This is the only executable proof that one file climbs to a system (card #1415).
- Owner choice: either in-source Output declarations get a ratified spelling, or the example moves `defaults` to `package.jet`.

**L-05 — The effects block rung is ratified but missing.** (high; ballot)
- `#Abilities(Env) { … }` gives E0927 "isn't a known applied rule" (p15).
- Four ratified rows still teach it: D-EFF1 SD:3148, D-SCAP1 SD:3257, D-EFFTREE1 SD:3203, D-EFFECT-DECL1 SD:3218.
- There is no module rung either, so effects climb fn → package → CLI, with the block and module steps missing.
- Owner choice: implement the rung, or retire it from the four rows.

**L-06 — `unsafe` uses different values per rung, and the teaching text points at the rejected one.** (high; unify names)
- Org takes `.Forbid / .Obligations / .GateOnly` (`GatePolicy.jet:41-63`). Package takes `.Deny | .Paths` (`Blocks/Policy.jet:135-145`; p03 E1206).
- In source, `#Policy(unsafe: …)` is a generic E0930 (p02).
- E0355 teaches `policy: { unsafe: .Forbid }` (`Policies.jet:194`), which E1206 rejects.

**L-07 — A `#!` line silently changes the authority rung.** (high; ballot)
- `script_mode.jet:1-5` and `jet help` ("A file starting with a #!/ line runs this way") confirm it. Nothing at the call site shows which rung is in force.
- Already tracked as SEC-01 (#3418, #4268); ballot draft B2 D-SCRIPT-AUTH1 is in `state-of-jet-2026-10-28.md`.

**L-08 — Effect vocabulary changes at every rung.** (medium; unify names)
- Each rung uses a different word:
  - fn: `-[E]>`;
  - block: `#Abilities`;
  - package: `authority.allow/deny/needs/trust`;
  - package path ceiling: `policy: { effects: { "path": -[…]> } }`;
  - CLI: `--allow/--deny/--gate`;
  - org: `JET_ORG_UNSAFE_POLICY`, which carries every gate despite its name ("fifteen" in its message against 16 rules).
- `PackageParse.jet:208` sends users to `authority.grants`, which `Loader.jet:346-383` rejects.
- D-EFFBUDGET1 (SD:3326) and D-PACKAGE-POLICY-SCOPE1 (SD:2569) still show `effects: {allow, deny}`.

**L-09 — The lint ladder crashes on the function rung.** (medium; add rung)
- `#allow(float_money, "…")` on a fn → internal compiler error, exit 101: "function marker `#allow` is not supported by the self-hosted checker yet" (p12). The field rung works.
- There is no module rung.
- The org file accepts gates only (`GatePolicy.jet:41-63`), but D-LINTPOLICY1=A (SD:6541-6553) says "org narrows".

**L-10 — Concurrency: `task.group` → service is a cliff.** (medium; ballot)
- `core.service` names trees, groups, and workers by String and sends String messages (`Service.jet:549,1196,1378,1900`). D-SERVICE1=D (SD:6988) promises typed builders and endpoint types.
- Restart policy exists only at the service rung (`:529`).

**L-11 — Test-ladder rungs are unbuilt.** (medium; add rung)
- Property tests are E2105 (p11). Doctests (`--docs`) and bench (`--measure`, the D-CLAIM-BENCH1 replacement) are E2104 (p11, p16).
- `jet doc --check` on a one-file script flags `fn run` as undocumented public API, exit 1 (p16), holding the lowest rung to the library rung.

**L-12 — The visibility ladder lost its middle rungs.** (medium; ballot)
- S18 (SD:2067, #4723) deleted D-STRUCT-LIFE1's `pub _name` / `pub` rungs (SD:8142). `_name` has no access meaning: `lib._helper()` works silently (p07).
- Hiding one helper requires a `priv module` wrapper. Package privacy "will come through the manifest" and has no rung today.

**L-13 — Audited gates claim every scope but are unwritable in source.** (medium; ballot)
- POLICY_RULES give the 16 gates ALL_SCOPES (`Policy.jet:119`).
- Source `#Policy` parses only gc / explicit_units / copies / sentries (`Policies.jet:50-66`).
- So the function and block rungs exist only through `--gate` (`Gates.jet:37-47`), and the org rung only through an env var.

**L-14 — The rung in force is hard to see in diagnostics.** (medium; unify names)
- E0355 says "inner declarations may not conflict" but names no outer site (p04, p05).
- `jet explain <key> <file>` shows the chain as byte spans (`p01_block_policy.jet:0..26`), not `file:line`.

**L-15 — `sentries` tightening direction looks inverted.** (medium; ballot)
- Turning sentries On inside an Off scope is E0355 "widening" (p04). Off inside On is allowed.
- For a safety net, re-enabling it locally is the tightening move.
- Ratified text (SD:2556, SD:7596) names only the refusal exit.

**L-16 — Build knob names differ by rung.** (medium; unify names)
- Values are written `full`/`off` in the manifest and on flags, but `.Full`/`.Off` in `fn build`.
- `--release`, `--profile`, and `--optimize` all say "how hard to optimize" (`jet help`).
- Pillar 4 says `build.jet`, but the file is `@build.jet` (D-ROLEFILE1=A).
- The layering itself (notes plus pin) is good: keep.

**L-17 — Web battery: three route forms and drifting names.** (medium; unify names)
- Routes can be written as `core_web.app().route()`, `#server.Get` methods, or `server.mux().get()` (`web/battery/run.jet`).
- Battery deps `web`/`games`/`gui` don't match `core.web`/`core.game`/`core.ui`.

**L-18 — Logging has one rung.** (medium; add rung)
- The level is a process-global string (`Log.jet:129,144`), with aliases (`:49`).
- There is no env, flag, package-setting, or per-module level. Services and CLIs need at least env plus per-module levels.

**L-19 — CLI raw argv has three names, and `#Job` vs `#CLI` command precedence is unwritten.** (low; unify names)
- Raw argv appears as `core.process.argv` (SD:343), `core.args` (SD:393), and `process.args()` (`script_mode.jet:14`).
- D-JOB-SUBCMD1 selects job words before CLI parsing (SD:7280). [INFERENCE] A job therefore shadows a same-named `#CLI` command without a diagnostic.

**L-20 — Inline `package {}` requires `name:` even for an authority-only script.** (low; ballot)
- p06 gives E0003 "Package needs a name field". E1803's autofix inserts the name (`EffectProjection.jet:121-123`).
- The cost is low, but it is ceremony on the lowest rung.

**L-21 — Lambda → fn arrow role changes.** (low; keep)
- `fn f(x: Int) -> x * 2` is E0080, which teaches the fix and says `jet fmt` applies it (p10).
- The change is ratified (D-SIG-AFTER1=A, #4512), and the climb is mechanical.

**L-22 — Typed-error climb needs a per-source conversion.** (low; keep)
- Each source error needs one `impl Src -> MyErr` (D-FAIL-CONV1/2). This is the accepted cost of a closed error domain. Revisit only if real programs show many sources.

**L-23 — FFI: Rust is off-ladder and `#Extern` prose is stale.** (low; unify names)
- `extern rust` (S50) does not use `use rust.*`; prior LANG-04 (#5482, #4280) covers this.
- The prose still says `#Extern module` (SD:3929, SD:4153), while the code says `#Import module`.

**L-24 — Stale rung spellings in ratified prose.** (low; unify names)
- `@build.os`/`T.@layout` (SD:3394, SD:3454, SD:5225-5258, SD:3997; `Markers.jet:7`).
- The D-EFFECT-OMIT1 example `fn bounded(path: String) String -[FS.Read]>` (SD:3248-3251) uses the pre-D-SIG-AFTER1 order.
- D-EFF4/5 says user `effect` declarations are "reserved, unminted" (SD:3171), but D-EFFECT-DECL1 made them live.
- `explicit_units: true` (package) vs `#Policy(explicit_units)` (source).

## Confirmed keeps

These ladders already meet pillar 4. Each needs no action beyond the name fixes listed above.

- **K-01 Command override ladder.** It climbs CLI → `fn run/build/dev/test` → `@run.jet`/`@build.jet`/`@dev.jet`/`@test.jet`, and `--show-default` reports the stock rung. Sources: D-CMDOVERRIDE1=A SD:7233, D-CMD-OVERRIDE1=C SD:7215, D-ROLEFILE1=A SD:7225, E3540.
- **K-02 Build-option layering.** A flag replacing a profile or code value prints a note, and a pin conflict is E2113 (`BuildOptions.jet:371-393`).
- **K-03 Policy package → module → fn → block chain.** It tightens and is enforced (p01, p04, p05). `jet explain <key> <file>` shows the chain (p01).
- **K-04 Memory.** The rungs run from default copies to `copies`/`gc` policy, then `use core.mem`, then `#Unsafe`, then `#Region` (S58, D-UNSAFE-OBLIG1=A, D-REGION1).
- **K-05 Serde.** The rungs run from auto-derive to `#Codable`, then field markers, then a hand codec (S55, `Examples/features/serde/`).
- **K-06 Formatter.** It has zero config by design (S44, D-FMTPROJECT1); a single rung is correct.
- **K-07 Typed CLI.** The rungs run from `fn run()` to `fn run(args: T)` with `#CLI`, then to a `#CLI(Standard)` command struct, with no rewrite between them (D-SHAPE-CLI1=A, D-CLI-GLOBAL1=E).
- **K-08 Comptime `$` facts.** The rungs run from facts to comptime blocks to `fn build` generation. Only the stale prose is a finding (L-24).
- **K-09 C FFI ladder.** It runs from `use "x.h"` to `use c.lib`, then `#Import module`, then `#FFI(c) fn` (D-FFI-UNIFY1). Only the Rust side is off the ladder (L-23).

## Owner questions (ballot candidates)

1. **D-LOOSE-GROW1 — keep file imports working when a folder becomes a package (L-01).**
   - Current: inside a package, `use "./app/report" as report` is E0623 and `use "../shared/text"` is E0602; call sites must change to bare names.
   - A: inside a package, keep `use "./app/report" as report` legal as an alias for the module `app.report`; `..` within the package root stays legal.
   - B: keep the current rule, and make `jet fix` rewrite `use "./app/report" as report` to `use app.report as report`.
   - C: no change.
2. **D-LADDER-LAW1 — one precedence vocabulary (L-02, L-14, L-16).**
   - Current: Policy says `Tighten`, BuildOptions says `b.pin(.optimize, .Full)` / E2113, FactLaw says `.Force` / E3521, and each has its own rung list.
   - A: one rung list, `org > workspace > package > module > fn > block`, plus the `command line` layer, owned by FactLaw. Policy and BuildOptions project onto it. `pin` becomes the only force word (`.Force` → `pin`), and `jet explain` prints `file:line` for each rung.
   - B: keep three engines, but ratify the D-CONF-MERGE1 text and make `jet explain` show all three with one format.
3. **D-WORKSPACE-INHERIT1 — the workspace rung (L-03).**
   - Current: root `policy:` / `settings:` / `authority:` do not reach members.
   - A: members inherit root `policy`, `settings`, and `authority.deny`; members may only tighten.
   - B: no inheritance; `members:` is index-only, and E-diagnostic on root `policy:` when `members:` is present.
4. **D-UNSAFE-VALUES1 — one `unsafe` spelling (L-06).**
   - Current: org `unsafe: .Forbid | .Obligations | .GateOnly`, package `unsafe: .Deny | .Paths([...])`, source `#Policy(unsafe: …)` E0930.
   - A: every rung uses `.Deny | .Paths([...]) | .Obligations`, and E0355 teaches `policy: { unsafe: .Deny }`.
   - B: every rung uses `.Forbid | .Obligations | .GateOnly`.
5. **D-ABILITIES-BLOCK1 — the effects block rung (L-05).**
   - Current: ratified `#Abilities(FS) { … }` gives E0927.
   - A: implement `#Abilities(E) { … }` and add a module rung, `#Abilities(E) module m { … }`.
   - B: retire `#Abilities` from D-EFF1/D-SCAP1/D-EFFTREE1/D-EFFECT-DECL1, and keep fn `-[E]>` as the smallest rung.
6. **D-SENTRY-DIR1 — sentries direction (L-15).**
   - Current: `#Policy(sentries: .On)` inside `.Off` is E0355.
   - A: `.On` is the tight value, so On-inside-Off is legal and Off-inside-On needs the outer rung's consent.
   - B: keep the current direction.
7. **D-SERVICE-TYPED1 — service rung (L-10).**
   - Current: `service.worker(tree, "name", handler, "handler", capacity)` and `service.send(tree, "endpoint", "message")`.
   - A: typed `service.worker(tree, handler)` keyed by fn identity, and `send(endpoint: Endpoint<Msg>, msg: Msg)`.
   - B: keep strings and amend D-SERVICE1.
8. **D-VIS-ITEM1 — per-item hiding (L-12).**
   - Current: hiding one helper requires `priv module helpers { fn helper() {} }`.
   - A: `priv fn helper()` is legal at item scope.
   - B: keep module-only privacy and drop the "will come through the manifest" note until it exists.
9. **D-CONT-OUTPUT1 — Outputs in source (L-04).**
   - Current: `defaults: { run: APP }` in `run.jet` is E0357.
   - A: allow `defaults:` as a recognised file-level field.
   - B: move `defaults` to `package.jet` in the continuity example.
10. **D-LOG-LADDER1 — log level rungs (L-18).**
    - Current: only `log.set_level("warn")` / `log.setup("level=warn")`.
    - A: `JET_LOG=warn,app.db=debug` env, plus `--log=` flag, plus package `settings: { log: "warn" }`, all resolved by the same law as L-02.
11. **D-ORG-POLICY1 — org rung reach (L-08, L-09, L-13).**
    - Current: `JET_ORG_UNSAFE_POLICY` carries gates only.
    - A: rename it to `JET_ORG_POLICY` and accept `lints` and `unsafe` keys.
    - B: keep it gates-only and amend D-LINTPOLICY1 to drop "org narrows".
12. **D-BUILD-KNOBS1 — build knob names (L-16).**
    - Current: `--release`, `--profile=release`, and `--optimize=full`; manifest `optimize: full` vs code `.Full`.
    - A: keep `--profile` and `--optimize`, make `--release` documented sugar for `--profile=release`, and accept `.Full` in the manifest.
    - B: no change.

## Strongest unverified assumption

That no workspace-layer writer feeds `FactLaw` for settings. `ContributionLayer.Workspace` exists (`FactLaw.jet:51`), and p14 shows only that policy is not inherited. A settings-inheritance probe was not run.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| L-01 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-02 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-03 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-04 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-05 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-06 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-07 | card | #3418 |
| L-08 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-09 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-10 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-11 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-12 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-13 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-14 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-15 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-16 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-17 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-18 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-19 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-20 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
| L-21 | decision | D-SIG-AFTER1=A |
| L-22 | no-action | archived: accepted cost of D-FAIL-CONV1/2; retained as evidence only |
| L-23 | card | #5482 |
| L-24 | no-action | report-only run: recommendation recorded; implementation is not authorized here |
<!-- /audit-dispositions -->
