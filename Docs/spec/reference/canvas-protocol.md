# Canvas protocol

Canvas is a source-backed projection protocol. Ordinary Jet source is the only
semantic source of truth; clients may cache viewport and other private editor
state, but graph facts and edits come from checked source. The AST coverage is
cataloged in [`canvas-parity.md`](canvas-parity.md). This reference is for
Canvas clients, devserver contributors, and tools that consume the project,
graph, debug, query, action, and source-control documents.

The executable schema and transaction paths are in
[`schema_api.rs`](../../../crates/jet-devserver/src/Canvas/schema_api.rs),
[`graph_projection.rs`](../../../crates/jet-devserver/src/Canvas/graph_projection.rs),
and [`edit_actions.rs`](../../../crates/jet-devserver/src/Canvas/edit_actions.rs).
The source write seam is covered by
[`source_model.rs`](../../../crates/jet-devserver/src/Canvas/source_model.rs) and
project writes by
[`project_transactions.rs`](../../../crates/jet-devserver/src/Canvas/project_transactions.rs).
Vocabulary is in [Jet vocabulary](../vocabulary.md).

## Envelope and routes

Canvas HTTP responses use the shared status envelope: `schema` is
`jet.status/v1`, `action` identifies the request, `ok` reports success, and the
Canvas payload is under `canvas`. Examples in this document show that inner
Canvas payload for readability; clients must preserve the outer status envelope
and its reports.

The devserver exposes the Canvas routes under `/__jet_canvas/...` and
`/canvas/...`. The `/panel/...` aliases expose the same documents for panel
clients. A route alias does not change the schema or authority boundary.

All schema constants in this reference have `schema_version` `1` unless a
field table says otherwise.

## Project document

`GET /__jet_canvas/project`, `GET /canvas/project`, and the corresponding
`/panel/project` alias return `jet.canvas.project`. The project document is the
workspace/package layer above file graphs. Its source truth is ordinary project
files:

- `single_file`: the opened `.jet` file;
- `package`: `package.jet` plus package source files;
- `workspace`: `workspace.jet`, member `package.jet` files and source files,
  environment source such as `env.jet`, and `.jet/lock`.

Top-level fields are:

| Field | Meaning |
|---|---|
| `protocol` | Literal `jet.canvas.project`. |
| `schema_version` | Integer schema version; the value is `1`. |
| `project_root` | Display path for the package or workspace root. |
| `project_revision` | Stable hash of the projected source-truth file set. |
| `entry` | Entry source path relative to `project_root`. |
| `mode` | `single_file`, `package`, or `workspace`. |
| `capabilities` | Checked panel capabilities; unsupported capabilities are omitted or disabled. |
| `workspace` | `workspace.jet` projection with member names and paths, or `null`. |
| `packages` | Parsed `package.jet` facts for the root package and members. |
| `targets` | Package/build targets with package paths and manifest source. |
| `outputs` | Valid build outputs exposed by the normal launcher. Choices stay local until command authority receives them. |
| `envs` / `services` | `env.jet` projection from Jetpack evaluation, including package references, prompts, secrets, and dev services. |
| `files` | Projected source-truth files with per-file revisions and kinds. |
| `parts` / `part_conflicts` | Source parts and scan conflicts associated with the projected files. |
| `locks` | `.jet/lock` facts used by the projection. |
| `diagnostics` | Project-level Jet diagnostics. |
| `source_control` | Git text-truth summary. |
| `state_policy` | The boundary between source semantics and private view state. |

A representative inner payload is:

```json
{"protocol":"jet.canvas.project","schema_version":1,"project_root":"/repo","project_revision":"sha256-...","entry":"packages/game/src/main.jet","mode":"workspace","workspace":{"path":"workspace.jet","members":[{"name":"game","path":"packages/game"}],"diagnostics":[]},"packages":[{"path":"packages/game","manifest":"packages/game/package.jet","name":"game","version":"0.1.0","target":"web","deps":[],"targets":[{"package":"game","target":"executable"}],"effects_enabled":false,"diagnostics":[]}],"targets":[{"package":"game","package_path":"packages/game","manifest":"packages/game/package.jet","target":"executable"}],"outputs":[{"package":"game","package_path":"packages/game","manifest":"packages/game/package.jet","target":"executable"}],"envs":[],"services":[],"files":[{"path":"packages/game/src/main.jet","revision":"sha256-...","kind":"source"}],"parts":[],"part_conflicts":[],"locks":[],"diagnostics":[],"source_control":{"truth":"git-text"},"state_policy":{"semantic":"source","local":["tabs","viewport","selection","breakpoints","watches","comment_boxes","staged_nodes"],"shared_visual":"source-anchored-comments"}}
```

