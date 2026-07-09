# Canvas/Blueprint parity postmortem

Source: tools/Tower/docs/plans/epoch-6/canvas-blueprint-parity-matrix.md (57 rows total,
enforced >=50 by tests/canvas.rs::canvas_blueprint_parity_matrix_is_classified),
tests/canvas.rs (2574 lines), tests/web_dev.rs (1117 lines), git log, .tower/tower.json.

## 1. Matrix row -> ratchet class

Class key: (a) interaction-verified real browser DOM events, (b) HTTP/protocol-level,
(c) grep/string-assertion on served HTML/JS text, (d) projection/JSON snapshot (in-process
Rust API call, no server/browser).

| Row (Area | capability) | Ratchet cited | Class |
|---|---|---|
| Workbench | Right-click action menu | tests/web_dev.rs | c |
| Workbench | Drag-off-pin action menu | tests/canvas.rs | d |
| Workbench | Built-in method search | tests/canvas.rs::canvas_actions_project_palette... | d |
| Workbench | Selection/marquee | tests/web_dev.rs | c |
| Workbench | Pan/zoom/fit | tests/web_dev.rs | c |
| Workbench | Child/parent nav | tests/web_dev.rs | c |
| Workbench | Drag nodes/groups | tests/web_dev.rs | c |
| Workbench | Align/distribute/tidy | tests/web_dev.rs | c |
| Workbench | Bookmarks/favorites | tests/web_dev.rs | c |
| Workbench | Inspector/details panel | tests/web_dev.rs | c |
| Hotkeys | Undo/redo | tests/canvas.rs | d |
| Node model | Function calls | tests/canvas.rs | d |
| Node model | Variables get/set | tests/canvas.rs | d |
| Node model | Branch/switch/loops | tests/canvas.rs | d |
| Node model | Virtualization/LOD | tests/web_dev.rs | c |
| Pins/wires | Exec/data pins | tests/canvas.rs | d |
| Pins/wires | Typed colored wires | tests/canvas.rs | d |
| Pins/wires | Incompatible refusal | tests/canvas.rs | d |
| Pins/wires | Auto-cast insertion | tests/canvas.rs | d |
| Pins/wires | Promote pin to variable | tests/canvas.rs | d |
| Pins/wires | Break/move links | tests/canvas.rs | d |
| Pins/wires | Drag-drop rewiring | tests/web_dev.rs | c |
| Types | Primitive/user types | tests/canvas.rs | d |
| Types | Effect/unsafe markers | tests/canvas.rs | d |
| Comments | Node bubbles | tests/canvas.rs | d |
| Comments | Comment boxes | tests/canvas.rs | d |
| Functions | Create function graph | tests/canvas.rs | d |
| Functions | Edit signature | tests/canvas.rs | d |
| Functions | Add/remove pins | tests/web_dev.rs | c |
| Functions | Extract to function | tests/canvas.rs | d |
| Macros/collapse | Collapse graph | tests/canvas.rs | d |
| Events | Event graph entry nodes | tests/canvas.rs | d |
| Events | Event dispatchers/interfaces | tests/canvas.rs | d |
| Interfaces | Trait/impl authoring | tests/canvas.rs | d |
| Tasks | Latent action parity | tests/canvas.rs | d |
| Debugger | Debug session selector | tests/canvas.rs | d |
| Debugger | Breakpoints/watches | tests/canvas.rs | d |
| Debugger | Active node/wire pulse | tests/canvas.rs | d |
| Debugger | Call stack/trace | tests/canvas.rs | d |
| Search/refactor | Find in graph/project | tests/canvas.rs | d |
| Search/refactor | Find refs/rename | tests/canvas.rs | d |
| Search/refactor | Source-to-graph jump | tests/canvas.rs | d |
| Search/refactor | Toggle graph/source view | tests/web_dev.rs | c |
| Accessibility | Keyboard-only authoring | tests/web_dev.rs | c |
| Learning | Docs/first-run overlay | tests/web_dev.rs | c |
| Runtime | Live-run loop | tests/web_dev.rs | c |
| Source control | Dirty/stale/conflict | tests/canvas.rs | d |
| Source control | Transaction diff preview | tests/canvas.rs, tests/web_dev.rs | c/d |
| Public protocol | Graph JSON schema | tests/canvas.rs | d |
| Public protocol | Edit transaction schema | tests/canvas.rs | d |
| Extensibility | Function library projection | tests/canvas.rs | d |
| Extensibility | 3rd-party behavior nodes | tests/canvas.rs | d |
| Validation | Formatter stability | tests/canvas.rs | d |
| Tests | Projection JSON snapshots | tests/canvas.rs | d |
| Tests | UI nonblank/interactions | tests/web_dev.rs::jet_dev_web_exposes_canvas_panel_and_graph | c |
| Tests | Unsupported-feature diagnostics | tests/canvas.rs | d |
| Tests | Rows cannot ship w/o tests | tests/canvas.rs (self-check) | d (meta) |

