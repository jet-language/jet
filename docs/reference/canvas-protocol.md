# Canvas Protocol

Canvas is a source-backed projection protocol. The `.jet` file is the only
semantic source of truth. Clients may cache viewport state locally, but graph
facts and edits come from checked Jet source. Current AST coverage status is
ratcheted in [`canvas-parity.md`](canvas-parity.md).

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Project Document V1

Endpoint: `GET /__jet_canvas/project` or `GET /canvas/project`.

The project document is the workspace/package layer above file graphs. It is
read-only in v1. Its source truth is ordinary Jet project files:

- single-file mode: the opened `.jet` file;
- package mode: `package.jet` plus package source files;
- workspace mode: `workspace.jet`, member `package.jet` files, member source files,
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
| `capabilities` | Checked panel capabilities. Graph, code, and diagnostics are always present; runtime output, terminal, service, preview, and designer appear only when supported. |
| `workspace` | `workspace.jet` projection with member package names/paths, or `null`. |
| `packages` | Parsed `package.jet` facts for the root package and workspace members. |
| `targets` | Package/build targets projected from `package.jet` with package path and manifest source. |
| `outputs` | The valid build outputs exposed by the normal IDE launcher. Selecting one updates the resident session run selection. |
| `envs` / `services` | `env.jet` projection from Jetpack module evaluation, including package refs, prompt, secrets, and dev services. |
| `files` | Projected source-truth files with per-file revisions and kinds. |
| `locks` | `.jet/lock` facts used by the projection. |
| `diagnostics` | Project-level Jet diagnostics. |
| `source_control` | Git text-truth summary handle; source control remains file text truth. |
| `state_policy` | Ratified boundary: semantics from source, private view state local. |

Example:

```json
{"protocol":"jet.canvas.project","schema_version":1,"project_root":"/repo","project_revision":"sha256-...","entry":"packages/game/src/main.jet","mode":"workspace","workspace":{"path":"workspace.jet","members":[{"name":"game","path":"packages/game"}],"diagnostics":[]},"packages":[{"path":"packages/game","manifest":"packages/game/package.jet","name":"game","version":"0.1.0","target":"web","deps":[],"targets":[{"package":"game","target":"executable"}],"effects_enabled":false,"diagnostics":[]}],"targets":[{"package":"game","package_path":"packages/game","manifest":"packages/game/package.jet","target":"executable"}],"outputs":[{"package":"game","package_path":"packages/game","manifest":"packages/game/package.jet","target":"executable"}],"envs":[],"services":[],"files":[{"path":"workspace.jet","revision":"sha256-...","kind":"workspace"}],"locks":[],"diagnostics":[],"source_control":{"truth":"git-text"},"state_policy":{"semantic":"source","local":["tabs","viewport","selection","breakpoints","watches","comment_boxes","staged_nodes"],"shared_visual":"source-anchored-comments"}}
```

Project documents do not create a Canvas project asset. Package/workspace
semantics must remain in `package.jet`, `workspace.jet`, source files, env source,
and `.jet/lock`. Local UI state such as tabs, zoom, selected nodes, breakpoints,
and watches may be cached locally; shared visual intent uses source-anchored
Canvas comments/collapse hints only when the user chooses to share it.

## Resident Session V1

Endpoint: `GET /__jet_canvas/session` or `GET /canvas/session`.

Canvas, text, graph, designer, preview, terminal, debugger, tests, and custom
server views read one resident session. The session response is the shared
identity and source stream; it is not a second semantic source of truth.

```json
{"protocol":"jet.canvas.session","schema_version":1,"session":{"id":"jet-session-123-1","source_revision":"sha256-...","accepted_revision":"sha256-...","last_good_revision":"sha256-...","last_good_program":"web-build-2","state":"ready","clients":2,"run":{"output":"web","target":"browser"},"debugger":{"state":"active"},"tests":{"state":"idle"},"history":{"count":4,"receipts":[{"kind":"replace_source","status":"accepted","before":"sha256-old","after":"sha256-new","client":"client-a"}]},"listeners":{"canvas":{"host":"127.0.0.1","port":8080,"transport":"canvas"},"application":{"host":"127.0.0.1","port":49152,"transport":"application","routes":"application-owned"}},"custom_servers":{"owner":"application","transport":"application","reload":"source-transaction"}}}
```