Project documents do not create a Canvas project asset. Package/workspace
semantics remain in `package.jet`, `workspace.jet`, source files, environment
source, and `.jet/lock`. Local tabs, zoom, selected nodes, breakpoints, and
watches may be cached locally. Shared visual intent uses source-anchored Canvas
comments or collapse hints only when the user chooses to share it.

## Resident session

`GET /__jet_canvas/session`, `GET /canvas/session`, and `/panel/session` return
`jet.canvas.session`. Canvas, text, graph, designer, preview, terminal,
debugger, test, and custom-server views read one resident identity and source
stream. The session is not a second semantic store.

```json
{"protocol":"jet.canvas.session","schema_version":1,"session":{"id":"jet-session-123-1","source_revision":"sha256-...","accepted_revision":"sha256-...","last_good_revision":"sha256-...","last_good_program":"web-build-2","state":"ready","clients":2,"run":{"output":"web","target":"browser"},"debugger":{"state":"active"},"tests":{"state":"idle"},"history":{"count":4,"receipts":[{"kind":"replace_source","status":"accepted","before":"sha256-old","after":"sha256-new","client":"client-a"}]},"listeners":{"canvas":{"host":"127.0.0.1","port":8080,"transport":"canvas"},"application":{"host":"127.0.0.1","port":49152,"transport":"application","routes":"application-owned"}},"custom_servers":{"owner":"application","transport":"application","reload":"source-transaction"}}}
```

Accepted and refused source/project transactions append receipts to shared
history. A stale revision is refused before write; undo and redo are checked
source transactions. `last_good_revision` and `last_good_program` change after a
successful rebuild, so a failed rebuild can show current diagnostics while the
preview and graph remain on the last accepted program. Reconnect reads the same
session object and preserves accepted edits, run selection, debugger state,
history, and last-good program for every client.

The Canvas listener and application listener are intentionally separate. Canvas
routes belong to the IDE transport. Application routes, custom hosts, ports,
middleware, and reload policy remain application-owned. In a web dev session,
`--canvas-port` selects the Canvas control listener and `--port` selects the
application preview listener; omitting either lets that listener choose its own
port.

## Project transactions

`POST /__jet_canvas/project/transaction`, `POST /canvas/project/transaction`,
and `/panel/project/transaction` edit package/workspace source through an
explicit multi-file envelope. They never write a Canvas project asset.

Required fields are:

| Field | Meaning |
|---|---|
| `schema_version` | Project transaction schema version, `1`. |
| `op` | Project operation name. |
| `project_revision` | Revision from `jet.canvas.project`; it must still describe the projected file set. |
| `files` | Touched source-truth files, each with `path` and expected `revision`. |
| `preview` | Optional boolean; `true` returns diff/audit without writing and defaults to `false`. |

The supported project operations are:

| `op` | Extra fields | Effect |
|---|---|---|
| `add_dependency` | `manifest`, `name`, `spec` | Inserts or updates one dependency through the manifest edit helper, then validates the package parser. |
| `remove_dependency` | `manifest`, `name` | Removes one dependency through the manifest edit helper, then validates the package parser. |
| `edit_pkg_field` | `manifest`, `field`, `value` | Edits a known package field or migration-era payload field, then validates the manifest parser. |
| `add_target` | `manifest`, `name`, `target` | Inserts or updates one package target and validates the manifest parser. |
| `create_package` | `package_path`, `name`, `target`, optional `entry` | Creates a package directory with `package.jet` and an entry source file. New files enter `files` with revision `missing`. |
| `add_workspace_member` | `workspace`, `member_path` | Creates or edits `workspace.jet` to include a package directory, then validates workspace evaluation. |
| `add_env_service` | `env`, `name`, optional `enable`, `port`, `run`, `ready`, typed `shutdown`, `data_dir` | Creates or edits `env.jet` to include a development service, then validates Jetpack evaluation. |
| `edit_game_selection` | target descriptors and `field_patch` | Edits a checked target selection; descriptors carry `source_id`, `revision`, `authored_instance_id`, and `source_span`. |
| `rename_binding` / `rename_function` | `source_id`, `from`, `to` | Renames the selected definition and resolved project references through one checked multi-file source transaction. |

