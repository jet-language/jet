# Canvas Protocol

Canvas is a source-backed projection protocol. The `.jet` file is the only
semantic source of truth. Clients may cache viewport state locally, but graph
facts and edits come from checked Jet source.

## Graph Document V1

Endpoint: `GET /__jet_canvas/graph` or `GET /canvas/graph`.

Top-level fields:

| Field | Meaning |
|---|---|
| `protocol` | Literal `jet.canvas.graph`. |
| `schema_version` | Integer schema version. Current value: `1`. |
| `source_id` | Display path for the source file that was projected. |
| `revision` | Stable source hash. Edit transactions must echo this. |
| `fmt_fingerprint` | Hash of the formatter-normalized source. Used to detect formatter drift. |
| `source_text` | Current source text. Canvas uses this for local undo/redo; clients may ignore it. |
| `graphs` | Function/test/lambda graph documents. |
| `diagnostics` | Jet diagnostics already emitted by parser/sema. Never rustc output. |
| `facts` | Semindex schema/version handles used by the projection, plus non-semantic Blueprint-parity facts. |

`facts.blueprint` contains source-derived Canvas affordances that do not change
program meaning:

- `event_dispatchers`: `core.event` creation, subscription, and emit calls with
  source spans, `EventScope` lifetime, and EventTrace overlay intent.
- `interfaces`: trait and trait-impl facts for Canvas interface views and
  create-impl transactions.
- `task_flows`: `core.tasks` spawn/join/channel/taskgroup facts for async rails.

Each graph contains source-backed records:

| Field | Meaning |
|---|---|
| `graph_id` | Stable semantic path plus source span. |
| `title` | Display name. |
| `source_span` | Byte span in the source file. |
| `nodes` | Structural source nodes. |
| `pins` | Typed input/output pins derived from front-end facts. |
| `wires` | Data/control/fallible/effect/proof/debug rails. |
| `regions` | Source-backed regions/comments. Canvas comment boxes are ordinary Jet comments. |
| `inline_exprs` | Editable Jet expression source rendered inline. |
| `rails` | Visual rail classes present in this graph: control, data, fallible, async, effect, proof, debug. |

Each pin carries `pin_id`, `node_id`, `name`, `direction`, `type`,
`capability`, `fallible`, `effect_grant_need`, and `source_span`. A v1 pin span
is anchored to its owning source node when the compiler does not yet expose a
narrower pin-specific span.

Canvas comment boxes persist as ordinary source comments:

```jet
// canvas:comment span=120..260 title="damage path" color="#2f80ed" alpha=0.25 bounds=(10,20,320,140)
```

The `span` anchor is shared truth. `title`, `color`, `alpha`, and `bounds`
carry visual intent only; stale anchors degrade to auto-layout/local view state.

Collapsed graph views also persist as ordinary comments:

```jet
// canvas:collapse span=120..260 title="validated path"
```

Reusable collapse uses `extract_inline_expr`, which writes an ordinary helper
function and replaces the selected expression with a helper call. `inline_helper_call`
replaces a helper call with the helper's single return expression after the
front end accepts the rewritten source.

Clients must ignore unknown top-level and nested fields. Unknown fields are
forward-compatible only when they are non-semantic. A future field must not carry
behavior that old clients would silently miss.

Rails are display facts only. They project Jet semantics already proven by the
front end: control flow, data flow, fallible propagation, async/task scopes,
effects/capabilities, unsafe/proof regions, and runtime debug overlays. A rail
never adds behavior.

## Debug Session V1

Endpoint: `POST /__jet_canvas/debug` or `POST /canvas/debug`.

Debug state is local editor state. Per D-CANVAS-DEBUGSTATE1, breakpoints are
anchored to source spans and the source hash; Canvas never writes breakpoints or
watches into `.jet` files unless a later shared-probe syntax is ratified.

Request fields:

| Field | Meaning |
|---|---|
| `schema_version` | Integer debug schema version. Current value: `1`. |
| `revision` | Source revision from the graph document. |
| `commands` | Debugger commands using the `jet debug` vocabulary: `step`, `next`, `continue`, `finish`, `locals`, `print`, `backtrace`. |
| `breakpoint_spans` | Local source-span anchors encoded as `start:end`. |
| `breakpoints` | Optional line breakpoints for clients that already mapped spans. |
| `watches` | Local names to print at the stopped frame. |

Successful response:

```json
{"protocol":"jet.canvas.debug","schema_version":1,"ok":true,"revision":"sha256-...","session":{"id":"local-source-span","state":"running","persistence":"local-source-span"},"overlay":{"debug_overlay":"running","active_line":3,"active_span":{"start":24,"end":36},"active_graph_id":"fn:main.jet::run@0-3","active_node_id":"...","active_wire_id":"...","breakpoints":[],"locals":[],"watches":[],"call_stack":[],"trace":[]}}
```

Unsupported interpreter/native boundaries return Jet diagnostics through the
same protocol. They never expose rustc output.

## Edit Transaction V1

Endpoint: `POST /__jet_canvas/transaction` or `POST /canvas/transaction`.

Required common fields:

| Field | Meaning |
|---|---|
| `schema_version` | Integer edit schema version. Current value: `1`. |
| `op` | Transaction name. |
| `revision` | Source revision from the graph document. |

Current transactions:

| `op` | Extra fields | Effect |
|---|---|---|
| `noop` | none | Reprojects without changing source. |
| `rename_binding` | `from`, `to` | Renames a Jet binding through source edits. |
| `edit_inline_expr` | `inline_expr_id`, `new_expr` | Replaces one inline Jet expression after front-end validation. |
| `promote_to_binding` | `inline_expr_id`, `name` | Inserts an ordinary Jet binding before the owning source line and replaces the inline expression with that name. |
| `insert_visible_conversion` | `inline_expr_id`, `callee` | Wraps an inline expression in an ordinary Jet conversion/function call. |
| `insert_call` | `graph_id`, `callee`, `args`, optional `bind` | Inserts an ordinary Jet call in a graph's source body. |
| `create_trait_impl` | `type_name`, `trait_name` | Appends an ordinary `impl Type.Trait { ... }` block with source-checked member stubs. |
| `break_link` | `wire_id` | Replaces the source expression behind a wire with `#Todo`, preserving Jet type checking. |
| `move_link` | `wire_id`, `replacement` | Rewrites the source expression behind a wire to another visible Jet name/path. |
| `replace_source` | `source` | Replaces the file with exact prior/future Jet source after formatting and front-end validation. Used by local undo/redo. |
| `insert_branch` | `graph_id` | Inserts an ordinary checked `if true { ... } else { ... }` branch skeleton. |
| `insert_switch` | `graph_id` | Inserts an ordinary checked `if 0 == { ... }` dispatch skeleton. |
| `insert_loop` | `graph_id` | Inserts an ordinary `loop { break }` skeleton. |
| `insert_fallible_rail` | `graph_id` | Inserts an ordinary fallible-result binding plus `?` propagation skeleton; front-end validation rejects non-fallible contexts. |
| `create_comment_region` | `graph_id`, `start`, `end`, `title`, `color`, `alpha`, `bounds` | Inserts an ordinary `// canvas:comment ...` source hint after the anchored source line. |
| `edit_comment_region` | `region_id`, optional `title`, `color`, `alpha`, `bounds` | Rewrites one Canvas comment hint line. |
| `move_comment_region` | `region_id`, `bounds` | Alias of `edit_comment_region` for geometry changes. |
| `resize_comment_region` | `region_id`, `bounds` | Alias of `edit_comment_region` for geometry changes. |
| `delete_comment_region` | `region_id` | Removes one Canvas comment hint line; program source remains. |
| `create_collapsed_region` | `graph_id`, `start`, `end`, `title` | Inserts an ordinary `// canvas:collapse ...` view hint. |
| `expand_collapsed_region` | `region_id` | Removes one collapse hint; source semantics stay unchanged. |
| `preview_extract_inline_expr` | `inline_expr_id`, `function`, `ret_type` | Returns an exact text diff for extracting an inline expression to a helper. |
| `extract_inline_expr` | `inline_expr_id`, `function`, `ret_type` | Inserts an ordinary helper function and replaces the expression with a call. |
| `inline_helper_call` | `inline_expr_id` or `start`/`end` | Replaces a direct helper call with the helper return expression. |

Unknown request fields are ignored by v1. Unknown operations fail with a
`jet.canvas.edit` error. A stale `revision` fails with `kind:"conflict"` before
any write.

Successful response:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"changed":true,"revision":"sha256-..."}
```

Failure response:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source changed since this Canvas graph was drawn"}
```

Every successful write runs through `jet fmt`, re-checks through the front end,
then reprojects from source. Canvas does not own a parser, checker, graph asset,
or semantic sidecar.

## Query V1

Endpoint: `POST /__jet_canvas/query` or `POST /canvas/query`.