Source and project transactions append accepted or refused receipts to this
shared history. They keep the existing revision guard: a stale revision is
refused before write, and undo/redo is another checked source transaction.
`last_good_revision` and `last_good_program` change only after a successful
rebuild, so a failed rebuild can expose current diagnostics while the preview
and graph remain on the last accepted program. Reconnect reads the same
session object, preserving accepted edits, run selection, debugger state,
history, and last-good program for every client.

The Canvas and application listeners are intentionally different. Canvas
routes belong to the IDE transport. Application routes, custom hosts, ports,
middleware, and reload policy remain application-owned and are only described
by the session boundary.

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
| `add_dependency` | `manifest`, `name`, `spec` | Inserts or updates one `deps:` entry in a Package file using the existing manifest edit helper, then validates the manifest parser before write. |
| `remove_dependency` | `manifest`, `name` | Removes one `deps:` entry from a Package file using the existing manifest edit helper, then validates the manifest parser before write. |
| `edit_pkg_field` | `manifest`, `field`, `value` | Edits known string fields in a Package file (or its migration-era `payload` block) and validates the manifest parser before write. |
| `add_target` | `manifest`, `name`, `target` | Inserts or updates one Package output/target entry and validates the manifest parser before write. |
| `create_package` | `package_path`, `name`, `target`, optional `entry` | Creates a package directory with `package.jet` and an entry `.jet` file, then validates the manifest parser and generated entry syntax before write. New files must appear in `files` with revision `missing`. |
| `add_workspace_member` | `workspace`, `member_path` | Creates or edits `workspace.jet` to include a package directory, then validates the workspace evaluator before write. Existing explicit member lists are edited in source; `find("./dir")` workspaces no-op when the member path is already covered. |
| `add_env_service` | `env`, `name`, optional `enable`, `port`, `run` (string array), `ready`, typed `shutdown`, `data_dir` | Creates or edits `env.jet` to include a dev service, then validates Jetpack module evaluation before write. |
| `rename_binding` / `rename_function` | `source_id`, `from`, `to` | Renames the selected semantic definition and every resolved project reference through one checked multi-file source transaction. The touched `files` envelope must include every changed source. |

