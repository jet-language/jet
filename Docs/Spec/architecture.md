# Jet compiler architecture

`Compiler/` is the source of truth for package layout and implementation. This
document states the durable architectural boundaries, execution tiers,
concurrency/plugin laws, Core module classification, and stable CLI exit codes.
(Historical package maps and Rust-transpiler rationale remain in git history:
`git show 20be9d938a5:Docs/Spec/architecture.md`.)

## Pipeline and execution tiers

```text
Source (.jet) -> Lex -> Parse -> Resolve/Imports -> Sema -> Comptime -> JetIR -> Native Backend -> Link/Run
```

- **Sema owns all user diagnostics (I3).** Codegen and the native backend lower
  checked facts only; a backend rejection of checked code is an ICE (exit 101,
  I2).
- **One meaning across tiers (I9):** Every feature and Core API executes the
  same Prelude/CoreLib runtime across O0 (`jet run`, `jet dev`, comptime child
  process), O1 (`-O1`), O2 (`jet build`), and web targets where applicable. No
  interpreter or second code generator exists.
- **Self-hosting:** `Compiler/` is written in Jet and builds itself from an
  admitted native seed (`Docs/Spec/Contributing/bootstrap.md`).
- **Compiler data layout:** Compiler phases use compact ID-indexed tables and
  phase-scoped arenas with no reference counting (`Docs/Spec/compiler-data.md`).

## Concurrency and reactive boundaries

D-DATARACE1=C is law. Safe Jet must make a data race impossible to compile for
every safe program, including reactive state and task crossings:

- Values crossing task or thread boundaries must be statically safe to transfer
  or share; unsynchronized mutable aliasing across tasks is rejected in sema.
- `Signal<T>`, `Derived<T>`, and `Computed<T>` use lock-ordered `Arc` storage so
  reactive graph updates remain data-race-free and deadlock-free without an
  external language runtime backstop.

## Plugin boundaries

- **Application plugins:** Export a typed, versioned ABI boundary with explicit
  capability grants; host and plugin exchange value/handle representations
  without sharing raw allocator ownership.
- **Compiler-extension plugins:** Hook into declared compile-time / lint /
  inspection extension points under explicit capability sandboxing; they cannot
  bypass sema safety checks or mutate compiler internals directly.

## Core namespace classification

Every module declared in `Compiler/Prelude/Core.rows` has one architectural
class and semantic home (enforced by `Tests/Guards/architecture_rows.jet`):

