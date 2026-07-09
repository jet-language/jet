# Canvas Protocol

Canvas is a source-backed projection protocol. The `.jet` file is the only
semantic source of truth. Clients may cache viewport state locally, but graph
facts and edits come from checked Jet source. Current AST coverage status is
ratcheted in [`canvas-parity.md`](canvas-parity.md).

## Project Document V1

Endpoint: `GET /__jet_canvas/project` or `GET /canvas/project`.

The project document is the workspace/package layer above file graphs. It is
read-only in v1. Its source truth is ordinary Jet project files:

- single-file mode: the opened `.jet` file;
- package mode: `pkg.jet` plus package source files;
- workspace mode: `workspace.jet`, member `pkg.jet` files, member source files,
  env source, and `.jet/lock`.

Top-level fields:

| Field | Meaning |
|---|---|
| `protocol` | Literal `jet.canvas.project`. |
| `schema_version` | Integer schema version. Current value: `1`. |
| `project_root` | Display path for the package/workspace root Canvas projected. |
| `project_revision` | Stable hash of the projected source-truth file set. |
| `entry` | Entry source path relative to `project_root`. |
| `mode` | `single_file`, `package`, or `workspace`. |
| `workspace` | `workspace.jet` projection with member package names/paths, or `null`. |
| `packages` | Parsed `pkg.jet` facts for the root package and workspace members. |
| `targets` | Package/build targets projected from `pkg.jet` with package path and manifest source. |
| `envs` / `services` | `env.jet` projection from Jetpack module evaluation, including package refs, prompt, secrets, and dev services. |
| `files` | Projected source-truth files with per-file revisions and kinds. |
| `locks` | `.jet/lock` facts used by the projection. |
| `diagnostics` | Project-level Jet diagnostics. |
| `source_control` | Git text-truth summary handle; source control remains file text truth. |
| `state_policy` | Ratified boundary: semantics from source, private view state local. |

Example:

```json
{"protocol":"jet.canvas.project","schema_version":1,"project_root":"/repo","project_revision":"sha256-...","entry":"packages/game/src/main.jet","mode":"workspace","workspace":{"path":"workspace.jet","members":[{"name":"game","path":"packages/game"}],"diagnostics":[]},"packages":[{"path":"packages/game","manifest":"packages/game/pkg.jet","name":"game","version":"0.1.0","target":"web","deps":[],"targets":[{"package":"game","target":"executable"}],"effects_enabled":false,"diagnostics":[]}],"targets":[{"package":"game","package_path":"packages/game","manifest":"packages/game/pkg.jet","target":"executable"}],"envs":[],"services":[],"files":[{"path":"workspace.jet","revision":"sha256-...","kind":"workspace"}],"locks":[],"diagnostics":[],"source_control":{"truth":"git-text"},"state_policy":{"semantic":"source","local":["tabs","viewport","selection","breakpoints","watches","comment_boxes","staged_nodes"],"shared_visual":"source-anchored-comments"}}
```

Project documents do not create a Canvas project asset. Package/workspace
semantics must remain in `pkg.jet`, `workspace.jet`, source files, env source,
and `.jet/lock`. Local UI state such as tabs, zoom, selected nodes, breakpoints,
and watches may be cached locally; shared visual intent uses source-anchored
Canvas comments/collapse hints only when the user chooses to share it.

## Project Transaction V1

Endpoint: `POST /__jet_canvas/project/transaction` or
`POST /canvas/project/transaction`.

Project transactions edit package/workspace source truth through an explicit
multi-file envelope. They never write a Canvas project asset.

Required common fields:

| Field | Meaning |
|---|---|
| `schema_version` | Integer project transaction schema version. Current value: `1`. |
| `op` | Project operation name. |
| `project_revision` | Project revision from `jet.canvas.project`. |
| `files` | Touched source-truth files, each with `path` and `revision`. |
| `preview` | `true` returns diff/audit without writing; `false` validates then writes. |