A preview or successful edit response uses `jet.canvas.project.edit` and records
the authority, touched files, and write class:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":true,"op":"add_dependency","preview":true,"changed":true,"project_revision":"sha256-...","after_project_revision":"sha256-...","writes":"preview_only","authority":["canvas.source_edit:project"],"audit":{"touched_files":[{"path":"packages/app/package.jet","revision":"sha256-...","changed":true}],"diagnostics":[]},"diff":"diff -- packages/app/package.jet\n--- before\n+++ after\n+    logging: ../logging,\n"}
```

A stale `project_revision` or touched-file `revision` fails before any write:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source file changed since this Canvas project was drawn"}
```

Apply mode builds overlays for all changed source files, runs the Jet formatter,
rechecks the front end, and evaluates the relevant package, workspace, or
environment input. A diagnostic leaves every source file unchanged. The shared
publish seam compares each expected source snapshot immediately before publish,
writes synced temporary files, atomically replaces changed files, and rolls back
already-published files if a later publish fails. The resulting audit distinguishes
`preview_only` from `source_transaction`; no graph-side semantic sidecar is
created.

## Graph document

`GET /__jet_canvas/graph`, `GET /canvas/graph`, and `/panel/graph` return
`jet.canvas.graph`. Add `?source_id=<project-relative .jet path>` to project a
source file inside the opened package or workspace. The server resolves that
path through the project document and rejects paths outside the project.

Top-level fields are:

| Field | Meaning |
|---|---|
| `protocol` | Literal `jet.canvas.graph`. |
| `schema_version` | Graph schema version, `1`. |
| `source_id` | Project-relative display path for the projected source file. |
| `revision` | Stable source hash echoed by edit transactions. |
| `fmt_fingerprint` | Hash of formatter-normalized source used to detect drift. |
| `source_text` | Current source text used by local undo/redo; clients may ignore it. |
| `graphs` | Function, test, and lambda graph documents. |
| `diagnostics` | Jet parser/sema diagnostics, not host-compiler output. |
| `facts` | Semindex handles and non-semantic Canvas facts. |
| `rails` | Display rail classes projected from checked semantics. |

A compact graph payload contains the fields clients must preserve:

```json
{"protocol":"jet.canvas.graph","schema_version":1,"source_id":"main.jet","revision":"sha256-...","fmt_fingerprint":"sha256-...","source_text":"fn square(n: Int) -> Int { n * n }\n","graphs":[],"diagnostics":[],"facts":{"semindex_schema_version":20,"blueprint":{},"handles":[]},"rails":[]}
```

Each graph carries a stable `graph_id`, title, `source_span`, nodes, pins,
wires, regions, inline expressions, and the graph's `rails`. Nodes carry
`node_id`, `kind`, `archetype`, title, source span, layout, badges, and edit
affordances. `archetype` is one of `value`, `function_exec`, `function_pure`,
`control`, or `entry`. Function, method, and dispatch-shaped calls use
`kind:"function"`; enum construction uses `kind:"variant"`. Exec pins use
type `exec`.

Pins carry `pin_id`, `node_id`, name, direction, type, optional role,
`pattern_source`, ability, fallibility, effect-grant need, and `source_span`.
Pattern-match and dispatch arms use `role:"arm"` and `pattern_source`; editable
arms also carry `pattern_source_span`. List-literal item pins carry a narrow
source span, `append_op:"remove_multi_input_element"`, and `element_index`.
When the compiler cannot expose a narrower pin span, the owning source node
anchors it. Wires carry source spans; control wires also carry the source spans
of their connected statements.

### Blueprint facts

`facts.blueprint` contains source-derived affordances that do not change program
meaning, plus an optional live event projection:

- `state_graphs` contains checked typestate states and edges. A state records
  `terminal`, `reachable` (or `null` when no entry transition exists), and its
  source span. This compile-time metadata is not runtime state storage.
- `runtime_events` is `null` unless Canvas is opened with
  `/canvas?pid=<live Jet pid>`. With a PID, the graph uses the same owner,
  identity, and age checks as `jet inspect live` and projects only
  `executed, payload-free` `Event`, `AsyncEvent`, and `DecisionHook`
  observations:
  subscriptions, queue and backpressure counts, priority, failures, and
  lifecycle. A source call that did not execute does not appear.
