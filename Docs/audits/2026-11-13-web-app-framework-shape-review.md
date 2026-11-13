# Adversarial review: the web and app framework shape (2026-11-13 draft)

Date: 2026-11-13. Reviewer: Claude Opus, lane G6. The reviewer did not
write the proposal and did not build any part of it.

Subject: `Docs/proposals/2026-11-13-web-app-framework-shape.md` as
submitted in G12Docs-webshape (2661 lines, on dev `93380f43ef`). Line
numbers below are that file's. The card is #6792. This review answers its
criterion 3 and also reviews the draft against criteria 6 (native and web
coverage) and 7 (styling), which the owner added on 2026-11-13.

**Overall verdict: fails.** Two of the five claimed strengths fail outright
(safety, completeness), and the other three hold only within limits that
the draft does not state. The draft is silent on most of criterion 6 and
almost all of criterion 7. Twenty-four of the 57 Jet blocks do not parse or
name things that do not exist at dev. Section 7 lists every gap; each is for
G12Docs-5 to engineer away in the proposal, or to name with its cost,
mitigation and card. The redesigned draft then needs a second review.

## Method

- **Code basis.** Every claim about today's code was checked with `git show`
  and `git grep` at `93380f43ef`.
- **Snippets.** All 57 ```` ```jet ```` blocks were extracted one file each
  and checked with `jet check`. The checks used the seed-b14 compiler
  (a0.0.45-dev). The published `jet` (a0.0.44) lags dev: it rejects dev's own
  `Examples/Data/Analysis/TypedQuery.jet`, so its verdicts on syntax are
  not used. The proposed packages (`app`, `kit`, `auth`, `native`) do not
  exist, so "nothing named `app`" errors are expected and ignored. Only
  parse errors, wrong current syntax and wrong claims about existing
  surfaces count as findings. A file stops at its first parse error, so a
  failing file may hold more.
- **Probes.** One probe file checked forms the draft writes inside its
  types (`validate` rules, duration and size literals, `FieldError`
  fields). Its results are quoted where used.
- **Sources.** The reviewer cites the primary documentation of each
  existing system by URL. The URLs come from the reviewer's knowledge and
  were not fetched in this run; G12Docs-5 must confirm each one before
  citing it in the proposal.

## 1. The claims under attack

The draft's section 1 rules (lines 45-84) and its section 6 and 7 tables
make these claims. Each is attacked in section 2.

| # | Claim | Draft lines | Verdict |
|---|---|---|---|
| C1 | Business logic is written once, in one package with no UI, and runs on the server, web, desktop and phones | 55-60, 1179-1191 | holds within limits |
| C2 | Seamless: one app graph, one route identity, a screen calls a server function like any method | 50-54, 66-68, 640-644 | holds within limits |
| C3 | Safe: the compiler checks the boundary; secrets never cross; validation runs on both sides; wire skew fails the build | 80-84, 1556-1608 | fails |
| C4 | Fast: resumable pages, per-route code, live reads run together, no hydration, no loader waterfalls | 2591-2594, 1884-1935 | holds within limits |
| C5 | Complete: every area of a business app, on every target, in one cohesive package | 1193-1935, 2587-2601 | fails |
| C6 | Styling is at least as easy as HTML and CSS and much better (card c7) | 626, 720-721, 1486-1490 | fails (not addressed) |

## 2. Claim by claim

### C1. Business logic once

**Best existing solutions.**

- **Kotlin Multiplatform** shares a `commonMain` module across Android, iOS,
  desktop, web and server, with `expect`/`actual` for platform pieces
  (https://kotlinlang.org/docs/multiplatform-expect-actual.html). It says
  outright which code is per platform.
- **Blazor** runs one C# component model on the server, in WebAssembly, and
  inside native MAUI apps through Blazor Hybrid
  (https://learn.microsoft.com/aspnet/core/blazor/hybrid/). It is the
  closest existing "one codebase, web plus desktop plus phone" system, and
  the draft does not compare against it.
- **Replicache and Zero** run the same mutator code on the client
  (predicted) and on the server (authoritative)
  (https://doc.replicache.dev/concepts/how-it-works,
  https://zero.rocicorp.dev/docs/introduction). That is the idea behind
  D-OPTIMISTIC1, and they have shipped it.
- **Convex** keeps server functions in one `convex/` directory and calls
  them through typed client hooks (https://docs.convex.dev/functions).

**Attacks.**

1. **The one shared graph builder holds server-only code.**
   `Screens/shell.jet` `crewboard()` (lines 620-637) opens the database
   (`logic.open_crew("crewboard.db")`) and the account store
   (`auth.sqlite("accounts.db")`). The desktop (line 1048) and phone
   (line 1111) entries call `screens.crewboard()`. The comment at 621-622
   says client builds never contain `crew`, but `crew` is a local in the
   same function body that the clients run. Placement would have to split
   one function's body between targets. No ratified decision does that, and
   the draft does not say how, or what a client sees when `open_crew` would
   have run.
2. **The phone app needs server code the server does not have.** The phone
   entry adds `.screen("/mine", mine, ...)` (line 1113) and calls
   `Crew.mine` (lines 1125, 1152). The logic package has no `mine` method
   (it has only a local `mine` inside two methods, lines 428 and 441). The
   server binary built from `Web/web.jet` would not serve it. So either
   the phone changes the server (logic is not shared), or the call fails at
   run time. The same graph does not run on every target (line 52).
3. **"About 85% shared" is measured loosely.** Counted on the draft's own
   blocks (non-blank lines, including comments and manifests): logic 301,
   screens 306, web 33, desktop 45, phone 50. That is 607 of 735 lines, or
   83%. Section 3.8's 330/330/45/60/60 do not match. The share holds
   roughly, but it counts a logic package that omits the hard parts:
   `close_from_issue` and `Crew.mine` are called and never written, and
   `seed_shared_project` (line 959) does not exist.
4. **A service locator under another name.** `app.service<logic.Crew>()`
   (lines 1006 and 1073) looks up a mounted value by type from a hidden
   context. Section O2 rejects option B for the same hidden lookup. The
   draft must choose one rule.
5. **Context is no longer an effect.** D-CONTEXT-EFFECT1=F (ratified
   2026-11-12; see `Docs/audits/context-implicit-input-review-2026-11-12.md`)
   removes `Context` as an effect name. The logic package writes
   `-[DB.Read, Context]>` and `-[DB.Write, Context, Time]>` on every
   method (lines 416-492, 2282). The signatures have to drop `Context`, and
   the draft's claims that rely on the effect row showing a context read
   (for example 1167, "`role_in` reads Ana's session from the request
   context") need another visible signal.

**Verdict: holds within limits.** One logic package is real for pure rules
and server methods. It fails for the graph builder (attack 1) and for any
target that adds server calls (attack 2). Fix both, and state the limits.

### C2. Seamless: one graph, one address, calls like methods

**Best existing solutions.**

- **TanStack Router** types path and search parameters end to end, and
  `Link` refuses an unknown route at compile time
  (https://tanstack.com/router/latest/docs/framework/react/guide/type-safety).
- **Remix and React Router** actions make forms work without JavaScript and
  revalidate loaders after every action
  (https://reactrouter.com/start/framework/actions).
- **tRPC** and **SvelteKit remote functions** give typed calls without a
  schema file (https://trpc.io/docs,
  https://svelte.dev/docs/kit/remote-functions).
- **Phoenix LiveView** keeps all state on the server, so there is no client
  cache to get wrong (https://hexdocs.pm/phoenix_live_view).

**Attacks.**

1. **String routes survive.** Rule 4 makes the state struct the address,
   yet `app.go_back_or("/")` (915), `app.link(..., to: "/")` (940) and the
   tabs `to: "/"` and `to: "/mine"` (1115-1116) are strings. Section 4.1
   registers `.screen("/", home, state: "")`, a `String` state, while the
   rule says two screens with one state type are an error: every app with
   two string-state screens breaks. 4.13's `Shop` collides with 4.4's
   `Shop`.
2. **Remote calls look local but are not.** A click handler calls
   `crew.move_task(...)` synchronously. The draft never says what the
   handler does while the call is in flight (seconds on a phone), whether
   it blocks the UI thread on native targets, or how a person cancels it.
   tRPC, Convex and SvelteKit all make the await visible.
3. **Every method is an endpoint.** Every non-`priv` method of a mounted
   struct becomes a public endpoint (O2 A). A helper made `pub` for a test
   becomes callable from the internet. Next.js documents this exact hazard
   for Server Actions
   (https://nextjs.org/blog/security-nextjs-server-components-actions). The
   draft cites Next.js as a lesson but repeats the default.
4. **Live data is not seamless across loops.** A live query inside a
   conditional, or a read that depends on another read's result, cannot run
   together with the others (section 4.13 claims they all do; see C4).
5. **O10 A mixes borrow and link.** It passes an `@Int` where
   `&ui.State<Int>` is expected and writes `count += 1` on a link
   (lines 2444-2470). The seed rejects `@Int` as a type, which is expected
   for paused work, but the draft never says which of the two the screen
   holds, or whether `+=` on a link writes through.

**Verdict: holds within limits.** The graph and typed calls are coherent for
online screens. Fix the string routes, state-type collisions and in-flight
semantics, and make exposure explicit.

### C3. Safety at the boundary

**Best existing solutions.**

- **Firestore security rules** make every client read and write go through
  declared rules, offline included
  (https://firebase.google.com/docs/firestore/security/get-started).
- **Convex** makes each function choose `query`, `mutation` or `action` and
  recommends argument validators and an auth check in every public function
  (https://docs.convex.dev/auth/functions-auth).
- **Rails** and **Django** check CSRF on every unsafe method by default and
  sign session cookies by default.
- **Stripe** requires idempotency keys for safe retries
  (https://docs.stripe.com/api/idempotent_requests). **GitHub** signs every
  webhook delivery
  (https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries).

**Attacks and checks.**

1. **Public unless the method reads `auth.user()`.** A write that forgets
   the read is public. `pin` and `bump` in section 4 are, as written. A
   refactor that moves the `auth.user()` call into a helper changes
   exposure silently. Convex, Rails and Django default the other way.
2. **The webhook is unauthenticated.** `github_closed` (lines 1004-1009)
   decodes the body and writes to the database without checking GitHub's
   `X-Hub-Signature-256`. Anyone who learns the URL closes tasks.
3. **The secret error codes are wrong.** Section 4.7 says
   `billing.stripe_key` flowing into a `String` result is E0737
   (line 1586). At dev, E0737 is `jet.FixedTimingSteer` and E0739 is
   `jet.FixedTimingCheckedArithmetic`: rules about fixed-timing code, not
   secret flow. The flow denial at dev is E0721 `jet.TaggedValueDenied`.
4. **`#Secret` is written in the wrong place.** Tags attach to types
   (`stripe_key: #Secret String`); the field-prefix form
   `#Secret name: T` that the draft uses (lines 295, 908, 1347, 1574) has
   no use at dev. The draft's `#Key` marker collides with the tag
   namespace: a dev fixture declares `Key: tag { ... }`.