Current transactions:

| `op` | Extra fields | Effect |
|---|---|---|
| `add_dependency` | `manifest`, `name`, `spec` | Inserts or updates one `deps:` entry in a `pkg.jet` file using the existing manifest edit helper, then validates the manifest parser before write. |
| `remove_dependency` | `manifest`, `name` | Removes one `deps:` entry from a `pkg.jet` file using the existing manifest edit helper, then validates the manifest parser before write. |
| `edit_pkg_field` | `manifest`, `field`, `value` | Edits known string fields in the `payload` block (`name`, `version`, `jet`, `description`, `license`, `repository`, `edition`) and validates the manifest parser before write. |
| `add_target` | `manifest`, `name`, `target` | Inserts or updates one `packages:` target entry and validates the manifest parser before write. |
| `create_package` | `package_path`, `name`, `target`, optional `entry` | Creates a package directory with `pkg.jet` and an entry `.jet` file, then validates the manifest parser and generated entry syntax before write. New files must appear in `files` with revision `missing`. |
| `add_workspace_member` | `workspace`, `member_path` | Creates or edits `workspace.jet` to include a package directory, then validates the workspace evaluator before write. Existing explicit member lists are edited in source; `find("./dir")` workspaces no-op when the member path is already covered. |
| `add_env_service` | `env`, `name`, optional `enable`, `port`, `init`, `ready`, `shutdown`, `data_dir` | Creates or edits `env.jet` to include a dev service, then validates Jetpack module evaluation before write. |

Successful response:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":true,"op":"add_dependency","preview":true,"changed":true,"project_revision":"sha256-...","after_project_revision":"sha256-...","writes":"preview_only","authority":["canvas.source_edit:project"],"audit":{"touched_files":[{"path":"packages/app/pkg.jet","revision":"sha256-...","changed":true}],"diagnostics":[]},"diff":"diff -- packages/app/pkg.jet\n--- before\n+++ after\n+    logging: path@../logging,\n"}
```

Failure response:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source file changed since this Canvas project was drawn"}
```

A stale `project_revision` or touched-file `revision` fails before any write.
Preview mode and rejected transactions leave source untouched. Apply mode writes
only after the changed `pkg.jet` parses through Jetpack's manifest parser.

## Graph Document V1

Endpoint: `GET /__jet_canvas/graph` or `GET /canvas/graph`.
Add `?source_id=<project-relative .jet path>` to project a source file inside
the opened package/workspace. Canvas resolves the path through
`jet.canvas.project` source-truth files and rejects paths outside the project.

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
| `regions` | Source-backed regions/comments. V2 comment boxes are local editor view state unless explicitly converted to shared source hints. |
| `inline_exprs` | Editable Jet expression source rendered inline. |
| `rails` | Visual rail classes present in this graph: control, data, fallible, async, effect, proof, debug. |

The editor shell reads existing v1 fields; no schema bump is needed for the
Blueprint-style sidebars. The left **Files** list comes from
`jet.canvas.project.files` and opens graphs by passing that row's path as
`source_id`. The **Functions** list comes from `graphs`. The **Variables** list
for the open function comes from `graph.function.params` plus local
binding/assignment/get nodes and their typed pins. The right **Details** panel
uses `graph.function` for editable function inputs/output, `inline_exprs` for
editable local initializer values, and existing rename/signature/inline-edit
transactions for writes.

Each node carries `node_id`, `kind`, `archetype`, `title`, `source_span`,
`layout`, `badges`, and `edit_affordances`. `archetype` is one of `value`,
`function_exec`, `function_pure`, `control`, or `entry`. Function, method, and
dispatch-shaped calls all project as `kind:"function"`; enum construction stays
`kind:"variant"`. Exec pins use `type:"exec"`.