- `event_dispatchers` contains checked source facts for `core.event`
  constructors and Event/AsyncEvent/Hook/DecisionHook/Subscription/EventScope
  calls. Each fact carries source span and text, resolved receiver type, and
  source-backed subscription scope when present. These facts are ordinary Jet
  source truth; they never populate `runtime_events` or claim execution.
- `interfaces` contains source-authored traits and trait-impl facts for
  interface views and create-impl transactions. Compiler-generated derives stay
  out of this source-authoring surface. Facts include module scope, associated
  types, canonical method signatures, required/default status, effect row, and
  source span.
- `task_flows` contains `task` spawn/join/channel and `task.group` facts for
  asynchronous rails.

Rails are display facts only. They project control flow, data flow, fallible
propagation, async/task scopes, effects and abilities, unsafe/proof regions, and
runtime debug overlays already proven by the front end. A rail never adds
behavior.

### Source-backed visual state

The editor shell reads `jet.canvas.project.files` for the Files rail and passes a
selected path as `source_id`. Functions come from `graphs`; variables come from
function parameters and local binding/assignment/get nodes. Details edits
function inputs/outputs through `graph.function`, inline initializers through
`inline_exprs`, and all writes through checked rename, signature, or inline-edit
transactions.

A Details control exists only when a live source operation exists. A child edit
rebuilds its parent expression and uses `edit_inline_expr`. Validation, stale
revision refusal, Escape, blur, undo, and redo preserve the original source
when the candidate is invalid.

Shared Canvas comments persist as ordinary source comments:

```jet
// canvas:comment span=120..260 title="damage path" color="#2f80ed" alpha=0.25 bounds=(10,20,320,140)
```

The span is shared truth. Title, color, alpha, and bounds carry visual intent;
stale anchors degrade to auto-layout or local view state. V2 `comment_boxes`,
`staged_nodes`, staged wires, and copy/paste clipboard state are private editor
state. They do not appear in graph JSON and do not write Jet until a staged node
is connected to a source-backed pin or a paste operation creates a valid source
transaction.

Source-backed paste uses `replace_source` with `source_edit:"paste_clone"`. The
client renames cloned bindings against the current source, inserts the clone
after the selected source span, and shows rename pairs in Details. The
clipboard carries the source revision; a changed revision rejects paste without
writing source. “Paste as staged” stays local until a compatible source-backed
connection materializes it. Entry and return anchors are not copyable. Mixed or
source-incompatible selections use staged fallback only when every selected node
has an insertable descriptor.

Collapsed views persist as ordinary comments:

```jet
// canvas:collapse span=120..260 title="validated path"
```

`extract_inline_expr` writes an ordinary helper function and replaces the
selected expression with a helper call. `inline_helper_call` replaces a helper
call with its single return expression after the front end accepts the rewrite.

Clients ignore unknown top-level and nested fields. Unknown fields are
forward-compatible only when they are non-semantic; a new field must not carry
behavior that an old client would silently miss.

## Debug session

`POST /__jet_canvas/debug`, `POST /canvas/debug`, and `/panel/debug` return
`jet.canvas.debug`. Debug state is local editor state. Breakpoints are anchored
to source spans and the source hash; Canvas does not write breakpoints or
watches to `.jet` files unless a shared-probe syntax is ratified
(D-CANVAS-DEBUGSTATE1).

Requests carry:

| Field | Meaning |
|---|---|
| `schema_version` | Debug schema version, `1`. |
| `source_id` | Optional project-relative source selected in the Files rail. |
| `revision` | Source revision from the graph document. |
| `session_id` | Returned by a running response to continue or stop that session. |
| `tier` | `jet-dev-interpreter` for an executing `jet dev` session or `native-lldb` for a compiled debug artifact. Continuations keep the tier. |
| `commands` | `step`, `next`, `continue`, `finish`, `locals`, `print`, or `backtrace`. |
| `breakpoint_spans` | Local source anchors encoded as `start:end`. |
| `breakpoints` | Optional line breakpoints for clients that already mapped spans. |
| `watches` | Local names evaluated at the stopped frame. |
| `stop` | Optional boolean that ends a live session without changing source. |

A running response projects one bounded runtime snapshot:

