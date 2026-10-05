# #3104: whole scripting-loop measurement (SCRIPT-F09)

Closer10, 2026-09-29. Current binary: `~/.cache/jet-dev/safe-jet.sh` →
`jet-debug-snapshot14` (debug build). Verdict: **BLOCKED**.

## Question

Can the three report-section-9 protocols (short CLI, embedded rule, data/service
job) be measured tonight with cold/warm/edit and prepare/first-call/repeated/
release phases separated, against matched peers, under the AGENTS.md per-cell
performance gate?

## Method and evidence

Environment identity, observed in `Tools/agent/jet-env`:

| Tool | Identity (observed) |
|---|---|
| jet (current) | `~/.cache/jet-dev/scratch/jet-debug-snapshot14`, a debug build (1.69 GB) |
| jet release | `target/release/jet`, 2026-09-28 11:15, sha256 prefix `3b8d8e9b09c891d4` |
| CPython | 3.14.7 (`/nix/store/b5bpi6…-python3-3.14.7`) |
| Node | v22.23.2 |
| Bun | 1.3.13 |
| Go | go 1.26.7 (`/nix/store/i77g9d…-go-1.26.7`) |
| rustc | 1.97.1 |
| rust-script, deno, hyperfine, perf | not on PATH |

Is the release binary current? No. It predates the syntax cutover
(`c0c5493a9`, 2026-09-29 00:47) and rejects the current short-CLI example:

```
$ JET=target/release/jet ~/.cache/jet-luna/safe-jet.sh run Examples/features/cli/typed_entry_args.jet
Error [E-ERR-SUFFIX]: `Error!` is a retired suffix failure contract.
  --> Examples/features/cli/typed_entry_args.jet:19:43
 19 |     #Doc("path to a config file") config: String?
exit=1
```

The debug binary runs the same example (see #3117 evidence: `--port 8080`
accepted, `--port abc` rejected at entry parse in 7.6 s wall). A debug
compiler's wall time is dominated by the unoptimized compiler. The gauntlet
harness refuses to treat that as representative (`run.mjs:3925-3933`, cited
in the prior card log).

The embedded-rule protocol cannot run on the current binary at all:

- `Examples/features/packages/sandbox_mathkit/run.jet` fails at check with
  E1257 on every tier (a stale interface snapshot);
- `library_loadable/host.jet` hits an internal compiler error on every tier.

Both are recorded in `3114-embedding-two-adapter-delivery.md`. So neither the
prepare/first-call/repeated/release phases nor a peer comparison can be
measured.

## Criteria

1. Phase separation: not measurable. There is no release jet for the current
   syntax, and the embedded-rule route is broken on every tier.
2. Equivalent data and explicit installation/cache state: the environment
   table above is recorded, but there are no matched runs.
3. Raw samples and digests: none taken, because a debug-compiler sample would
   be a mismatched cell under the performance gate.
4. Split with SCRIPT-F27: confirmed. #3117 owns the verdict loop; this card
   owns execution/setup/prepare-call performance.

## Change since the prior log

Go is now installed in jet-env (go 1.26.7). The prior BLOCKED log said it was
missing, so that part of the blocker is gone.

## Unblock conditions

- A release `jet` built from the post-cutover tree (commit and sha recorded).
- `sandbox_mathkit` and `library_loadable/host.jet` pass on all tiers.
- Then run the plan's 20-samples-per-cell protocol.
