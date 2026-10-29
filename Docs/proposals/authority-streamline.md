# Authority streamline: one ladder, one block, one table

Status: proposal, 2026-10-24, lane O0Load. Nothing here is implemented.
Tree audited: dev `8d2082ea63`. Line numbers are at `f714e05fe7`; no audited file changed between the two.
Probes ran with the 1589c62507 compiler, one at a time; transcripts are in `/mnt/jetscratch/lanes/O0Load/authres/` (`probes.md` for P1-P15, `me/` for p1-p2).
Owner choices are in ballots `D-AUTH-AMBIENT3` and `D-AUTHORITY-DENYROW1`.
The full site list for the `holds` cutover is `authority-streamline-sites.tsv` beside this file.

## 0. Summary

Jet already has one ratified authority model (D-AUTHORITY-MODEL1=A) and one manifest block (D-AUTHORITY-MANIFEST1=A).
The code does not yet match either.
Four kinds of drift remain:

1. **The ladder is broken.** `package.jet` and the inline `package { … }` block share one compiler parser, but tooling, diagnostics and labels split them.
2. **The common setting is two levels deep.** `authority.holds.allow` should be `authority.allow`, as the owner directed.
3. **The vocabulary has eleven copies.** They already disagree: one lists `Crypto`, `Random` and `Proc`, which are not effects.
4. **Several surfaces parse but never act.** `grants`, `trust.services` and `policy.effects` are parsed, but no production path enforces them.

The cutover keeps every working behavior.
It deletes the duplicates, closes the gaps and leaves one canonical spelling.

## 1. Fact map, verified

Each row was checked in source and, where marked, by a probe.

| Claim | Verdict | Evidence |
|---|---|---|
| Inline block and `package.jet` use one compiler parser | TRUE | `Driver/Loader.jet:991` `jet_driver_parse_package_root`; the inline body is fed at its original offsets (`:1005-1012`) |
| They use one tooling parser | FALSE | `PackageFacts.parse` (`PackageParse.jet:18`) runs `validate_defaults`; `pkg_manifest_parse` (`Manifest.jet:191-203`) skips it; `Entry.jet:347` is a third reader |
| Deny wins across layers | TRUE | `Cli/Authority.jet:131-135`; probes P1 and P2 |
| E2102 refuses `--allow=X --deny=X` | TRUE, same invocation only | `Front.jet:749`; probe P4 exits 2 |
| No package block grants `IO`, `Mem.Alloc`, `Exec.Args` | TRUE | `Cli/Authority.jet:26`; probe P3 |
| A written block replaces that default | TRUE, even a deny-only block | probe p2: `deny: [Exec]` alone makes `print` fail E1803 |
| Sema reads package policy | ONLY memory denials | `Effects/Checks.jet:2316-2318` |
| `#FX` regions are ceilings, not grants | TRUE | probe P12b: `#FX(Net)` around `print` fails E0712 for `IO` |
| Package allow gates declared rows | FALSE; it gates performed effects | probe P12: calling `-[Net]>` code that does no Net runs |
| `authority.grants`, `trust`, `providers` reach the compiler | FALSE | `Loader.jet:392-393` refuses them, E1221 "not supported by the current compiler package-policy boundary"; probes P8b, P9b |
| `grants` are enforced anywhere | FALSE | `pkg_effect_enforce` (`EffectBudget.jet:320`) has only test callers |
| `trust.services` is enforced | FALSE | stored and serialized only (`CanonicalPackageFacts.jet:240-242`) |
| `policy.effects.<path>` is enforced | FALSE | parsed (`Blocks/Policy.jet:112-121`), shown by `FactsDebug.jet:304`, never compared |
| Unknown leaves are refused | FALSE | probe P14: `allow: [FS.Bogus]` is accepted |
| `Time.Wait` in `allow` does anything | FALSE | probe P6: `time.sleep` asks for `Time`; listing `Time.Wait` changes nothing |
| `needs` is live | TRUE | plugin and FFI host imports (`Plugin.jet:596-600`, `BuildBridge.jet:241-263`) and the lock; sandbox `lend` is dead |

Two findings contradict ratified law and are bugs, not choices.
`time.sleep` needs a `Time` grant, but D-AUTH-AMBIENT2=A says waiting never needs a grant.
`FS.Bogus` is accepted, but D-EFF2 says a package budget must name a declared leaf exactly.

## 2. The ladder: inline block equals `package.jet`

Owner rule: `package.jet` is exactly the body of `package { … }`, implicitly wrapped, with no extra indent.
Same keys, same parser, same meaning.

