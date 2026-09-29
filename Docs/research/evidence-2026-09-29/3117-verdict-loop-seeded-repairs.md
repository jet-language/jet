# #3117: verdict-loop quantities on seeded script repairs (SCRIPT-F27)

Closer10, 2026-09-29. Binary: `~/.cache/jet-luna/safe-jet.sh` →
`jet-debug-snapshot14`, a debug build, so the latencies below include an
unoptimized compiler and are not performance claims. Verdict: **PARTIAL**.
Criterion 1 is met; criterion 2 is met for this agent's own repair session
only; criterion 3 is blocked.

## Method

- Three seeded defects run on the current binary. Scratch:
  `~/.cache/jet-test-scratch/Closer10/{f27,stream,mk}`.
- For each defect the following is recorded:
  - the rejection stage and whether any side effect happened;
  - the diagnostic text an agent sees (fidelity and actionability);
  - the edit/check/repair steps and the context read (context economy);
  - the accepted revision and whether the repair is deterministic.
- The agent doing the repair is this worker, Closer10. It is one session,
  not an independent sample.

## Seed 1: typed CLI argument (`Examples/features/cli/typed_entry_args.jet`, copied unchanged)

| Quantity | Jet (observed) | Peer: CPython 3.14.7 argparse (`f27/peer_cli.py`, same three fields) |
|---|---|---|
| Rejection stage | Entry parse, before the user body. `run … -- --port abc` gives `` `--port` expects an Int, got `abc` `` / `fix: pass a whole number to --port`, exit 2 | Argv parse. `--port abc` gives `usage: …` / `error: argument --port: invalid int value: 'abc'`, exit 2 |
| Side effects before rejection | none: no `print` output | none |
| Env-var source (`PORT=abc`) | `invalid int for --port`, exit 2. It has no `fix:` line and names the flag, not the env var `PORT`. | Uncaught `ValueError` traceback, exit 1: the stock peer does not type-check env defaults |
| Latency (wall, including compile) | 7.64 s | 0.1 s |
| Repair | 1 edit (the argument value) and 1 rerun. The diagnostic alone was enough; no source was read. | 1 edit and 1 rerun |
| Accepted revision | `-- --port 8080` prints `8080` / `false` / `(none)`, exit 0 | `8080` / `false` / `(none)`, exit 0 |
| Determinism | same text on each run | same |

Finding: the env-sourced failure is less actionable than the argv failure.
The two paths through the same `#CLI` field produce different message
shapes.

## Seed 2: malformed external record

The seed is `~/.cache/jet-luna/stream-probe/malformed.csv`: 15.5 MB with an
unterminated quote at row 200,001 (generator `gen.py`). The program is
`stream/csv_stream.jet`, which uses the file-backed `csv.reader(^input, EncodingLimits.safe())`.

| Quantity | Jet (observed) | Peer: CPython 3.14.7 `csv` streaming (`stream/peer.py hostile`) |
|---|---|---|
| Rejection stage | Compile, not data. AOT and default `jet run` reject the generated Rust for `csv.reader(^input, limits)` (ICE, exit 101) before any row is read. The interpreter run did not finish within this run. | No rejection: the unterminated quote is swallowed to EOF (`stopped after 200001 rows with 200 matches`), exit 0 |
| Side effects | none (nothing executes) | none, but a silently wrong result |
| Actionability | An ICE: "This is a bug in jet, NOT in your program" plus a rustc log path. A script author cannot act on it. | none: no diagnostic |
| Accepted revision | not reached | n/a |

Fidelity note: the peer's default is to accept the malformed record. Jet
could not be observed rejecting it with a typed `EncodingError` on any tier
in this run.

## Seed 3: sandbox guest failure (`Examples/features/packages/sandbox_mathkit/run.jet`)

This seed was not injected. The committed example already fails, which makes
it a real defect.

| Quantity | Jet (observed) |
|---|---|
| Rejection stage | Check, before any guest or host execution: `E1257 Plugin interface plugin__mathkit has no export is_enabled` (run.jet:20) and the same for `greet` (:22), on default, `--interpret` and `--release` |
| Side effects | none; nothing executes |
| Actionability | The fix text says "Use a registered export name or update the plugin interface snapshot". It does not name the snapshot path (`.jet/cache/api/plugin__mathkit.api`, staged from `fixture-state/`) or a command that regenerates it. |
| Context read to repair | 5 files: run.jet, the golden harness staging code (`tests/golden.rs:113-134`), `fixture-state/cache/api/plugin__mathkit.api`, a wasm export-string scan, and `sandbox_src/run.jet` |
| Edit | 2 lines added to the `.api` snapshot (`fn greet(name: String) String`, `fn is_enabled(flag: Bool) Bool`) |
| Accepted revision | Partly accepted. With the 2-line snapshot fix, the check passes. On `--interpret` (with an inline package authority block, because `--interpret` rejects `--allow`, E2102) it prints `scale(6, 7) = 42.0`, `hypot(3, 4) = 5.0`, `gcd(48, 18) = 6`, then `is_enabled` fails at runtime with `user-error:plugin call is_enabled: no exported function has this name` (the guest exports `is-enabled`). `greet(Ada) = hello, Ada!` works. So the snapshot repair moves the failure from check to runtime, and a second, host-side name-mapping defect remains. Default and `--release` fail earlier with E0704: the host build compiles `crates/jet-foundation` from the live checkout, and its build script panics because `CoreModuleExports.rs` is stale against `Prelude/Core.jet` (concurrent tree state). No run reached the golden. |

## Criteria

1. **Met.** All three seeds record the stage and the side effects.
2. **Met for one repair session.** The edit, check and repair steps, the
   context read and the accepted revision are recorded above; this is not
   token counts. It is not an independent agent-eval run: the plan's
   `Tools/agent-eval/run-cold-context.mjs` was not run, and no `tasks.json`
   was authored.
3. **Blocked.** No tool-assisted peer set is pinned in the repository. The
   peer cells above are plain stock CPython argparse/csv at a recorded
   version, which is not the plan's "tool-assisted" peers (mypy/typer,
   zod). No superiority claim is made; the peer latency is lower because
   Jet's includes a debug compile.