```json
{"protocol":"jet.canvas.debug","schema_version":1,"ok":true,"source_id":"main.jet","revision":"sha256-...","session":{"id":"canvas-debug-1","state":"running","tier":"jet-dev-interpreter","persistence":"local-source-span","source_id":"main.jet","revision":"sha256-..."},"overlay":{"debug_overlay":"running","runtime_state":"live","source_id":"main.jet","revision":"sha256-...","active_line":12,"active_span":{"start":240,"end":258},"active_graph_id":"fn:main.jet::run@1-20","active_node_id":"fn:main.jet::run@1-20:stmt:7","active_wire_id":"","wire_path":[],"breakpoints":[{"line":12,"source_span":{"start":240,"end":258},"state":"valid"}],"locals":[{"name":"total","type":"Int","value":"6"}],"watches":[],"call_stack":["#0 run() at main.jet:12"],"trace":[],"limits":{"locals_truncated":false,"watches_truncated":false,"call_stack_truncated":false,"trace_truncated":false,"wire_path_truncated":false}}}
```

The first request creates a source- and revision-bound session. `runtime_state:
"live"` means values, stack, active node, and wire path came from the current
paused runtime snapshot. Finished values are historical and do not pulse the
graph. A disconnected or stale session clears its overlay before reporting the
state. A later request sends `session_id` and commands to continue the same
session.

Stop requests check source path, revision, and tier before removing the session.
A mismatched tier or source is refused; a stale request returns `conflict` and
keeps a newer live session. The bounded limits are 32 live sessions, 64
commands, 128 breakpoints, 32 watches, 32 call-stack frames, and 128 trace
entries. Unsupported interpreter/native boundaries return structured Jet
diagnostics and never expose host-compiler output. `native-lldb` never silently
falls back to the interpreter while claiming a native tier.

## Edit transactions

`POST /__jet_canvas/transaction`, `POST /canvas/transaction`, and
`/panel/transaction` apply a source edit. Required fields are `schema_version`,
`op`, and `revision`; `source_id` selects a project-relative source file when
present. A stale revision fails before any write.

The `jet.canvas.edit` operation set is:

| `op` | Extra fields and effect |
|---|---|
| `noop` | Reprojects without changing source. |
| `rename_binding` | `from`, `to`; renames a binding through source edits. |
| `rename_function` | Renames a function and project references where selected by the client. |
| `create_function` | Creates a function through the checked source transaction. Use the current `fn name(params) -> Type { ... }` form. |
| `edit_function_signature` | Replaces a function signature after formatting and front-end checks. |
| `edit_inline_expr` | `inline_expr_id`, `new_expr`; replaces an inline expression after validation. |
| `promote_to_binding` | `inline_expr_id`, `name`; inserts an ordinary binding and replaces the inline expression. |
| `insert_visible_conversion` | `inline_expr_id`, `callee`; wraps an expression in an ordinary checked call. |
| `break_link` | `wire_id`; replaces the source expression behind a wire with `#Todo`. |
| `move_link` | `wire_id`, `replacement`; rewrites the source expression to another visible name or path. |
| `reorder_statements` | Graph and statement spans plus optional `position` (`before`/`after`); moves one statement in the same checked block. Cross-block moves are refused. |
| `add_pattern_arm` | Graph/node spans and `pattern`; appends a checked pattern arm. |
| `edit_pattern_arm` | Graph and pattern spans plus `pattern`; replaces an arm pattern, then formats and checks. |
| `remove_pattern_arm` | Graph and pattern spans; removes an arm, but not the last remaining arm. |
| `toggle_switch_state` | Graph/node spans; removes an existing `#Off`/`#DebugOnly` marker or adds `#Off` to a checked statement. |
| `append_multi_input` | Node spans and optional `element`; appends a list-literal element. |
| `remove_multi_input_element` | Node and element spans; removes one element and its adjacent separator. |
| `insert_call` | `graph_id`, `callee`, `args`, optional binding and pin/wire fields; inserts an ordinary Jet call. |
| `replace_source` | `source`; replaces the file with exact prior/future Jet source after formatting and validation. |
| `replace_source` with `source_edit:"exec_convergence"` | Resolves a checked incoming convergence using `extract`, `helper`, or `duplicate`, then writes only the complete validated candidate. |
| `insert_branch` | Optional graph and exec-pin fields; inserts an ordinary checked `if true { ... } else { ... }` skeleton at the saved target. |
| `insert_switch` | Optional graph and exec-pin fields; inserts an ordinary checked `if 0 == { ... }` dispatch skeleton. |
| `insert_loop` | Optional graph and exec-pin fields; inserts an ordinary `loop { break }` skeleton. |
| `insert_fallible_rail` | Optional graph and exec-pin fields; inserts a fallible-result binding and `?` propagation; non-fallible contexts are rejected. |
| `create_comment_region` | Graph, source span, title, color, alpha, and bounds; inserts a `// canvas:comment` source hint. |
| `edit_comment_region` | Region ID and optional title, color, alpha, and bounds; rewrites one comment hint. |
| `move_comment_region` | Region ID and bounds; geometry alias of `edit_comment_region`. |
| `resize_comment_region` | Region ID and bounds; geometry alias of `edit_comment_region`. |
| `delete_comment_region` | Region ID; removes one comment hint while leaving program source intact. |
| `create_collapsed_region` | Graph, source span, and title; inserts a `// canvas:collapse` view hint. |
| `expand_collapsed_region` | Region ID; removes one collapse hint without changing program semantics. |
| `preview_extract_inline_expr` | `inline_expr_id`, `function`, `ret_type`; returns an exact extraction diff without writing. |
| `extract_inline_expr` | Same fields; inserts an ordinary helper function and replaces the expression with a call. |
| `inline_helper_call` | `inline_expr_id` or `start`/`end`; replaces a direct helper call with its return expression. |
| `create_trait_impl` | `type_name`, `trait_name`; appends an ordinary `impl Type.Trait { ... }` with checked stubs. Traits with associated-type choices are refused. |
| `preview_canvas_action` | Action ID, graph, callee, and arguments; checks an action candidate and returns a source diff without writing. |