| Namespace | Class | Semantic home |
|---|---|---|
| `core.archive` | Host | Package bridge; comptime rejects unsupported host work. |
| `core.args` | Kernel | `Core/Args/Args.jet`; comptime marshals values. |
| `core.auth` | Kernel | `Core/Auth/Auth.jet` and the crypto entropy kernel; comptime marshals values. |
| `core.archive.gzip` | Host | Native compression boundary; comptime rejects unavailable calls. |
| `core.archive.zstd` | Host | Native compression boundary; comptime rejects unavailable calls. |
| `core.compute` | Kernel | `Core/Compute/Compute.jet`; comptime marshals values. |
| `core.crypto` | Kernel | `Core/Crypto/` owns typed crypto rules; every engine marshals values. |
| `core.crypto.expert` | Kernel | The audited raw-byte route shares the crypto Prelude symbols; the `#Unsafe` gate is policy, not a second engine implementation. |
| `core.data` | Kernel | `Core/Math/Stats.jet` and `Core/Data/Plot.jet`; comptime marshals values. |
| `core.data line renderers need groups` | Kernel | `Core/Data/Plot.jet` owns line-renderer argument validation. |
| `core.data line renderers need options` | Kernel | `Core/Data/Plot.jet` owns line-renderer option validation. |
| `core.data: argument must be `[DataGroup]`` | Kernel | `Core/Data/Plot.jet` owns `DataGroup` validation. |
| `core.data: argument must be `[Float]`` | Kernel | `Core/Math/Stats.jet` owns numeric input validation. |
| `core.encoding` | Kernel | Encoding dispatch and value validation use the shared encoding Prelude/Core kernels. |
| `core.encoding.jsonl.to_string: expected a list` | Kernel | JSONL value validation stays with the encoding kernel. |
| `core.encoding.base32` | Kernel | `Core/Encoding/Base32.jet` owns encode and edition-aware decode; comptime marshals values. |
| `core.encoding.base64` | Kernel | `Core/Encoding/Base64.jet` owns encode and edition-aware decode; comptime marshals values. |
| `core.encoding.cbor` | Kernel | Typed CBOR marshals through the shared Dyn/codec kernel. |
| `core.encoding.csv` | Kernel | Typed CSV marshals through the shared codec kernel. |
| `core.encoding.hex` | Kernel | `Core/Encoding/Hex.jet` owns encode/decode; comptime marshals values. |
| `core.encoding.json` | Kernel | Typed JSON uses the shared Dyn/codec kernel. |
| `core.encoding.jsonl` | Kernel | JSONL uses the shared typed JSON kernel. |
| `core.encoding.toml` | Kernel | Typed TOML uses the shared codec kernel. |
| `core.encoding.xml` | Kernel | `Core/Encoding/XMLKernel.jet`; comptime marshals `Dyn` values. |
| `core.encoding.yaml` | Kernel | Typed YAML uses the shared codec kernel. |
| `core.builtin` | Intrinsic | String length and map projections marshal through the shared builtin value operations. |
| `core.sys` | Host | Explicit comptime environment authority and policy boundary. |
| `core.event` | Host | Callback registration crosses the runtime closure boundary; the host only marshals shared event values. |
| `core.files` | Host | Explicit comptime filesystem authority and policy boundary. |
| `core.text.fmt` | Kernel | `Core/Text/Fmt.jet` owns number, decimal, grouping, byte, duration, ordinal, plural, and padding rules; adapters marshal values. |
| `core.term` | Host | I/O authority boundary (`Core/Term/Term.jet`). |
| `core.math.abs: non-numeric argument` | Mixed | Numeric argument validation stays with the shared math rules. |
| `core.math` | Mixed | `Core/Math/` owns residual integer, gcd/lcm, factorial, and erf-family rules; true numeric intrinsics remain intrinsic. |
| `core.net.mime` | Kernel | `Core/Net/MIME.jet`; comptime marshals values. |
| `core.net` | Mixed | `Core/Net/IP.jet` owns IP/socket parse and field rules; sockets and DNS stay authority-bound. |
| `Path` / `core.files` | Kernel + Host | `Core/Files/Path.jet` owns lexical join/parent/extension/stem/normalize rules; filesystem work stays host-bound and accepts `String | Path`. |
| `core.perf` | Host | Runtime performance authority; comptime rejects unavailable calls. |
| `core.process` | Host | Process authority boundary and REPL host adapter. |
| `core.math.random` | Mixed | `Core/Math/Random.jet` owns deterministic seeded RNG rules; ambient RNG stays host-bound. |
| `core.reactive.loadable` | Kernel | `Core/Reactive/Loadable.jet` owns tag/presence rules; typed payloads and handles remain adapters. |
| `core.reflect` | Intrinsic | Compiler-owned type metadata construction. |
| `core.regex` | Kernel | The generated regex engine exposes one shared matching and validation kernel. |
| `core.units` | Kernel | `Core/Units/Units.jet`; all tiers marshal `(value, uncertainty)`. |
| `core.image` | Kernel | `Core/Image/Image.jet` owns format detection, PNG and JPEG decode and encode, resizing and every budget check; comptime marshals values. |
| `core.actor` | Jet | `Core/Actor/Actor.jet` writes spawn, send, ask and supervision over `task` and `channel`, each supervised turn in a `task.group` child; no compiler support. |
| `core.service` | Kernel | Typed tree declarations and the shipped mailbox, lifecycle, authenticated directory/routing, handoff, rollback, partition, reconciliation, and typed observability paths use `Core/Service/Service.jet`; structured export uses the existing `core.log` sink. |
| `core.data.sketch.cms` | Jet | `Core/Data/Sketch/CMS.jet` owns seeded hash selection, unit saturation and additive source merge over plain value storage. |
| `core.data.sketch.hll` | Jet | `Core/Data/Sketch/HLL.jet` owns the 256-register estimator over plain value storage; seeded FNV lives in `Core/Data/Sketch.jet`. |
| `core.data.sketch.reservoir` | Jet | `Core/Data/Sketch/Reservoir.jet` owns the shipped xorshift seed, Algorithm R and slot order over plain value storage. |
| `core.data.sketch.tdigest` | Jet | `Core/Data/Sketch/Tdigest.jet` owns unit-weight insertion, compression and step quantiles over plain value centroid storage. |
| `core.compute.solve` | Kernel | `Core/Compute/Solve.jet`; comptime marshals state. |
| `core.sync` | Kernel | `Core/Sync/Sync.jet`; comptime marshals values. |
| `core.testing` | Mixed | Shared deterministic helpers call Prelude; harness and host work stay adapters. |
| `core.text` | Kernel | `Core/Text/` plus the Unicode tables under `Core/Text/Runtime/` owns text/Unicode rules, including scalar names; comptime marshals values. |
| `core.text.ends_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
| `core.text.starts_any: non-list argument` | Kernel | Text argument validation stays with the text kernel. |
| `core.time` | Mixed | `Core/Time/` owns civil, duration, and zone rules; clock reads remain host effects and every engine marshals the same Prelude symbols. |
| `core.net.tls` | Host | Native TLS boundary; comptime rejects unavailable calls. |
| `core.ui` | Intrinsic | `CtValue` constructors and field projection only. |
| `core.ui.box() needs UINode children` | Intrinsic | UI node-shape validation stays with the intrinsic constructor path. |
| `core.ui.box() needs [UINode]` | Intrinsic | UI list validation stays with the intrinsic constructor path. |
| `core.ui.node_role(): missing role` | Intrinsic | UI role validation stays with the intrinsic constructor path. |
| `core.net.url.data: first argument must be a Mime` | Kernel | URL data validation stays with the shared URL/MIME kernel. |
| `core.net.url.data: mime.sub` | Kernel | URL MIME field validation stays with the shared kernel. |
| `core.net.url.data: mime.top` | Kernel | URL MIME field validation stays with the shared kernel. |
| `core.net.url` | Kernel | `Core/Net/URL.jet` owns URL parse/render/percent rules; comptime marshals values. |
| `app` | Host | `Core/App/App.jet` owns the app's live bindings, endpoints, blobs and notices; device, push and dialog work crosses the host. |
| `jet_builtins` | Intrinsic | `Core/Builtins/Builtins.jet` declares the kernel operations, storage types and markers the compiler itself provides to Core; programs never name it. |
| `core` | Namespace | The root of the Core module tree; it declares nothing. |
| `core.browser` | Host | `Core/Browser/Browser.jet` runs the WebDriver BiDi session in Jet over `core.net.ws`; launching a browser, files and the network stay host-bound. |
| `core.browser.host` | Host | `Core/Browser/Host.jet` asks the page's `core.ui.host` provider for DOM event routes and element values. |
| `core.browser.storage` | Jet | `Core/Browser/Storage.jet` owns the storage namespaces and the provider request each store makes. |
| `core.browser.storage.local` | Host | `Core/Browser/Storage/Local.jet`: the page's localStorage through the `core.ui.host` provider, else a process store. |
| `core.browser.storage.session` | Host | `Core/Browser/Storage/Session.jet`: the page's sessionStorage through the `core.ui.host` provider, else a process store. |
| `core.build` | Jet | `Core/Build/Build.jet` describes build steps as plain values; `jet build` runs them in its sandbox. |
| `core.collections` | Jet | `Core/Collections/Collections.jet` owns counters, heaps and bisection over arrays. |
| `core.collections.set` | Jet | `Core/Collections/Set.jet` owns ordered unique string sets. |
| `core.color` | Jet | `Core/Color/Color.jet` owns the one Core color value and its hex, HSL and other spellings. |
| `core.compiler` | Intrinsic | `Core/Compiler/Compiler.jet` names the extension points library code runs under inside the compiler; their Meta effects are in `Core/Compiler/Meta.jet`. |
| `core.compiler.lang` | Intrinsic | `Core/Compiler/Lang.jet` holds the closed menus of marker arguments that the compiler consults. |
| `core.context` | Namespace | The task context the compiler provides; no source declares it. |
| `core.crypto.random` | Host | `Core/Crypto/Random.jet` is the fail-closed CSPRNG; every request crosses the OS entropy provider. |
| `core.crypto.uuid` | Mixed | `Core/Crypto/UUID.jet` owns UUID formatting, parsing and name-based construction; v4 and v7 entropy crosses `core.crypto.random`. |
| `core.crypto.vault` | Host | `Core/Crypto/Vault.jet` validates names and reasons; the vault provider owns repository state and key material. |
| `core.data.arrow` | Host | `Core/Data/Arrow.jet` carries a foreign Arrow batch's owner; the provider validates and releases it. |
| `core.data.loader` | Jet | `Core/Data/Loader.jet` owns the `DataLoader<T>` lifecycle; payload bytes come from its provider. |
| `core.data.plot` | Jet | `Core/Data/Plot.jet` owns text and SVG charts. |
| `core.data.sketch` | Jet | `Core/Data/Sketch.jet` owns sketch-state framing and the seeded FNV hash. |
| `core.data.stream` | Jet | `Core/Data/Stream.jet` owns the one-shot `DataStream<T>` cursor. |
| `core.db` | Host | `Core/DB/DB.jet` owns SQL values and typed row selection; the private SQLite driver holds the connection. |
| `core.db.query` | Mixed | `Core/DB/Query.jet` owns typed queries over rows; reading a CSV file is file-bound. |
| `core.db.sql` | Jet | `Core/DB/SQL.jet` is the one SQL grammar and row-selection kernel, with no connection authority. |
| `core.debug` | Intrinsic | `Core/Debug/Debug.jet` declares the stop record the runtime passes to `context.on_stop` and the records `context.trace` reads. |
| `core.devtools` | Host | `Core/Devtools/Devtools.jet` owns the inspector identity and field envelope; the raw leaf publishes through the global sink. |
| `core.encoding.binary` | Jet | `Core/Encoding/Binary.jet` owns integer packing and the named float packers. |
| `core.encoding.eetf` | Jet | `Core/Encoding/Eetf.jet` owns the Erlang external term format codec. |
| `core.encoding.ini` | Jet | `Core/Encoding/Ini.jet` owns the INI codec. |
| `core.encoding.validate` | Jet | `Core/Encoding/Validate.jet` owns the use-site rule builder, sharing the `validate` block's vocabulary. |
| `core.files.data` | Host | `Core/Files/Data.jet` reads a file loader's payload and the CSV and JSON record cursors. |
| `core.files.path` | Mixed | `Core/Files/Path.jet` owns path construction and string math; existence queries are file-bound. |
| `core.files.sealed` | Host | `Core/Files/Sealed.jet` seals and opens whole files with `core.crypto`'s chunk engine, publishing through a private temporary. |
| `core.game` | Intrinsic | `Core/Game/Game.jet`: scene, replay and backend are compiler-owned handles; the runtime owns frame scheduling and presentation. |
| `core.git` | Host | `Core/Git/Git.jet` runs git command blocks in a work tree. |
| `core.grammar` | Jet | `Core/Grammar/Grammar.jet` owns typed grammar definitions, parse trees and checked text-literal lowering. |
| `core.grammar.error` | Jet | `Core/Grammar/Error.jet` owns grammar parse and compile-time format diagnostics. |
| `core.grammar.plan` | Jet | `Core/Grammar/Plan.jet` owns compiled grammar rule tables and AST node plans. |
| `core.grammar.runtime` | Jet | `Core/Grammar/Runtime.jet` executes compiled grammar plans over source text. |
| `core.grammar.sink` | Jet | `Core/Grammar/Sink.jet` streams grammar parse events into typed builders and printers. |
| `core.http` | Mixed | `Core/HTTP/HTTP.jet` owns HTTP/1.1 message semantics; the transport leaf is host-bound. |
| `core.http.client` | Host | `Core/HTTP/Client.jet` owns client configuration and session policy around the transport leaves. |
| `core.http.server` | Host | `Core/HTTP/Server.jet` serves HTTP/1.1 over `core.net` TCP or `core.net.tls`. |
| `core.jobs` | Host | `Core/Jobs/Jobs.jet` owns the durable queue's storage, lease and receipt transactions over the SQLite bridge. |
| `core.log` | Host | `Core/Log/Log.jet` owns the structured logger, its levels, fields and spans; sinks write. |
| `core.math.combinatorics` | Jet | `Core/Math/Combinatorics.jet` owns permutations, combinations, products and the counts `core.math` lacks. |
| `core.math.stats` | Mixed | `Core/Math/Stats.jet` owns descriptive statistics; random sampling takes the `Rand` effect. |
| `core.mem` | Intrinsic | `Core/Mem/Mem.jet`: allocator and pointer values are compiler-owned, and each raw memory primitive is a compiler intrinsic. |
| `core.mem.scope` | Namespace | Scoped allocation names the compiler provides; no source declares them. |
| `core.meta` | Intrinsic | `Core/Meta/Meta.jet` declares the module and package metadata the compiler fills. |
| `core.mod` | Host | `Core/Mod/Mod.jet` owns loader sequencing and typed refusals; mapping a compiled unit stays native. |
| `core.net.dtls` | Mixed | `Core/Net/DTLS.jet` owns DTLS 1.2 framing, handshakes, cookies and replay protection over `core.net` UDP and `core.net.tls` crypto. |
| `core.net.ip` | Jet | `Core/Net/IP.jet` owns IPv4 and IPv6 addresses and CIDR networks. |
| `core.net.ws` | Host | `Core/Net/Ws.jet` validates the URL and handshake; the provider owns socket I/O and the frame codec. |
| `core.plugin` | Intrinsic | `Core/Plugin/Plugin.jet` is the WASM Component boundary; the compiler attaches the component interface and `load` is the native route. |
| `core.prelude` | Intrinsic | `Core/Prelude/Prelude.jet`: `keep` is the compiler's use-site barrier; the other helpers are plain Jet. |
| `core.reactive` | Mixed | `Core/Reactive/Reactive.jet` owns the value transitions; the runtime provider retains the `computed`, `derived` and `effect` closures. |
| `core.rt` | Host | `Core/Rt/Rt.jet` runs realtime callback streams and reads their provider receipt. |
| `core.target` | Jet | `Core/Target/Target.jet` declares the machines a build can target and derives each machine's denial row. |
| `core.tasks` | Mixed | `Core/Tasks/Tasks.jet` owns task state transitions; timers and the scheduler's identity are host cells. |
| `core.text.html` | Jet | `Core/Text/HTML.jet` owns XML-safe escaping and bounded entity decoding. |
| `core.text.parse` | Jet | `Core/Text/Parse.jet` owns scanning, splitting and number parsing. |
| `core.text.wrap` | Jet | `Core/Text/Wrap.jet` owns paragraph filling and indentation in display columns. |
| `core.time.calendar` | Jet | `Core/Time/Calendar.jet` owns month and week calendars. |
| `core.time.expiring` | Jet | `Core/Time/Expiring.jet` owns deadline comparison; the caller owns the clock. |
| `core.tui` | Mixed | `Core/TUI/TUI.jet` owns models and layout; terminal facts cross provider seams. |
| `core.ui.font` | Jet | `Core/UI/Font.jet` reads TrueType faces and lays out glyphs. |
| `core.ui.host` | Host | `Core/UI/Host.jet` owns the host capability rules; the provider gives primitives only. |
| `core.ui.host.accessibility` | Host | `Core/UI/Host/Accessibility.jet` sets node metadata once the provider reports accessibility. |
| `core.ui.host.clipboard` | Host | `Core/UI/Host/Clipboard.jet` owns typed UTF-8 clipboard text over the provider's primitives. |
| `core.ui.host.drag_drop` | Host | `Core/UI/Host/DragDrop.jet` decodes the provider's file and text drop events. |
| `core.ui.host.ime` | Host | `Core/UI/Host/IME.jet` decodes the provider's input-method events. |
| `core.ui.host.shortcuts` | Host | `Core/UI/Host/Shortcuts.jet` owns the shortcut registry and its matching; the provider takes key claims. |
| `core.ui.xkb` | Jet | `Core/UI/Xkb.jet` compiles XKB keymaps and tracks keyboard state. |
| `core.watcher` | Host | `Core/Watcher/Watcher.jet` watches files, processes and ports. |
| `core.web` | Host | `Core/Web/Web.jet` builds server-rendered apps on `core.http`. |
| `core.web.devserver` | Jet | `Core/Web/DevServer.jet` describes the hot-reload wrapper around an app. |
| `core.web.forms` | Jet | `Core/Web/Forms.jet` owns typed HTML forms and their validation. |
| `core.web.query` | Jet | `Core/Web/Query.jet` owns a client's cached query state and mutation transitions. |
| `core.web.router` | Jet | `Core/Web/Router.jet` owns typed routes, navigation and cache state. |
| `core.web.store` | Jet | `Core/Web/Store.jet` owns typed state history, transactions and patches. |
| `core.web.table` | Jet | `Core/Web/Table.jet` owns typed rows, sorting, filtering and paging. |
| `core.web.virtual` | Jet | `Core/Web/Virtual.jet` owns windowed list geometry. |

## Exit codes (stable contract)

The `jet` driver returns one of a small, stable set of exit codes so
scripts and CI can branch on the outcome without parsing output. This
table is pinned by the `Tests/CLI/` transcripts and is part of the public
contract — codes are never repurposed.

| Code | Meaning | Who produces it |
|------|---------|-----------------|
| `0`   | Success. (Includes the no-args greeting — orientation, not an error.) | driver |
| `1`   | An unhandled entry error report, or a driver-reported user problem such as a failed `check`, missing file, or failed `test`. | fallible-by-default entry boundary or driver |
| `2`   | Usage error: unknown subcommand (E2101), unknown/ambiguous flag (E2102), or a missing required argument. | driver |
| `70`  | A built program breached or stopped at runtime (`panic`/`require`, `#Todo`, a raw Prelude panic, S36, or another program-side fault). Every producer enters the shared report and cleanup boundary; `jet run` forwards the result. | the user's program / Prelude boundary |
| `101` | Jet's own compiler defect (I2/R5): a back end failed on the checked program or the compiler reached an impossible state. Never a user-program exit. | compiler |