Queries are read-only. They use the shared semindex facts that LSP uses for
definitions, references, rename ranges, and impact analysis.

| `op` | Extra fields | Effect |
|---|---|---|
| `find` / `project_search` | `query` | Finds matching graph nodes, definitions, references, and source text. |
| `references` | `symbol` | Returns definition/reference sites plus impact facts. |
| `source_to_graph` | `start`, `end` | Maps a source byte span to matching graph nodes/inline expressions. |
| `preview_rename` | `symbol`, `to` | Returns the exact text diff for a rename without writing source. |
| `actions` / `palette_entries` | none | Returns source-backed palette entries and ratified Canvas actions derived from checked semindex facts. |

Successful response:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"find","revision":"sha256-...","results":[{"kind":"definition","title":"run","graph_id":"...","node_id":"...","source_span":{"start":0,"end":3}}],"impact":null,"diff":null}
```

A stale `revision` fails with `kind:"conflict"` before any query result is used.

## Canvas Actions V1

Canvas action boundary: package behavior runs through Jet's existing checked
front end, executable TIR, and JIT preview path. Canvas does not get a separate
runtime, compiler, or graph asset store. An action may return a source
transaction or preview, but it never writes files directly.

Terms:

| Term | Meaning |
|---|---|
| Palette entry | Read-only function/type/docs metadata projected from ordinary Jet code. |
| Canvas action | Behavior-producing Jet action with explicit authority and audited output. |
| External adapter | Opt-in native/tool bridge for heavyweight integrations. |

Query actions:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"actions","revision":"sha256-...","results":[],"impact":null,"diff":null,"actions_schema_version":1,"actions":[{"action_id":"canvas.action:main.jet:square","kind":"canvas.action","title":"square","callee":"square","engine":"checked-tir+jit","authority":["canvas.source_edit:current_file"],"writes":"source_transaction_only"}]}
```

Preview an action:

```json
{"schema_version":1,"op":"preview_canvas_action","revision":"sha256-...","graph_id":"fn:main.jet::run@0-3","action_id":"canvas.action:main.jet:square","callee":"square","args":["1"]}
```

Successful response:

```json
{"protocol":"jet.canvas.action","schema_version":1,"ok":true,"changed":true,"engine":"checked-tir+jit","execution":"preview","writes":"source_transaction_only","authority":["canvas.source_edit:current_file"],"diff":"--- before\n+++ after\n+    square(1)\n"}
```

The audit payload records package id/version/hash, touched files, diff, and
diagnostics. A future external adapter must ask for extra authority before any
tool, file, network, cache, or unsafe access.

## Functions And Callback Views V1

Every function graph carries source-backed metadata:

```json
{"function":{"name":"on_start","signature":"pub fn on_start(limit: Int = <default@31-32>) -> Int","visibility":"public","docs":"Starts the scene.","pure":false,"unsafe":false,"returns":"Int","params":[{"name":"limit","type":"Int","default":true,"default_source":"1"}],"edit_affordances":["rename_function","edit_function_signature","create_function","source_jump"]}}
```

Function edits are ordinary source transactions:

```json
{"schema_version":1,"op":"edit_function_signature","revision":"sha256-...","graph_id":"fn:main.jet::on_start@25-33","signature":"pub fn on_start(limit: Int = 1) -> Int"}
{"schema_version":1,"op":"rename_function","revision":"sha256-...","from":"on_start","to":"on_begin"}
{"schema_version":1,"op":"create_function","revision":"sha256-...","name":"helper","params":"value: Int","ret_type":"Int"}
```

Canvas validates each candidate with the same formatter and semantic checker as
other edits before writing the file.

Until the first-class Event/Hook system in #286 is ratified, event graphs are
views over ordinary callback-shaped functions. A function named `on_start`
projects:

```json
{"event_views":[{"kind":"callback_event","title":"start","function":"on_start","semantics":"ordinary_jet_function","dispatch":"framework_callback","pending_first_class_events":"#286"}]}
```

## Source Control V1

Endpoint: `GET /__jet_canvas/source-control` or `GET /canvas/source-control`.

Canvas treats Git text as source-control truth. It reports dirty state, exact
`git diff` text for the `.jet` file, and recent file history. It never creates a
graph lock, checkout state, or binary asset source of truth.

Successful response:

```json
{"protocol":"jet.canvas.source_control","schema_version":1,"ok":true,"revision":"sha256-...","available":true,"dirty":true,"status":"M Source/main.jet","diff":"diff --git ...","history":["abc123 initial"]}
```