The server ignores unknown request fields in v1. Unknown operations return a
`jet.canvas.edit` response with `kind:"unsupported"`. Semantic failures return
the rendered Jet diagnostic and a structured `diagnostics` array. Successful and
failure envelopes include the source revision; a successful response also
returns the committed `source_text` used by local undo/redo.

A pin-drag insertion remains source truth. `wire_expr` becomes a call argument
or `wire_inline_expr_id` replaces an input expression in the same transaction.
The returned graph projects the real wire from source; Canvas never stores a
semantic edge in a graph asset. Control-wire endpoint dragging sends
`reorder_statements` with the source spans of the connected statements.

Successful and failure shapes are:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"changed":true,"revision":"sha256-...","source_text":"fn square(n: Int) -> Int { n * n }\n"}
```

```json
{"protocol":"jet.canvas.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source changed since this Canvas graph was drawn"}
```

Every successful write runs through the Jet formatter, rechecks through the
front end, and reprojects from source. The final publish compares and publishes
against the request's source snapshot. If source changes after validation, the
transaction returns `kind:"conflict"` without replacing it. Failed saves create
no undo entry. A matching checkpointed semantic-op receipt may describe the
operation, but Canvas trusts only the receipt whose file path and source revision
match; a hand edit never inherits an older receipt.

## Query and action catalog

`POST /__jet_canvas/query`, `POST /canvas/query`, and `/panel/query` are
read-only. They use shared semindex facts for definitions, references, rename
ranges, and impact analysis. `source_id` selects a file; `revision` must match
that file. Cross-file queries snapshot the projected files and reject a changed
project before returning mixed-source results.

| `op` | Effect |
|---|---|
| `find` | Finds matching nodes, definitions, references, and source text in the selected file. |
| `project_search` | Searches matching nodes, definitions, references, and source text across projected files. |
| `references` | Returns project-wide definition/reference sites and impact facts. |
| `source_to_graph` | Maps a source byte range to matching nodes and inline expressions. |
| `preview_rename` | Returns a single-file diff or, with a current `project_revision`, a per-file atomic rename diff. |
| `actions` / `palette_entries` | Returns source-backed `project_functions` metadata and one ranked Canvas action list. |
| `core_catalog` / `corelib_catalog` | Returns read-only `core.*` modules and members from the canonical Core reference. |

Project query results include `result_limit` and `truncated`; the server keeps
at most 200 result sites and the client discloses when a search must narrow.
A project rename preview returns `diff.files` with each path, before revision,
after revision, and changed flag. Applying that diff uses the exact touched-file
envelope.

The Canvas Library is a read-only view over `actions`. It groups checked
`canvas.core_catalog` and ordinary `canvas.action` entries by `module_path`,
shows signatures, documentation source, typed pins, and availability reasons,
and uses `insert_call` for source edits. Core entries marked `available:true`
carry `insert_callee`, `insert_op:"insert_call"`, source-edit authority, and
`writes:"source_transaction_only"`. The client uses `insert_callee` rather than
deriving a spelling from a display title or ordinary `callee` field. Staged
`needs_canvas_defaults` and `method_only` rows remain local until a compatible
wire runs the checked transaction; a `method_only` row carries its typed
`receiver_type` and never receives an invented `Value` receiver.

An action descriptor and preview look like:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"actions","revision":"sha256-...","results":[],"impact":null,"diff":null,"actions_schema_version":1,"project_functions":[{"name":"square","signature":"fn square(n: Int) -> Int","callee":"square","insert_callee":"square","module_path":"main.jet","pure":true,"ret":"Int","pins":[{"name":"n","direction":"input","type":"Int"}],"default_args":["1"],"available":true,"insert_op":"insert_call"}],"actions":[{"action_id":"canvas.action:main.jet:square","kind":"canvas.action","title":"square","callee":"square","insert_callee":"square","engine":"checked-tir+jit","authority":["canvas.source_edit:package"],"package_id":"app","version":"0.1.0","touched_files":["main.jet"],"writes":"source_transaction_only"}]}
```