### 2.1 What holds today

The compiler driver already reads both carriers through one token loop.
Every top-level key (`name`, `version`, `edition`, `runtime`, `authority`, `policy`, `allocator`, `deps`, `configs`, `outputs`, `settings`, `build`) is accepted by both.
`policy.effects` is accepted in both (probes P10, P10b).

### 2.2 Divergences to remove

| # | Divergence | Inline | `package.jet` | Site |
|---|---|---|---|---|
| L1 | Error wrapping | every field error becomes E1362 "The inline Package body is malformed" and drops the reason | keeps the specific code and reason, for example E1221 | `Loader.jet:1242-1256`; probes P8, P9, P15 vs P8b, P9b |
| L2 | Carrier name in reasons | Why says "`package.jet` has a malformed `authority:` block" | same text | `Loader.jet:207-209` |
| L3 | Missing name | "Package needs a `name` field" | "package manifest has no name"; Jetpack says "`package.jet` needs a `name` field" | `Loader.jet:1234-1237`; `Manifest.jet:286-289` |
| L4 | Default-policy label | "package.jet default" and "package.jet (which has no authority block)" for an inline block | correct | `Cli/Authority.jet:163, 269-279`; probe P5 |
| L5 | Tooling validation | `PackageFacts.parse_inline` runs `validate_defaults` | `pkg_manifest_parse` and `Resolver.jet:162` skip it | `PackageParse.jet:42`; `Manifest.jet:192-199` |
| L6 | Duplicate `authority:` | driver: "`authority.holds` is declared more than once" | tooling: BadEffectsBlock "declared more than once" | `Loader.jet:226, 338`; `PackageParse.jet:136` |
| L7 | Unknown top-level key | driver skips it silently | driver skips it silently; tooling reports UnknownField | `Loader.jet:1223`; `PackageParse.jet:167` |
| L8 | Authority sub-keys | driver refuses `grants`, `trust`, `providers` | same refusal; tooling accepts all five | `Loader.jet:386-401`; `Blocks/Authority.jet:162-184` |
| L9 | Retired top-level `grants:` | driver ignores it | tooling reports RetiredAuthorityField | `Loader.jet:1223`; `PackageParse.jet:208` |

L8 breaks the owner rule from both sides.
A single-file program cannot declare trust or providers, and neither can a `package.jet` built by `jet run`.

### 2.3 The fix

One parser owns the Package body: `Jetpack/PackageModel/Source/Manifest/PackageParse.jet`.
The driver calls it for both carriers and keeps only the fields the compiler consumes.
Keys the compiler does not consume (`grants`, `trust`, `providers`) pass through to Jetpack instead of failing.

Every diagnostic names the carrier by its real path: "the `package { … }` block in app.jet" or "package.jet".
E1362 survives only for a block that does not parse as a Package body at all.
Field errors keep their own codes on both carriers.
Unknown keys fail on both carriers with one code.
E1363 (both carriers in one directory) stays.

Callers to migrate: `Loader.jet:991-1264, 1511`; `Manifest.jet:191-203`; `PackageParse.jet:18, 27, 35`; `Cli/Request.jet:495-504`; `ScriptDeps.jet:56`; `Authority/Resolver.jet:162`; `Cli/Entry.jet:347` (members, outputs, profiles); `Lock/Parse.jet:247`; `PolicyLoad.jet:10-16`; `Pipeline.jet:424-428`.
Labels: `Cli/Authority.jet:28, 163, 167, 269-279`; `Foundation/Policy/Authority.jet:334-342`; `EffectProjection.jet:70`.
Tests: the E1362 rows in `tests/ui` and `tests/cases`, plus jetdoc goldens that print "package.jet authority.holds" (`Canvas03Tests.jet:409`).
Spec: `spec.md:1637-1644`; `reference/cli.md:620-626`; `Docs/spec/packaging/` gains the ladder rule, which it lacks today.

## 3. The flattened block

D-EFFECTS-BLOCK1 is open, recommending a top-level `effects:` block (its option A).
The owner direction picks its option B: `allow` and `deny` sit directly under `authority`.
Main should record that outcome; the ballot is otherwise moot.

```jet
authority: {
    allow: [FS.Read, Net, Time],
    deny: [Exec, Mem.Alloc(above: 65536), Mem.Rc, Panic],
    needs: [FS.Read, Net.Connect],
    grants: { "image-codec": [FS.Read] },
    trust: { default: prompt, ci: deny, require: attested },
    providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } },
}
```

