# `jet dev` against the web dev incumbents: Vite, Bun, Next.js, Deno, Astro, Rsbuild, Parcel (2026-11-04)

Date: 2026-11-04. Owner inquiry: "Compare jet dev against incumbent best in
business, like vite, bun etc. in the web dev world end to end, for features,
functionality, visuals, what info is shown in the cli, what is not shown."

Repo measured: dev HEAD `6c0d4b4e32`. Compiler: the admitted base
`7c672809607d`. Method: the mine-for-jet-v2 skill. The run directory is
`/mnt/jetscratch/lanes/Main/research/mine/jetdev-vs-incumbents-2026-11-04/` (called RUN below).
It holds every raw capture, the probe harness and the source text.
The ledger has 18 sources, 42 segments and 154 claims, and `check_coverage` passes.
Screenshots and terminal renders are in
[`Docs/assets/research/2026-11-04-jet-dev/`](../assets/research/2026-11-04-jet-dev/).
The retained evidence is in its `evidence/` folder:

- `coverage.json`, `ledger.json`, `claims.json` and `segments.json`;
- `probe-results.json`, the probe output for all eight tools;
- `terminal-captures.txt`, every quoted terminal log as plain text.

Review status: **pending fresh-context review and recall audit.** The card
drafts and the two ballots named below are drafts, not Tower records.

## Summary

Today `jet dev` is a file watcher that restarts the program on every save.
For a web app it prints no address, no ready line and no request log, and the page gets no update.
Any syntax, type or runtime error takes the site offline until the next good save.
Every incumbent prints a clickable URL and a ready time, and keeps serving on error.
Five of the seven show the error inside the page, and six of the seven update the page by themselves within 22 to 122 ms.

Jet's design for this is already ratified: the spec section "Web development dashboard" (`Docs/spec/spec.md:4310-4342`).
That design matches or beats the incumbents on most points. It mirrors the status in the terminal and the browser, and it keeps serving the last good build under an error overlay, as Bun does.
It is unbuilt. The gap is implementation, not design, with two exceptions that need the owner:

- how a tab learns of a new build: the ratified timer poll against the push every incumbent uses (ballot D-FE-DEVPUSH1);
- who gets a key press when the program itself reads the keyboard (ballot D-DEV-KEYS1).

Where Jet already leads:

- type errors are caught in dev;
- the diagnostics explain themselves (Why/Fix/More);
- it binds to 127.0.0.1 by default;
- a new project needs no dependency install;
- the served app is tiny (1.5 MB).

## Method and versions

All tools ran on one Linux x64 machine (CachyOS, kernel 7.2.8), one at a time, under a systemd memory cap.
Each incumbent was created with its own official template, unchanged except for the probe edits below.

| Tool | Version | Template | Dev command |
|---|---|---|---|
| Jet | base 7c672809 | hand-written 18-line `web.app().ssr()` site, plus `jet new demo` | `jet dev` |
| Vite | 8.3.5 | `create-vite` react-ts | `vite` |
| Next.js | 16.4.5 (Turbopack) | `create-next-app` app-tw | `next dev` |
| Astro | 7.4.5 | `create-astro` minimal | `astro dev` |
| Rsbuild | 2.3.1 | `create-rsbuild` react-ts | `rsbuild dev` |
| Parcel | 2.16.4 | `create-parcel` react-client | `parcel` |
| Bun | 1.3.13 | `bun init` React full-stack | `bun --hot src/index.ts` |
| Deno | 2.8.3 | `deno init --serve` | `deno serve --watch` |

Node 24.21.0 drove the npm-based tools.
The probe (`RUN/harness/`) used headless Chrome through the DevTools protocol, and every probe ran the same sequence:

1. Start the server and time it to the first HTTP 200.
2. Load the page, and click the counter three times where there is one.
3. Change the heading text, and time it to the first visible change in the tab without our reload.
4. Check whether the counter still reads 3.
5. Break the file three ways (a syntax error, a type error and a runtime throw in the page), screenshot the tab, then reload and screenshot again.
6. Record the HTTP status while the file was broken, and time the recovery.
7. Start a second copy on the same port.
8. Type `h`, `r`, `o` and `q` into a real PTY.
9. Read RSS and idle CPU over 10 s.

The terminal captures are `script(1)` logs, rendered to PNG by `RUN/harness/render.mjs`.
Docs for Expo CLI and the Astro dev toolbar were read but not run.

### Probe numbers

Times are in ms. The edit→browser column is the time until the open tab showed the new text with no reload by us.

| Tool | Start → first 200 | Edit → browser | State kept after edit | RSS | Idle CPU per 10 s |
|---|---:|---:|---|---:|---:|
| **Jet** | 1443 | **none** (edit → new server 1525) | no (full restart) | ~999 MB | ~17 ms |
| Vite | 338 | 41 | yes | 274 MB | 0 |
| Next.js | 1909 | 122 | yes | ~1006 MB | 70 ms |
| Astro | 1654 | 103 (full reload) | no | ~559 MB | 40 ms |
| Rsbuild | 476 | 55 | yes | ~376 MB | 30 ms |
| Parcel | 1111 | 22 | yes | ~581 MB | 0 |
| Bun | 160 | 24 | yes | ~83 MB | 30 ms |
| Deno | 1072 first, 253 cached | none (edit → new server 261) | no (server only) | ~79 MB | 0 |

