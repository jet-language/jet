# Web backend: JavaScript DOM operations and WebAssembly logic

The web backend partitions the checked program between JavaScript for browser/DOM work
and WebAssembly for eligible logic. This page is for web application authors
and compiler contributors choosing a target, reading partition diagnostics, and
checking the source-backed examples. The Jet-hosted CLI builds the web target
from the IR (see [architecture](../architecture.md), the IR consumers): today
it selects the whole program to WebAssembly, and the JavaScript/WebAssembly
partition below is not built yet. Web diagnostics are registered in
[`Compiler/Prelude/Diagnostics.jet`](../../../Compiler/Prelude/Diagnostics.jet).

The ratified decisions are D-WEBBACKEND1, D-WASM1, D-JSBIND1, D-WEBKIND1, and
D-DOMGEN1. Per-function partition pins use `#Target(JS)` and `#Target(Wasm)`;
`#Export(wasm)` marks a Wasm entry called from generated JavaScript (D-EXPORT-WASM1).

## Build and inspect

`jet build --target web FILE` (also `--target wasm.browser`, the browser's
target machine, `--os=web`, or `os: web` in the selected profile) writes
five files beside the entry in `.jet/build/`, whatever the entry is named:
`app.wasm` (the program partition), `rt.wasm` (the runtime partition),
`web.manifest.json`, the entry `app.js` and its host `jet_dom_runtime.js`.
`node .jet/build/app.js` runs the program with the process's arguments and
environment and exits with its status. A page or a harness imports
`jet_main` from `app.js`: it resolves when the program exits 0 and rejects
with `JetHostExitError` (its `status`) otherwise, `JetHostWasmError` on a
trap, or `JetHostAbiError` when the files are not one build's. A stack
overflow, which the JavaScript engine refuses with an exception of its own,
is no trap: the host gives the E3012 stop `jet run` prints, naming the
innermost Jet function at its declaration line with that line's source (read
from `app.wasm.map`, so none when the map is missing), and the program exits
70.
`instantiateWasm` and `takeWasmError` from `jet_dom_runtime.js` drive the
modules directly; the `jet_export_run` that `instantiateWasm` returns runs the
program once and answers a promise of its exit status.

The manifest's `binding` and `app.js`'s `JET_TARGET_BINDING` hold the same
target binding: the machine the program was checked for (`targetMachine`,
`wasmTriple`, `targetEnvironment`, `runtimeLayer`, `providerIdentity` and
`targetDossierIdentity`, the SHA-256 of the dossier `jet inspect target
wasm.browser --json` prints) and the SHA-256 of each module the build wrote
(`runtimeIdentity` for `rt.wasm`, `artifactIdentity` for `app.wasm`). Before
it runs any program code, the host checks that the two agree and that each
module's bytes hash to its identity. Otherwise it stops with `target binding
mismatch`, so a module or manifest that another build wrote never runs
under this entry.

```sh
jet build --target web Examples/Basics/Hello.jet
node Examples/Basics/.jet/build/app.js
```

`jet run` and `jet test` run Linux x86-64 images only and refuse `os: web`; a
package's `build: fn` targets refuse it too. `jet dev` of a `#Target(Web)`
program, or with `--port=N` (the application preview listener), makes this
web build and serves its directory on `127.0.0.1:N` (without `--port`, a port
the listener picks), printing `App preview: http://127.0.0.1:N/` on stdout;
`/` is `index.html`, and `GET /__jet_dev_version` answers the revision of the
last good build, a `__jet_dev_version` file each good build writes beside it.
Every change to the package's sources rebuilds it; a failed rebuild prints its
report and keeps the last good build served. A full-stack
`App` entry is a server, and `jet dev` runs it natively. Under node the host
file system is the program's; a page has none, so file calls fail with
ENOSYS. The build refuses a `task.group` with `E-WEB-UNSUPPORTED`.

A program that waits does not block the page. Where the JavaScript engine
has promise integration (`WebAssembly.Suspending` and
`WebAssembly.promising`), a wait, such as `time.sleep`, a lock or a
channel, suspends the waiting thread on a promise the host settles at the
wake or the deadline, and the page goes on meanwhile: its timers, its
rendering and its other scripts run. The program's run, each click, each
drop and each change of the page's lifecycle a launched app hears
(`graph.launch`) is one entry, and entries take turns: one that arrives
while another is suspended runs once that one has ended. A device
request the browser answers later (`app.ask`, `app.take_photo`,
`app.location`, `app.open_file`, `app.open_folder`, `app.kept_folder`, the
reads of a file the page keeps, a folder's listing and writes, each
reading `app.location_updates`, `app.orientation_updates` and
`app.heading_updates` give, and each call through a `remote_at` Remote,
which the browser's `fetch` sends, through the page's UI provider)
suspends the program the same way until the browser's promise settles.
The program's output so far shows before the page acts on the request.

Tasks run as they do natively, with the same scheduler: its threads and
its tasks' fibers are coroutines on the page's one JavaScript thread, each
on its own stack in the program's memory. One runs at a time, until it
waits, switches fibers or ends; then the next one due runs. A task that
computes without waiting holds the page until it does. A generator's
producer is such a task, so a generator runs on the page as it does
natively.

Without promise integration the program can neither suspend nor hold the
page, so its first such wait or request stops it with E3001, naming the
missing `WebAssembly.Suspending` and `WebAssembly.promising`; it never runs
on past the wait out of order. Its first task start stops it the same way,
after the host prints `jet: this web host can't run tasks: it has no
JavaScript Promise Integration (WebAssembly.Suspending and
WebAssembly.promising)`.

