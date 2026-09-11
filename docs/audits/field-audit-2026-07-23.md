---
title: Field audit: day-zero peer gaps, actionable only
---
# Field audit — 2026-07-23 (day-zero frame)

Rule for this report: every language, Jet included, ships tomorrow with no history. Age, trust, and package counts are not findings. Every gap names work Jet can do.

Verified shipped Jet base: rustc AOT + Cranelift JIT + interpreter; `jet run/build/test/check/bench/fmt/repl/dev`; LSP + vscode/zed/tree-sitter; web/canvas target; D-MEM1 memory model; `T ? E` + `?`; comptime tiers; structured concurrency; allocator families; C FFI bridge; edition law D-REL1–5; 405 golden examples.

## Peers

**Rust** — systems code, wasm, CLIs. Rust does better: borrows are first-class, so you can return and store them; deep traits; strong IDE rename/refactor; a rich lint set. Jet does better: a memory model beginners can learn; one comptime instead of three macro systems; a JIT dev loop. Verdict: Jet takes the beginner; Rust takes the expert who hits the second-class-borrow ceiling. Do: audit what that ceiling blocks in real code, then ballot the sanctioned way past it; bring LSP rename/refactor to parity; add a lint tier. Flip: an expert ports a borrow-heavy Rust library without giving up.