Jet's recovery from a broken file to a good one took 1424 ms for a syntax error, 1474 ms for a type error and 1716 ms for a runtime panic: one full rebuild and restart each.
The Jet app process itself is about 1.5 MB; the rest of the ~999 MB is the `jet dev` process tree.
These are single runs on one machine and are not gauntlet numbers.

## Matrix

✅ = present and observed. ◐ = partial. ❌ = absent. "doc" = documented, not run.

| Aspect | Jet | Vite | Next | Astro | Rsbuild | Parcel | Bun | Deno |
|---|---|---|---|---|---|---|---|---|
| Prints the URL on start | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Prints ready time | ❌ | ✅ 109 ms | ✅ 193 ms | ✅ 160 ms | ✅ 0.05 s | ✅ 375 ms | ✅ "Bundled page in 33ms" | ❌ |
| Network/LAN URL or hint | ❌ | hint | ✅ by default | hint | hint | ❌ | ❌ | binds 0.0.0.0 |
| Default bind | 127.0.0.1 | localhost | all interfaces | localhost | localhost | localhost | localhost | 0.0.0.0 |
| Request log | ❌ | ❌ | ✅ with split timing | ✅ | ❌ | ❌ | errors only | ❌ |
| Page updates by itself on edit | ❌ | ✅ HMR | ✅ Fast Refresh | ✅ reload | ✅ HMR | ✅ HMR | ✅ HMR | ❌ |
| Component state kept on edit | ❌ | ✅ | ✅ | ❌ | ✅ | ✅ | ✅ | n/a |
| Site stays up during a syntax error | ❌ refused | ✅ | ✅ 500 + overlay | ✅ 500 + overlay | ✅ | ✅ 500 + page | ✅ | ❌ refused |
| In-page overlay for a syntax error | ❌ | ✅ | ✅ | ✅ | ✅ | ◐ error page | ✅ over dimmed page | ❌ |
| Type errors reported in dev | ✅ E0113 (but the site goes down) | ❌ ran | ❌ ran | ❌ ran | ❌ ran | ❌ ran | ❌ ran | ❌ ran |
| Runtime error in a page | server dies | blank page | overlay | overlay | blank page | overlay | overlay | 500 text |
| Throw in a request handler | server dies | n/a | 500, server lives | 500 | n/a | n/a | 500, server lives | 500, server lives |
| Port in use | corelib panic | next port | stack trace | names the running server | next port | stack trace | plain sentence + code frame | stack trace, keeps watching |
| Keyboard shortcuts | ❌ (echoed) | key + Enter | ❌ | key + Enter | key + Enter | ❌ | ❌ | ❌ |
| `--port` | ❌ read as a file | ✅ | ✅ | ✅ | ✅ | ✅ | via code | ✅ |
| `--open` | silently ignored | ✅ | ❌ | ✅ | ✅ | ✅ | ❌ | ❌ |
| Color in terminal | ❌ (1 SGR code) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Explains each error (why, fix) | ✅ Why/Fix/More | ❌ | ◐ docs link | ◐ hint | ❌ | ❌ | ❌ | ❌ |
| Template works on first `dev` | ❌ serves nothing | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| New project install | 0 deps, 0.17 s | npm | 360 pkgs, 48 s, 16 vulns | npm | npm | 127 pkgs, 5 s | 11 pkgs, 0.57 s | jsr on first run |
| Browser console sent to terminal | ❌ | doc (`server.forwardConsole`) | ◐ indicator | ❌ | ❌ | ❌ | doc (`console: true`) | ❌ |
| Second instance detected | ❌ | ❌ | ❌ | ✅ PID + stop/--force | ❌ | ❌ | ❌ | ❌ |
| QR code for a phone | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ (Expo: doc ✅) |

## What each tool prints

### Start banner

Jet ([render](../assets/research/2026-11-04-jet-dev/term-banner-jet.png)):

```
jet dev: watching /…/work/jet/app (Ctrl-C to stop)
```

That is the whole banner. After every save Jet prints `jet dev: change detected, rebuilding`, then the same "watching" line again. It never prints an address, a build time or a "ready" word.
The program did bind 127.0.0.1:5311 (`JET_APP_PORT`, read at `Core/Web/Web.jet:374-380`; the default is 8080), but the user is not told.

Vite ([render](../assets/research/2026-11-04-jet-dev/term-banner-vite.png)):

```
  VITE v8.3.5  ready in 109 ms

  ➜  Local:   http://localhost:5401/
  ➜  Network: use --host to expose
  ➜  press h + enter to show help
```

Next.js ([render](../assets/research/2026-11-04-jet-dev/term-banner-next.png)):