When the page has a `document`, the host shows what the program's `core.ui`
backend paints. Each paint (a `ui.mount`, or a `paint` after `layout`)
replaces the children of `#jet-app`, which the host creates in `document.body`
if the page has none: one element per leaf, placed absolutely at its frame (a
`button` for a button, an `input` for a text input, a `span` for text, a `div`
for any other node; a box stacks its children and adds no element). A click on
a button runs its Jet handler once the program's `run` has returned 0. Each
edit of a text input built with `on_change:` (the browser's `input`) runs its
handler on the input's value; the input the repaint puts in its place keeps
the focus and the selection. A drop on a text input built with `on_drop:`
runs its handler once per dropped item, in the order `core.ui.host.drag_drop`
reads a drop: nonempty `text/plain` as a `UIDropItem.Text`, then each
`text/uri-list` line that is neither empty nor a `#` comment as a
`UIDropItem.URI`. A drop of files on the element of an
`app.drop_files` body hands them to the page's UI provider, which keeps
them, then runs the body's handler, which takes them from the provider as
picked files. The elements of an `app.draggable` body are `draggable`, and
its first element that is neither a button nor an input takes focus
(`tabIndex` 0). A drop on an element of an `app.drop_zone` body, of what a
drag started on such an element, runs the dragged body's handler, which
picks its value up, then the zone's, which takes it; any other drop runs
the zone's handler alone, which refuses it. On the focused element Space
picks the body up and outlines the first drop zone, the arrow keys outline
the next or the previous one, Enter moves it there and Escape puts it
back; its `contextmenu` offers a move to each drop zone of its kind after
any `app.context_menu` entries. The paint a handler causes (for example a
`ui.reactive_render` effect re-running on a signal it read) replaces the
elements again. An app whose graph has screens draws them the same way:
`graph.launch()` opens the page's address (`location.pathname` and
`location.search`, else `/`) and paints its screen, and after each handler
the host runs the app's draw hook, which opens the screen again and paints
it when another screen opened or a live binding the screen read changed.
Under `node app.js` there is no document and nothing is recorded.

## Partition and ABI

The hybrid boundary is:

- view and DOM operations become generated JavaScript calls through
  `jet_dom_runtime.js`;
- pure native computation becomes a `wasm32-unknown-unknown` module with a
  generated loader and bridge;
- explicit `#Target(JS)` compute calls use the browser-owned WebGPU Prelude
  adapter, which awaits queue work and readback;
- browser partition inference follows Browser effect facts and `#Target`
  ceilings.

The bridge accepts ABI-safe scalars, `String`, lists, maps, and explicitly
codable values. A value whose representation is not part of that boundary must
be represented or rejected; it is not copied through a guessed JavaScript or
Wasm layout. `#Export(wasm)` makes a selected Wasm entry callable from generated
JavaScript. `web.manifest.json` records module-to-bucket assignments for the
loader and `--explain-partition`.

This is a checked-program boundary, not a second web language. It does not imply
full Jet-to-JavaScript transpilation, WASI imports, or that every `core.ui`
provider is available in a browser.

## Diagnostics and failure behavior

Unsupported constructs fail before emission with registered diagnostics:

- `E-WEB-UNSUPPORTED` — the construct cannot be lowered for the selected
  web partition;
- `E-WEB-CROSS-PARTITION` — a direct JS/Wasm call lacks a generated bridge;
- `E-WEB-ABI-TYPE` — a value is not valid at the JS/Wasm boundary;
- `E-WEB-TARGET-BROWSER` — a Wasm function requests browser-only targeting or
  DOM access;
- `E-WEB-RUN` — a web-targeted program was sent to the native execution path.

Browser WebGPU calls use the browser-owned asynchronous Prelude. Unsupported
providers and operations return typed unsupported results; they do not silently
fall back to CPU. Native hosts retain the typed fail-closed WebGPU provider.

## Source-backed examples

The top-level source set under `Examples/Apps/Web/` is the contract checked
by [`Tests/Cases/WebExamplesDoc/web_examples_are_documented_and_have_expected_outputs.case`](../../../Tests/Cases/WebExamplesDoc/web_examples_are_documented_and_have_expected_outputs.case).
Each file is named here so documentation review cannot silently omit a new example:

- `AppConfigReadiness.jet`
- `AppFieldErrors.jet`
- `AppHello.jet`
- `AppImages.jet`
- `AppRouteTable.jet`
- `AppTypedArgs.jet`
- `BrowserSurface.jet`
- `FormsTable.jet`
- `JokesAPI.jet`
- `QueryLifecycle.jet`
- `StateSurface.jet`
- `StorageLocalSession.jet`
- `StoreHistory.jet`
- `UIShowcase.jet`
- `UIWebClick.jet`
- `UIWebMount.jet`
- `UIWebReactive.jet`
- `WebApp.jet`
- `WebCompute.jet`
- `WebComputeWebGPU.jet`
- `WebHello.jet`
- `WebWASMCallback.jet`
- `WebWASMIntExport.jet`

Each example's golden sits beside its source in `Examples/Apps/Web/`: a
`.out` file, a browser `.web.out` or `.harness.out` file, or a `.case`
golden. The example source and harness are the executable truth for API
spellings and output behavior.

## Related boundaries

- Target markers and ratified web decisions: [syntax decisions](../syntax-decisions.md).
- `core.web` browser storage and events: [Core library](core-library.md).
- General diagnostic conventions: [diagnostics](../diagnostics.md).