Successful response:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":true,"op":"add_dependency","preview":true,"changed":true,"project_revision":"sha256-...","after_project_revision":"sha256-...","writes":"preview_only","authority":["canvas.source_edit:project"],"audit":{"touched_files":[{"path":"packages/app/package.jet","revision":"sha256-...","changed":true}],"diagnostics":[]},"diff":"diff -- packages/app/package.jet\n--- before\n+++ after\n+    logging: ../logging,\n"}
```

Failure response:

```json
{"protocol":"jet.canvas.project.edit","schema_version":1,"ok":false,"kind":"conflict","message":"source file changed since this Canvas project was drawn"}
```

A stale `project_revision` or touched-file `revision` fails before any write.
Preview mode and rejected transactions leave source untouched. Apply mode writes
only after the changed source overlay passes the Jet front end and the changed
Package file parses through Jetpack's manifest parser. Project rename commits
recheck the whole project revision while holding the shared source transaction
seam, then publish every changed file with rollback if a later publish fails.
The publish step compares each expected source snapshot again, atomically
replaces each changed file from a synced temporary, and uses the same source
transaction seam for rollback when a later file cannot be published. A conflict
or I/O failure leaves every source file and the client's undo history at its
previous committed snapshot.

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
| `source_id` | Project-relative display path for the source file that was projected. Every graph response uses this same id, including a graph selected from the project file rail. |
| `revision` | Stable source hash. Edit transactions must echo this. |
| `fmt_fingerprint` | Hash of the formatter-normalized source. Used to detect formatter drift. |
| `source_text` | Current source text. Canvas uses this for local undo/redo; clients may ignore it. |
| `graphs` | Function/test/lambda graph documents. |
| `diagnostics` | Jet diagnostics already emitted by parser/sema. Never rustc output. |
| `facts` | Semindex schema/version handles used by the projection, plus non-semantic Blueprint-parity facts. |

`facts.blueprint` contains source-derived Canvas affordances that do not change
program meaning, plus an optional live Event projection:

- `state_graphs`: checked typestate diagram facts from sema. Each state carries
  `terminal`, `reachable` (`null` when no entry transition exists), and its
  source span; each edge carries its operation, source state, and destination.
  This is erased compile-time metadata, not runtime state storage.
- `runtime_events`: `null` unless Canvas is opened as `/canvas?pid=<live Jet
  pid>`. With a PID, the graph request uses the same owner/identity/age checks as
  `jet inspect live` and projects only executed, payload-free
  `Event`/`AsyncEvent`/`DecisionHook` observations: subscriptions, queue and
  backpressure counts, priority, failures, and lifecycle. Source calls that did
  not execute never appear.
- `event_dispatchers`: checked source facts for `core.event` constructors and
  Event/AsyncEvent/Hook/DecisionHook/Subscription/EventScope calls. Each fact
  carries the source span and source text, the resolved receiver type, and the
  source-backed subscription scope when one exists. These facts are ordinary
  Jet source truth; they never populate `runtime_events` and do not claim that
  a source call executed. The Events panel exposes source jumps and the
  existing checked `core.event` action palette for creating another ordinary
  call.
- `interfaces`: source-authored trait and trait-impl facts for Canvas interface
  views and create-impl transactions. Compiler-generated derives stay out of
  this source-authoring surface. Each fact carries its module scope, associated
  types, canonical method signatures, required/default status, effect row, and
  source span. The Traits panel exposes source jumps and a checked
  implementation action for traits without associated-type choices; it does
  not guess those choices.
- `task_flows`: `task` spawn/join/channel/`task.group` facts for async rails.

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
| `inline_exprs` | Editable Jet expression source rendered inline. Composite values keep one source-backed expression anchor; Details may edit validated list/map items or nested tuple/struct members by rebuilding that expression before the existing `edit_inline_expr` transaction. |
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

Details renders an editable control only when the field has a live source
operation. A child edit rebuilds its parent expression, then uses the checked
`edit_inline_expr` transaction. Validation, stale revision refusal, Escape,
blur, undo, and redo keep the original source when an edit is not valid.

Each node carries `node_id`, `kind`, `archetype`, `title`, `source_span`,
`layout`, `badges`, and `edit_affordances`. `archetype` is one of `value`,
`function_exec`, `function_pure`, `control`, or `entry`. Function, method, and
dispatch-shaped calls all project as `kind:"function"`; enum construction stays
`kind:"variant"`. Exec pins use `type:"exec"`.

Each pin carries `pin_id`, `node_id`, `name`, `direction`, `type`, optional
`role`, optional `pattern_source`, `ability`, `fallible`,
`effect_grant_need`, and `source_span`. Pattern-match branch and dispatch arm
exec pins use `role:"arm"` plus `pattern_source` so Canvas can render one
labeled output row per source arm; editable arms also carry
`pattern_source_span`. List-literal item pins carry narrow `source_span`,
`append_op:"remove_multi_input_element"`, and `element_index`.
A v1 pin span is anchored to its owning source node when the compiler does not
yet expose a narrower pin-specific span.

Loop execution outputs use `role:"loop_body"` and `role:"loop_done"` with
names `body` and `done`. Early-return outputs use `role:"early_return"` and
name `return`. These pins use the owning source node span, so rewire and
preview actions retain source provenance. A second compatible execution drop
opens a no-write convergence preview. The preview keeps the incoming and target
pin identities plus their source spans, lets the user name an extracted helper,
and applies only through `replace_source` with
`source_edit:"exec_convergence"`. The server resolves the checked AST, formats
the ordinary Jet helper/calls, sema-checks the complete candidate, and writes
only that candidate. A structured join after a branch remains one downstream
source step; it is not extracted.

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

Source-backed paste uses a `replace_source` transaction with
`source_edit:"paste_clone"`. The client renames cloned bindings against the
current source, inserts the clone after the selected source span, and shows the
rename pairs in Details. The clipboard carries the source revision; a changed
revision rejects paste without writing source. “Paste as staged” always keeps
the result local until a compatible source-backed connection materializes it.
Entry and return anchors are not copyable. A mixed or source-incompatible
selection uses staged fallback only when every selected node has an insertable
descriptor; otherwise the UI explains the refusal before any source
transaction. Undo and redo restore the exact checked source through the same
revision-guarded transaction path. A rejected or failed restore puts the
popped history entry back before it reports the refusal, so the current source
and the recoverable undo/redo history remain intact. The Canvas state panel
shows the server reason for stale/conflicting restores and offers source
recovery or reload.

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
effects/abilities, unsafe/proof regions, and runtime debug overlays. A rail
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
| `source_id` | Optional project-relative `.jet` source selected in the Canvas file rail. The revision, breakpoints, values, and execution state apply to this file. |
| `revision` | Source revision from the graph document. |
| `session_id` | Returned by a running response and required to continue or stop that source- and revision-bound session. Omit it to start a session. |
| `tier` | Optional execution tier. The canonical values are `jet-dev-interpreter` for the executing `jet dev` session and `native-lldb` for a compiled debug artifact. A continuation must keep the session's tier. |
| `commands` | Debugger commands using the `jet debug` vocabulary: `step`, `next`, `continue`, `finish`, `locals`, `print`, `backtrace`. |
| `breakpoint_spans` | Local source-span anchors encoded as `start:end`. |
| `breakpoints` | Optional line breakpoints for clients that already mapped spans. |
| `watches` | Local names to print at the stopped frame. |
| `stop` | Optional boolean. With `session_id`, ends the live session without changing source. |

Successful response:

```json
{"protocol":"jet.canvas.debug","schema_version":1,"ok":true,"source_id":"main.jet","revision":"sha256-...","session":{"id":"canvas-debug-1","state":"running","tier":"jet-dev-interpreter","persistence":"local-source-span","source_id":"main.jet","revision":"sha256-..."},"overlay":{"debug_overlay":"running","runtime_state":"live","source_id":"main.jet","revision":"sha256-...","active_line":12,"active_span":{"start":240,"end":258},"active_graph_id":"fn:main.jet::run@1-20","active_node_id":"fn:main.jet::run@1-20:stmt:7","active_wire_id":"","wire_path":[],"breakpoints":[{"line":12,"source_span":{"start":240,"end":258},"state":"valid"}],"locals":[{"name":"total","type":"Int","value":"6"}],"watches":[],"call_stack":["#0  run()  at main.jet:12"],"trace":["breakpoint hit  main.jet:12  in run()"],"limits":{"locals_truncated":false,"watches_truncated":false,"call_stack_truncated":false,"trace_truncated":false,"wire_path_truncated":false}}}
```

The first request creates a live source-level session. runtime_state:"live"
means the values, stack, active node, and wire path came from the current
paused runtime snapshot. finished values are historical only; Canvas labels
them as such and does not pulse the graph. A disconnected or stale session
clears its cached overlay before showing the disconnected/stale state. A later
request sends
the returned `session_id` and one or more commands to continue the same
source/revision session. The active line, node, wire, locals, watches, call
stack, and trace are all projections of that one stop. A finished or stopped
session has no active source or graph IDs, so Canvas never pulses without a
live session. The response includes `tier` so a compiled run can project its
native lldb/DAP session through the same protocol while `jet dev` projects its
executing tier's own session.

Stop requests are checked against the live session's source path, revision, and
tier before the session is removed. A mismatched tier or source is refused; a
stale request returns a conflict and keeps a newer live session. A session
whose own revision is stale is invalidated. The overlay's `limits` field flags
truncation of large locals, watch values, call stacks, or traces.

`native-lldb` compiles the current source with the debugger line map, then
uses the native lldb adapter with the same bounded replay history. If the
native toolchain or debugger is unavailable, the request returns a structured
diagnostic and leaves the source unchanged; it never falls back to the
interpreter while claiming a native tier.

Breakpoints that no longer map to the current source are returned with
`"state":"stale"`; they are not silently moved. Session state is bounded to
32 live sessions, 64 commands, 128 breakpoints, 32 watches, 32 call-stack
frames, and 128 trace entries. Stale revisions, missing sessions, runtime
disconnects, and unsupported debugger boundaries return a structured Canvas
diagnostic. Source edits are never part of this endpoint and remain intact on
every failure.

Canvas sends only breakpoint anchors for the current source revision. When a
source reload, source switch, stale response, or runtime disconnect abandons a
live session, it sends a bounded stop request for that exact source, revision,
and tier. A failed cleanup does not clear or rewrite source; the server's live
session cap bounds any unreachable session.

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
| `source_id` | Optional project-relative `.jet` path from `jet.canvas.project.files`; when present, the transaction applies to that selected source file. |

Current transactions:

| `op` | Extra fields | Effect |
|---|---|---|
| `noop` | none | Reprojects without changing source. |
| `rename_binding` | `from`, `to` | Renames a Jet binding through source edits. |
| `edit_inline_expr` | `inline_expr_id`, `new_expr` | Replaces one inline Jet expression after front-end validation. |
| `promote_to_binding` | `inline_expr_id`, `name` | Inserts an ordinary Jet binding before the owning source line and replaces the inline expression with that name. |
| `insert_visible_conversion` | `inline_expr_id`, `callee` | Wraps an inline expression in an ordinary Jet conversion/function call. |
| `insert_call` | `graph_id`, `callee`, `args`, optional `bind`, optional `wire_origin_pin_id`, `wire_target_pin`, `wire_expr`, `wire_inline_expr_id` | Inserts an ordinary Jet call in a graph's source body. When opened from a pin drag, `wire_origin_pin_id` names the origin pin, `wire_target_pin` names the new node pin chosen by the client, `wire_expr` supplies the origin value expression for output-pin fan-out, and `wire_inline_expr_id` replaces an input pin's inline source expression with the new call in the same transaction. |
| `reorder_statements` | `graph_id`, `moved_start`, `moved_end`, `anchor_start`, `anchor_end`, optional `position` (`before`/`after`) | Moves one source statement within the same checked block, formats, rechecks, and reprojects. Canvas uses this for exec-wire endpoint rewiring. Cross-block moves fail with `can't move a step into a different branch yet`; semantic reorder failures return the normal Jet diagnostic payload. |
| `add_pattern_arm` | `graph_id`, `node_start`, `node_end`, `pattern` | Appends a checked Jet pattern arm to a branch/dispatch node. `pattern` may be written with or without leading `==`; new arm bodies use a sema-safe default `return …` for value functions or `print("canvas arm")` for `Void` functions. |
| `edit_pattern_arm` | `graph_id`, `pattern_start`, `pattern_end`, `pattern` | Replaces one arm pattern source, then formats, checks, and reprojects. Bad patterns return normal Jet diagnostics and leave source unchanged. |
| `remove_pattern_arm` | `graph_id`, `pattern_start`, `pattern_end` | Deletes one pattern arm and its body. Removing the last remaining arm is refused in plain language before source would become invalid. |
| `toggle_switch_state` | `graph_id`, `node_start`, `node_end` | Removes an existing `#Off`/`#DebugOnly` marker or adds `#Off` to a checked statement, then formats, checks, and reprojects. State-contained nodes are refused without changing source. |
| `append_multi_input` | `node_start`, `node_end`, optional `element` | Appends an element to a list literal source node. Clients normally supply a type-derived default element and open inline edit after reproject. |
| `remove_multi_input_element` | `node_start`, `node_end`, `element_start`, `element_end` | Removes one list element, including the adjacent separator, then formats, checks, and reprojects. |
| `create_trait_impl` | `type_name`, `trait_name` | Appends an ordinary `impl Type.Trait { ... }` block with source-checked stubs for required members. Default-body members are not regenerated; traits with associated types are refused until their choices exist in source. |
| `break_link` | `wire_id` | Replaces the source expression behind a wire with `#Todo`, preserving Jet type checking. |
| `move_link` | `wire_id`, `replacement` | Rewrites the source expression behind a wire to another visible Jet name/path. |
| `replace_source` | `source` | Replaces the file with exact prior/future Jet source after formatting and front-end validation. Used by local undo/redo. |
| `replace_source` with `source_edit:"exec_convergence"` | `graph_id`, `from_pin_name`, `from_start`, `from_end`, `target_start`, `target_end`, `strategy` (`extract`/`helper`/`duplicate`), `function`, optional `helper_name` | Resolves one source-backed incoming convergence from the checked AST. The target span may cover one or more contiguous complete expression statements selected in the graph. `extract` creates the named helper; `helper` requires an exact existing helper body; `duplicate` copies the target span with an explicit warning. Formatting, sema validation, stale-span refusal, and the atomic write happen before the ordinary edit response. |
| `insert_branch` | `graph_id`, optional `wire_origin_pin_id`, `wire_target_pin` (`exec`) | Inserts an ordinary checked `if true { ... } else { ... }` branch skeleton. A saved exec input inserts before its owning source statement; a saved exec output inserts after it. A stale or unknown pin is refused without writing source. |
| `insert_switch` | `graph_id`, optional `wire_origin_pin_id`, `wire_target_pin` (`exec`) | Inserts an ordinary checked `if 0 == { ... }` dispatch skeleton at the saved exec target, or at the graph body when no target is supplied. |
| `insert_loop` | `graph_id`, optional `wire_origin_pin_id`, `wire_target_pin` (`exec`) | Inserts an ordinary `loop { break }` skeleton at the saved exec target, or at the graph body when no target is supplied. |
| `insert_fallible_rail` | `graph_id`, optional `wire_origin_pin_id`, `wire_target_pin` (`exec`) | Inserts an ordinary fallible-result binding plus `?` propagation skeleton at the saved exec target, or at the graph body when no target is supplied; front-end validation rejects non-fallible contexts. |
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
side graph asset. Convergence uses the same endpoint and records the resulting
source in the normal undo stack; rejected or stale candidates leave the
preview and source recoverable.

