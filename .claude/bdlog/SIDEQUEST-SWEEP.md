# Sidequest burndown sweep ledger — 2026-08-26 overnight

One ledger for every deferred proof, defect, and owner gate from the overnight
sidequest burndown. Worked as a single sweep before morning handoff.

## Deferred proof

| Card | Claim | Proof command | Status |
| --- | --- | --- | --- |
| #2211 | Hangar admission fixes (Envelope + store) | `cargo test -p jet-pkg-model Envelope` and `cargo test -p jetpack --lib Store::NixCache Store::Ingest::share_tests` | RUN GREEN (pre-close) |
| #2211/#2232 | Real `jetpack env --prep -y` exits 0 | run on this machine from repo root | pending lane C |
| #2226 | explain E1803 golden | targeted snapshot/golden suite for diagnostics | pending sweep |
| #2229 | scaffold-then-run flow test | targeted test named by worker | pending sweep |
| #2230 | web button measurement | `cargo test --test web_build` (worker already drove real Chromium: height 36px) | pending sweep |
| #2231 | copy_dir tier parity | default + release run of site/generate.jet (worker ran both green); targeted evaluator test | pending sweep |
| #2170 | Canvas unify slice (criterion 7) | tests/canvas.rs concurrent regression test | pending sweep |

## Defects found

| Where | Defect | Disposition |
| --- | --- | --- |
| lane-dispatch.mjs | could not brief sidequest (epoch-less) cards | fixed in-session (LANE_EPOCH=all); papercut logged |
| jetpack env | hangar lock timeout cascade under long admissions (`timed out waiting for jetpack lock .../hangar.lock`) | lane C diagnosing |
| jetpack env | E1350 detail swallowed ("not a valid canonical NAR" hides io error) | lane C fixing (error transparency) |

## Owner gates

| Card | Gate |
| --- | --- |
| #2170 | criterion 11: owner visual acceptance of unified Canvas session |
| #2219 | ballot required before sealed-manifest implementation (posture chosen in interview; measurements pending) |
| #2233 | decide-lane card, owner-only; untouched |
| #2220 | TUI look-and-feel likely wants owner visual pass after implementation |