The final boundary keeps the same report and changes only its transport.
Native command-line output prints the frame and uses code 1. A Web target
raises a typed error object, a Wasm module returns a host-readable error
value, and a Service writes one structured report record. The selected build
target chooses that boundary; runtime inspection never does.

Explicit `process.exit(code)` and `os.stop(code)` use the same cleanup law.
The boundary runs deferred closes in reverse declaration order, scope guards in
reverse registration order, and `atexit` handlers in registration order. A
host kill or abort skips all three mechanisms; finalizers registered after the
stop are never run.

Presentation is TTY-aware (E2-M3): color and progress appear only when
the relevant stream is a terminal. `NO_COLOR` and `--color=never` force
plain output; `FORCE_COLOR` and `--color=always` force color; `--color=auto`
(the default) defers to TTY detection. Piped or CI output is always plain,
deterministic, ANSI-free bytes — scripts never parse escape sequences.

## Testing strategy

The [Jet testkit](../../Compiler/JetTestKit/Source/Kit.jet) owns diagnostic
snapshots (`ui`), executable examples and runtime fixtures (`golden`), and CLI
scenarios (`cases`). `Compiler/package.jet` names the corpora as `suites:` and
runs them through `test:` profiles: `jet test Compiler --profile
golden|ui|cases|corpora|full [FILTER]` runs them in process with the running
`jet` (or the jet `--compiler` names) as the compiler under test, and `-u`
blesses the selected rows. The selected suites share one pool sized by cores
and free memory (`--jobs`, `--memory`). Rows print as cases finish; each
suite's report and failure list keep suite-then-path order, whatever the
pool's size. Census and release tooling that shards or lists the corpus
across processes passes `--shard`, `--list`, `--cases` and `--results` to the
same command. Rows stage in memory outside the checkout, their files counted
against the pool's memory budget; history stays on disk. O0, O1 and O2
compare the real exit and exact streams; diagnostic text remains a product
surface, not a blessable implementation accident.