```json
{"schema_version":1,"op":"preview_canvas_action","revision":"sha256-...","graph_id":"fn:main.jet::run@0-3","action_id":"canvas.action:main.jet:square","callee":"square","args":["1"]}
```

The preview server checks that `callee` exactly matches the descriptor encoded by
`action_id`. A successful preview reports `jet.canvas.action`, uses the
`checked-tir+jit` engine, and writes only through a `source_transaction_only`
path:

```json
{"protocol":"jet.canvas.action","schema_version":1,"ok":true,"changed":true,"engine":"checked-tir+jit","execution":"preview","writes":"source_transaction_only","authority":["canvas.source_edit:package"],"audit":{"package_id":"app","version":"0.1.0","hash":"sha256-...","touched_files":["main.jet"],"diagnostics":[]},"diff":"--- before\n+++ after\n+    square(1)\n"}
```

A Canvas action never gets a private runtime, compiler, or graph asset store. An
`external adapter` is an opt-in native/tool bridge that must request additional
authority before tool, file, network, cache, or unsafe access.

## Command authority and receipts

The command endpoint is `POST /__jet_canvas/command`, `POST /canvas/command`,
or `/panel/command`. It accepts only whitelisted Canvas command actions:
`run`, `check`, `test`, `build`, and `service.start`. Requests may include
`source_id`; the checked revision, diagnostics, and receipt then refer to that
selected project file. `run` passes selected `output` and `target` values as
exact CLI arguments. `test`, `build`, and `service.start` require
`confirmed:true`; `service.start` runs the real `jet services up` supervisor
path. The endpoint does not accept arbitrary argv, and a long-running `dev`
command is not advertised as a Canvas action.

```json
{"schema_version":1,"action_id":"canvas.command:check","revision":"sha256-...","source_text":"fn run() -> Int {\n    missing\n}\n"}
```

A receipt records the checked command and its result:

```json
{"protocol":"jet.canvas.command_receipt","schema_version":1,"ok":true,"action_id":"canvas.command:check","title":"Check project","revision":"sha256-...","checked_revision":"sha256-...","command":["jet","check","main.jet"],"writes":"none","success":false,"exit_code":1,"elapsed_ms":42,"stdout":"","stderr":"Error [E0107]: ...","diagnostics":[{"code":"E0107","severity":"error","what":"nothing named `missing` exists here","why":"only names that have been defined can be used","fix":"define `missing` before this line","message":"nothing named `missing` exists here","rendered":"Error [E0107]: ...\n Why: ...\n Fix: ...\n","source_span":{"start":15,"end":22,"line":2,"column":5},"source_path":"/repo/main.jet"}]}
```

For Check, `source_text` is optional. When present, Canvas checks the open
buffer through the same front end without writing it to disk. The
`checked_revision` tags diagnostics so later checks and projections clear stale
rows and bubbles. Execution receipts are `missing` until an approved command
runs for the exact source revision. A future external adapter requests extra
authority before any tool, file, network, cache, or unsafe access.

## Function and callback views

Every function graph carries source-backed metadata. The signature spelling in
metadata follows current Jet syntax:

```json
{"function":{"name":"on_start","signature":"pub fn on_start(limit: Int{<default@31-32>}) -> Int","visibility":"public","docs":"Starts the scene.","pure":false,"unsafe":false,"returns":"Int","params":[{"name":"limit","type":"Int","default":true,"default_source":"1"}],"meta":{"category":"Movement","tunable":true},"edit_affordances":["rename_function","edit_function_signature","create_function","source_jump"]}}
```