5. **The password leaks through `confirm`.** Section 4.3 makes `password`
   secret but leaves `confirm: String{""}` plain (line 1348). Everything the
   draft says about secrets (never filled back, never logged) is false for
   the copy. `crypto.reveal_eq` has no use at dev.
6. **The offline queue replays without idempotency.** If a write reaches
   the server and the response is lost, the queued retry runs it again.
   `create_task` makes a duplicate. The draft has no request key.
7. **Validation is not on both sides as written.** Line 447 says decoding
   runs `validate`. The 4.11 test needs `app.wire(...)` to trigger it, and
   at dev the `validate` block accepts only
   `check(condition, at: field, "message")`. The probe:
   `title.trim().min_len(1).max_len(200)` gives
   `jet.InvalidValidateRule`. The draft's rule chains (lines 1352-1354)
   are new syntax that the draft does not flag.
8. **Unhandled errors.** `create_task(...)` in `new_task` has no `?`, nor
   does `crew.board(...)` in `export_here`. `role:` in `board` takes
   `role_in(...)`, which returns `Role TaskError!`. 4.3 `sign_up` returns
   `[FieldError]!` and 4.2 returns `DBError` with no `impl app.CallError`,
   against 4.1's own rule. `impl [FieldError] -> TaskError` does not
   parse ("Expected a name after `impl`, found `[`").
9. **`FieldError` fields.** The draft builds
   `FieldError{path: ..., message: ...}`. At dev the field is `reason`
   (probe: "`FieldError` has no field `message`").
10. **Wire-skew safety has no story for removal.** D-WIRESKEW1 fails the
    build on a breaking change. The draft does not say how a field is ever
    removed (an expiry window, a minimum app version, forced update). The
    store's review delay makes this a real cost.
11. **Tokens on phones.** O7 A uses bearer tokens in the Keychain. Neither
    refresh, revocation nor what happens when a token expires during a
    long offline stretch (queued writes then fail auth) is specified.

**Verdict: fails.** The default exposure rule, the unsigned webhook, the
leaked confirm field and non-idempotent replay are each a security bug in
the flagship example. The wrong error codes and the field-prefix
`#Secret` mean the draft's description of D-SECRET-CT1 does not match the
code.

### C4. Performance

**Best existing solutions.**