`JET=/absolute/path/to/jet jet Tools/Agent/VerifyFull.jet` runs the complete
native suite inventory and source-pinned compiler registries, collecting
independent failures. Missing, refused, skipped and non-PASS verdicts do not
close the gate. It uses an already-built compiler, never Cargo to build Jet's
compiler/tests. The gate's command list and strict
receipt validator live in that one file; it has no closeout-bypass flag. Under
owner directive #59, native cutover executes those runners directly and reports
formal qualification/closeout separately as **NOT MET (deferred)**, never as a
suite pass.

Testkit runs each case's commands directly, as the case wrote them; no
wrapper or outer process trace stands between a row and the `jet` under test.
The FFI helper cache is the run's own `ffi` directory when the rows stage in
memory, else the kit state's `ffi` directory, unless a case sets its own
`JET_FFI_CACHE_DIR`, which is passed through verbatim.
The missing-Cargo fixture always runs with a case-local PATH containing no
Cargo or rustup shim and a cleared case-private foreign cache. Installed tools
or a warmed helper cannot turn its E0703 contract into a skipped observation.

Proven UI support modules and route fixtures use `// @fixture_module`.
Only testkit's collection reads this marker; it is neither language syntax nor
a runtime directive. Collection excludes these files while preserving their
names and imports for the real parent fixtures. An absent expected-output file
never causes exclusion: unmarked UI fixtures remain discovered and require an
owner-reviewed snapshot.
A UI fixture's `// @requires REQUIREMENT` lines take the host requirements of
a case's `requires:` (`tool:cargo`, `os-name:linux`, `env:NAME=VALUE`, ...). On a
host that lacks one the fixture reports SKIP-HOST and does not run, whatever
its known-failing row. The lines in a `.fixed.jet` companion gate only the
companion's rows. Every fixture that builds a Rust bridge declares
`// @requires tool:cargo`; the missing-Cargo fixture never does.
The repository's own `#Test` claims that drive a host tool (Node, LLDB,
Cargo, clang++) start with the testkit's `kit_require_tool(name) ?? return`:
with the tool absent from PATH the claim reports skip, and the `packages`
profile records its row as skipped; with it present the claim runs. Jetpack's
bootstrap unit lists the helper's one file, `JetTestKit/Source/RequireTool.jet`,
in its Foundation section, which every calling area's closure holds. This is
testkit plumbing, not a user-facing conditional skip.
The runner stages the complete `Tests/UI` and `Tests/UILint` trees once per run
in external scratch, where each project case compiles rooted at its own
directory. A loose case's source root is its whole directory, and the compiler
snapshots every `.jet` file under it, so each loose case is staged alone in its
own scratch: the case, its `.fixed.jet` companion, and its directory's support,
which is every file there but another case and its companion and snapshots,
and every subdirectory that holds no case. Sibling imports, convention route
roots, embedded data and authored fixture state therefore resolve as in the
case's own directory, and no case compiles beside another. When outer
tracing is enabled, run-wide scratch also remains until the final trace audit;
ordinary runs remove it after reporting.
UI `@jetpack_e1250` copies its program as `main.jet` into a private scratch
package with `name: "e1250_fixture"`, `version: "1"` and `jet: "a0.9"`; it runs
public `jet build main.jet --offline`, requires a nonzero refusal, and compares
combined stdout/stderr. `@web_target` uses public
`jet build FILE --target=web`, not an internal compiler hook. Target availability
prerequisites do not substitute for a fixture's intended web semantic diagnostic.
`@a11y_cli` uses public `jet lint --a11y FILE`, the only surface of the
accessibility lints (E2930/E2931), requires a nonzero exit, and compares the
rendered stderr with the accessibility summary line.
`@cost_cli` uses public `jet lint --cost FILE`, the surface of the hidden
dynamic cost lint (L2510), requires a nonzero exit, and compares the rendered
stderr with the hidden-cost summary line.
`@report_cli` uses public `jet lint FILE`, the only surface of the advice
(the lints on the report tier), and compares the rendered stderr without its
"N reports found" count; a lint fixture keeps its lint acceptance, since the
command exits 0.

The case corpus keeps no retirement or pending table: every `.case` under
`Tests/Cases` is active, and the user-Rust interoperability acceptance cases
report their own verdicts like any other case. Rust performance peers and
historical fixture provenance are not Jet implementation dependencies.