`#Meta(category: "...", tunable)` projects source-backed `meta` facts on
annotated function and binding nodes. Unannotated items use `meta: null`. The
field is read-only in the graph; edits go through source transactions.

`facts.enum_variants` supplies unit-variant choices such as
`{"name":"Fast","source":"Mode.Fast"}`. Details uses these records for enum
fields and same-type binding facts for reference fields. Scalar, enum, and
reference controls submit `edit_inline_expr` or `edit_function_signature`, so
revision checks, semantic validation, formatting, source spans, undo, and reload
remain one path.

Function edits are ordinary source transactions:

```json
[
  {"schema_version":1,"op":"edit_function_signature","revision":"sha256-...","graph_id":"fn:main.jet::on_start@25-33","signature":"pub fn on_start(limit: Int{1}) -> Int"},
  {"schema_version":1,"op":"rename_function","revision":"sha256-...","from":"on_start","to":"on_begin"},
  {"schema_version":1,"op":"create_function","revision":"sha256-...","name":"helper","params":"value: Int","ret_type":"Int"}
]
```

Event views remain views over ordinary callback-shaped functions under the
callback boundary. A function such as `on_start` can project:

```json
{"event_views":[{"kind":"callback_event","title":"start","function":"on_start","semantics":"ordinary_jet_function","dispatch":"framework_callback","pending_first_class_events":"#286"}]}
```

The callback view does not create an event sidecar or alter saved Jet text.

## Source control

`GET /__jet_canvas/source-control`, `GET /canvas/source-control`, and
`/panel/source-control` return `jet.canvas.source_control`. Git text is the
source-control truth. The response reports project dirty state, recent entry-file
history, and per-file status/diff for the package/workspace source set. Canvas
never creates a graph lock, checkout state, or binary asset source of truth.

```json
{"protocol":"jet.canvas.source_control","schema_version":1,"ok":true,"revision":"sha256-...","project_revision":"sha256-...","project_root":"/repo","available":true,"dirty":true,"dirty_files":2,"status":"M packages/app/main.jet\n?? packages/app/helper.jet","diff":"","history":["abc123 initial"],"files":[{"path":"packages/app/main.jet","revision":"sha256-...","kind":"source","available":true,"dirty":true,"status":"M packages/app/main.jet","diff":"diff --git ...","semantic_ops":[{"kind":"rename","from":"report","to":"summarize"}]}]}
```

## Review Lens M3

The Review lens reads this response with the **text diff first**. Git already
defines file and hunk additions and deletions, so Canvas does not need a custom
binary-asset diff model. A current added or modified hunk may link to a source
span and graph node when the current graph exposes an overlapping span. When
`semantic_ops` contains a checkpoint-matching receipt, Review shows the recorded
operation and targets; it does not infer a rename from similar text.

Deleted text is marked deleted because it has no current source span. Other
changes without a matching node remain text-only. These labels keep source truth
visible without fabricating graph history. Refresh reads source-control and the
current graph again. Review actions do not write Jet source or Git state.

A semantic receipt is metadata about a source transaction. `semantic sidecars`
are not semantic truth or a replacement for source text; stale or missing
receipts leave the text diff unchanged.

## Proof

`GET /__jet_canvas/proof`, `GET /canvas/proof`, and `/panel/proof` report what is
known for the selected source revision: front-end check state, Git text state,
debug persistence, and whether a command-authority receipt exists. Optional
`source_id` selects a project-relative source. Proof never manufactures
build/run evidence.

Until a real Canvas command runs for the exact source revision,
`command_receipts.state` and `proof.state` are `missing`. After a whitelisted
command returns, the proof rail embeds the receipt and marks that revision
current.

```json
{"protocol":"jet.canvas.proof","schema_version":1,"ok":true,"source_id":"helper.jet","source_path":"/repo/helper.jet","revision":"sha256-...","check":{"state":"ok","diagnostics_count":0,"message":"front end check passed"},"source_control":{"truth":"git-text","available":true,"dirty":false,"status":""},"debug":{"state":"local-only","persistence":"local-source-span"},"command_receipts":{"state":"missing","reason":"no Canvas command authority receipt has run for this source revision"},"proof":{"state":"missing","stale":true,"reasons":["no check/build/run receipt for this source revision"]}}
```