No row cites a headless-browser/DOM-driven test. Repo-wide grep for
playwright/puppeteer/webdriver/selenium/chromium/headless across tests/, Source/Canvas/*,
Source/CmdDevWeb.rs returns zero hits.

## 2. Count summary

- 57 "shipped" rows total (matrix enforces >=50 via canvas_blueprint_parity_matrix_is_classified).
- 0 of 57 are class (a) interaction-verified.
- ~33 rows cite tests/canvas.rs only -> class (d): in-process calls to jet::Canvas::graph_json_for_file /
  apply_transaction_json / query_json_for_file, asserting on JSON substrings. Never touches the HTTP
  server, the browser, or rendered pixels.
- ~23 rows cite tests/web_dev.rs -> class (c): a single 630-line test function
  (jet_dev_web_exposes_canvas_panel_and_graph, lines 277-903) that spawns the real `jet dev --target=web`
  process, then does ~350 `html.contains("id=\"...\"")` / `js.contains("function fooBar")` string
  assertions against the raw served HTML/JS text. No DOM is constructed, no click/drag/keyboard event is
  dispatched, no pixel is read back for real render output (the "nonblank pixel" claim is itself just a
  string check that a JS variable name `window.__jetCanvasNonblankPixels` exists in source, not that it
  ever evaluates true in a running page).
- 1 row (Source control | Transaction diff preview) mixes c+d.
- The matrix's own gate test only requires the Ratchet column contain the substring "tests/" (or "#275")
  — it never inspects what the named test actually does. A row can "ship" against a test that greps for a
  CSS class name.

## 3. Five structural reasons prior attempts produced "test-green but unusable"

1. **No real interaction ratchet exists anywhere in the suite.** tests/web_dev.rs's one UI test
   (jet_dev_web_exposes_canvas_panel_and_graph) never opens a DOM — it string-matches served HTML/JS
   text. A button can have the right `id=` attribute and zero working click handler and the test still
   passes. This is the single biggest gap: "shipped" for Workbench/Hotkeys/Pins/Search rows means
   "the string exists in the JS bundle," not "clicking it does the thing."

2. **The enforcement mechanism checks citation, not depth.** canvas_blueprint_parity_matrix_is_classified
   (tests/canvas.rs:2506) only asserts a shipped row's Ratchet column contains "tests/" or "#275" —
   it cannot and does not verify the cited test exercises the claimed capability. This makes "shipped"
   a paperwork state, not a functional one; any agent can flip a row by adding one `.contains()` line.

3. **Most "shipped" rows bypass the server and browser entirely.** ~33 of 57 rows cite tests/canvas.rs,
   which calls jet::Canvas Rust functions in-process and asserts JSON substrings. This proves the
   projection/transaction layer is correct — it says nothing about whether a human operating the actual
   rendered `<canvas>` element (drag a node, wire a pin, use the palette) can do so successfully. The two
   test files test different, non-overlapping layers, and neither layer is "the product."

4. **Acceptance criteria were quietly downgraded without a ballot.** Card #265's body (tower.json:13932)
   explicitly required "Playwright desktop/mobile screenshots, keyboard interaction tests, nonblank
   canvas assertions." Playwright was never added to the repo (no config, no package.json, no CI step —
   confirmed by search). One agent log (card #356, 2026-07-07, tower.json:18122) claims an ad hoc manual
   Playwright run with "zero console errors," but it is not committed, not reproducible, and not gating
   anything — the bar silently reverted to string assertions and nobody raised that as an owner-facing
   change.

5. **Attempts rewrote instead of fixed, and the shallow tests hid regressions across rewrites.** Git
   history shows 4 distinct passes over Source/Canvas/*.rs + the JS bundle (close-parity 98ce1574 -> UI
   fix a1734f4e -> UI/UX overhaul 4487fa33..2b5a062d -> v2 rounds bb0e3f31..1c36bf8f, card #368), each
   replacing large chunks of markup/JS (new ids, renamed functions, new palette catalogs). Each time, the
   test suite was "fixed" by appending new `.contains()` assertions for the new strings, never by adding a
   test that would catch a wiring regression (e.g., drag-off menu opens but the click handler silently
   no-ops). Because no test drives real interaction, four successive full rewrites could each report
   green while the underlying click/drag/wire UX degraded or never worked end-to-end.

## 4. What the epoch plan must change so shipped == usable

- Add a real class-(a) ratchet: headless-browser driven interaction tests (the repo has zero
  playwright/puppeteer/webdriver infra today — this is a genuine new dependency decision, I6/ballot-gated)
  that construct the DOM, dispatch real pointer/keyboard events, and assert on resulting graph/source
  state, not on presence of an id string.
  This is a ballot-first gate per AGENTS.md (new toolchain dependency) — must not be silently added by an
  agent.
- Change canvas_blueprint_parity_matrix_is_classified to require, for every "shipped" row that is a
  Workbench/Hotkeys/interaction-family row, a citation to a class-(a) test specifically — not just any
  "tests/" substring.
- Separate the two existing suites explicitly in the matrix Ratchet column (mark class c/d today) so the
  next agent can see at a glance which rows are still unverified at the interaction level; do not let a
  new "shipped" row cite only tests/canvas.rs or only the string-assertion test.
- Stop the rewrite-over-rewrite pattern: before a 5th pass touches Source/Canvas/*.rs, get owner
  sign-off on what was actually broken in attempt 4 (a real bug list, not "redo the UI"), and keep the
  next pass to fixes with regression tests, not a wholesale re-architecture that resets the shallow test
  coverage again.
