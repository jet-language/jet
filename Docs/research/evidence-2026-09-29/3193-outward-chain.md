# #3193 — BIG-F11 outward-operation contract across semantic boundaries

Closer09, 2026-09-29. Binary via `~/.cache/jet-luna/safe-jet.sh`
(jet-current snapshot14 for v0–v5 `check`; snapshot16 for the rest). Source
head `e9c708fa7`. Fixtures: `~/.cache/jet-test-scratch/Closer09/effects/`
(v0–v5, copied from the overnight ProveOther run with its
`PREDECLARED.md`) and `~/.cache/jet-test-scratch/Closer09/free/` (v1b, v4b,
v6, v7, `outward_chain.jet`).

## Rules the verdicts rest on

- `Docs/spec/spec.md` "Function and block ceilings" (:1385-1400): a function
  may omit its row and sema infers the complete transitive row; `-[]>` is an
  empty upper bound, any effect in the body is a purity violation; an omitted
  effect is E0740 naming the introducing call.
- `syntax-decisions.md` S60 (:2634): `-[]>` is the checked purity signature;
  violations name the impure call path.
- `spec.md` "Higher-order and trait effects" (:1420-1456): an unknown function
  value contributes the maximal set.
- Failures must be handled or declared by each caller (E2402/E2404 family);
  `&` marks write-access arguments (E0202).

## Predeclared verdicts vs observed (`jet check`)

| variant | change | predeclared | observed | agree |
|---|---|---|---|---|
| v0_base | `shout = s.upper()`; `announce -[IO]>`, `pure_user -[]>` | valid | ok | yes |
| v1_pure_subst | `shout = "{s}!".upper()` | valid | E3401 `pure_user calls the impure function shout` (10:5) | **no** |
| v1b | same body with `shout -[]>` declared | shout accepted; pure_user valid | shout itself accepted, **pure_user E3401** (20:5); unannotated copy E3401; `s.upper()` copy accepted; `(s + "!")` copy E0109 | **no** |
| v2_outward_op | shout gains `print` | pure_user E3401; announce valid | pure_user E3401 (11:5); announce valid | yes |
| v3_failure | shout gains `ShoutError!` | both callers rejected | E2402 at announce (12:11) and pure_user (16:5) | yes |
| v4_state_alias | `&log` write in shout | only stale_caller (missing `&`) rejected | E0202 stale_caller (15:11) **and** E3401 pure_user (11:5) | **no** |
| v4b | `record_direct(&log) -[]>` writes directly; `pure_via -[]>` calls unannotated `record` with the same body | both valid | record_direct accepted; **pure_via E3401** (14:5) | **no** |
| v5_foreign | shout calls `extern rust "base64@0.22"` `b64encode` | pure_user rejected; foreign body trusted, not checked | announce E0740 `uses the effect Browser, DB, Env, Exec, FFI, FS, GPU, Log, Net, Rand, Secret, Time`; pure_user E3401; E1220 budget x5 on shout | partly (see criterion 3) |
| v6_transitive | print two calls below `pure_user`; `relay_net -[Net]>` | relay_net E0740 IO; pure_user E3401 | relay_net E0740 `uses the effect IO` (16:27); pure_user E3401 names `shout` at the `relay(s)` call (21:5) | yes |
| v7_task | task body prints; a second task body is pure | (no prediction written) | announce `-[IO]>` E0740 `Time.Wait` (task join); pure_user and pure_quiet E3401 | recorded |

v1/v1b and v4/v4b are direct-vs-transitive disagreements: the same body is
accepted when it carries `-[]>` itself, but a `-[]>` caller of it (or of an
unannotated copy) is rejected. In v1b the callee is *declared* `-[]>` and
accepted, yet its caller is told it is impure.

## Tiers for the valid chain (`free/outward_chain.jet`)

`run3.sh outward_chain.jet check,run,interp,aot`:

| tier | result |
|---|---|
| `jet check` | ok, no problems |
| default `jet run` | `HI` `YO` `OK` `(empty)` `A` `B` `[a, b]` |
| AOT | identical to default run |
| `--interpret` | exit 1, `E0956 core.builtin.upper() isn't supported by the current evaluator yet` (13:7) |

The interpreter gap is the generic "report as a compiler bug" E0956, not a
ratified unsupported-tier diagnostic.

## Criterion 3 statement (concurrency and foreign edges)

- Task edge (v7): checked. Spawning and joining a task reaches `Time.Wait`
  even when the task body is pure; `-[IO]>` alone rejects it (E0740) and
  `-[]>` rejects it (E3401). The task body's own effects (print → IO) are
  charged to the spawning function.
- Foreign edge (v5, `extern rust`): the parameter and return types are the
  declared Jet types (checked at the call); the foreign body's effects are
  unknown and charged as the maximal set (every effect root in E0740 and
  E1220). No fact labels the edge "trusted" or "foreign"; the maximal set is
  the only signal, and E3401 does not name which effect made `shout` impure.
- Generated `#Import module` wrapper: not exercised (the only example,
  `Examples/features/foundations/bridges`, needs its native adapter build).

## Verdict

- Criterion 1: met. Every variant has a predeclared outcome tied to the rules
  above (v0–v5 from `effects/PREDECLARED.md`, written before the overnight run;
  v1b/v4b/v6 from their file headers); disagreements are defects below.
- Criterion 2: not met. Direct and transitive callers disagree (v1b, v4b).
  Body changes do propagate (v2, v3: fresh process per run, no reused summary).
- Criterion 3: met as a statement of checked vs unknown facts (above); the
  generated-wrapper edge was not exercised.
- Criterion 4: not met. check/default/AOT agree on the valid chain; the
  interpreter fails with an unratified E0956.
- Criterion 5: met; presentation issues (E3401 not naming the effect or the
  full path) belong to BIG-F04; no summary API added.
- Criterion 6: not met; no fixtures blessed (the snapshots would pin the
  disagreeing verdicts).

## Defects

1. Declared-pure callee reported impure: `free/v1b_interp_annotated.jet`,
   `fn shout(s: String) -[]> String { "{s}!".upper() }` is accepted, but
   `fn pure_user(s: String) -[]> String { shout(s) }` → E3401. Same for an
   unannotated copy (v1). Expected: accepted.
2. Parameter write accepted directly, rejected transitively:
   `free/v4b_param_write.jet`, `record_direct(&log) -[]>` with `&log.push(s)`
   accepted; `pure_via(&log) -[]>` calling unannotated `record` with the same
   body → E3401. Expected: one verdict for both.
3. Interpreter: `core.builtin.upper()` unsupported (E0956) in
   `free/outward_chain.jet` 13:7 while default run and AOT run it.