```
▲ Next.js 16.4.5 (Turbopack)
- Local:         http://localhost:5410
- Network:       http://192.168.0.77:5410
✓ Ready in 193ms
✓ Running next.config.ts took 22ms
- Cache Components enabled
- Partial Prefetching enabled

 GET / 200 in 213ms (next.js: 116ms, application-code: 98ms)
 GET /nope 404 in 201ms (next.js: 182ms, application-code: 19ms)
```

Astro ([render](../assets/research/2026-11-04-jet-dev/term-banner-astro.png)):

```
 astro  v7.4.5 ready in 160 ms
┃ Local    http://localhost:5420/
┃ Network  use --host to expose
12:00:57 watching for file changes...
12:00:58 [200] / 12ms
12:01:01 [404] /nope 3ms
```

Rsbuild ([render](../assets/research/2026-11-04-jet-dev/term-banner-rsbuild.png)):

```
Rsbuild v2.3.1

  ➜  Local:    http://localhost:5430/
  ➜  Network:  use --host to expose
  ➜  press h + enter to show shortcuts

start   build started...
ready   built in 0.05s
```

Parcel ([render](../assets/research/2026-11-04-jet-dev/term-banner-parcel.png)). A spinner rewrites one line until:

```
Server running at http://localhost:5440
✨ Built in 375ms
```

Bun ([render](../assets/research/2026-11-04-jet-dev/term-banner-bun.png)):

```
$ bun --hot src/index.ts
🚀 Server running at http://localhost:5450/
Bundled page in 33ms: src/index.html
```

The 🚀 line is template code, not Bun itself; the "Bundled page" line is Bun's own.

Deno ([render](../assets/research/2026-11-04-jet-dev/term-banner-deno.png)):

```
Watcher Process started.
deno serve: Listening on http://0.0.0.0:5460/
```

### The edit cycle

Jet ([render](../assets/research/2026-11-04-jet-dev/term-jet-edit-cycle.png)). A good save prints two lines and restarts the program:

```
jet dev: change detected, rebuilding
jet dev: watching /…/work/jet/app (Ctrl-C to stop)
```

The open tab is not told. In the probe it kept showing the old text until we reloaded.
The page had no reload client and opened no WebSocket. Its only console entry was one 404 for a missing resource.

Every incumbent except Deno opened one WebSocket per tab and pushed the change down it:

| Tool | Socket |
|---|---|
| Vite | `ws://localhost:5401/?token=…` |
| Next.js | `ws://localhost:5410/_next/hmr?id=…` |
| Astro | Vite's socket |
| Rsbuild | `ws://localhost:5430/rsbuild-hmr?token=…` |
| Parcel | `ws://localhost:5440/` |
| Bun | `ws://localhost:5450/_bun/hmr` |

None of them prints a terminal line for a successful edit, except Rsbuild's `ready built in …`. Silence on success is the norm.

### Errors in the terminal

A syntax error, made by dropping one `}`.

Jet prints five problems for one mistake, in plain text with no colour. The excerpt below shortens the second E0003 and the E0083, and leaves out the third and fourth E0003:

```
Error [E0003]: Expected a call, binding, assignment, or `return`, found a piece of quoted text
  --> /…/app/run.jet:4:27
    |
  4 | fn greeting() -> String { "hello v2"
    |                           ^^^^^^^^^^
 Why: Inside a function body, write a call, binding, assignment, or `return`
 Fix: e.g. print("hello") or x :: 1
More: jet-lang.dev/e/E0003
Error [E0003]: Expected a call, binding, assignment, or `return`, found the keyword `fn`
  --> /…/app/run.jet:6:1
…  (two more E0003 for the next two `fn` lines)
Error [E0083]: `{` opened on line 4 is never closed
  --> /…/app/run.jet:4:25
 …
 Why: Every `{` needs a matching `}`, but the end of the file came first
 Fix: Add `}` where the block ends
More: jet-lang.dev/e/E0083
5 problems found
run `jet explain E0003` to learn more
```

The root cause (E0083) prints last. #3720 c28 and c29 already ask for one root-cause report that prints first.
Vite prints one Oxc parse error with a code frame ([render](../assets/research/2026-11-04-jet-dev/term-vite-syntax.png)).

A type error (`fn greeting() -> String { 42 }`):

```
Error [E0113]: This value is Int (a whole number), but `greeting` returns String
  --> /…/app/run.jet:4:27
    |
  4 | fn greeting() -> String { 42 }
    |                           ^^
 Why: `greeting` declares String as its result, so the value it hands back must be String
 Fix: Put the value in text with interpolation: "{value}"
More: jet-lang.dev/e/E0113
1 problem found
```

This is Jet's clearest lead. With the equivalent TypeScript type error, all seven incumbents served the page with the wrong value, and none showed the error in the page (`RUN/cap/*/probe-app/3-error-2-type-reloaded.png`).

A panic in a route handler ([render](../assets/research/2026-11-04-jet-dev/term-jet-handler-panic.png)):

