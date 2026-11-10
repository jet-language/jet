# D-FFI-EFFECTS1 review: named foreign libraries and declared rows on C bindings

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-FFI-EFFECTS1. Earlier pass: BallotFix REPORT.md, section D-FFI-EFFECTS1.

## Question

Should a foreign effect name its library, as `FFI.C("sqlite3")`? And may a C binding declare the effects each
of its functions uses?

## Findings that changed the ballot

1. **The every-root allow refusal was a defect, and it is fixed.** The ballot's baseline came from t6c, where
   probes native and named stopped with E0749 listing every root. Commit `d80fbffd3b` (2026-11-10,
   "#6281 c146 fix: a foreign call is held to the package `allow` row by the effects it has") states the
   rule Main set:
   1. An undescribed C call has the effect `FFI.C`, so a row that lists it checks clean.
   2. A binding with a descriptor is held to its declared effects.
   3. `FFI.C` never covers Jet-side effects.

   On a0.0.43, native (`allow: [FFI, IO, Mem.Alloc]`) and named (`allow: [FFI.C, IO, Mem.Alloc]`) both print
   `native-c-ok`. The situation, the story, B's technical text, the rec's why and the whyNot entries
   for B and C all claimed "an allow-listed package cannot call C". They are rewritten.
2. **The every-effect reach survives in two places.** The commit says so ("Function `-[…]>` ceilings (E0740)
   and `deny` rows (E0749) still treat an undescribed C call as reaching every effect"), and two new probes
   show it:
   - **ceiling**: `flip: fn(n: I32) -[IO]> I32 { c.negate(n) }` gets E0740, "`flip` uses the effect
     `Browser, DB, Env, Exec, FFI, FFI.C, FS, GPU, Log, Meta, Net, Rand, Secret, Time`".
   - **deny**: `allow: [FFI.C, IO, Mem.Alloc], deny: [FS]` with a call to `negate` gets E0749, "`run` reaches
     the `FS` effect".

   This is now the motivation for A's declared rows, and for D's best case.
3. **Both prerequisites are still unbuilt.** The declared row on an `#Import` function is E0003, "Expected `=`
   before the C symbol, found `-`" (probe declared, 6:28). `allow: [FFI.C("nativeffi")]` is E1221, "names a
   right without an argument" (probe bugs/e1221_named_ffi).
4. **Census, rerun on dev c460ba91e5** (Core, Compiler, Jetpack, Tools, Examples, tests):
   - 121 `#Import c.` blocks in 120 files (the earlier count was 119 in 118).
   - 58 `c@` dependencies in 56 files.
   - 1,088 allow lines naming FFI, in 1,085 files.
   - 303 deny rows in 118 files.
5. **Peer correction.** Deno's per-library form is real: `--allow-ffi[=<PATH>...]` and `--deny-ffi[=<PATH>...]`.
   Deno also warns that a loaded library "can read files, open sockets, or do anything the operating system
   lets the process do, regardless of which `--allow-*` flags you passed". That is the same limit as A's
   in-program claim, and the comparison note now says so.

## Probes (a0.0.43)

| Probe | Command | Result |
|---|---|---|
| native | `jet run .` / `jet inspect rights run.jet` | `native-c-ok`; rights lists granted `Exec.Args, FFI, IO, Mem.Alloc`, no error |
| named | `jet run .` | `native-c-ok` |
| ceiling (new) | `jet run .` | E0740 at 9:20, every root |
| deny (new) | `jet run .` | E0749 at 9:1, FS |
| declared | `jet run .` | E0003 at 6:28 |
| bugs/e1221_named_ffi | `jet check .` | E1221 |

## Peers (sourced)

- Deno: `--allow-ffi[=<PATH>...]` and `--deny-ffi`. <https://docs.deno.com/runtime/reference/permissions/>.
  Native code escapes the sandbox. <https://docs.deno.com/runtime/fundamentals/security/>
- Java (JEP 454): `--enable-native-access=M` grants native access per module, and other callers get
  `IllegalCallerException`. <https://openjdk.org/jeps/454>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| An allow-listed package cannot call C | Rerun on a0.0.43 | **Failed**: fixed by d80fbffd3b. Rewritten |
| Declared rows matter | Does anything still see every effect? | Holds: function rows and deny rows do (ceiling, deny) |
| A changes no meaning | Do existing rows change? | Holds: bare FFI and FFI.C keep covering every library, and unwritten functions keep the whole row |
| Rows are trusted claims | Compare with peers | Holds: Deno documents the same limit. `.Sandbox` enforces |
| Callback hiding | A C function takes a Jet callback | Holds: the call adds the callback's row |

## Remaining losses

- An in-program written row is a claim, enforced only under `.Sandbox`.
- Grants gain one line per library, and `jet add` writes it.

## Different-family review (GPT, 2026-11-10)

| Claim | Attack and independently checked evidence | Verdict |
|---|---|---|
| Broad allow still refuses C | Reran native/named: both print native-c-ok. Stale comments and the reading surface still said E0749. | Fails; all three corrected. |
| Narrow ceilings and denials remain conservative | Reran ceiling E0740, deny E0749, declared arrow E0003 and named effect E1221. | Holds on a0.0.43; no implementation of A proved. |
| An empty declared row makes ceiling's IO-only function pass | A retains FFI.C(lib), so IO alone is insufficient even for pure C. | Fails; removed the promise. |
| A library is identified by a bare local key | Two packages can use sqlite3 for unrelated dependencies, or two aliases can name one dependency. | Fails; resolve keys to locked dependency identity before coverage. |
| Every new library needs a separate grant | Bare FFI.C deliberately covers all. | Fails; only narrow grants distinguish additions. |
| A row can be trusted just by writing it | Read ratified D-FFI-CONTRACT1=A: evidence binds to actual artifacts and reachable native edges; native trust requires its audited boundary. | Fails; one canonical descriptor carries evidence, not a second self-assertion path. |
| Sandbox always enforces rows; process means protection | Read D-FFI-PLACE1 and Deno's native-code warning. Mediation of every import/resource is needed; process isolation alone does not restrict OS authority. | Holds only within stated threat model and supported operations; no fallback to native placement. |
| Callback row union solves callbacks | Retained callbacks run later and may enter from foreign threads; cleanup also has effects. | Fails as a complete guarantee; descriptor must cover lifetime, thread entry, later authority and cleanup. |

Read ratified D-EFFECT-ARGS1, D-DECL-TYPES1 and D-EFFTYPE-GRANT1 directly. Deno security and JEP 454 were read independently. The earlier numeric census is not independently remeasured and is not used to rank options.

Agree with repaired A. No new option was needed: B, C and D already separate the two axes fairly. Do not post stale E0749 allow examples, a claim that IO alone admits foreign calls, or proof-by-row and unconditional sandbox-enforcement claims.