| Key | Meaning | Was | Checked by |
|---|---|---|---|
| `allow` | rights this package's own program holds at run time | `holds.allow` | E1803 before host state changes |
| `deny` | rights it must never use, including deny-only rights | `holds.deny` | E1803; memory entries also become whole-program `!Mem…` prohibitions (E0921), as today |
| `needs` | host imports a guest or plugin asks its host for | `needs` | plugin and FFI bridge, lock |
| `grants` | per-dependency delegation, keyed by package | `grants` | E1220 budget (must be wired, §6) |
| `trust` | fetch prompts and the provenance floor | `trust` | trust gate, fetch, verified reuse |
| `providers` | per-registry bounds for foreign package managers | `providers` | registry fetch, cache identity |

Deny-only rights (`Panic`, `Mem`, `Time.Wait`) are legal only in `deny`.
In `allow` they fail with one code whose fix deletes the entry, because they need no grant.
Parameterized entries such as `Mem.Alloc(above: N)` stay legal only in `deny`, as today (`Blocks/Authority.jet` effect-list reader).

D-EFFECTS-BLOCK1 warned that `allow` beside `grants` may read as a dependency grant.
Two things answer it.
`grants` is always a map keyed by package name, and `allow` is always a list.
The E1803 and E1220 texts say "this package's own code" and "the dependency `x`" respectively.

Migration for the flattening, greenfield with no alias:

- Parsers: `Loader.jet:332-384` (driver holds parser); `Blocks/Authority.jet:139-152` (`AuthorityHolds` record and reader); `PackageParse.jet:212-216`; `Lock/Write.jet:282-320`; `Lock/Parse.jet:250`; `JetSemIndex/Codec/Package.jet:102, 334` (the `"holds"` wire key).
- Readers of `.holds`: `EffectProjection.jet:62-72`; `EffectBudget.jet:322-326`; `Cli/Authority.jet:158-171`; `Checks.jet:2316-2318` (provenance text); `PackageFacts.jet:437, 477`; `CanonicalPackageFacts.jet:217-245`.
- Texts: E1803 registry row `DiagnosticRows.jet:7394-7409`; `Cli/Authority.jet:289-350`; `Foundation/Policy/Authority.jet:302-342`; `EffectProjection.jet:105-111`; E1221 fix text `Manifest.jet:349-352`; `Loader.jet:312-316, 371`.
- Sources: 2,130 files mention `holds` in an authority sense (`authority-streamline-sites.tsv`).
  By group: `tests/cases` 1,317; `Examples/features` 317; `Tools/agent-eval` 169; `Docs/audits` 63 (historic, leave); `Compiler/JetSema` 38; `Tools/gauntlet` 27; `Tools/perf` 24; `Jetpack/PackageModel` 20; others under 15 each.
- Spec: `spec.md:1624-1650, 1664-1670, 4494-4497`; `reference/cli.md:499-500, 620-640`; `Docs/site` (5 files).
- `jet fix` gains one mechanical rewrite, `holds: { allow, deny }` to `allow, deny`, used once for the cutover.

## 4. One vocabulary table

D-META-ONE1=A already says the vocabulary is written in `Compiler/Prelude/Effects.jet` and that every consumer reads it.
In fact eleven tables exist, and Prelude is the least complete.

| # | Table | Site | Drift |
|---|---|---|---|
| V1 | `effect` declarations | `Prelude/Effects.jet:8-22` | 15 lines; no `Panic`, no `Mem`, no leaves except `FS.Read`, `FS.Write` |
| V2 | `AUTHORITY_EFFECT_ROOTS` | `Foundation/Policy/Authority.jet:11-13` | 13 grantable roots |
| V3 | `sema_effect_grantable_roots` | `Sema/Effects/Checks.jet:347-349` | copy of V2 |
| V4 | root index | `Checks.jet:378-392` | 15 roots, numbered |
| V5 | `sema_effect_builtin_leaves` | `Effects/Declarations.jet:28-29` | 29 leaves absent from V1 (`DB.Read`, `Exec.Args`, `Time.Wait`, 19 `FFI.*` …) |
| V6 | `pkg_effect_root_known` | `Manifest/Blocks/Authority.jet:91-97` | 15 roots |
| V7 | ambient basics and deny-only | `Foundation/Policy/Authority.jet:20, 34-37`; `Cli/Authority.jet:26` | two copies |
| V8 | action words for E1803 Why | `Cli/Authority.jet:235-267`; `Foundation/Policy/Authority.jet:372-388` | two copies |
| V9 | `InspectRights` rows | `Cli/InspectRights.jet:30-46` | irreversible bits copied from V1 |
| V10 | `core.compiler` `enum Effect` | `Core/Compiler/Lang.jet:25-38`, generated into `CoreEnumRows.jet:14`, `StructureRegistry.jet:36`, `CoreModuleExports.rs:226` | **wrong set**: has `Crypto`, `Random`, `Proc`; lacks `Rand`, `Log`, `GPU`, `FFI`, `Mem`, `Panic` |
| V11 | Rust bridge copies | `Integrations/…/jet-foundation/src/Effects.rs:32-35`, `Authority.rs:38-39, 570-582, 973` | copies of V1, V7, V8 |

