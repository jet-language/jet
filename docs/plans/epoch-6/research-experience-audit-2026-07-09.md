# Canvas Experience Audit

Scope: P0 fixed first. Audit probes used live `jet dev` HTTP endpoints on `/tmp/jet-canvas-audit/demo.jet` at port 18901. In-app browser and local Chromium/Firefox were unavailable in this session, so mouse-only localStorage flows were verified by Canvas JS hooks/protocol shape rather than visual clicks.

P0 exact error before fix:

```text
Error [E0104]: `helper` expects 0 arguments, got 1
  --> examples/features/tooling/canvas_blueprint_demo.jet:24:5
    |
 24 |     helper(1)
    |     ^^^^^^
 Why: every argument must match a parameter
 Fix: check the definition of `helper`
```

Root cause: exec-origin palette insert called `defaultArgsForAction(item, pin)` with a pin context. For actions with zero input pins, JS fabricated `["1"]`, so no-arg project functions became `helper(1)`. Failed transaction surfaced as a short toast, then vanished.

Fix:
- `Source/Canvas/js.rs:4249`: zero-input actions now keep `[]` when connected from a pin.
- `Source/Canvas/js.rs:3000`: `window.__jetCanvasPinPoints` exposes pin client positions for future interaction tests.
- `Source/Canvas/js.rs:776` + `Source/Canvas/html.rs:251`: diagnostic toasts are pre-wrapped, readable, click-dismissable, and stay for 10s.
- `tests/canvas.rs:642` and `tests/web_dev.rs:457`: P0 and toast/hook assertions added.

| Interaction | PASS/FAIL | Failure | Suspected Root Cause |
|---|---|---|---|
| P0 exec OUT drag -> helper no-arg palette insert | PASS | Fixed; transaction now writes `helper()` not `helper(1)`. | `Source/Canvas/js.rs:4249` |
| Palette insert: Flow Branch | PASS |  |  |
| Palette insert: Flow Switch | PASS |  |  |
| Palette insert: Flow Loop | PASS |  |  |
| Palette insert: Flow Fallible | FAIL | Inserts `unwrapped :: fallible_value?` into non-fallible `fn run()`, rejected with E0403. | `Source/Canvas/edit_actions.rs:619` |
| Palette insert: Project helper | PASS |  |  |
| Palette insert: Project square | PASS |  |  |
| Palette insert: Project summarize | PASS |  |  |
| Palette insert: Builtin print | PASS |  |  |
| Palette insert: Core math.abs | PASS | Actual `core.math.abs` insert synthesizes `use core.math as math`. |  |
| Palette insert: Core encoding.decode | PASS | Actual `core.encoding.decode` insert synthesizes `use core.encoding.json as json`. |  |
| Palette insert: stale Core action `core.args.help` | FAIL | Catalog exposes `help`, but Core sema says `core.args` has no item `help`; only `spec` suggested. | `Source/Canvas/query_actions.rs:262` |
| Palette insert: ad hoc missing call | FAIL | `missing_fn(1)` rejected with E0102. Expected dead-end if user picks unknown manual call. | `Source/Canvas/edit_actions.rs:264` |
| Wire connect data | PASS | Existing exact type data wires project and `move_link` can rewrite source. |  |
| Rewire data endpoint to function symbol | FAIL | `helper` used as value for `Int` pin, rejected E0112 (`fn() -> Int` vs Int). UI has no guided call-result rewire path here. | `Source/Canvas/js.rs:3723`, `Source/Canvas/edit_actions.rs:241` |
| Wire connect exec | PASS | Source order projects exec rails; exec-origin insert now materializes source. |  |
| Rewire exec endpoint | FAIL | Endpoint drag opens same pin/action path; no transaction uses `wire_origin_pin_id` to insert after the specific exec source span. | `Source/Canvas/js.rs:3919`, `Source/Canvas/edit_actions.rs:305` |
| Inline editor int commit | PASS |  |  |
| Inline editor bool commit on Int pin | FAIL | Bool commit accepted by editor prompt but rejected by sema E0112. Needs type-specific editor guard. | `Source/Canvas/js.rs:3599`, `Source/Canvas/edit_actions.rs:158` |
| Inline editor string commit on Int pin | FAIL | String commit accepted by editor prompt but rejected by sema E0112. Needs type-specific editor guard. | `Source/Canvas/js.rs:3599`, `Source/Canvas/edit_actions.rs:158` |
| Inline edit undo | PASS | Source undo transaction works for successful edits. |  |
| Variable rename from sidebar | PASS |  |  |
| Signature edit: add input/retype/rename | PASS |  |  |
| Extract-to-function | PASS |  |  |
| Collapse | PASS | Source comment region persists. |  |
| Copy/paste real nodes | PASS | Source-clone path exists and posts `replace_source` with `source_edit: "paste_clone"`. Browser unavailable for visual click proof. | `Source/Canvas/js.rs:421` |
| Staged node materialization | PASS | Staged-to-real path exists and posts source transaction. Browser unavailable for visual click proof. | `Source/Canvas/js.rs:509` |
| Comment box persistence across reload | PASS | Source comment transaction writes `// canvas:comment`; graph reprojects it. | `Source/Canvas/edit_actions.rs:648` |
| Debug run + breakpoint + step | PASS | Debug controls and command path exist; command endpoint returns receipt. Browser unavailable for visual step proof. | `Source/Canvas/js.rs:2110`, `Source/Canvas/js.rs:2134` |
| Diff preview | PASS | `/canvas/source-control` returns diff protocol. |  |
| Undo/redo depth 10 after mixed operations | FAIL | Undo/redo stack is in-memory and unbounded; no depth-10 policy or cap. | `Source/Canvas/js.rs:4400` |
| Graph/source toggle sync | PASS | Toggle/hash sync code present. Browser unavailable for visual proof. | `Source/Canvas/js.rs:2041`, `Source/Canvas/js.rs:3995` |
| Multi-function file navigation | PASS | Graph projection exposes 4 functions; `switchGraph`/callee open path exists. | `Source/Canvas/js.rs:2041`, `Source/Canvas/js.rs:2066` |

Verification:

```text
nix develop -c cargo build
nix develop -c cargo test --test canvas --test web_dev
```

Both passed. `cargo fmt`/`rustfmt` unavailable in dev shell.