Each pin carries `pin_id`, `node_id`, `name`, `direction`, `type`, optional
`role`, optional `pattern_source`, `capability`, `fallible`,
`effect_grant_need`, and `source_span`. Pattern-match branch and dispatch arm
exec pins use `role:"arm"` plus `pattern_source` so Canvas can render one
labeled output row per source arm. A v1 pin span is anchored to its owning
source node when the compiler does not yet expose a narrower pin-specific span.

Shared Canvas comment hints persist as ordinary source comments:

```jet
// canvas:comment span=120..260 title="damage path" color="#2f80ed" alpha=0.25 bounds=(10,20,320,140)
```

The `span` anchor is shared truth. `title`, `color`, `alpha`, and `bounds`
carry visual intent only; stale anchors degrade to auto-layout/local view state.
V2 free comment boxes, staged nodes, staged wires, and copy/paste clipboard
state are private editor view state. They do not appear in graph JSON and do not
write Jet source until a staged node is connected to a source-backed pin or a
paste operation creates a valid source transaction.

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
| `insert_call` | `graph_id`, `callee`, `args`, optional `bind`, optional `wire_origin_pin_id`, `wire_target_pin`, `wire_expr`, `wire_inline_expr_id` | Inserts an ordinary Jet call in a graph's source body. When opened from a pin drag, `wire_origin_pin_id` names the origin pin, `wire_target_pin` names the new node pin chosen by the client, `wire_expr` supplies the origin value expression for output-pin fan-out, and `wire_inline_expr_id` replaces an input pin's inline source expression with the new call in the same transaction. |
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
any write. Semantic failures return the exact rendered Jet diagnostic text and a
structured `diagnostics` array for persistent panels and node bubbles.

Pin-drag insertion is still source truth: the transaction writes either an
ordinary call statement (`wire_expr` becomes a call argument) or an ordinary call
expression in the target input (`wire_inline_expr_id`). The returned graph then
projects the real wire from source; Canvas does not persist semantic edges in a
side graph asset.

Successful response:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"changed":true,"revision":"sha256-..."}
```

Failure response:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source changed since this Canvas graph was drawn"}
```

Diagnostic failure response:

```json
{"protocol":"jet.canvas.edit","schema_version":1,"ok":false,"kind":"diagnostic","message":"Error [E0107]: ...","revision":"sha256-...","diagnostic_revision":"sha256-...","diagnostics":[{"code":"E0107","severity":"error","what":"nothing named `x` exists here","why":"only names that have been defined can be used","fix":"define `x` before this line","message":"nothing named `x` exists here","rendered":"Error [E0107]: ...\n Why: ...\n Fix: ...\n","source_span":{"start":42,"end":43,"line":3,"column":11},"source_path":"/repo/main.jet"}]}
```

Every successful write runs through `jet fmt`, re-checks through the front end,
then reprojects from source. Canvas does not own a parser, checker, graph asset,
or semantic sidecar.

## Query V1

Endpoint: `POST /__jet_canvas/query` or `POST /canvas/query`.

Queries are read-only. They use the shared semindex facts that LSP uses for
definitions, references, rename ranges, and impact analysis.
Requests may include `source_id` to query a different source file inside the
opened project. The `revision` must match that selected source file.

| `op` | Extra fields | Effect |
|---|---|---|
| `find` / `project_search` | `query` | Finds matching graph nodes, definitions, references, and source text. |
| `references` | `symbol` | Returns definition/reference sites plus impact facts. |
| `source_to_graph` | `start`, `end` | Maps a source byte span to matching graph nodes/inline expressions. |
| `preview_rename` | `symbol`, `to` | Returns the exact text diff for a rename without writing source. |
| `actions` / `palette_entries` | none | Returns source-backed palette entries, `project_functions`, and ratified Canvas actions derived from checked semindex facts. |
| `core_catalog` / `corelib_catalog` | optional `query` | Returns read-only `core.*` modules and members from the canonical Core library reference. |

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

