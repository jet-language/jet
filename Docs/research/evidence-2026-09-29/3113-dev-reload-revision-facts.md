# #3113 — dev reload: failed-edit revision facts (criteria 3–4)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`, built 2026-09-29 00:47), tree HEAD `e9c708fa7`. Run via `~/.cache/jet-luna/safe-jet.sh`.

## Question
While `jet dev` holds a rejected edit, do the machine-readable diagnostics and the displayed active revision agree? Criterion 4 says `/__jet_dev_status` should carry `diagnostic_revision` equal to the rejected candidate, and `last_good_revision`/`accepted_revision` equal to what `/__jet_dev_version` shows.

## Method
1. The gauntlet app (`Tools/gauntlet/axes/live-reload/jet/run.jet`) cannot start on snapshot14. `jet dev --json` prints one `jet.status/v1` `diagnostics ok=false` record with two E0769 errors from the embedded Core: `` `viewport` is a label-only parameter of `preview` `` at `<corelib>/Core/ui/ui.jet:104` and the same error for `playground` at `:109`. The tree `Core/ui/ui.jet:104,109` now passes `viewport: viewport`, so a later snapshot should get past this. Reproduced with `~/.cache/jet-test-scratch/Closer07/devreload/drive.py`: connection refused for 240 s.
2. As a replacement I used a web app that does not use core.ui: `~/.cache/jet-test-scratch/Closer07/devreload/app2/{run.jet,package.jet,index.html}`. It has the gauntlet package/HTML and `#[Target(Web), HTML(Path{"index.html"})] fn run() { print("reload-before") }`.
3. Drivers:
   - `drive2.py` saves in place (truncate+write, like `open(...,"w")`).
   - `drive3.py` saves atomically (write a temp file, then `os.replace`).
   Each driver starts `safe-jet.sh dev --watch=on --json --target=web --port=P run.jet`, then makes a good edit (`reload-before` → `reload-after`), then an invalid edit (`print("reload-after" +)`), then a recovery edit. It polls `/__jet_dev_version`, `/__jet_dev_status` and `/app.js`.

## Evidence
### Atomic saves (`drive3.py`, 1 run)
| moment | `/__jet_dev_version` | `/__jet_dev_status` |
|---|---|---|
| ready (142 s cold) | 1 | `{"version":1,"state":"ready",…,"code":"","diagnostic":"","clients":0,"last_build_ms":26857}` |
| good edit, +0.0 s | 1 | `state":"building"` |
| good edit, +12.4 s | 2 | `state":"ready"`, `last_build_ms":12339` |
| invalid edit, +5 … +30 s (6 polls) | 2 | `{"version":2,"state":"error","message":"error · E0003 · 0 clients","file":"run.jet","code":"E0003","diagnostic":"Error [E0003]: Expected a value, found `)` --> run.jet:3:27 …"}` |
| recovery edit, +11 s | 3 | `state":"ready"` |

`jet dev --json` stdout contained only `App preview: http://127.0.0.1:35199/`. It emitted no `jet.status/v1` or `jet.report/v3` record for any watch-cycle build. The E0003 diagnostic went to stderr as human text (`jet dev  [error] E0003 · 0 clients` plus the rendered report).

### In-place saves (`drive2.py`, 2 runs)
- Both runs: after the good edit, the version stayed `1` and status stayed `ready` for the whole 180 s window. In run 2, stderr shows no `[building]` line between ready and the invalid edit. The good in-place save was never picked up.
- Run 1 stderr (`dev2.run1.stderr`) shows `[error] E0003` at `run.jet:1:1` with an empty line 1: a build read a truncated (empty) file. A second error then followed at `3:27`.
- Both runs: the invalid in-place edit was picked up (state `error`, code `E0003`, version still `1`).

## Verdict
Criterion 1–2 facts are re-observed on the current binary with atomic saves: the rejected edit keeps the last-good revision (2), does not bump the version, and recovery moves to 3.

Criterion 3 is NOT met:
- `/__jet_dev_status` (`crates/jet-devserver/src/WebHost.rs:1491-1500`, `DevStatus::json`) carries no revision identity: no candidate, `diagnostic_revision`, `accepted_revision` or `last_good_revision`. A machine consumer cannot tell that the E0003 belongs to a rejected candidate rather than to revision 2. The Session fields exist (`Session.rs:401-407`, `mark_error` sets `diagnostic_revision` at `:818`), but this route does not project them.
- In `--json` mode, watch-cycle diagnostics are not emitted as machine records at all; only the startup failure is.

Criterion 4 is NOT met: the named fields are absent from the route, and `cargo test -p jet-devserver failed_edit_revision_facts_agree` does not exist in the tree (grep over `crates/`, `tests/`: no match).

## Defects
1. `/__jet_dev_status` omits revision identity during a failed edit (see above). Expected: `diagnostic_revision` = the rejected candidate, and `last_good_revision` = `accepted_revision` = `/__jet_dev_version`.
2. `jet dev --json` emits no machine record for watch-cycle build failures or successes (stdout holds only `App preview:`).
3. In-place saves: 2 of 2 runs lost the first good in-place save after ready (no rebuild in 180 s), and 1 run compiled a truncated empty file (E0003 at 1:1). An atomic save was picked up in 12.4 s (1 of 1). Repro: `python3 ~/.cache/jet-test-scratch/Closer07/devreload/drive2.py` vs `drive3.py`.
4. Snapshot14 embedded Core rejects `core.ui` apps (E0769 at `<corelib>/Core/ui/ui.jet:104,109`). This looks fixed in the tree, but it is unverified until a new snapshot exists.

## Rerun vs inspect
From this run:
- A good save reruns the build: a new version and a new `app.js`.
- A failed save reruns only the compile: the version is unchanged and the last-good artifact keeps being served.
- `/__jet_dev_status` and `/__jet_dev_version` only inspect; they do not rerun anything.

The counted side effect in the served web entry runs in the browser. No browser was attached (`clients: 0`), so no entry-code rerun count was observed.
