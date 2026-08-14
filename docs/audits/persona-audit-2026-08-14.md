# Persona audit — 2026-08-14

**Card:** #1924

**Status:** first-session lens is now part of the reusable persona-audit workflow; the `#820-#825` backend chain is incomplete, so runtime timing remains unproven in this pass.

**Method:** Four fresh personas cover beginner, experienced CLI developer, graphics beginner, and unattended coding agent. This pass reads the current skill, report corpus, and truthfulness guard. Builds, tests, linters, formatters, and devtools were skipped by instruction, so this report records no invented elapsed time, window, or pixel evidence.

## Personas

### P1 Mara — first-time programmer · small command-line tool

Core loop: read the first example, write one `fn run`, run `jet check`, repair the first diagnostic, then run the file.

- **Pull:** `print`, `input`, plain diagnostics, and `NO_COLOR` describe a low-ceremony start.
- **Push:** the first command's elapsed time is unproven. A readable diagnostic cannot prove first-session speed.
- **Verdict:** usable-with-friction. Static evidence describes the loop; execution evidence is intentionally absent.
- **Reaction:** not collected; this pass did not run the persona loop.

### P2 Devon — TypeScript developer · subcommand CLI

Core loop: declare commands, ask for help, run a valid subcommand, inspect JSON on failure, and repeat until the tool works.

- **Pull:** one CLI surface, `--help`, `--json`, and named `Why` and `Fix` fields make the repair path discoverable.
- **Push:** stale help, unresolved machine paths, and missing machine edits can add repair turns. Cards #1901, #1873, and #1877 own those gaps.
- **Verdict:** usable-with-friction. The surface is legible; no live latency or repair count was measured.
- **Reaction:** not collected; this pass did not run the persona loop.

### P3 Inez — graphics beginner · first window

Core loop: create a first-party window, draw one pixel, change its color, and rerun after each edit.

- **Pull:** the lens names the joy event precisely: a usable window followed by a visible first pixel.
- **Push:** the `#820-#825` backend chain is incomplete. This pass exercised no window creation or frame receipt, so the graphics loop cannot claim completion.
- **Verdict:** blocked. First-session evidence is not-proven.
- **Reaction:** not collected; this pass did not run the persona loop.

### P4 Luna — unattended coding agent · small diagnostic migration

Core loop: read repository context, edit one file, run the checker, read the structured verdict, apply one repair, repeat, and stop when clean.

- **Pull:** JSON Lines, source spans, `Why`, `Fix`, and stable report fields can make the loop mechanical.
- **Push:** this pass did not run the checker. Verdict fidelity, latency, and repair determinism remain unproven; machine path and edit gaps remain carded by #1873 and #1877.
- **Verdict:** blocked for this audit run. The unattended loop needs a real checker run.
- **Reaction:** not collected; this pass did not run the persona loop.

## First-session delight lens

Record both checks for every persona. `not-applicable`, `not-proven`, and `blocked` are different states. No value below is a zero or an inferred pixel.

The report gates window measurements on the complete `#820-#825` backend chain:
`#820` → `#821` → `#822` → `#823` → `#824` → `#825`. The current `core.game`
default is headless/no-op. This pass had no windowed run, backend/input receipt,
or frame receipt.

| persona | time-to-first-window | first-pixel | state |
| --- | --- | --- | --- |
| Mara | not-applicable — CLI project has no window target | not-applicable — no window is created | usable-with-friction |
| Devon | not-applicable — CLI project has no window target | not-applicable — no window is created | usable-with-friction |
| Inez | not-proven — window target exists, but the `#820-#825` backend chain is incomplete; no milliseconds/backend/input receipt | not-proven — no frame evidence because no windowed run exists | blocked |
| Luna | not-applicable — checker project has no window target | not-applicable — no window is created | blocked |

Required evidence after the `#820-#825` backend chain is complete:

1. Start the clock before the first project command.
2. Record milliseconds, backend, size, and input for `time-to-first-window`.
3. Record milliseconds from window creation, backend, and a frame receipt for `first-pixel`.
4. Repeat after one edit so the result is not a one-off startup artifact.

## Agent-optimality read

| quantity | current read |
| --- | --- |
| Verdict fidelity | Typed diagnostics and structured report fields are shipped; this pass did not run the checker. |
| Verdict latency | No timing claim; the required edit-to-verdict measurement remains unproven. |
| Verdict actionability | `Why`, `Fix`, spans, and JSON fields help; machine edits and path resolution remain gaps. |
| Context economy | JSON Lines and one report schema reduce parsing work; skipped execution leaves no token-per-progress measurement. |
| Repair determinism | The intended loop is one report to one obvious edit; no live repair count was observed. |

## Four questions

1. **How does Jet win?** Shipped safety and one typed diagnostic path can give beginners and agents the same meaning. The first-window advantage is a ratified product goal, not shipped runtime proof.
2. **What does Jet avoid?** It avoids treating missing timing as zero, treating a window handle as a pixel, or calling a static read a completed agent loop. Exposure remains until the backend and live evidence exist.
3. **What does this say about AI-driven development?** An agent needs a fast, stable, structured verdict. Without a real checker invocation, fidelity, latency, actionability, context economy, and repair determinism cannot all be claimed.
4. **What concrete surfaces must Jet cover?**
   - **Covered with proof:** `jet check` surface definitions, human diagnostics, `--json`, `NO_COLOR`, report fields, and the skill rows `time-to-first-window` and `first-pixel`.
   - **Worth checking:** `jet ?`, `--help`, PTY order, wide-character spans, first-window input shape, backend identity, and frame receipt format.
   - **Missing:** measured first-window milliseconds, measured first-pixel milliseconds, backend/input receipts, and frame evidence from a real windowed run.

## Micro sweep

| area | current read |
| --- | --- |
| Syntax | `fn run` and CLI declarations are visible in the proposed first loops. |
| Ergonomics | The obvious edit-check-repair path is named; first-session timing is not measured. |
| Surfaces | Human, JSON, help, and interactive terminal paths differ; the new lens gives reports one shared first-session surface. |
| APIs, types, and methods | `--json`, `--color`, report fields, `time-to-first-window`, and `first-pixel` are named; a window API is not shipped here. |
| Defaults | Plain output and honest non-result states are safer defaults than invented timing. |
| Naming | `time-to-first-window` and `first-pixel` name separate events and start points. |
| Error text and diagnostics | `Why` and `Fix` are readable; machine edits and paths still need proof. |
| UX and DX | The later project loop and first-session delight loop are now separate; runtime feel remains unobserved. |
| Tooling and CLI shape | Help and JSON exist; interactive redraw and window/frame receipts need live checks. |
| Ceremony versus control | Beginners get a short path; experts get explicit backend, input, and evidence fields when the measurement exists. |

## Strongest unverified assumption

Jet may feel good in the first session once a windowed backend lands, but source shape and report design cannot prove the time to a usable first-party window or visible first pixel.