```
Stop [E3001]: `panic: boom in about`
  --> run.jet:8 in about
    |
8 | fn about() -> WebPage { panic("boom in about") }
    |                         ^^^^^
 Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, a bounds/key check triggered at runtime, or another program-side stop. The shared report boundary owns the program-side stop; an unhandled entry error is a returned report, not E3001.
 Fix: Fix the logic that led to the failure. Program-side stops exit 70 through the shared boundary; unhandled entry errors print their report and exit 1; exit 101 is reserved for Jet defects.
More: jet-lang.dev/e/E3001
```

The whole server exits with this. Requests for `/` that worked a moment earlier get connection refused.
The Why and Fix text speaks of exit codes and "the shared report boundary", which does not help someone who wrote a web page.

Bun, for the same mistake in a handler ([render](../assets/research/2026-11-04-jet-dev/term-bun-handler-throw.png)), prints the code frame and `GET - /api/hello/boom failed`, answers 500 and keeps serving:

```
26 |       if (name === "boom") throw new Error("boom in handler");
                                          ^
error: boom in handler
      at /api/hello/:name (/…/bun-throw/src/index.ts:26:38)
GET - /api/hello/boom failed
```

### Errors in the browser

| Tool | Syntax error | Runtime error in the page | Status while broken |
|---|---|---|---|
| Jet | "This site can't be reached" ([shot](../assets/research/2026-11-04-jet-dev/browser-jet-syntax-error.png)) | same | connection refused |
| Vite | overlay over the page ([shot](../assets/research/2026-11-04-jet-dev/browser-vite-syntax-overlay.png)) | blank page, error only in DevTools ([shot](../assets/research/2026-11-04-jet-dev/browser-vite-runtime-blank.png)) | 200 |
| Next.js | "Build Error" card plus a "1 Issue" badge ([shot](../assets/research/2026-11-04-jet-dev/browser-next-syntax-overlay.png)) | "This page couldn't load" plus overlay ([shot](../assets/research/2026-11-04-jet-dev/browser-next-runtime.png)) | 500 |
| Astro | Vite overlay ([shot](../assets/research/2026-11-04-jet-dev/browser-astro-syntax-overlay.png)) | Vite overlay | 500 |
| Rsbuild | overlay ([shot](../assets/research/2026-11-04-jet-dev/browser-rsbuild-syntax-overlay.png)) | blank page | 200 |
| Parcel | full error page, no overlay | overlay ([shot](../assets/research/2026-11-04-jet-dev/browser-parcel-runtime-overlay.png)) | 500 / 200 |
| Bun | "1 Build Error" card over the dimmed last good page, "Errors during a build can only be dismissed by fixing them." ([shot](../assets/research/2026-11-04-jet-dev/browser-bun-syntax-overlay.png)) | overlay ([shot](../assets/research/2026-11-04-jet-dev/browser-bun-runtime-overlay.png)) | 500 |
| Deno | connection refused | "Internal Server Error" text | refused / 500 |

The first Jet page loads fine ([shot](../assets/research/2026-11-04-jet-dev/browser-jet-initial.png)).
Every incumbent with an overlay cleared it on its own after the fix: 39 to 162 ms after the save.

Bun's dimmed last-good page under an error card is what Jet's ratified dashboard already specifies: "During a rebuild the browser dims the last good page. A failed build expands the strip into an overlay with the verbatim front-end diagnostic while serving the last good artifacts" (spec.md:4323-4325).

### Port already in use

Jet ([render](../assets/research/2026-11-04-jet-dev/term-jet-port-conflict.png)):

```
Stop [E3001]: `panic: web app could not listen on 127.0.0.1:5312: HTTP I/O failed during bind on `127.0.0.1:5312` failed: tcp listen failed: Address in use (os error 98)`
  --> <corelib>/Core/Web/Web.jet:405 in __app_serve
      |
405 |     server :: http_server.__bind_entry("127.0.0.1:{port}", mux, None, None) ?? panic("web app could not listen on 127.0.0.1:{port}: {err}")
      |                                                                                ^^^^^
locals: port = 5312
 Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, …
```

The message says "failed" twice and shows the raw `(os error 98)`. It points into the corelib, not into the user's code, and it never says "another server is using this port".

Vite ([render](../assets/research/2026-11-04-jet-dev/term-conflict-vite.png)) and Rsbuild ([render](../assets/research/2026-11-04-jet-dev/term-conflict-rsbuild.png)) move on to the next port:

```
Port 5401 is in use, trying another one...
  VITE v8.3.5  ready in 123 ms
  ➜  Local:   http://localhost:5402/
```

```
info    port 5430 is in use, using port 5431.
```

Astro ([render](../assets/research/2026-11-04-jet-dev/term-conflict-astro.png)) recognises its own running server:

```
Another astro dev server is already running.
  URL:  http://localhost:5420
  PID:  76299
Run `astro dev stop` to stop it, or use `astro dev --force` to replace it.
```

Bun names the likely cause in plain words, with a code frame:
`error: Failed to start server. Is port 5450 in use?`

Next ([render](../assets/research/2026-11-04-jet-dev/term-conflict-next.png)), Parcel and Deno print a stack trace:
`Error: listen EADDRINUSE: address already in use :::5410`.
The Parcel docs promise a fallback port, but with an explicit `-p` it errored.

### Keys