Control wires are source-anchored. Each wire still has `source_span`; control
wires also carry `from_source_span` and `to_source_span`, the source spans of
the statements connected by the exec rail. Dragging a control-wire endpoint to a
compatible exec pin sends `reorder_statements` with those statement spans.

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
then reprojects from source. Canvas does not own a parser, checker, or graph
asset. Toolchain refactors may publish a checkpointed semantic-op receipt;
Canvas only reads a receipt whose file path and source revision match. A hand
edit never inherits an older receipt.
The final publish is compare-and-publish against the request's source snapshot.
If the source changes after validation, the transaction returns `kind:"conflict"`
without replacing it; the successful response's `source_text` is the committed
snapshot used by local undo/redo. Failed saves never create an undo entry.

## Query V1

Endpoint: `POST /__jet_canvas/query` or `POST /canvas/query`.

Queries are read-only. They use the shared semindex facts that LSP uses for
definitions, references, rename ranges, and impact analysis.
Requests may include `source_id` to query a different source file inside the
opened project. The `revision` must match that selected source file.
`project_search` and `references` snapshot every projected source file, return
each result's `source_id` and file `revision`, and reject a changed project
before returning mixed-source results. `project_revision` may bind the query to
the project document already shown by the client.

| `op` | Extra fields | Effect |
|---|---|---|
| `find` | `query` | Finds matching graph nodes, definitions, references, and source text in the selected file. |
| `project_search` | `query` | Finds matching graph nodes, definitions, references, and source text across the projected source files. |
| `references` | `symbol` | Returns project-wide definition/reference sites plus impact facts. |
| `source_to_graph` | `start`, `end` | Maps a source byte span to matching graph nodes/inline expressions. |
| `preview_rename` | `symbol`, `to`, optional `project_revision` | Without `project_revision`, returns the single-file diff. With a current `project_revision`, resolves the selected definition through semindex, returns every definition/reference site across the projected source files, and returns a per-file diff envelope for the matching atomic project transaction. |
| `actions` / `palette_entries` | none | Returns source-backed `project_functions` metadata and one authoritative, ranked Canvas action list derived from checked semindex facts. The browser menu consumes the action list once. |
| `core_catalog` / `corelib_catalog` | optional `query` | Returns read-only `core.*` modules and members from the canonical Core library reference. |