The `core_catalog` query is browse-only. Core entries in the actions palette are
source-backed insert candidates when `available:true`: they carry module path,
signature, `pure`, `insert_callee`, `insert_op:"insert_call"`, source document,
ordinary source-edit authority, and `writes:"source_transaction_only"`. Rows with
`available:false` stay visible and disabled; `unavailable_reason_code` is the
machine-readable reason and `denied_reason` is the hover text. They still execute
only after the existing `insert_call`/preview source transaction validates.

Terms:

| Term | Meaning |
|---|---|
| Palette entry | Read-only function/type/docs metadata projected from ordinary Jet code. |
| Canvas action | Behavior-producing Jet action with explicit authority and audited output. |
| Command action | Existing Jet/Jetpack command surfaced with authority, command argv, write class, and approval state. |
| External adapter | Opt-in native/tool bridge for heavyweight integrations. |

Query actions:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"actions","revision":"sha256-...","results":[],"impact":null,"diff":null,"actions_schema_version":1,"project_functions":[{"name":"square","signature":"fn square(n: Int) -> Int","callee":"square","module_path":"main.jet","pure":true,"ret":"Int","pins":[{"name":"n","direction":"input","type":"Int"}],"default_args":["1"],"available":true,"insert_op":"insert_call"}],"actions":[{"action_id":"canvas.action:main.jet:square","kind":"canvas.action","title":"square","callee":"square","engine":"checked-tir+jit","authority":["canvas.source_edit:package"],"package_id":"app","version":"0.1.0","touched_files":["main.jet"],"writes":"source_transaction_only"},{"action_id":"canvas.core_catalog:core.math:abs","kind":"canvas.core_catalog","title":"abs · core.math","module_path":"core.math","callee":"math.abs","insert_callee":"math.abs","insert_op":"insert_call","engine":"checked-tir+jit","execution":"source_transaction","available":true,"authority":["canvas.source_edit:package"],"writes":"source_transaction_only","signature":"abs(x)","pure":true,"source":"docs/reference/core-library.md"},{"action_id":"canvas.core_catalog:core.args:help","kind":"canvas.core_catalog","title":"help · core.args","module_path":"core.args","available":false,"unavailable_reason_code":"method_only","denied_reason":"Use this as a method on an ArgsSpec value.","writes":"source_transaction_only"},{"action_id":"canvas.command:run","kind":"canvas.command","title":"Run program","op":"command_authority","engine":"jet-cli","execution":"external_command","available":true,"command":["jet","run","main.jet"],"authority":["canvas.command:run","canvas.source_edit:package"],"package_id":"app","version":"0.1.0","touched_files":["main.jet"],"writes":"none","requires_confirmation":false}]}
```

Preview an action:

```json
{"schema_version":1,"op":"preview_canvas_action","revision":"sha256-...","graph_id":"fn:main.jet::run@0-3","action_id":"canvas.action:main.jet:square","callee":"square","args":["1"]}
```

Successful response:

```json
{"protocol":"jet.canvas.action","schema_version":1,"ok":true,"changed":true,"engine":"checked-tir+jit","execution":"preview","writes":"source_transaction_only","authority":["canvas.source_edit:package"],"audit":{"package_id":"app","version":"0.1.0","hash":"sha256-...","touched_files":["main.jet"],"diagnostics":[]},"diff":"--- before\n+++ after\n+    square(1)\n"}
```

The audit payload records package id/version/hash, touched files, diff, and
diagnostics. Command actions do not execute through the source-preview endpoint:
Canvas shows exact `command`, `authority`, `writes`, `available`, and
`denied_reason`; the Run button opens that authority card instead of simulating a
run.

Execute an approved command:

```json
{"schema_version":1,"action_id":"canvas.command:check","revision":"sha256-...","source_text":"fn run() {\n    missing\n}\n"}
```

Endpoint: `POST /__jet_canvas/command` or `POST /canvas/command`.

The endpoint accepts only whitelisted Canvas command actions (`run`, `check`,
`build`). `build` writes build outputs and requires `confirmed:true`. The server
does not accept arbitrary argv.

Successful receipt:

```json
{"protocol":"jet.canvas.command_receipt","schema_version":1,"ok":true,"action_id":"canvas.command:check","title":"Check project","revision":"sha256-...","checked_revision":"sha256-...","command":["jet","check","main.jet"],"writes":"none","success":false,"exit_code":1,"elapsed_ms":42,"stdout":"","stderr":"Error [E0107]: ...","diagnostics":[{"code":"E0107","severity":"error","what":"nothing named `missing` exists here","why":"only names that have been defined can be used","fix":"define `missing` before this line","message":"nothing named `missing` exists here","rendered":"Error [E0107]: ...\n Why: ...\n Fix: ...\n","source_span":{"start":15,"end":22,"line":2,"column":5},"source_path":"/repo/main.jet"}]}
```

For Check, `source_text` is optional. When present, Canvas checks that open
buffer text in memory through the same front-end machinery and does not write it
to disk. `checked_revision` tags diagnostics so later rechecks and reprojects
clear stale rows and node bubbles.

Execution receipts are missing until an approved command path runs the exact
source revision. A future external adapter must ask for extra authority before
any tool, file, network, cache, or unsafe access.

## Functions And Callback Views V1

Every function graph carries source-backed metadata:

```json
{"function":{"name":"on_start","signature":"pub fn on_start(limit: Int = <default@31-32>) -> Int","visibility":"public","docs":"Starts the scene.","pure":false,"unsafe":false,"returns":"Int","params":[{"name":"limit","type":"Int","default":true,"default_source":"1"}],"meta":{"category":"Movement","tunable":true},"edit_affordances":["rename_function","edit_function_signature","create_function","source_jump"]}}
```

`#Meta(category: "...", tunable)` projects as `meta: {"category": <string|null>,
"tunable": <bool>}` on annotated function metadata and binding nodes. Unannotated
items use `meta: null`. The field is source-backed and read-only in v1.

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

Canvas treats Git text as source-control truth. It reports project dirty state,
recent entry-file history, and per-file status/diff for the package/workspace
source-truth file set. It never creates a graph lock, checkout state, or binary
asset source of truth.

Successful response:

```json
{"protocol":"jet.canvas.source_control","schema_version":1,"ok":true,"revision":"sha256-...","project_revision":"sha256-...","project_root":"/repo","available":true,"dirty":true,"dirty_files":2,"status":"M packages/app/main.jet\n?? packages/app/helper.jet","diff":"","history":["abc123 initial"],"files":[{"path":"packages/app/main.jet","revision":"sha256-...","kind":"source","available":true,"dirty":true,"status":"M packages/app/main.jet","diff":"diff --git ..."}]}
```

## Proof V1

Endpoint: `GET /__jet_canvas/proof` or `GET /canvas/proof`.

Optional query: `source_id=<project-relative .jet path>`.

Canvas proof reports what is known for the selected source revision: front-end
check state, Git text state, debug persistence, and whether a command authority
receipt exists. It does not manufacture build/run proof. Until a real Canvas
command runs for the exact source revision, `command_receipts.state` and
`proof.state` are `missing`. After a whitelisted command returns, the proof rail
embeds the receipt and marks the revision current.

Successful response:

```json
{"protocol":"jet.canvas.proof","schema_version":1,"ok":true,"source_id":"helper.jet","source_path":"/repo/helper.jet","revision":"sha256-...","check":{"state":"ok","diagnostics_count":0,"message":"front end check passed"},"source_control":{"truth":"git-text","available":true,"dirty":false,"status":""},"debug":{"state":"local-only","persistence":"local-source-span"},"command_receipts":{"state":"missing","reason":"no Canvas command authority receipt has run for this source revision"},"proof":{"state":"missing","stale":true,"reasons":["no check/build/run receipt for this source revision"]}}
```