- **Qwik** pioneered resumability: no hydration replay, handlers loaded on
  first interaction (https://qwik.dev/docs/concepts/resumable/). D-RESUME1
  is the same idea; the draft should compare against Qwik directly.
- **Convex** re-runs a query only when a document in its tracked read set
  changes, and shares one result among subscribers with the same arguments
  (https://docs.convex.dev/realtime).
- **Phoenix LiveView** sends minimal diffs over one socket and pays no
  client runtime (https://hexdocs.pm/phoenix_live_view).
- **React Server Components with streaming** flush HTML as data arrives
  (https://react.dev/reference/react-dom/server/renderToPipeableStream).

**Attacks.**

1. **Live-query fan-out.** Section 7 item 4 admits one re-run per
   (arguments, session) key. The draft has no shared-result mode, no cap,
   no back-pressure, and no answer for many servers: an invalidation on one
   server must reach subscribers on another, which needs a broker. O6
   mentions realtime but not multi-node invalidation.
2. **Predicate read sets are expensive.** "Rows where `pinned`" as a read
   set (line 1319) means compiling a closure to SQL and matching every write
   against every live predicate. That is LINQ-to-SQL and EF Core's hardest
   problem (https://learn.microsoft.com/ef/core/querying/how-query-works);
   the draft treats it as free.
3. **"Loader waterfalls" are overstated.** Reads inside a conditional, and
   reads whose arguments depend on an earlier read, still run in sequence.
   The claim needs that limit.
4. **Prediction can diverge.** The client replays `move_task`'s closure but
   cannot run the server's checks (role, version), and `time.now()` differs
   on the client. The draft names flicker; it does not name wrong
   `updated` stamps or a card that is valid locally and refused remotely.
5. **The one-megabyte client runtime** (section 7 item 1) has no budget,
   no measurement plan and no comparison with Qwik's or LiveView's.
6. **No streaming.** Server rendering waits for every live read. Slow reads
   delay first paint. Card c6 asks for streaming; the draft does not
   cover it.

**Verdict: holds within limits.** Resumability and per-route code are
sound. The live-query cost model, multi-server invalidation and streaming
are open.

### C5. Completeness

#### C5a. The card c1 areas

All twelve areas have a section. These are incomplete or contradicted:

- **Offline (4.8, O4).** The `.Offline` notice "Saved on this device" in 4.10
  (line 1765) is false unless the app sets `.offline(.Queue)`. Two queued
  edits of one task conflict with themselves on `version`. The browser
  queue is per tab, so two tabs replay twice. `app.collection` sync states
  no permissions (compare Firestore rules, Replicache's server-side push
  authority). The draft compares with neither ElectricSQL
  (https://electric-sql.com/docs) nor PowerSync
  (https://docs.powersync.com).
- **Realtime (4.9).** The chat example's `use` lacks `db` and `auth`.
- **Tests (4.11).** `notes_app` (lines 1811, 1822) is defined only inside
  O4, and `seed_shared_project` does not exist.
- **Count of amendments.** Section 5 says "four places" amend ratified
  decisions (line 1939). The ballot table lists A1-A3 and amendments under
  O1 B, O2 A and B, and O10 B: more than four.

#### C5b. Card c6: everything a native app needs

The draft covers windows, a menu bar, tabs, deep links and store signing
in passing. Against the card's list:

| Need (card c6) | Draft | What dev has |
|---|---|---|
| Windows, menus, tray, menu bar | windows and menu bar in 3.5; **no tray, no context menus** | `Core/Ui` Wayland windows and menus |
| Notifications, including push | **missing**; section 7 item 5 defers push to "a separate card" without naming it | none |
| Background work | **missing** | none for apps |
| File system and file pickers | 3.5 uses `core.ui.host.files`, which **does not exist** (seed: "There is no core module `core.ui.host.files`") | `core.files`; a file-chooser mention in `Core/Ui/Host.jet` |
| Camera, sensors, location | **missing** | `Core/Web/Browser.jet` location for test browsers only |
| Deep links and app links | `.deep_links(host:)` only; **no** `apple-app-site-association` or `assetlinks.json` served by the web app, **no** custom schemes on desktop | none |
| Clipboard, drag and drop, system share | share in the phone entry; **no clipboard or drag and drop example** | `Core/Ui/Host/Clipboard.jet`, `DragDrop.jet` (desktop) |
| Permissions | **missing** (camera, location, notifications all need runtime prompts and manifest entries) | none |
| Accessibility and localization | a table row (1486-1490) naming D-A11Y-EXPORT1 and D-I18N1; **no example** | `Core/Ui/Host/Accessibility.jet` |
| Dark mode and system theme | **missing** | `Core/Ui/Dbus.jet` reads the color scheme on Linux |
| App lifecycle | **missing** (suspend, resume, terminate, restore beyond one table row at 1445) | none |
| Install, update, signing | `jet ship android` signs (1859); **no update channel** for desktop, **no** notarization, **no** store update policy against D-WIRESKEW1 | none |

The best existing solutions here: **Expo** ships one SDK module per device
need with a config plugin that writes the manifest permission entries
(https://docs.expo.dev/versions/latest/); **Capacitor** offers the same as
plugins over a web view (https://capacitorjs.com/docs/apis); **Tauri 2**
covers tray, menus, notifications, deep links, updater and signing on
desktop and phones as plugins (https://v2.tauri.app/plugin/); **Electron**
has `autoUpdater` and code signing guides
(https://www.electronjs.org/docs/latest/api/auto-updater); **Flutter**
reaches the rest through platform channels and its plugin registry
(https://docs.flutter.dev/platform-integration/platform-channels).

#### C5c. Card c6: the web side

| Need (card c6) | Draft |
|---|---|
| SEO and metadata | `.head(description:)` once (line 994). **No** per-screen title, canonical URL, Open Graph, `robots`, sitemap or structured data. Next.js's metadata API is the bar (https://nextjs.org/docs/app/building-your-application/optimizing/metadata). |
| Streaming and server rendering | rendering yes; **streaming missing** |
| Images and assets | `ui.image(...)` with D-WEB-IMAGE1 (line 1910); **no** asset pipeline, hashing or fonts |
| Uploads | **missing** (multipart, size limits, progress, resumable uploads, storage) |
| Caching headers | **missing** (`Cache-Control`, ETags, immutable hashed assets, CDN) |
| Progressive web app install | **missing** (manifest, icons, service worker; the offline queue in 4.8 needs a service worker that the draft never mentions) |

**Verdict on C5: fails.** Card c6's coverage table does not exist, and
most rows of it have no example.

### C6. Styling (card c7)

The draft has one theme line, `ui.theme(accent: ui.hex("#2e6be6"),
radius: 8)` (line 626), one grid (720-721) and a rendering table
(1486-1490). It has no styling section. At dev, `Core/Ui` has `Color` and
`SizeConstraint` and no style system; the web host positions every element
absolutely with inline pixel styles
(`Compiler/JetWeb/Source/Web/Host.jet:389-393`; the draft cites lines
479-491, which is wrong). D-THEME2 and D-LAYOUT2 exist only as ratified
rows in `Docs/spec/syntax-decisions.md`; the repo holds no design text for
them that the draft could rest on.

**Best existing solutions, at their best.**

- **CSS** itself now has cascade layers to tame specificity
  (https://developer.mozilla.org/en-US/docs/Web/CSS/@layer), container
  queries for component-level responsive layout
  (https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_containment/Container_queries),
  native nesting, `:focus-visible`, `prefers-color-scheme` and
  `prefers-reduced-motion`. Grid and flexbox are mature. Its weakness is
  global scope and the cascade, not missing power.
- **Tailwind** puts design tokens into utility classes, removes dead CSS at
  build time and keeps styles beside markup (https://tailwindcss.com/docs).
- **vanilla-extract** and **StyleX** give typed, scoped, build-time CSS with
  conflict resolution by "last style wins" instead of specificity
  (https://vanilla-extract.style, https://stylexjs.com).
- **SwiftUI** modifiers and **Jetpack Compose** `Modifier` chains are typed,
  scoped by construction, and adapt to dark mode and Dynamic Type through
  the environment
  (https://developer.apple.com/documentation/swiftui/view-modifiers,
  https://developer.android.com/develop/ui/compose/designsystems/material3).
- **Flutter** `ThemeData` and `ThemeExtension` give typed tokens across
  every platform (https://api.flutter.dev/flutter/material/ThemeData-class.html).

**Attacks on each "better than CSS" guarantee card c7 asks for.** The draft
makes none of these claims yet; G12Docs-5 must make them with these limits
in view, or they fail on arrival.

1. **Type-checked style values.** TypeScript CSS-in-JS (vanilla-extract,
   StyleX) and SwiftUI already type style values. "Better than CSS" holds;
   "better than the best" needs more: units checked against the property
   (no `padding: Color`), token references checked to exist, and contrast
   checked (below).
2. **No dead or conflicting rules.** Tailwind and StyleX already remove dead
   styles at build time. A conflict check is only meaningful if styles can
   still conflict; with styles as values attached to one node, the honest
   claim is "conflicts cannot be written", and the draft must show what
   happens when a theme, a component default and a call site all set one
   property (which wins, and is that visible?).
3. **No specificity or cascade surprises.** Removing the cascade also
   removes inheritance that people rely on (font, color, direction). The
   draft must say what inherits, through what (an environment, like
   SwiftUI's), and how a nested screen overrides it.
4. **Scoped styles.** CSS Modules, Shadow DOM and every component framework
   already scope. The claim holds only if the web output also cannot leak:
   the generated HTML class names and any user-supplied CSS (third-party
   widgets, embeds) need a stated rule.
5. **Compile-time errors for invalid layouts.** Most layout failures are
   content-dependent (long text, a German translation, a 320-pixel phone,
   200% text size), so they cannot be compile-time errors. What sema can
   catch is narrow: impossible constraints, a grid with zero columns, a
   fixed size that cannot hold its minimum content at a declared breakpoint.
   The claim must be stated at that size, with the rest covered by preview
   tests (D-UI preview registry exists in `Core/Ui/Ui.jet`).
6. **"Orders of magnitude better."** Nothing measurable backs it. Proposed
   measures: lines to style the Crewboard board against HTML plus CSS,
   Tailwind and SwiftUI; count of style bugs a checker catches in a seeded
   corpus; time to apply a dark theme. Without a measure the phrase cannot
   appear in public text (AGENTS.md: a design may be called better only
   where a passing review backs that exact claim).
7. **One style model, three renderers.** Web output is semantic HTML with
   browser layout (D-ONEAPP-WEBOUT1), while native targets use Jet's own
   layout. The same style must lay out the same way under both, or the
   draft must name where they differ (text shaping, line breaking,
   scrollbars, subpixel rounding). This is the risk that sank many
   cross-platform layout engines and it is unmentioned.

**Missing coverage, every row of card c7:** layout (stack, grid, alignment,
spacing) beyond one grid call; typography and fonts; color and themes
including dark mode; responsive and adaptive layout for phone, desktop and
web; states (hover, focus, pressed, disabled); animation and transitions;
reusable styles and design tokens; per-platform adaptation; accessibility
(contrast, focus rings, reduced motion). The card also asks for the same
Crewboard screens styled under each alternative, side by side with HTML and
CSS, Tailwind and SwiftUI or Flutter. None of it exists.

**Verdict on C6: fails** (not addressed).

## 3. The snippets against the seed compiler

These fail on syntax or name a surface that is wrong at dev. Each line names
the snippet's first line in the draft.

| Draft line | Problem (seed-b14 message) | Current form |
|---|---|---|
| 195, 224, 597 | `logic: ./Logic`, `../Logic`: "Expected a value, found `.`" | a quoted path in the dependency row |
| 185-232 | top-level `name:` and `deps:` in a manifest: "Constant `name` must be ALL_CAPS" | the manifest's `package` block |
| 255, 1279 | stacked `#Codable` and `#PublishedSchema`: `jet.StackedRuleLines` | `#[Codable, PublishedSchema]` |
| 393 | `impl [FieldError] -> TaskError`: "Expected a name after `impl`, found `[`" | convert from a named type |
| 389 | `rank` returns its borrowed `tasks`: `ReadOnlyValueNotOwned` | take the list by move, or return a new one |
| 646, 688, 866 | `if cond { a } else { b }` as a value in a list: "Expected `->` after the condition of a value-producing `if`" | `if cond -> a else -> b` |
| 1755 | `stale == Err(...)`: bare case pattern | `.Err(...)` |
| 1037 | `core.ui.host.files`: "There is no core module" | none at dev |
| 1096, 2191, 2401 | `comptime if build.target.os == {`: "computes a value but doesn't do anything" | `comptime if #build.target.os == {`; `form` is not a dispatch subject |
| 1864 | `.{ services: .{ ... } }`: "Expected a value, found `.`" | a named struct literal |
| 2034 | `#Server`: "isn't a known applied rule"; the draft says `#Server.Get` and `#Server.Post` already exist (line 2615) | not at dev |
| 2420 | `#Target(IOS)`: "is not a known web partition" | `#Target` names web partitions only |
| 2322 | top-level `me: auth.user()`: a constant must be ALL_CAPS | inside a function |
| 2357-2376 | `outputs: { ... }`: inferred struct literal with no expected type | a typed literal |
| probe | `title.trim().min_len(1)` in `validate`: `jet.InvalidValidateRule` | `check(cond, at: field, "message")` |
| probe | `200.ms`, `30.s`, `30.days`, `64.KiB`, `4.MiB`: "only works on a declared struct or tuple field" | `Duration.milliseconds(200)`; sizes as integers |
| probe | `FieldError{message:}`: no field `message` | `reason` |

Also not at dev and not marked as new: `jet inspect choices` (no command),
`ui.State<T>`, `@T{...}`, `reveal_eq`, `#Query`, `#Key`, `.split()` on a
string list in the form the draft uses.

These hold at dev as the draft states: `Core/Web` is about 7,000 lines
(7,143); the web battery is 244 lines; `Core/App/App.jet` declares package
`app`; `AppHello`, `FormsTable`, `StateSurface` with `web.live`,
`web.invalidate` and `web.auth("session")`; `mutate_with_invalidations`;
`reactive.signal(100)` (in `Examples/Apps/UI/Reactive.jet`, not the web
examples); the `.csr`, `.ssr`, `.island` and `.hydration_dev` modes;
`migration X { add f: T = v }`; the `validate { check(...) }` block;
`distinct Int`; `String{""}` defaults; `impl A -> B` conversions;
`#Transact {`; `SQL{...}`; `DBScope`; `core.ui.host`.

## 4. Comparison gaps

Section 6 compares with Next.js, SvelteKit, LiveView, Leptos, Dioxus,
Flutter, Expo, Tauri and KMP. It omits the systems closest to its
strongest ideas, each of which does something the shape does not:

| System | Closest to | What it does that the shape does not |
|---|---|---|
| Qwik | D-RESUME1 | shipped resumability with measured costs |
| Convex | live queries, read sets | shares one query result among subscribers; scheduled functions; explicit query/mutation split |
| Replicache, Zero | D-OPTIMISTIC1 | mutators with request IDs (idempotent replay), server-authoritative rebase |
| Firestore | offline plus live plus rules | per-document security rules enforced on sync; multi-tab persistence |
| ElectricSQL, PowerSync | offline Postgres or SQLite sync | partial replication by declared shape with permissions |
| Blazor (Server, WASM, Hybrid) | one codebase, web plus native | one component model on every target, in production |
| Remix, React Router | forms, revalidation | progressive enhancement without JavaScript |
| TanStack Router | typed routes | typed search parameters with validation |
| Hotwire | server-rendered UI | HTML over the wire with no client framework |
| Meteor | isomorphic live data | the original "same code on client and server" with latency compensation, and its lessons on fan-out |

## 5. Ratification dependencies

- The draft's effect rows conflict with D-CONTEXT-EFFECT1=F (C1 attack 5).
- The build cards #6553-#6558 are paused by the owner; their designs are
  used throughout. The draft names this (line 16) but the examples read as
  if those designs were settled.

## 6. Verdicts

| Claim | Verdict |
|---|---|
| C1 logic once | holds within limits: not for the graph builder or target-added server calls |
| C2 seamless | holds within limits: string routes, state-type collisions, in-flight call semantics |
| C3 safety | **fails** |
| C4 performance | holds within limits: fan-out, multi-server invalidation, predicate read sets, streaming |
| C5 completeness | **fails**: card c6 coverage missing |
| C6 styling | **fails**: card c7 not addressed |

## 7. Gaps for G12Docs-5

Each must be engineered away in the proposal or named with its cost,
mitigation and card. Items marked *(card)* need a card or ballot that Main
files.

1. Split `crewboard()`: server resources must not live in the graph builder
   that clients run. Show where `open_crew` and `auth.sqlite` go.
2. Phone-only screens that need new server methods: either the method
   lives in logic and the server serves it, or target-only screens are
   limited to existing calls. Write `Crew.mine` and `close_from_issue`.
3. Remove `app.service<T>()` or justify it against O2's rejection of B.
4. Drop `Context` from every effect row (D-CONTEXT-EFFECT1=F) and restate
   how a context read stays visible.
5. Replace every string route (`"/"`, `"/mine"`) with a state value;
   resolve the `state: ""` collision and the two `Shop` types.
6. Specify in-flight remote calls: pending state, cancellation, native UI
   thread, timeouts.
7. Exposure: make endpoints private by default or require an explicit
   `#Public` or auth rule per method; show the error for a write that names
   neither. *(ballot option)*
8. Verify the webhook signature in `github_closed`.
9. Fix the secret codes (E0737 and E0739 are fixed-timing rules; the flow
   denial is E0721) and write tags on types (`name: #Secret String`).
   Rename the `#Key` marker away from the tag namespace.
10. Make `confirm` secret, or drop it.
11. Idempotency keys on queued and retried writes; per-device rebase of two
    queued edits to one row; one queue across tabs; behavior when the token
    expires offline. *(card)*
12. State `app.collection` sync permissions.
13. Say what validation syntax is new (`min_len`, `email()`, `range`) and
    mark it for a ballot, or use `check(...)`; fix `FieldError.reason`;
    reconcile line 447 with the 4.11 test.
14. Add `?` or handling to `create_task`, `crew.board`, `role_in`; add the
    missing `impl app.CallError` conversions; drop
    `impl [FieldError] -> TaskError`.
15. Wire-skew removal policy: minimum app version, expiry, forced update.
    *(card)*
16. Token refresh and revocation for O7 A.
17. Live-query cost model: shared results, caps, multi-server invalidation
    (broker), predicate-to-SQL compilation limits. *(card)*
18. State the waterfall limit (conditional and dependent reads).
19. Prediction: what the client may not predict (server checks,
    `time.now()`), and how a refused prediction is shown.
20. Client runtime budget per target, with a measurement plan.
21. Streaming server rendering. *(card)*
22. Card c6 coverage table with one example per native need: tray,
    context menus, notifications and push, background work, file pickers,
    camera, sensors, location, app links (AASA and assetlinks served by the
    web app), custom schemes, clipboard, drag and drop, permissions,
    accessibility, localization, dark mode, lifecycle, desktop updates,
    notarization. Name each gap's card. *(cards)*
23. Card c6 web rows: per-screen metadata and SEO, streaming, assets,
    uploads, caching headers, PWA install with a service worker. *(cards)*
24. Card c7 styling section: every row of the card, the Crewboard screens
    under each alternative, side by side with HTML plus CSS, Tailwind and
    SwiftUI or Flutter, and each guarantee restated within the limits in
    C6 attacks 1-7, with a measure for "better". *(ballot)*
25. Fix the Host.jet citation (389-393, not 479-491).
26. Fix every snippet in section 3's table, or mark each form as proposed
    new syntax that needs a ballot (`200.ms`, `#Target(IOS)`,
    `comptime if build.target.form`, `#Server`).
27. Define `seed_shared_project`, `notes_app`, `User`, `UserID`,
    `stripe_charge`, `Receipt`, `create_task_in`, `Doc`, `Docs`,
    `DocView`, `RoomID`, `Message`, or show where they come from; add `db`
    and `auth` to 4.9's `use` and the `native` dependency to the phone
    manifest.
28. `t.due ?? ""` mixes a date and a string in the CSV export, which also
    prints the status enum unformatted.
29. Correct "four places" in section 5 to the real count of amendments.
30. Measure section 3.8 from the final blocks.
31. Add the section 4 comparisons with sources.
32. 4.10's `.Offline` notice must depend on `.offline(.Queue)`.

# Round 2: revision 2 of the proposal

Date: 2026-11-13. Reviewer: Claude Opus, lane G6. The reviewer did not
write either revision and built no part of the shape.

Subject: revision 2 of `Docs/proposals/2026-11-13-web-app-framework-shape.md`
as submitted in G12Docs-webshape-r2 (4,215 lines, base dev `93380f43ef`).
Line numbers in this round refer to that file. This round checks the 32
round-one gaps and tests criteria c6 (native and web coverage), c7
(styling) and c8 (Crewboard as a gap-finding probe, the comparison matrix
and the claims register CL1–CL9).

**Overall verdict for round 2: fails.** Revision 2 is a large step. Every
Jet block is now a real file. All 41 files reproduce on seed-b14, and the
native and web coverage asked for by c6 is nearly complete. Twenty-five of
the 32 round-one gaps are closed, and the other seven are only partly
closed. It still fails c8 on four counts:

1. None of the nine "goes further" claims holds as stated against the
   strongest system in its area.
2. Two claims cite evidence that the seed contradicts: CL1 cites the stub
   checks, and CL5 cites E0721.
3. Crewboard works around at least ten framework needs in app code, and
   names none of them as a gap.
4. Three new examples carry security bugs.

c7 is partly met. Section R2-6 lists every gap for G12Docs-5 in one pass.

## R2-1. Method

- **Reproduction.** The reviewer copied G12Docs-5's checking kit to the G6
  scratch and ran `build.mjs`. It then ran
  `check.sh` with seed-b14 (`jet@a0.0.45#1e8010c1…`), one file at a time
  under an 8 GiB cap, with the base `93380f43ef` as the toolchain root.
  All 41 results match Appendix A.3 exactly:
  - 31 files are clean, the uncaught style bugs b7 and b8 among them;
  - `new_task.jet`, `task.jet` and `sign_in.jet` each report
    `jet.UnknownField`;
  - `screens_test.jet` reports two `jet.ModuleAsValue` and four
    `jet.UninferableTypeArgument`;
  - b1–b6 each report their seeded error.
- **Fidelity.** A script compared every labeled Jet block in the proposal
  with the kit file of that name:
  - every labeled Crewboard and example block is byte-identical to its
    file, and so are the six stub files of section 10;
  - the eight unlabeled blocks are the section 7 alternatives, marked "not
    checked".
- **Probes.** The reviewer added three probe files to the checked copy:
  - **CL1.** `Screens/zz_probe_cl1.jet` is a screen that calls
    `crew.webhook_secret(...)` and `crew.on_issue_event(...)`. Neither is
    an endpoint.
  - **CL5.** `Logic/zz_probe_cl5.jet` and `Logic/zz_probe_cl5b.jet` move
    `Project.webhook_secret` (`#Secret String`) into a plain `String`
    field, an interpolated string, a closure result, an error payload and
    `print`.

  Results are quoted where used.
- **Other checks.**
  - **Cards.** Tower card reads confirm that the criteria revision 2 cites
    exist on #3709, #6556, #6558, #6818 and #6819–#6823.
  - **Code.** Code claims were checked with `git show` and `git grep` at
    `93380f43ef`:
    - `Host.jet:389–393`;
    - the E0721, E0737 and E0739 rows;
    - spec.md:882 ("A derived decoder validates");
    - the line counts in 3.8.
- **Sources.** The URLs this round adds come from the reviewer's knowledge
  and were not fetched in this run. G12Docs-5 must open each before citing
  it.

## R2-2. The 32 round-one gaps

| # | Verdict | Evidence or residual |
|---|---|---|
| 1 | closed | `crewboard()` (854–867) holds no store; only `Web/web.jet` opens one (1225–1233) |
| 2 | closed | `Crew.mine` (617–621) and `close_from_issue` (685–700) are written |
| 3 | closed | `app.service` is gone. Residual: `#App.SigningKey` (704) is found by marker, an implicit lookup that `signature: .GitHub` (679) does not name; see R2-6 item 19 |
| 4 | closed | no effect row names `Context` |
| 5 | closed | links take state values. Residual: neither the path template ↔ field binding (`:project` to `BoardView.project`) nor a link to a state no graph registers is stated as a compile error, and the stubs cannot show it (`screen: fn<F>`, `go: fn<V>`; 3405, 3437) |
| 6 | **partly** | 4.1 states pending, cancel, timeout and the UI thread. Missing: where a button handler's error goes. Moves drop it with `_:` (981, 1897, 2131), and 4.1 (1508–1510) routes only form and read errors. 3.7 step 1 ("busy until the call settles") contradicts step 2 (a queued write may settle hours later) |
| 7 | closed in design | markers, and O2 A. CL1's evidence is wrong (R2-4) |
| 8 | closed | signature checked before decode. Residual: no replay or redelivery rule; GitHub signs no timestamp (R2-6 item 16) |
| 9 | **partly** | the codes are right (E0721 is `jet.TaggedValueDenied`; E0737 and E0739 are fixed-timing rules), tags are on types, `#Key` is gone. But the text that a flow of `#Secret` "into a plain value, a response or a log is E0721" (316–318, 1655–1657, table at 186) is false on seed-b14: every probe flow in CL5 checks clean. `#Secret` at dev is the fixed-timing tag; its client-path rule is D-SECRETEDGE1 (#6557, paused) |
| 10 | closed | no confirm field; `reveal: true` |
| 11 | closed | #3709 has the request-key, two-edit, two-tab and offline-expiry criteria |
| 12 | closed | 4.8 (1698–1702) |
| 13 | closed | `check(...)` only; spec.md:882 confirms D-VALIDATE-DECODE1=B |
| 14 | closed | conversions exist. Residual: TR2 reads `app.Remote<X>` as `X`, so no checked file exercises `impl app.CallError -> TaskError` or a client call's error path |
| 15 | closed | #6558 criterion |
| 16 | closed | #6818 criterion |
| 17 | closed | #3709 criteria. Residual: 4.9 (1718–1719) says results are shared for "the same caller rights", but the card says "reads no caller-dependent input". Every Crewboard query reads `app.caller()` through `role_in` or `me`, so the probe's own queries never share |
| 18 | closed | 4.6 |
| 19 | closed in 4.6 | but CL4 contradicts it (R2-4) |
| 20 | closed | #6556 criterion |
| 21 | closed | `.stream(after:)`; #6812 has no criteria yet |
| 22 | closed, with defects | 5.1; defects in R2-3 |
| 23 | closed | 5.2 |
| 24 | **partly** | see R2-5 |
| 25 | closed | `Host.jet:389–393` verified |
| 26 | closed | 41 of 41 reproduce, and every block is byte-identical to its file. Limits: TR2, and builder generics the stubs leave unconstrained (`screen<F>`, `theme<T>`, `mount<S>`, `api<S>`), so the graph itself (routes, screen parameters, which service a screen receives) is not type-checked by anything |
| 27 | closed | the undefined names are gone; the phone manifest lists `native` |
| 28 | closed | label and ISO date columns. Residual: CSV formula injection (R2-6 item 18) |
| 29 | closed | two amendments. Amendment 2 (3095–3098) exists only because Jet has no bound method values, and that language gap has no card (R2-6 item 1) |
| 30 | closed | recounted: Logic 353, Screens 321 + 43, manifests 88 |
| 31 | closed | URLs not fetched by the reviewer |
| 32 | **partly** | 4.8 (1693–1696) says the web shows nothing, but the `waiting` comment in `realtime.jet` (1743–1744) says "the web shows it when the service worker holds writes" |

Twenty-five rows are closed (two of them closed in design or text only), and
seven are partly closed: 6, 9, 24 and 32, plus 5, 22 and 26 because their
residuals are counted below. Every residual appears once in R2-6.

## R2-3. Card c6: native and web coverage

**Verdict: met, with defects.**

- **Coverage.** Every row the owner listed has a row and a checked example:
  - native (5.1): windows, menus, tray, menu bar; notifications and push;
    background work; files and pickers; camera, sensors, location; deep and
    app links; clipboard, drag and drop, share; permissions; accessibility;
    localization; dark mode; lifecycle; install, update and signing;
  - web (5.2): SEO, streaming, assets, uploads, caching and PWA install.
- **Tables.** The coverage tables map each need to its example and to each
  target.
- **Cards.** Every gap names a card. But #6799–#6817 each have zero
  criteria, so "named with its card" names cards that cannot be proven
  yet. Main should give each card executable-example criteria from its
  5.1 or 5.2 example.

**Defects.**

1. **The web form of files cannot work as typed.** `native.open_file`
   returns `String?` and `drop_files` passes `[String]`, both paths (3849,
   3851), and `import_from` reads them with `files.read` (2003). A browser
   gives no paths and has no `core.files`. The claim that "on the web the
   same call opens the browser's file chooser" (1995–1996) fails. The API
   needs a file handle type that a browser `File`, a phone content URI and
   a desktop path all satisfy, as the File System Access API and Tauri's
   dialog plugin do.
2. **Push can name a screen the target does not have.** `app.notify(to:
   who, ..., open: V)` reaches every device the account registered,
   including Web Push (1916–1921). But the phone graph alone adds
   `/mine` (1359). A push that opens `MineView{}` lands on a web graph
   with no such route. This also fails CL3's own round-two test (R2-4).
3. **The c6 examples are not wired into Crewboard.**
   - The phone manifest gives reasons for notifications, the camera and
     location (270–274), but no Crewboard code sends the "assigned to you"
     push: `tell_assignee` (1919) is never called.
   - `Task` has no attachment or place field, and no Crewboard screen or
     method uses `photo_for_task` or `where_done`.

   As standalone snippets they cannot find the gaps that wiring them in
   would find: send after commit, attachment storage, and upload
   authorization.
4. **Server-side scheduled work is missing.** The "due today" reminder
   runs as device background work, which the proposal calls "a floor, not
   a promise" (1944–1948). The reliable form is a server job that sends a
   push. The proposal has no server job, queue or cron surface. The
   strongest systems:
   - Oban (https://hexdocs.pm/oban);
   - Solid Queue and Active Job in Rails;
   - Laravel queues and its scheduler;
   - Convex scheduled functions and crons
     (https://docs.convex.dev/scheduling/scheduled-functions).

   This is a need worked around in app code instead of named.
5. **Background work on the web.** 5.1 says "no" (1851), but #6801's title
   promises "the web (background sync)". One of them must change.
6. **Desktop single instance.** A second launch through
   `crewboard://tasks/42` (2075) must reach the running app. Electron's
   `requestSingleInstanceLock` and Tauri's single-instance plugin are the
   references. The proposal does not state it.

## R2-4. Card c8: the claims register, claim by claim

The proposal may call a claim "goes further" (or "best") only when it
beats the strongest system in its area. Each verdict below names that
system.

| ID | Claim | Strongest existing, at its best | Verdict |
|---|---|---|---|
| CL1 | No method is reachable from a client unless marked | **Convex internal functions:** `internalQuery`, `internalMutation` and `internalAction` exist only under `internal`, and client hooks accept only public references, so the TypeScript build refuses the call (https://docs.convex.dev/functions/internal-functions). tRPC exposes only router procedures. | **not carried.** At best it matches Convex. The cited evidence is false: the CL1 probe calls the non-endpoints `webhook_secret` and `on_issue_event` from a screen, and seed-b14 checks it clean. Only `priv` methods fail, through ordinary privacy. TR2 removes the boundary the claim is about. The proposal's own O2 table (2871) lists Convex under option A. |
| CL2 | Every decoder runs the type's `validate` block | **ASP.NET Core `[ApiController]`** runs model validation on every action and answers 400 automatically (https://learn.microsoft.com/aspnet/core/web-api/#automatic-http-400-responses). **FastAPI with Pydantic** validates every request body on decode (https://fastapi.tiangolo.com/tutorial/body/). | **not carried; matches.** The 8.1 validation row names Blazor but omits these two. Rules that need the database (the assignee is a project member, a title is unique) cannot live in a pure `validate` block. Ecto's `unsafe_validate_unique` and constraint-to-error mapping, and Laravel's `exists` and `unique` rules, cover them. `update_task` accepts any `AccountID` as the assignee (643–656). |
| CL3 | One route identity serves web addresses, deep links and push | **Navigation Compose type-safe routes:** `@Serializable` route classes are the destinations, and `navDeepLink<Route>` maps URLs onto them (https://developer.android.com/guide/navigation/design/type-safety). **Expo Router:** every screen is a URL, deep and universal links land on the same routes, and typed routes are available (https://docs.expo.dev/router/introduction/). | **not carried.** It matches Compose's design, and the proposal's own test breaks it: graphs differ per target (the phone adds `MineView`), so a push that opens `MineView` is reachable by push but not by link on the web. Fixing this needs per-target route checks or one route set for all targets. |
| CL4 | Prediction runs the server's own method body | **Meteor methods:** the same method body runs as a client stub for latency compensation (https://guide.meteor.com/methods.html). **Zero custom mutators and Replicache mutators:** one shared mutator runs on client and server (https://zero.rocicorp.dev/docs/custom-mutators). | **not carried.** Meteor did this first, and 8.2 itself says "matches" for Meteor (3200). Section 4.6 (1624–1628) says the client cannot run `role_in` or the version check, and `move_task`'s body needs both (660–673). So the prediction is a subset of the body, not the body. Name the subset rule. |
| CL5 | A secret value cannot reach any client path | SvelteKit refuses to import `$env/static/private` into client code at build time; Next.js `server-only` fails the build on a client import; the Rust `secrecy` crate gives a secret type with no serialization by default. All three work at module or type level, not by value flow. | **not carried today.** If D-SECRETEDGE1 (#6557) is built, it would go further than all three. But seed-b14 accepts every CL5 probe flow, including `print("key {p.webhook_secret}")`. `#Secret` at dev is the fixed-timing tag; nothing yet denies a client path, a reply or a log. Say "proposed in #6557", not "E0721". |
| CL6 | A webhook cannot be mounted without a signature check | Stripe's SDK `constructEvent` verifies the signature with a timestamp tolerance against replay; Laravel Cashier verifies by middleware by default. | **holds, narrowed.** The true statement is "a method marked `#App.Webhook` cannot skip the check". A third-party POST can still land on an `#App.Public` method with no signature. GitHub deliveries carry no timestamp, so add `X-GitHub-Delivery` de-duplication (https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks). With both, CL6 can stand as "goes further" over frameworks where the check is a recipe. |
| CL7 | Network failures are typed cases the screen must handle | **Elm** `Http.Error` is a value the update function must match (https://package.elm-lang.org/packages/elm/http/latest/Http#Error). Leptos server functions return `Result<T, ServerFnError>`. | **not carried.** 8.2 (3208) already says "matches on typed errors (CL7)", which contradicts 8.1 (3152). The round-two test finds a failure that is not handled: the move button's `_: crew.move_task(...)` (981) has no stated error destination (gap 6), and no checked file exercises `CallError`, because TR2 erases `Remote`. |
| CL8 | Removing a wire field follows a compiler-checked policy | **buf breaking** refuses wire-incompatible Protobuf changes (https://buf.build/docs/breaking/). **Apollo GraphOS schema checks** test a change against the operations real clients sent (https://www.apollographql.com/docs/graphos/platform/schema-management/checks). | **not carried; matches.** `min_client` plus an expiry window plus a typed `UpdateRequired` is a sound policy. Apollo's usage-based check knows which clients still read a field, and the proposal's policy does not. |
| CL9 | Tokens need dark values; icon buttons need labels; images need text alternatives | **Jetpack Compose** `Image` and `Icon` take a required `contentDescription` (null must be written out for decorative images). **vanilla-extract `createThemeContract`** makes a theme that misses a token a type error (https://vanilla-extract.style/documentation/api/create-theme-contract/). | **not carried; matches.** The 8.1 accessibility row (3170) names SwiftUI, React Native and Flutter, and omits Compose, the strongest case. |

**The matrix (8.1, 8.2) has these further defects.**

- 8.2 says Qwik is "behind: Jet hydrates" (3194). That contradicts D-RESUME1 and 4.13 ("Pages resume instead of hydrating").
- 8.1 has no performance row. 4.13 is "behind", and it belongs in the matrix.
- These areas have no row at all: authorization policy, server jobs, email, rate limiting and observability. The probe needs each one (R2-6 items 4–10).

**Workarounds in Crewboard that are not named as gaps.** Card c8 says every
need the framework cannot meet cleanly is a gap with a card, not app code.

1. **Authorization is hand-written in every method.** `role_in` and `can_edit`
   are repeated in each method (583, 596, 625, 645, 662), and `mine` leaks:
   it returns every task assigned to the caller, including tasks in
   projects the caller has left (617–621).
   - **Strongest:** row-level policies, as in Supabase or Postgres RLS
     (https://supabase.com/docs/guides/database/postgres/row-level-security),
     Firestore rules, and Laravel policies.
   - **Already in the framework:** #3709 has the criterion "row policies
     are typed, compiled, and enforced on every live query/mutation path"
     (D-DBPOLICY1). Crewboard does not use it, and the proposal does not
     mention it.
2. **The signed-in caller is optional inside `#App.SignedIn`.**
   `app.caller()` returns `AccountID?` (3429), so every method unwraps it
   through `me()` (710–712), even though the marker already guarantees a
   session. tRPC's protected procedures and Phoenix's `on_mount` hand
   over a non-optional user.
3. **Concurrency control is hand-written and not atomic.**
   - `update_task` reads, compares `version`, then writes (643–657). Two
     concurrent edits can both pass the check, and one is lost.
   - `move_task` has no version check at all, so a queued move replays
     over a newer change.
   - **Strongest:** Rails `lock_version` and EF Core concurrency tokens
     compare and set in one statement
     (https://learn.microsoft.com/ef/core/saving/concurrency).
4. **No transactions.** These writes are not atomic:
   - `close_from_issue` writes N rows in a loop (687–698);
   - the CSV import makes one remote call per row (2005–2007), and a
     failure leaves a partial import.

   At dev, `#Transact` covers `Shared<T>` memory only (spec "Inside
   `#Transact`"). The proposal has no database transaction, and no batch
   call across the remote boundary.
5. **Joins and N+1 queries.** `my_projects` runs one `card()` per membership,
   and `card()` makes two queries of its own (571–579, 722–726). `search`
   then repeats `my_projects` inside a loop (606–614). Ecto `preload`,
   Rails `includes` and Prisma `include` are the references.
6. **Search is an in-memory scan.** `matching` filters each project's rows in
   app code (521–537, 609). No predicate-to-SQL rule can compile it, and
   no full-text index exists. Postgres full-text search and Laravel Scout
   are the references.
7. **Offline create needs a server ID.** `new_task` navigates to
   `TaskView{id: task.id}` after `create_task` (1063–1064), but under
   `.offline(.Queue)` the ID does not exist until the queue drains. Client-
   generated IDs, as in Replicache, Zero and Linear, are the references.
8. **Notifications are not sent after commit.** `tell_assignee` (1919) is
   not called. Calling it inside a method would send before the commit and
   could notify about a write that rolls back. Rails `after_commit` and
   jobs enqueued in the same transaction (Oban, Laravel
   `afterCommit`) are the references.
9. **The probe skips the flows that find account gaps.** "Ask an owner to
   invite you" (893) has no invite method, and there is no project
   creation, password reset, email verification, rate limit on sign-in,
   MFA or passkeys. The Rails 8 authentication generator (with
   `rate_limit`), Laravel Fortify, `phx.gen.auth` and ASP.NET Core Identity
   ship these. 4.7 claims that sessions match Rails, but the account
   lifecycle has no row and no card.
10. **The API is shaped around seed limits, and no card is named.**
    3.9 (1453–1459) states the rule:
    - free functions instead of bound methods;
    - an explicit `ui.style<CrewTheme>(...)`, because a closure's annotated
      parameter does not drive inference;
    - handlers typed `fn() E!` instead of `fn() -> R E!`.

    Amendment 2 to D-LIVEQUERY1 (3095–3098) is the same workaround, and so
    is the three-line closure around every S1 A style (6.1). The seed
    findings sit under #6819, but #6819's three criteria do not include
    them. Bound method values, closure-parameter inference, and
    `UninferableTypeArgument` for an `R` that returns nothing each need a
    criterion, or a language card and ballot. The public API must not
    freeze around them.

## R2-5. Card c7: styling

**Verdict: partly met.**

**What is met.**

- Every c7 row has a row in 6.3.
- S1 A, B and C are each shown, with HTML+CSS, Tailwind, SwiftUI and
  Flutter beside them.
- The guarantees in 6.4 are limited honestly, and 6.5 replaces "orders of
  magnitude" with measures.
- Six of the eight seeded bugs are caught today, as stated.

**What is not met.**

1. **One card, not "the same Crewboard screens".** Only the board card is
   styled in every form (6.2). The parts where CSS is strongest are never
   compared side by side: the board's adaptive layout (three columns or a
   picker, `ui.by_width`), dark mode as a whole theme, and motion.
   At least the board screen, with its compact and regular layouts, must
   appear in each form.
2. **"No dead rules" is missing.** c7 lists it among the guarantees, and
   6.4 does not mention it. S1 A can state it: an unused style function or
   token is a lint. S1 C cannot until #6817 checks class use both ways.
3. **The owner's floor, "at least as easy as HTML and CSS", is not shown.**
   The proposal's own count (2663–2678) puts S1 A at 24 + 14 lines for the
   card, against 15 for HTML and CSS and 7 for Tailwind. Part of the gap is
   the `ui.style<CrewTheme>(fn(t: CrewTheme) { ui.Style{...} })` wrapper,
   which exists because of the inference gap in R2-4 workaround 10. Show
   the form S1 A takes once that gap is closed, and count again.
4. **CL9's "goes further" for dark values fails against vanilla-extract
   theme contracts** (R2-4). Revise the verdict at 2686 and 3176 to
   "matches".
5. **Theme binding is unchecked.** A style asks for `ui.style<CrewTheme>`,
   but the graph's `.theme<T>` is unconstrained (stub at 3402). Nothing
   says what happens when the graph's theme is not `CrewTheme`, or inside
   `ui.with_theme` with another token type. State that it is a compile
   error, and how sema knows.
6. **Missing accessibility rows:** increased contrast and forced colors
   (CSS `prefers-contrast` and `forced-colors`, SwiftUI
   `colorSchemeContrast`, Windows high contrast), and a keyboard or
   single-pointer alternative to dragging (WCAG 2.2 SC 2.5.7, for
   `drag.jet`).
7. **Missing layout rows:** overlays (dialogs, popovers, sheets, toasts and
   their stacking), scroll containers and sticky headers, and view
   identity. `ui.local` (961) and `kit.list` rows have no stated identity
   or key, so moving or filtering cards can move local state to the wrong
   row. SwiftUI's structural identity and Compose's `key` both define
   this.
8. **Fairness of the side-by-side.** The SwiftUI version (2600–2625) has
   hover and a shadow, but leaves out the focus ring that the HTML version
   and the S1 forms have. Add it, and count again.

## R2-6. Gaps for G12Docs-5, in one pass

Each item must be engineered away in the proposal, or named with its cost,
mitigation and card. *(card)* means Main files a card or criterion.

**Claims and evidence**

1. Card the language gaps behind the API rule in 3.9 and amendment 2 of
   D-LIVEQUERY1: bound method values, closure-parameter inference, and the
   `R`-returns-nothing inference. Do not freeze public API around them.
   *(card)*
2. Fix CL1's evidence. Either make the stub prove exposure (a `Remote<S>`
   type that offers only marked methods), or say the claim is unproven.
   Change the verdict to "matches Convex".
3. Rewrite every statement that a `#Secret` flow is E0721 today (186,
   316–318, 1655–1657) as "proposed in #6557 (D-SECRETEDGE1)". Cite the
   probe result.
4. Change CL2, CL3, CL4, CL7, CL8 and CL9 to "matches", naming the system
   from R2-4 in each case, unless a narrower claim survives. Narrow CL6 to
   "a method marked as a webhook", and add redelivery de-duplication.
5. Make CL3 survive its own test. Either check per target that every
   state a shared screen links to, or a push opens, has a route on every
   target that can receive it, or give all targets one route set.
6. Fix 8.2's Qwik row, and the CL7 contradiction between 8.1 and 8.2. Add
   a performance row to 8.1.

**Crewboard as a probe** (each item is a need the app works around today)

7. Use D-DBPOLICY1 row policies for project membership instead of
   hand-written `role_in` checks, or name why not. Fix the `mine` leak.
   *(card if the policy surface is not in #3709)*
8. Inside `#App.SignedIn`, give the method a non-optional caller.
9. Make writes atomic and concurrency-checked: a compare-and-set update
   for `version`, a version on `move_task` (or say the move is last
   writer wins), a database transaction for `close_from_issue`, and a batch
   or transaction for the CSV import. *(card)*
10. Give relation loading and a search index a surface or a card. Rewrite
    `my_projects` and `search` without N+1 queries. *(card)*
11. Client-generated IDs for creates under `.offline(.Queue)`, or state
    that a create waits online. *(criterion on #3709)*
12. Add after-commit side effects (an outbox) and server jobs, queues and
    cron. Move the "due today" reminder to a server job that pushes, and
    send `tell_assignee` after commit from `update_task`. *(card)*
13. Add the account lifecycle to Crewboard and the matrix: project
    creation, invitations, email delivery, password reset, verification,
    a sign-in rate limit, MFA and passkeys. *(card)*
14. Give button and menu handlers a stated error destination, and stop
    writing `_:` on calls whose failure the person must see (981, 1897,
    2131, 2324). Resolve 3.7 steps 1 and 2: what a queued write's button
    shows.
15. Wire the c6 examples into Crewboard: an attachment on `Task` through
    the upload service, and the assignee push. Name what that finds.

**Security bugs in new examples**

16. `on_issue_event`: de-duplicate redeliveries by `X-GitHub-Delivery`. A
    signature with no timestamp does not stop replay.
17. `uploads.jet` (2313–2326) has three bugs:
    - **Path traversal.** It saves to `"{folder}/{task}-{file.name}"`
      with the client's file name (OWASP path traversal).
    - **No authorization.** Any signed-in caller can attach to any task.
    - **Unchecked type.** The type filter `accept:` is client-side only,
      and `kind` is what the client declares.

    Give the framework a storage surface that names files itself and
    checks content, as Active Storage does, and show the role check.
18. CSV export (1301–1311): escape cells that start with `=`, `+`, `-` or
    `@` in `core.encoding.csv`, or state the rule (OWASP CSV injection).
19. Presence (1736–1741, stub 3432): who may subscribe to
    `app.presence(TaskView{id})`? Today anyone who can build the value can
    see who views a task they cannot read. Require the screen's own access
    rule. Also name the `#App.SigningKey` lookup explicitly in the
    webhook marker.
20. Validate that `assignee` is a member of the project (643–656). This
    needs a server-side, database-aware validation surface that maps to
    `FieldError`. *(card)*

**Coverage and styling**

21. Files: give pickers and drops a file handle type that works in a
    browser, not `String` paths (3849–3851, 2003). *(criterion on #6802)*
22. Background work on the web: make 5.1 and #6801 agree. Add
    single-instance handling to desktop links. *(criterion on #6804)*
23. Fix the `realtime.jet` comment on the web queue (1743–1744).
24. Reconcile 4.9's sharing rule with #3709's criterion. State that
    caller-dependent queries such as Crewboard's do not share, and give
    their cost.
25. Styling:
    - style the board screen, with both layouts, in every form;
    - add the "no dead rules" guarantee;
    - add the focus ring to the SwiftUI form, and recount;
    - show S1 A without the inference wrapper;
    - state that the theme-type check is a compile error;
    - add rows for contrast and forced colors, the dragging alternative,
      overlays, scrolling and view identity. *(criteria on #6817)*
26. Main: give #6799–#6817 executable-example criteria taken from their 5.1
    and 5.2 examples, so that each named gap can be proven. *(cards)*
27. Report the seed warning `jet.RedundantFinalReturn` (L0530) on
    `model.jet:43`. It fires on a `validate {` block that has no `return`,
    so it is a false positive. Add it to A.6 under #6819.
28. Follow the ratified syntax, or say why not:
    - D-BRACE-LAYOUT1=A puts commas between data members, and no struct
      body in the proposal has them;
    - D-RECORD-BRACE1=A retires the paren record that keys `members`
      (568, 735).

    The seed accepts commas between fields today. Name the record key as
    waiting on #6251 c12.

## R2-7. Verdicts

| Claim or criterion | Round 2 verdict |
|---|---|
| C1 logic once | **holds.** The graph is client-safe, and target screens call only shared methods |
| C2 seamless | **holds within limits.** The route-table binding and per-target routes are not checked (items 5, 14) |
| C3 safety | **fails.** The `#Secret` client rule is not built and is described as if it were. The uploads and presence examples have authorization bugs, and the webhook has no replay rule (items 3, 16, 17, 19) |
| C4 performance | **holds within limits.** Nothing is measured. Caller-dependent live queries do not share (item 24) |
| C5 completeness (c6) | **met, with defects** (R2-3; items 12, 13, 21, 22) |
| C6 styling (c7) | **partly met** (R2-5; item 25) |
| c8 probe and matrix | **fails.** No claim in CL1–CL9 carries "goes further" as stated, and ten workarounds are not named as gaps (R2-4) |
| CL1 | not carried: matches Convex; the evidence is false |
| CL2 | not carried: matches ASP.NET Core and FastAPI |
| CL3 | not carried: matches Navigation Compose; fails its own test |
| CL4 | not carried: matches Meteor and Zero |
| CL5 | not carried today: not built at dev; goes further only once #6557 is built |
| CL6 | holds, narrowed to marked webhooks, with replay de-duplication added |
| CL7 | not carried: matches Elm |
| CL8 | not carried: matches buf and Apollo checks |
| CL9 | not carried: matches Compose and vanilla-extract |

Once G12Docs-5 closes items 1–28, a third round needs to check only
those items, the claim verdicts, and a re-run of the kit.