Proposal: V1 becomes the only table and gains every fact the others hold.

```jet
effect IO $ambient "prints text or reads input"
effect Mem $deny_only "allocates memory"
effect Mem.Alloc $ambient
effect Time "reads the clock"
effect Time.Wait $deny_only "waits"
effect Net $irreversible "uses the network"
```

- `$ambient` marks the beginner floor (`IO`, `Mem.Alloc`, `Exec.Args`).
- `$deny_only` marks `Panic`, `Mem` and `Time.Wait`.
- The string is the action phrase E1803 prints.
- Every leaf now in V5 is declared here, so D-EFF2 exact-leaf checking can refuse `FS.Bogus`.

V2-V9 become reads of the registered declarations.
V10 is regenerated from V1 by `Compiler/Bootstrap/generate-core-sources.mjs`, which already embeds V1 as `JET_CLI_EFFECT_SOURCE` (`CoreSources.jet:6`).
V11 is generated by the same step or deleted with its bridge.
`Tools/agent/gen-core-tables.mjs:41` and `check-core-surface-ledger.mjs:1440-1450` read the generated rows.

## 5. How the pieces relate: one page

### Beginner path

Write a program and run it.
Printing, reading input, allocating and reading arguments need nothing.
When your program first touches files, the network or a process, `jet run` stops before anything happens and prints the exact fix.
For one run, add the flag it names, such as `jet run --allow=Net app.jet`.
To keep it, paste the row it prints into a `package { … }` block at the top of the file.
When the project grows, move that block's body into `package.jet` unchanged.

```jet
package {
    name: "fetcher"
    authority: { allow: [Net] }
}
```

### Expert path

Jet checks authority in three layers. Each answers a different question.

| Layer | Written as | Question | Checked | Error |
|---|---|---|---|---|
| Grant | package `authority.allow` / `deny`; CLI `--allow` / `--deny`; REPL prompt | may this run touch the host? | before host state changes, over effects the program performs | E1803, E2102 |
| Ceiling | `-[FS, Net]>` rows; `#FX(Net) { … }` regions; package `policy.effects.<path>` | may this code reach that effect at all? | compile time | E0740, E0712 |
| Prohibition | `-[!Net]>`, `-[!Mem.Alloc]>`; package `deny` of a `Mem` right | must this code never reach it? | compile time, through calls | E0749, E0921 |

- The grant layer composes outward: ambient floor, package block, invocation. A deny at any layer wins (D-RIGHTS-CLI1=B). Today a written package block replaces the floor; ballot D-AUTH-AMBIENT3 asks whether it should add.
- A ceiling never grants. A `Net` call inside `#FX(Net)` still stops with E1803 before the run when the package does not allow `Net` (code-read, `Cli/Authority.jet:150-218`; not probed).
- A row states what code may do. A grant is needed only when code really performs the effect (probe P12).
- The same vocabulary and the same root-or-leaf coverage rule serve all three layers.
- The REPL has no package layer. Its flags and prompts form the grant layer, and `--deny` beats any prompt.

## 6. Deletions and gaps

Each item lists its sites. Items marked GAP are ratified behavior with no production caller; the cutover wires them rather than deleting them, so nothing is lost.