Successful response:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"find","revision":"sha256-...","results":[{"kind":"definition","title":"run","graph_id":"...","node_id":"...","source_span":{"start":0,"end":3}}],"impact":null,"diff":null}
```

A stale `revision` or `project_revision` fails with `kind:"conflict"` before
any query result is used. Project responses include `result_limit` and
`truncated`; the server keeps at most 200 result sites and the client discloses
when it must narrow the search. A project rename preview has
`diff.files:[{"path","revision","after_revision","changed"}]`; those
before revisions are the exact touched-file envelope for the apply request.

## Canvas Actions V1

Canvas action boundary: package behavior runs through Jet's existing checked
front end, executable TIR, and JIT preview path. Canvas does not get a separate
runtime, compiler, or graph asset store. An action may return a source
transaction or preview, but it never writes files directly.

The Canvas Library panel is a read-only view over the existing `actions` query.
It groups checked `canvas.core_catalog` and ordinary `canvas.action` entries by
`module_path`, shows each entry's signature, documentation source, typed pins,
and availability reason, and uses the same `insert_call` transaction as the
action palette for edits. Package facts shown beside the library come from the
existing `/canvas/project` projection. The panel does not invent module names,
parse source, or write a graph-side representation; a stale, rejected, or
ill-typed transaction leaves ordinary Jet source unchanged.

The `core_catalog` query is browse-only. Core entries in the actions palette are
source-backed insert candidates when `available:true`: they carry module path,
signature, `pure`, `insert_callee`, `insert_op:"insert_call"`, source document,
ordinary source-edit authority, and `writes:"source_transaction_only"`. Rows with
`callee` and `insert_callee` carry the checked source spelling, including an
import alias; the client must use `insert_callee` and must not derive a fallback
from the display title or ordinary `callee` field.
`available:false` stay visible. Entries with `stageable:true` and
`stage_reason_code` `needs_canvas_defaults` or `method_only` remain active and
place a dashed local node; the first compatible wire runs the existing checked
`insert_call` source transaction. Other unavailable rows stay disabled;
`unavailable_reason_code` is the machine-readable reason and `denied_reason` is
the hover text. A `method_only` row carries its typed `receiver_type`; Canvas
does not invent a `Value` receiver when that type is unknown. No palette action
bypasses the existing source transaction validation.

Terms:

| Term | Meaning |
|---|---|
| Palette entry | Read-only function/type/docs metadata projected from ordinary Jet code. |
| Canvas action | Behavior-producing Jet action with explicit authority and audited output. |
| `receiver_type` | Typed method receiver required by a staged `method_only` entry. |
| Command action | Existing Jet/Jetpack command surfaced with authority, command argv, write class, and approval state. |
| External adapter | Opt-in native/tool bridge for heavyweight integrations. |

Query actions:

```json
{"protocol":"jet.canvas.query","schema_version":1,"ok":true,"op":"actions","revision":"sha256-...","results":[],"impact":null,"diff":null,"actions_schema_version":1,"project_functions":[{"name":"square","signature":"fn square(n: Int) Int","callee":"square","insert_callee":"square","module_path":"main.jet","pure":true,"ret":"Int","pins":[{"name":"n","direction":"input","type":"Int"}],"default_args":["1"],"available":true,"insert_op":"insert_call"}],"actions":[{"action_id":"canvas.action:main.jet:square","kind":"canvas.action","title":"square","callee":"square","insert_callee":"square","engine":"checked-tir+jit","authority":["canvas.source_edit:package"],"package_id":"app","version":"0.1.0","touched_files":["main.jet"],"writes":"source_transaction_only"},{"action_id":"canvas.core_catalog:core.math:abs","kind":"canvas.core_catalog","title":"abs · core.math","module_path":"core.math","callee":"math.abs","insert_callee":"math.abs","insert_op":"insert_call","engine":"checked-tir+jit","execution":"source_transaction","available":true,"stageable":false,"stage_reason_code":"","stage_reason":"","authority":["canvas.source_edit:package"],"writes":"source_transaction_only","signature":"abs(x)","pure":true,"source":"docs/reference/core-library.md"},{"action_id":"canvas.core_catalog:core.args:help","kind":"canvas.core_catalog","title":"help · core.args","module_path":"core.args","available":false,"stageable":true,"stage_reason_code":"method_only","stage_reason":"Use this as a method on an ArgsSpec value.","unavailable_reason_code":"method_only","denied_reason":"Use this as a method on an ArgsSpec value.","writes":"source_transaction_only"},{"action_id":"canvas.command:run","kind":"canvas.command","title":"Run program","op":"command_authority","engine":"jet-cli","execution":"external_command","available":true,"command":["jet","run","main.jet"],"authority":["canvas.command:run","canvas.source_edit:package"],"package_id":"app","version":"0.1.0","touched_files":["main.jet"],"writes":"none","requires_confirmation":false}]}
```

Preview an action:

```json
{"schema_version":1,"op":"preview_canvas_action","revision":"sha256-...","graph_id":"fn:main.jet::run@0-3","action_id":"canvas.action:main.jet:square","callee":"square","args":["1"]}
```

The preview server checks that `callee` exactly matches the descriptor callee
encoded by `action_id`; a mismatch is rejected before source validation and
leaves source unchanged.

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
`build`). Requests may include the graph's `source_id`; the command, checked
revision, diagnostics, and receipt then refer to that selected project file.
`build` writes build outputs and requires `confirmed:true`. The server does not
accept arbitrary argv.

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
{"function":{"name":"on_start","signature":"pub fn on_start(limit: Int{<default@31-32>}) Int","visibility":"public","docs":"Starts the scene.","pure":false,"unsafe":false,"returns":"Int","params":[{"name":"limit","type":"Int","default":true,"default_source":"1"}],"meta":{"category":"Movement","tunable":true},"edit_affordances":["rename_function","edit_function_signature","create_function","source_jump"]}}
```