| Tool | Hint shown | Keys |
|---|---|---|
| Jet | none | none. `h r o q` echoed as `hroq`, then `^C` |
| Vite | `press h + enter to show help` | r restart, u show URL, o open, c clear, q quit (each + Enter) |
| Rsbuild | `press h + enter to show shortcuts` | c, o, q, r, u (each + Enter) |
| Astro | none until h | s sync content, o open, q quit (each + Enter) |
| Next, Parcel, Bun, Deno | none | none |
| Expo (doc) | key list under the QR | single keys, no Enter; off in CI |

Jet has ratified single keys (`r` re-run, `R` restart fresh, `t` tests, `f` failed claims, `q` quit, and `v` for the dashboard detail), but none is built.
A later criterion on #2756 (c3) says ordinary keys are never stolen and the dashboard opens on Ctrl+].
That conflict is ballot D-DEV-KEYS1.

### Flags

`jet help dev` lists `--canvas`, `--app`, `--share`, `--token`, `--swap`, `--restart`, `--watch`, `--target`, `--json`, `--quiet` and `--color`, plus about thirty build settings.
It lists no `--port`, `--host` or `--open`. What the probes got:

| You type | Jet says |
|---|---|
| `jet dev --port 5317` | `Error [E2105]: Can't securely read the file `5317`` |
| `jet dev --bogus` | nothing; starts watching |
| `jet dev --open` | nothing; starts watching, no browser |
| `jet dev --swap` | `Error [E2104]: `jet run --swap` is not built yet` |
| `jet dev --target=web` | `Error [E2104]: `jet run --target` is not built yet` |
| `jet dev --canvas` | `Error [E2104]: `jet dev` is not built yet` |
| `jet dev --app run --share lan` | `Error [E2104]: `jet dev` is not built yet` |

The spec's own dashboard form is `jet dev <file.jet> --target=web` (spec.md:4312), and today it is refused with the wrong verb.

### `fn dev` and the template

D-ENTRY-SHAPE1 ratified `fn dev(d: Dev)` as the way to configure `jet dev`.
A package with `dev.jet` holding `fn dev(d: Dev) { d.serve(port: 5313) }`, and a copy of the repository's own `Examples/features/devloop/dev_context`, both print:

```
jet dev: in-process hot reload isn't available yet; restarting the program on each change
internal compiler error: the Jet backend failed on `…/dev.jet`: 1 compiler fault:
  ir.object object: -: entry function dev must take no parameters and return nothing
This is a bug in jet, NOT in your program. Please report it,
attaching your source file.
```

The check that fires is the native entry rule at `Compiler/JetIR/Source/X64Module.jet:407`.

`jet new demo` takes 0.17 s and prints:

```
created demo/
  package.jet
  run.jet
  .gitignore
next: cd demo && jet dev
```

The template's header comment says "Start the live browser app: `jet dev`" and "Build static browser files: `jet build --target web`".
In practice:

- `jet dev` prints only the watching line and serves nothing, because the counter mounts on `ui.null_backend()`;
- `jet run` prints nothing;
- `jet build --target web` prints nothing, and no `index.html` appeared.

Every incumbent's template showed a working page on its first `dev` command.

The repository's reference web app (`Examples/features/web/jet_web`, run from a copy) stops under `jet dev` with `Stop [E3001]: `panic: id field`` at `run.jet:160 in render_snapshot`.

## What is shown, and what is not

Shown by most incumbents, missing from Jet:

1. The address, as a clickable URL, on the first screen. All seven show it.
2. A ready time. Six of seven show it, and Next also times the config file.
3. A network hint (`use --host to expose`) or the LAN URL (Next, by default).
4. A help hint for keys (Vite, Rsbuild).
5. Per-request lines with status and time (Next splits framework time from application time; Astro shows a timestamp).
6. The version in the banner (Vite, Next, Astro, Rsbuild).
7. Which port was chosen when the asked-for one was busy (Vite, Rsbuild).
8. Colour: a green check or arrow for ready, red for errors.

Shown by Jet, missing from most incumbents:

1. Type errors in dev, with Why/Fix/More and a stable code (E0113) that `jet explain` expands.
2. A rebuild line for every change. Most incumbents stay silent on a good edit, except Rsbuild.
3. The values of local variables at a runtime stop (`locals: port = 5312`), which no incumbent shows.

Shown by no one in the terminal:

- the browser's runtime errors (Vite and Bun can forward them only when that is turned on);
- how many tabs are connected (Jet's ratified header will show this);
- the memory in use.

Hidden by Jet that should not be:

- the port, which comes from `JET_APP_PORT` or the 8080 default;
- that a save is waiting for a slow build;
- that the server is down after an error;
- that an unknown flag was ignored.

## Gaps, ranked by user impact

Rank is by how many first-session users hit the gap, then how badly.

| # | Gap | Who hits it | Evidence | Tracked by |
|---|---|---|---|---|
| G1 | No URL and no live reload for a web app: the page never updates by itself | every web user, first minute | banner, probe `edit_browser_self_update_ms: null`, no WebSocket | #4275 c82; D-WEBAPP-SERVE1, D-FE-DEVSRV1; draft jet-dev-web-dashboard; ballot D-FE-DEVPUSH1 |
| G2 | A panic in one request handler kills the whole server | anyone with a bug in a handler | dev-throw.log; Bun returns 500 and keeps serving | #4635 c1 |
| G3 | A compile error takes the site offline, with no overlay and no "last good" page | every user, every typo | probe: ECONNREFUSED for syntax, type and runtime | D-FE-DEVSRV1 (unbuilt); draft jet-dev-web-dashboard; #5201 c4 |
| G4 | `fn dev(d: Dev)`, the ratified way to configure dev, is an internal compiler error, and so is the repo's own example | anyone who follows the docs to set a port | dev-devport.log, dev-dev_context.log | D-ENTRY-SHAPE1; draft jet-dev-entry-ice |
| G5 | No ready line or build time | everyone | banner | D-FE-DEVSRV1; draft jet-dev-web-dashboard |
| G6 | No request log | anyone debugging a route | banner and session | D-FE-DEVSRV1 (`v` key); draft jet-dev-web-dashboard |
| G7 | Port in use is a corelib panic with a raw OS error | anyone with a second terminal | dev-s2-conflict.log | #4635 c2; #3720 c14, c48 |
| G8 | No keys: typed letters are echoed | every TTY user | dev-keys-tty.log | D-RUN-SESSION1; #2756 c3; draft jet-dev-session-keys; ballot D-DEV-KEYS1 |
| G9 | Flags are missing (`--port`, `--host`), silently accepted (`--bogus`, `--open`) or reported with the wrong verb (`jet run --swap`) | anyone who tries a flag from another tool | flag_*.log, help-dev.log | #6418, #3720 c15, #5069; draft jet-dev-flag-hygiene |
| G10 | About 1 GB resident and a 1.5 s full restart per save | everyone on a laptop | probe | #2756 c1; draft jet-dev-memory |

Lesser gaps:

| # | Gap | Tracked by |
|---|---|---|
| G11 | The `jet new` template serves nothing under `jet dev`, and `jet build --target web` is silent and writes no `index.html` | #6418 c25; draft jet-web-template-loop |
| G12 | One missing `}` gives five errors, with the root cause last | #3720 c28, c29 |
| G13 | E3001 Why/Fix text speaks of exit codes and "the shared report boundary"; locations point into `<corelib>` | #3720 c14, c48 |
| G14 | No colour in `jet dev` output (one SGR code in a TTY capture) | draft jet-dev-web-dashboard (header and state words) |
| G15 | Browser runtime errors never reach the terminal | draft jet-dev-browser-console |
| G16 | No way for an agent to find or stop a running session, and a second `jet dev` panics on bind | draft jet-dev-agent-status |
| G17 | State is lost on every edit (full restart); five incumbents keep component state | #6386 c13 (#Persist) |
| G18 | No LAN URL or QR; `--share lan` is E2104 | #5931 |
| G19 | The repository's reference web app (`Examples/features/web/jet_web`) panics under `jet dev` | #6552 |
| G20 | Every `fn dev` package prints "in-process hot reload isn't available yet" | draft jet-dev-entry-ice c3 |

Not worth a card now, because no first session needs them:

- an HTTPS dev certificate (Vite and Next have flags);
- a proxy to a backend API (Vite `server.proxy`);
- custom middleware;
- `.env` file loading with a public-prefix rule (Vite `VITE_`);
- telemetry notices (Next).

## Where Jet leads

1. **Type errors in dev.** Jet stops on a type error with a plain explanation (E0113). All seven incumbents served a page that was wrong by type and showed no error in it.
2. **Diagnostics that teach.** Every error has a code, Why, Fix and a `More:` link, and `jet explain` expands the code. No incumbent has all four.
3. **Locals at a stop.** `locals: port = 5312` shows the values at the failing line.
4. **Safe default bind.** 127.0.0.1. Next binds all interfaces and Deno binds 0.0.0.0, both by default.
5. **No dependency install.** `jet new` takes 0.17 s with zero packages. `create-next-app` installed 360 packages in 48 s and reported 16 moderate vulnerabilities. Parcel took 127 packages and 5 s.
6. **Small served app.** The app process is about 1.5 MB, and idle CPU is about 17 ms per 10 s, against 70 ms for Next.
7. **Server rendering built in.** `web.app().ssr()` renders on the server. Bun's dev server bundles HTML but leaves SSR to the user.
8. **Typed 404.** Unmatched paths go to the app's own not-found response (`Web.jet:403-404`).
9. **No pre-bundling step.** Vite pre-bundles dependencies on first start and after lockfile changes. Jet has no npm graph to pre-bundle.
10. **A better design on paper.** The ratified dashboard already has everything below, which each incumbent has only in part:
    - one status shared by the terminal header and a browser strip;
    - the last good page kept, dimmed, under an error overlay, as Bun does;
    - a reconnecting state;
    - a client count;
    - `--json` status records for agents.

## Alignment

| Source mechanism | Jet today / planned | What it would add | Disposition | Evidence |
| --- | --- | --- | --- | --- |
| URL and "ready in N ms" on start (all seven) | ratified, unbuilt (live-reload serving decision, dashboard spec) | the build time on the ready line | #4275 c82; draft jet-dev-web-dashboard c1 | banners above |
| One WebSocket per tab pushes each change (six of seven) | ratified timer poll, unbuilt | push delivery and a channel for console lines | ballot D-FE-DEVPUSH1 | probe sockets and timings |
| Error overlay over the dimmed last good page (Bun; Vite, Next, Astro, Rsbuild similar) | ratified, unbuilt | nothing beyond the spec | D-FE-DEVSRV1=D; draft jet-dev-web-dashboard c2 | browser screenshots |
| A throwing handler answers 500 and the server lives (Bun, Next, Deno) | in flight on #4635 c1 | nothing beyond #4635 | #4635 c1 | dev-throw.log, throw.log |
| Port in use: next free port (Vite, Rsbuild), or "already running" with the URL and PID (Astro) | #4635 c2 asks for a typed error | the "already running" check | #4635 c2; draft jet-dev-agent-status c1 | conflict captures |
| Request log with status and time (Next, Astro) | ratified `v` detail, unbuilt | nothing; Next's framework/app time split does not apply to Jet | draft jet-dev-web-dashboard c4 | session captures |
| Keys (Vite, Rsbuild and Astro use key + Enter; Expo uses single keys) | ratified single keys, unbuilt; contradicted by #2756 c3 | the mode chosen from the checked program | ballot D-DEV-KEYS1; draft jet-dev-session-keys | dev-keys-tty.log |
| Browser console sent to the terminal (Bun `console: true`, Vite `server.forwardConsole`) | none | new | draft jet-dev-browser-console | Bun and Vite docs |
| Background session with status, stop and logs (Astro); open endpoint (Expo) | none | new | draft jet-dev-agent-status | Astro and Expo docs |
| Component state kept across an edit (Vite, Next, Rsbuild, Parcel, Bun) | ratified `#Persist` on restart, unbuilt | nothing beyond the decision | #6386 c13 | probe `state_kept_after_edit` |
| LAN URL (Next) and QR (Expo) | `--share lan` registered, unbuilt | nothing beyond #5931 | #5931 | flag capture |
| Dependency pre-bundling (Vite) | not needed: no npm graph | none | no-action (Jet lead) | Vite docs |
| Type checking kept out of the dev server (Vite, Astro `check`) | Jet checks types in dev | none; Jet leads | no-action (Jet lead) | type-error probes |

## Decisions in conflict, sent to the owner as ballots

1. **D-FE-DEVPUSH1: how a tab learns of a new build.**
   The ratified dashboard has tabs poll `/__jet_dev_status` with a short lease. The option it came from names 400 ms.
   #2756 c1 asks Jet to beat Vite, and Vite updated the page 41 ms after the save. A 400 ms poll adds 200 ms on average before the tab knows.
   Every incumbent pushes over one WebSocket per tab. The recommended option A pushes the same snapshot down a socket and keeps the poll as the fallback while reconnecting. No user code changes.
   File: `/mnt/jetscratch/lanes/Main/ballots/D-FE-DEVPUSH1.json` (validator OK).
2. **D-DEV-KEYS1: who gets a key when the program reads the keyboard.**
   D-RUN-SESSION1 ratified single keys, but #2756 c3 says ordinary keys are never stolen (Ctrl+]).
   The recommended option A keeps single keys unless the checked program can read the keyboard. In that case jet dev's keys wait behind Ctrl+], and the status line says so.
   File: `/mnt/jetscratch/lanes/Main/ballots/D-DEV-KEYS1.json` (validator OK).

Note: D-FE-DEVSRV1, D-RUN-SESSION1 and D-WEBAPP-SERVE1 are archived in Tower with their finished cards, but they are still ratified, and the spec carries the dashboard text.

## Card drafts and attachments

The drafts are in `RUN/cards/`, for Main to file after review:

- `jet-dev-web-dashboard.md`: build the ratified dashboard (G1, G3, G5, G6, G14).
- `jet-dev-entry-ice.md`: fix `fn dev(d: Dev)` (G4, G20).
- `jet-dev-flag-hygiene.md`: `--port` and `--host`, unknown flags, the E2104 verb, help (G9).
- `jet-dev-session-keys.md`: the keys, after D-DEV-KEYS1 (G8).
- `jet-web-template-loop.md`: the `jet new` template and a web build (G11).
- `jet-dev-memory.md`: RSS and the edit cycle (G10).
- `jet-dev-browser-console.md`: browser errors to the terminal (G15).
- `jet-dev-agent-status.md`: find, reuse and stop a session (G16).

`RUN/cards/attach-notes.md` lists the evidence to add to:

- #4635 c1 and c2;
- #3720 c14, c15, c28, c29 and c48;
- #2756 c1 and c3;
- #4275 c82;
- #5069;
- #5931;
- #6386 c13;
- #6418;
- #6552 c1;
- #5201 c4.

## Sources

Documentation read on 2026-11-04 (ledger segments in RUN/segments.json):

- Vite: [CLI](https://vite.dev/guide/cli.md), [server options](https://vite.dev/config/server-options.md), [features and HMR](https://vite.dev/guide/features.md), [env and modes](https://vite.dev/guide/env-and-mode.md), [dependency pre-bundling](https://vite.dev/guide/dep-pre-bundling.md)
- Next.js: [CLI](https://nextjs.org/docs/app/api-reference/cli/next.md), [devIndicators](https://nextjs.org/docs/app/api-reference/config/next-config-js/devIndicators.md), [local development](https://nextjs.org/docs/app/guides/local-development.md)
- Astro: [CLI](https://docs.astro.build/en/reference/cli-reference/), [dev toolbar](https://docs.astro.build/en/guides/dev-toolbar/)
- Rsbuild: [dev server](https://rsbuild.rs/guide/basic/server.md), [HMR](https://rsbuild.rs/guide/advanced/hmr.md)
- Parcel: [development](https://parceljs.org/features/development/)
- Bun: [full-stack dev server](https://bun.com/docs/bundler/fullstack.md), [watch mode](https://bun.com/docs/runtime/watch-mode.md)
- Deno: [`deno serve`](https://docs.deno.com/runtime/reference/cli/serve.md)
- Expo: [Expo CLI](https://docs.expo.dev/more/expo-cli.md)

Prior-art registry check (`check_sources.py --registry Docs/spec/reference/prior-art.md`): the Astro dev toolbar is already tracked (prior-art.md:657). The other 16 URLs are new to the registry, and adding them is left to Main.

Jet code read: `Compiler/JetCli/Source/Cli/Front.jet:1794-1801`; `Compiler/JetCli/Source/Cli/Host.jet:2766-2919` (dev loop) and `:1700` (web template); `Core/Web/Web.jet:355-407`; `Compiler/JetIR/Source/X64Module.jet:407`; `Docs/spec/spec.md:4310-4342`.

## Coverage and confidence

`check_coverage.mjs` passes: 18 sources, 42 segments and 154 claims (109 from documentation, 45 from live captures).
Four documentation segments are below the item-density floor, and each has a written justification:

- `astro-cli/01`
- `bun-fullstack/02`
- `expo-start/02`
- `vite-features-hmr/03`

Each one is mostly example code or option tables for a feature already itemised once.
Every documentation page was fetched as Markdown and read in full, not just searched.

The recall audit has not been run: it needs a fresh-context reviewer.

Identity:

- Jet: compiler base `7c672809607df1ce1815eb21f8c07413224dd976`, dev HEAD `6c0d4b4e32`;
- incumbent versions: the table under Method.

Two repository examples were exercised from copies: `Examples/features/devloop/dev_context` and `Examples/features/web/jet_web`.

Measurement limits:

- one run per tool, on one machine;
- the browser was polled every 20 ms and the server every 50 ms, so times are only that precise;
- RSS is the whole process tree read once while idle;
- no warm-up runs were discarded.

Contradictions found:

- The Parcel docs promise a fallback port, but an explicit `-p` errored.
- The spec's dashboard form `jet dev <file.jet> --target=web` is refused with E2104.
- The ratified poll conflicts with #2756 c1 (ballot D-FE-DEVPUSH1).
- The ratified single keys conflict with #2756 c3 (ballot D-DEV-KEYS1).

Strongest unverified assumption: that the ~999 MB belongs to the `jet dev` compiler session rather than to a one-time peak. RSS was read once, and no heap profile was taken. Draft jet-dev-memory criterion 1 asks for the split.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| G1 | card | #4275 |
| G2 | card | #4635 |
| G3 | decision | D-FE-DEVSRV1=D |
| G4 | decision | D-ENTRY-SHAPE1=A |
| G5 | decision | D-FE-DEVSRV1=D |
| G6 | decision | D-FE-DEVSRV1=D |
| G7 | card | #4635 |
| G8 | decision | D-RUN-SESSION1=A |
| G9 | card | #6418 |
| G10 | card | #2756 |
| G11 | card | #6418 |
| G12 | card | #3720 |
| G13 | card | #3720 |
| G14 | decision | D-FE-DEVSRV1=D |
| G15 | no-action | new card draft cards/jet-dev-browser-console.md awaits Main's Tower write after fresh-context review |
| G16 | no-action | new card draft cards/jet-dev-agent-status.md awaits Main's Tower write after fresh-context review |
| G17 | card | #6386 |
| G18 | card | #5931 |
| G19 | card | #6552 |
| G20 | decision | D-ENTRY-SHAPE1=A |
| HTTPS dev certificate | no-action | no first-session need; Vite and Next keep it behind a flag; revisit with deploy work |
| Dev proxy and middleware | no-action | no first-session need for a Jet app that owns its server routes |
| `.env` loading and public prefix | no-action | environment ownership is tracked by #4275 (typed configuration and environment ownership); nothing dev-server specific found |
| Telemetry notice | no-action | jet dev printed no telemetry notice and `jet help dev` lists no telemetry setting, so there is nothing to disclose |
<!-- /audit-dispositions -->