| # | Item | Kind | Sites |
|---|---|---|---|
| X1 | CLI re-implementation of the Foundation policy (`parse_root`, `parse_right`, `covers`, sorted set, ambient constant, projection, owner labels, fix text, action words) | duplicate | `Cli/Authority.jet:26, 91-110, 131-135, 158-218, 235-350`; keep `Foundation/Policy/Authority.jet` and `EffectProjection.jet:62-72` |
| X2 | Driver authority validator and its texts | duplicate | `Loader.jet:220-267, 289, 309-316, 332-401` (replaced by §2.3) |
| X3 | Vocabulary tables V2-V11 | duplicate | §4 |
| X4 | `sema_effect_scope_bounds`, `sema_effect_check_scope_bounds` | dead | `Checks.jet:1601, 1623` (no callers) |
| X5 | `SandboxAuthority.lend`, `verdict`, `authorize_import` | dead | `Authority/SandboxAuthority.jet:74-133` (no callers; `needs` stays live through plugins) |
| X6 | `pkg_e1258` | dead | `Manifest.jet:396-400` |
| X7 | `holds` level and `AuthorityHolds` record | shape | §3 |
| X8 | `RetiredAuthorityField` for top-level `grants:` | migration aid | `PackageParse.jet:198, 208, 217` (greenfield: unknown key error suffices) |
| X9 | Deny-only rights written in `allow` | noise | 231 `.jet` files list `Time.Wait` in an allow row, for example `Examples/embedded/wasi/package.jet:10` and 40+ `Examples/features/concurrency/*.jet` |
| X10 | `E1362` field wrapping | diagnostic | `Loader.jet:1242-1256` |
| X11 | Second E1803 fix-text builders (four variants) | duplicate | `Cli/Authority.jet:289-350`; `Foundation/Policy/Authority.jet:302-332`; `EffectProjection.jet:105-111`; REPL `TurnFacts.jet:532-544` keeps only its REPL clause |
| G1 | `grants` enforcement, E1220 budget, lock provenance | GAP | `EffectBudget.jet:320-392`; wire into `jet build` and `jet run` dependency resolution |
| G2 | `policy.effects.<path>` ceilings (D-TEAMPOLICY1=A) | GAP | `Blocks/Policy.jet:112-121, 254-279`; compare each module's inferred row with the ceiling of its path, E0740 family |
| G3 | `trust.services` per-service prompts (D-JPK-GRANTCMD1=A) | GAP | `Blocks/Authority.jet:294-305` parses it; wire it into the `jetpack services up` start gate beside `TrustGate.jet:115-124` |
| G4 | `time.sleep` asks for `Time` | BUG vs D-AUTH-AMBIENT2 | classify waits as `Time.Wait`; probe P6 |
| G5 | Unknown leaves accepted | BUG vs D-EFF2 | `Blocks/Authority.jet` `pkg_parse_effect_list` checks roots only; probe P14 |
| G6 | E1803 fix after a CLI `--deny` says "adjust the denial in `authority.holds.deny`" | BUG | probe P3 extra; `Cli/Authority.jet:294` must name the layer that denied |

## 7. Owner ballots

| Id | Question | Recommendation |
|---|---|---|
| D-AUTH-AMBIENT3 | Does a written `allow` add to the beginner floor, or replace it? | A: add. A floor right leaves only through `deny`. |
| D-AUTHORITY-DENYROW1 | Is every package `deny` also a compile-time prohibition, as memory denials already are? | A: yes. One meaning for `deny`, reported by `jet check` with the call chain. |

Evidence for D-AUTH-AMBIENT3, from single-line allow rows in `.jet` files: 138 of 187 (74%) restate `IO` and `Mem.Alloc`.
A deny-only block such as `deny: [Exec]` silently removes printing today (probe p2).
Every E1803 fix must warn "list every effect the program needs" (`DiagnosticRows.jet:7404`).
Under option A, `jet fix` drops redundant floor entries, and the warning text is deleted.

Evidence for D-AUTHORITY-DENYROW1: `jet check p2.jet` prints "ok: `p2.jet` has no problems", while `jet run p2.jet` refuses it with E1803.
Sema reads only memory denials from the package (`Checks.jet:2316-2318`).
The E1803 gate already computes reachable effects before the run (`Pipeline.jet:4197-4220`), so checking a denial earlier adds no new analysis.
Migration for option A: `Checks.jet:2316-2318` widens from memory names to every denied right; E0749 gains package-deny provenance; `tests/ui` rows that expect a clean check under a package deny change.

## 8. Cutover order

1. Vocabulary (§4). Every later step reads the one table.
2. One Package parser (§2.3), which removes X2, X8, X10 and the labels.
3. Flatten `holds` (§3) with the one-shot `jet fix` rewrite over the sites list.
4. Delete X1, X4-X6, X11; fix G4-G6.
5. Wire G1 and G2; apply the ballot outcomes.

Each step is one landable patch and passes the chain, goldens and the authority rows in `tests/ui` and `tests/cases` on its own.