`#Meta(category: "...", tunable)` projects as `meta: {"category": <string|null>,
"tunable": <bool>}` on annotated function metadata and binding nodes. Unannotated
items use `meta: null`. The field is source-backed and read-only in v1.

The graph `facts.enum_variants` map supplies unit-variant choices as
`{"name":"Fast","source":"Mode.Fast"}` records. Details uses these records
for enum fields and uses same-type binding facts for reference fields. Scalar,
enum, and reference controls all submit the existing `edit_inline_expr` or
`edit_function_signature` transaction, so revision checks, sema validation,
formatting, source spans, undo, and reload remain one path.

Function edits are ordinary source transactions:

```json
{"schema_version":1,"op":"edit_function_signature","revision":"sha256-...","graph_id":"fn:main.jet::on_start@25-33","signature":"pub fn on_start(limit: Int{1}) Int"}
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
{"protocol":"jet.canvas.source_control","schema_version":1,"ok":true,"revision":"sha256-...","project_revision":"sha256-...","project_root":"/repo","available":true,"dirty":true,"dirty_files":2,"status":"M packages/app/main.jet\n?? packages/app/helper.jet","diff":"","history":["abc123 initial"],"files":[{"path":"packages/app/main.jet","revision":"sha256-...","kind":"source","available":true,"dirty":true,"status":"M packages/app/main.jet","diff":"diff --git ...","semantic_ops":[{"kind":"rename","from":"report","to":"summarize"}]}]}
```

## Review Lens M3

The Review lens reads this response and keeps the text diff first. Git already
defines file and hunk additions and deletions, so Canvas does not need a custom
binary-asset diff model. Each current added or modified hunk may link to a
source span and graph node when the current graph exposes an overlapping span.
When `semantic_ops` contains a checkpoint-matching receipt, Review shows the
recorded operation and its targets. It does not infer a rename from similar
text.

Review marks deleted text as deleted because it has no current source span. It
marks other changes without a matching current node as text only. These labels
keep source truth visible without fabricating graph history. Refresh reads the
response and current graph again; Review actions do not write Jet source or Git
state.

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