**Go** — network services, CLIs, ops glue. Go does better: sub-second builds; fast `go run` start; a race detector; a built-in profiler (pprof); http and json in the box. Jet does better: sum types + `?` beat `if err != nil`; no GC pauses; scoped tasks beat leaked goroutines. Do: prove the dev-loop budgets (#666/#669/#676/#677); finish the http/encoding wave (#699–#730); add a profiler story; state and test whether the memory model rules out data races. Flip: edit-to-run under one second on a real service.

**Zig** — C replacement, embedded, cross builds. Zig does better: `zig cc` cross-compiles C to any target with bundled libcs; it imports C headers directly; binaries are tiny. Jet does better: memory safety; comptime is at parity. Do: add C header import to the FFI bridge arc; give cross-compile a first-class UX; set a binary-size budget. Flip: one jet command builds a project with C dependencies for another target.

**Python** — learning, scripts, glue. Python does better: repl-first work; zero-setup single-file scripts; instant start. Jet has a repl and an interpreter tier, so this is the closest fight for the beginner facet. Do: measure and budget script start time; ship notebook #442; polish the repl (completion, inline help). Flip: `jet run script.jet` feels as fast as `python script.py`.

**TypeScript** — web apps, tool scripts. TS does better: the browser is the runtime; feedback is live; structural types absorb messy data. Jet ships a web/canvas target and a dev server. Do: widen DOM/web API bindings; add browser debugging (source maps); ballot the union/any type (idea b4eclxq) before web data APIs bake casts in. Flip: a small real web app in Jet with no JS escape hatch.

**Kotlin** — services, Android, DSLs. Kotlin does better: flow typing (smart casts) removes casts after a check; type-safe builders make config read clean. Jet's typed config is the direct rival to those builders. Do: audit whether Jet narrows an optional after a check; if not, ballot it. Flip: no cast ceremony after `if x != none`.

**C#** — enterprise apps, game scripting. C# does better: a first-class step debugger; lazy LINQ queries; hot reload with state kept. Do: debugger #12; the lazy protocol (surface-research §1) — it is also the LINQ answer; check what state `jet dev` keeps across reloads. Flip: set a breakpoint and step a jet program in vscode.

**Swift** — apps, progressive disclosure. Swift does better: declarative UI (SwiftUI) and playgrounds. Do: nothing this epoch; a declarative layer above canvas is a later candidate, and notebook #442 covers playgrounds. Verdict: park it.

**Java** — large services, tooling depth. Java does better: mature profiler and heap tools; hot-swap debugging. The actionable items repeat Go (profiler) and C# (debugger). No new work item.

**Julia** — numeric and scientific work. Julia does better: linear algebra in the box; broadcasting on every operator; repl-driven science. Jet has fan-out `f.[…]` and unit literals. Do: settle the math/BLAS route (nixpkgs interim is ratified); ballot vector/matrix core types before a scientific stdlib bakes. Flip: a matrix example that reads as clean as Julia.

**Elixir** — long-running fault-tolerant services. Elixir does better: supervision restarts failed parts; you can inspect a live system. Jet chose a narrow restart rule on purpose; keep it. Do: make `jet live`/`services` show a running service's tasks and state. Flip: inspect a stuck task without stopping the service.

**Nim / Crystal** — Python-feel compiled code. Their pitch is Jet's pitch. Jet already avoids their dialect footgun (I8, no macro dialects). No unique gap; the binary-size budget (Zig row) covers the rest.

**Odin** — game code. Odin does better: vector/matrix/SOA types in the box. Do: the same vector/matrix ballot as Julia; one decision serves both. Flip: a small game loop needs no hand-rolled math types.

**Nix** — reproducible builds and system config. Day-zero Nix does better: binary-cache substitution; one lockfile pins a whole system; module merge with priorities. Jet does better: typed config; real diagnostics (I2/I4); one language from program to OS. Do: ship #653/#470 merge law with provenance; prove hangar substitution; land jetpack Phase 1 as the daily driver. Flip: you replace nix-shell for a week and do not go back.

## Ranked backlog (all actionable; existing cards cited)

Core:
1. Error why-chain — a failure through many `?` sites is anonymous today. surface-research §3. Card candidate.
2. Lazy iteration protocol — unlocks adapters, LINQ-class queries, and streams. §1, deferred slot.
3. Type ballots before stdlib bakes: union/any (idea b4eclxq) and vector/matrix (Julia + Odin + games).
4. Expert borrow-ceiling audit — name what second-class borrows block, then ballot the way out if one is missing.

Stdlib:
5. http/encoding wave (#699–#730, in flight); math/BLAS route via the ratified nixpkgs interim.

Tooling:
6. Dev-loop budgets proven (#666/#669/#676/#677) — this one item answers Go, Python, and TS feel.
7. Debugger #12; a profiler story; repl completion/help; browser source maps.
8. jetdoc #86 unfreeze — every peer ships a doc generator.

Packaging:
9. Publish UX + first-party semver gate (#6/#423 + surface-research §5); cross-compile UX (the Zig bar).

Docs:
10. A narrative first-hour tour. Cheapest item here; every peer ships one; spec and examples do not cover the first hour. Card candidate.

## Keep list — footguns Jet already avoids

No async two-color split. No null. No lifetime syntax. No macro dialects (I7/I8). rustc stays hidden (I2). One package tool. Typed config, never text templates. No detached spawn. Golden-tested examples (I5). Editions + `jet fix` ratified before day zero (D-REL1–5).

## Focused audit: AI-agent work as Jet programs

### Scope and method

This section asks whether an agent can use Jet instead of Bash, Python, or Node for daily work.

The work includes code tasks and non-code tasks. It includes files, processes, data, networks, browsers, documents, external services, and agent protocols.

The review uses current official product documents and the Jet tree. It does not count age, adoption, community size, or package totals.

Current agents use these main tool classes:

- Read, list, search, and change files.
- Run builds, tests, linters, package tools, Git, and development servers.
- Start background and interactive processes.
- Parse and produce structured data.
- Search the web and call APIs.
- Control browsers and desktop applications.
- Work with documents, spreadsheets, presentations, images, audio, and video.
- Call issue trackers, source hosts, email, calendars, chat, databases, and monitoring systems.
- Load skills, hooks, plugins, and MCP servers.
- Plan work, run workers, keep state, and return stable machine output.

This list matches current agent products. Gemini exposes shell, file, web, memory, planning, and MCP tools. Its shell supports background and PTY sessions.
Its sandbox isolates file and command changes. GitHub Copilot plugins contain agents, skills, hooks, MCP, and LSP data.
OpenAI models expose hosted shell, patch, computer use, skills, MCP, and tool search.

Sources:

- [OpenAI GPT-5.6 Sol tools](https://developers.openai.com/api/docs/models/gpt-5.6-sol)
- [Gemini CLI tool list](https://geminicli.com/docs/reference/tools/)
- [Gemini CLI shell and PTY](https://geminicli.com/docs/tools/shell/)
- [Gemini CLI sandbox](https://geminicli.com/docs/cli/sandbox/)
- [GitHub Copilot plugins](https://docs.github.com/en/copilot/concepts/agents/about-plugins)
- [GitHub Copilot hooks](https://docs.github.com/en/copilot/concepts/agents/hooks)
- [MCP specification](https://modelcontextprotocol.io/specification/2025-06-18)

### Peer results

**Bash** runs commands, redirections, pipelines, expansions, and interactive jobs with very little setup.
Its artifact gives direct terminal control. Its expansion order and exit rules also create hidden behavior.

Jet already does better for scripts. `sh"..."` makes each hole one argv item.
`core.process` has typed specs, pipelines, environment overlays, timeouts, output limits, signals, and live streams.
Jet does not pass user data through a shell.

Bash still does better for interactive jobs, arbitrary file-descriptor routing, and instant startup.
The Bash manual treats each pipeline as a job and gives the shell terminal control.

Verdict: Jet is the safer script language now. Bash still wins the interactive terminal session.

Flip criteria:

- A Jet child can use a PTY, resize it, receive input, and stop its full process tree.
- Binary streams and explicit file redirection use the same `ProcessSpec`.
- A small unchanged script starts no slower than the fastest peer on the same host.

Source: [GNU Bash manual](https://www.gnu.org/software/bash/manual/bash.html).

**Python** gives one readable language for paths, subprocesses, JSON, text, data, notebooks, and quick analysis.
Its process API supports binary streams, file descriptors, process groups, user and group changes, and direct argv.
Its single-file startup and REPL remain strong.

Jet does better on command safety, static input and output types, memory safety, effect visibility, and user errors.
Jet also keeps one package tool and one execution model.

Python still does better on startup, notebooks, process control depth, browser automation, and document or media work.
Cards #741, #749, and #442 already own the first three user-experience gaps.
Cards #291 and #288 already own the base process and file APIs.

Verdict: Jet can replace Python glue after the speed and JIT cards land.
It cannot replace the full agent-tool ecosystem until the protocol and domain-package gaps close.

Flip criteria:

- The agent corpus needs no Python helper for a local operation.
- Jet matches Python on cold and warm task time.
- Jet uses fewer repair turns on malformed input and failed commands.

Source: [Python subprocess documentation](https://docs.python.org/3/library/subprocess.html).

**Node** gives agents async subprocesses, streams, filesystem APIs, HTTP, browser-shaped APIs, and JSON-native protocol work.
Current agent CLIs also use Node packages for PTYs, MCP servers, hooks, and browser control.

Jet does better on command construction, types, memory safety, structured concurrency, native output, and one sync-looking model.
Jet avoids Node's separate sync and async file families.

Node still does better on MCP libraries, browser automation, PTY packages, and direct integration with current agent hosts.
Cards #300, #301, #716–#720, and #438 already cover network, HTTP, WebSocket, and web application depth.
They do not cover MCP or browser control.

Verdict: Jet can replace ordinary Node scripts after the current network and speed work.
It cannot replace Node as an agent-tool host today.

Flip criteria:

- One typed Jet function can become a CLI command and an MCP tool without duplicate schemas.
- Current Codex, Gemini, Claude Code, and Copilot clients pass one conformance suite.
- A first-party Jet package controls a real browser without JavaScript.

Source: [Node child-process documentation](https://nodejs.org/api/child_process.html).

### Workload coverage and board dedup

| Agent workload | Jet status | Canonical owner | New work? |
| --- | --- | --- | --- |
| List, walk, glob, stat, temp, lock, watch, atomic write | Shipped base; native proof remains | #288 | No |
| Regex, Unicode, text search | Shipped | #298 and current Core | No |
| Safe command, pipeline, env, timeout, signals, captured output | Shipped | archived #291 | No |
| PTY, binary child streams, process groups, tree cleanup, resource limits | Missing from public `core.process` | None | Yes |
| Fast single-file execution | Too slow; warm proof still measured tens of seconds through the required development environment | #741, #666, #687, #688, #727–#730 | No |
| REPL and notebook | Partial | #749, #442 | No |
| JSON, TOML, YAML, CSV, XML, CBOR, canonical and streaming data | Broad base; depth remains | #296, #710–#715 | No |
| Network, HTTP, WebSocket, URL, TLS | Partial; deep live proof remains | #300, #301, #716–#720 | No |
| DB, tables, numeric and accelerator work | Partial | #117, #237, #307, #443 | No |
| Typed CLI parsing and help | Shipped | archived #290 | No |
| Stable semantic search and source changes | Shipped codemod engine; public read API remains | shipped `jet inspect codemod`, #549, #696, #751 | No |
| Build, test, watch, debug, profile, budgets | Partial | #211, #439, #12, #441, #726, #241 | No |
| Packages, registry fetch, reproducible environments, cross-builds | Partial | #398, #423, #758, Epoch 4 | No |
| Generic sandbox for agent-run commands | Build recipes only | #398 is reusable substrate, not the public product | Yes |
| MCP client and server, tool schemas, resources, prompts, transports, OAuth | No Jet package or card found | None | Yes |
| Agent hooks and host adapters | Language primitives exist; adapters do not | MCP work can own adapters | Yes, inside MCP work |
| Browser and desktop automation | Internal browser harness only | #390 does test coverage, not a public Jet API | Later package |
| Office documents and rich media | No complete public package set found | #180 gives an interim FFI bridge | Later package |
| SaaS, email, calendars, issue trackers, chat, monitoring | External connectors only | MCP work is the common integration path | No per-service Core cards |

The current tree already ships more than the older field report credited.
`core.process` is not a gap.
The semantic codemod engine is not a gap.
The typed CLI model is not a gap.
The file and encoding depth cards already exist.

Do not add a shell syntax card.
Do not add another process builder.
Do not add another patch or AST-mutation system.
Do not add one Core module for every SaaS product.

### Ranked net-new backlog

#### 1. Agent workload conformance corpus

Card #769 owns one benchmark and proof gate. Do not create one feature card for each command.

Collect real, anonymous agent tasks across these groups:

- Repository search and edit.
- Build, test, debug, and Git work.
- Data cleanup and report generation.
- API and database work.
- Browser and desktop work.
- Document and media work.
- MCP tools and hooks.
- Long-running and interactive command work.

Implement each task in Jet, Bash, Python, and Node.
Run the same task on Linux, macOS, and Windows where the task applies.

Measure:

- Task success.
- Source tokens.
- Agent tool calls and repair turns.
- Cold and warm elapsed time.
- Peak memory.
- Output stability.
- Failure quality.
- Orphan process count.
- Sandbox escapes.
- Cross-platform changes.

The flip rule is simple. Jet must beat the best peer on task success and safety.
It must match the best peer on agent effort and elapsed time.

#769 must reuse #741, #688, #288, #296, #300, #301, #398, #423, and #696.
It must report those cards as blockers or evidence.
It must not copy their implementation scope.

#### 2. First-party agent protocol kit

Card #768 owns one official package for MCP client and server work in Epoch 9.
Use the current MCP specification and keep protocol versions explicit.

The package must support:

- JSON-RPC lifecycle and capability negotiation.
- Tools, resources, prompts, roots, sampling, and elicitation.
- Structured tool output and resource links.
- Progress, cancellation, pagination, logging, and errors.
- Standard input and output transport.
- Streamable HTTP transport.
- OAuth 2.1, PKCE, resource indicators, audience checks, and safe token storage.
- Bounded messages, output limits, deadlines, and secret redaction.
- Current protocol-version negotiation and compatibility tests.

One Jet function signature must produce one JSON Schema.
The same schema must drive CLI help, MCP input, validation, and generated documentation.
Reuse `ArgsSpec`, `#Codable`, typed reflection, `DataTree`, `core.http`, and `core.process`.

Do not add agent syntax.
Do not add a second reflection or schema engine.
Do not copy compiler-extension #549.
That card serves compiler guests, not general agent tools.

Acceptance must use real Codex, Gemini, Claude Code, and Copilot clients.
It must include one local stdio tool and one authenticated remote tool.

The MCP specification defines JSON-RPC, stdio, Streamable HTTP, tools, resources, prompts, client features, progress, and cancellation.
Its security model requires consent and treats tool metadata as untrusted.

Sources:

- [MCP overview](https://modelcontextprotocol.io/specification/2025-06-18/basic/index)
- [MCP transports](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports)
- [MCP tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)
- [MCP authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization)

#### 3. Generic safe command executor

Card #770 owns the agent-facing executor in Epoch 3 and consumes #398's native isolation engine.
Do not build a second sandbox.

The product needs:

- A workspace root with explicit read and write paths.
- Network off by default, with named host and port grants.
- Environment and secret allowlists.
- CPU, memory, process, output, and time limits.
- No inherited handles, devices, credentials, or ambient home access.
- Full descendant cleanup on exit, timeout, cancellation, or client loss.
- A plan and audit receipt.
- Linux, macOS, and Windows native proof.

The beginner path should take a command and a workspace.
The expert path should show the complete authority set and entered OS boundary.

This needs an owner ballot for the command and API spelling.
It does not need new language syntax.

#### 4. Interactive child-session completion

Card #771 extends the one `core.process` model from archived #291.
Do not create a terminal-process sibling module.

The missing work is:

- PTY or ConPTY allocation.
- Terminal resize.
- Raw byte input and output.
- Explicit file and descriptor redirection.
- Process groups and job identity.
- Tree-wide interrupt, terminate, kill, wait, and reap.
- CPU, memory, file, process, and output limits.
- Backpressure and bounded transcript capture.
- Cancellation through structured task scopes.

The default must remain non-interactive, bounded, and safe.
An agent requests a terminal only when a program needs one.

This needs an owner ballot because it adds public Core functions and types.
No new shell syntax is needed.

### Later package wave

The conformance corpus should decide the order of desktop, document, and media packages.
Browser automation is already a proven agent workload and card #772 owns it in Epoch 3.
Do not create one broad speculative artifact card before #769 produces evidence.

Card #772 owns browser automation.
Use WebDriver BiDi as the portable base and add CDP only for missing expert controls.
Reuse #301 WebSocket and HTTP work.

Required browser proof:

- Navigation, DOM and accessibility trees.
- Click, type, upload, download, dialogs, and screenshots.
- Network inspection and request control.
- Multiple tabs, frames, workers, and browser contexts.
- Deterministic browser installation through Jetpack.
- Cleanup, timeouts, secret redaction, and sandbox boundaries.

Office and media work should follow recorded corpus failures.
Use #180 FFI bridges until a native package earns a first-party slot.

### Required order

1. Finish #741 and the #688 parity wave.
2. Run #769, the agent conformance corpus.
3. Build #768, the MCP kit and real-client proof.
4. Build #770 over #398's native isolation boundary.
5. Complete #771 interactive process sessions.
6. Finish #772 browser automation; add artifact packages only from measured #769 gaps.

### Final verdict

Jet already has the right shell-replacement design.
Its checked `Sh` values are safer than Bash, Python shell strings, and Node shell execution.

Jet is not the better full agent language today.
Startup, JIT coverage, registry fetch, MCP, generic sandboxing, and interactive process control block that claim.

Most work is already on the board.
Five planning cards now own the proven gaps: #768 MCP, #769 corpus, #770 safe executor, #771 interactive process sessions, and #772 browser automation.
Artifact packages wait for #769 corpus evidence.

No new syntax is required for this strategy.
The new public module and command shapes still need owner ballots before implementation.

