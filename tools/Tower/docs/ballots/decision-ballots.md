# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use the
exact bold labels):** every full decision card carries `**Gist:**` (one VERY short
plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually differ —
subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark the
recommended one `(recommended)`). Close with `**Recommendation:**` + a one-line why.
Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a separate Q&A facet,
so keep them out of the recommendation. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card when
it's time to decide it.

---

## Open decisions

_Five decisions are open for owner pick (sweep 2026-06-25). Each is a full Focus-Mode card below; every other non-frozen board card now has a vetted plan and is ready to implement on your "go"._

## Plugin target — board card c81

### D-PLUGIN1 — Plugin target model & ABI substrate (rec B)

**Gist:** Decide what a package that declares the `plugin` target actually builds, and how a running Jet program loads and calls it safely.

**Story.** Doris ships a desktop note-taking app in Jet. Power users keep asking to write their own little add-ons — a word-counter, a spell-checker, an export-to-PDF button — and drop them in a `plugins/` folder without rebuilding her whole app. She wants to declare those add-ons as Jet packages with `target: plugin`, hand them to her app at runtime, and call them. The reserved `plugin` keyword promises exactly this, but today it is rejected with E1210. The question this card settles: when Doris's app loads a stranger's plugin, is that load sandboxed and safe by default, or does it run as raw native code in her process?

**In the wild:**
```jet
// --- host app: note_app/pack.jet ---
package {
    name: "note_app"
    target: app
}

// --- host app: src/main.jet ---
fn main() {
    let bar = PluginHost.discover("./plugins")   // load every plugin in the folder
    for p in bar.plugins {
        print("loaded: {p.name}")
        p.on_export(current_note())              // call across the ABI boundary
    }
}

// --- a third-party plugin: pdf_export/pack.jet ---
package {
    name: "pdf_export"
    target: plugin          // <-- E1210 today; this card unblocks it
    entry: "src/plugin.jet"
}

// --- pdf_export/src/plugin.jet ---
pub fn name() -> Text { "Export to PDF" }
pub fn on_export(note: Note) { write_pdf(note) }
```

**Other languages:**
```rust
// Rust: native cdylib + dlopen. No language-level safety — the whole
// boundary is `unsafe`, and an ABI mismatch is undefined behavior.
let lib = unsafe { libloading::Library::new("plugins/pdf_export.so")? };
let on_export: libloading::Symbol<unsafe extern "C" fn(*const Note)> =
    unsafe { lib.get(b"on_export")? };
unsafe { on_export(&note) };   // trusting a stranger's .so inside your process
```
```wat
;; WASM component model: the plugin is a sandboxed module with a typed
;; interface. The host grants capabilities explicitly; a misbehaving
;; plugin can't touch host memory, the filesystem, or the network.
(component
  (import "host:note/types" (instance ...))
  (export "on-export" (func (param "note" $note))))
```

**Tradeoffs:** (subagent-reviewed)

| Option | Safety (I1) | New dep (I6) | Perf | Use-case fit |
|---|---|---|---|---|
| A — Native cdylib + `@unsafe` gate | Every load is expert-gated unsafe; footgun if anyone forgets | None (new rustc `--crate-type` path only) | Fastest; direct call | Trusted first-party plugins only; hostile-plugin story is "don't" |
| B — WASM sandbox (recommended) | Safe by construction; no gate needed; capability-scoped | New WASM runtime in stdlib — owner-approval gate | Marshaling cost per call; fine for coarse calls | General app plugins from untrusted authors; the beginner default |
| C — Out-of-process RPC | Process isolation; safe | New RPC/IPC layer | Highest latency; serialize every call | Heavyweight isolation; no shared typed state |

- **Option A — Native cdylib + `@unsafe` gate.** The `plugin` target compiles to a native shared library (`cdylib` via rustc, the first non-`bin` crate type in the codebase) with a C ABI. The host loads it with `dlopen`-style machinery. Crossing that boundary is inherently unsafe — a wrong signature or a stale ABI is undefined behavior — so under I1 the load site must sit behind a user-written `@unsafe`/`@audit` gate. This makes loading *any* plugin an expert-tier act: Doris cannot offer a safe plugin folder to ordinary users without auditing every load. No new dependency, fastest calls, but the default footgun is exactly what I1 forbids.
```jet
fn main() {
    @unsafe {
        @audit("native plugin load: trusting .so authors with full process access")
        let bar = PluginHost.discover("./plugins")   // native dlopen, no sandbox
        bar.plugins[0].on_export(current_note())
    }
}
// Without the @unsafe gate:
//   error[E12xx]: loading a native plugin is an unsafe operation
//     this crosses a native ABI boundary; a mismatched plugin is undefined behavior
//     fix: wrap the load in `@unsafe { @audit("…") … }` (expert tier)
```
- **Option B — WASM sandbox (recommended).** The `plugin` target compiles to a sandboxed WASM module. The host loads and calls it through a typed interface; the plugin runs in an isolated sandbox and can only touch what the host explicitly grants. This is I1-clean by construction — no `@unsafe`, no audit, no footgun — so Doris's plugin folder is safe for strangers' code out of the box, which is the whole beginner promise. The honest cost (I6): this introduces a WASM runtime as a new stdlib external dependency, which requires owner approval and, per the Epoch-3 dependency rule, must eventually be a native Jet/Rust implementation. Call-boundary marshaling adds latency versus a raw native call.
```jet
// pdf_export/pack.jet
package {
    name: "pdf_export"
    target: plugin          // compiles to a sandboxed wasm32 module
    entry: "src/plugin.jet"
}

// host — no @unsafe needed; the sandbox is the safety boundary
fn main() {
    let bar = PluginHost.discover("./plugins")   // safe by default
    for p in bar.plugins {
        p.on_export(current_note())              // marshaled across the sandbox
    }
}
```
- **Option C — Out-of-process RPC.** The `plugin` target builds a standalone executable the host launches as a child process and talks to over a JSON-RPC-style protocol (LSP-shaped). Strongest isolation — a crashing or hostile plugin cannot corrupt the host — and language-agnostic. But every call is serialized across a process boundary (highest latency), it carries process-management complexity, and there is no shared typed state, which rules it out for any future compiler-hook plugin that needs the typed AST.
```jet
fn main() {
    let bar = PluginHost.spawn("./plugins")      // each plugin = a child process
    for p in bar.plugins {
        p.on_export(current_note())              // RPC round-trip per call
    }
}
```

**Recommendation:** B — a sandboxed WASM substrate is the only option where loading an untrusted plugin is safe by default with no `@unsafe` gate, which is precisely what I1 and the beginner experience demand; it is one clear path for all app plugins, and the sandbox stays correct as the surface grows. The new WASM runtime dependency (I6) is a real, owner-gated cost named here honestly, not a ranking factor. Native cdylib (A) is the natural *future expert opt-in* layered on top of B — deferred to its own card, not committed now — so the safe default ships first and the expert reach comes later without making the footgun the default.

**Owner Q1 — deferred sub-decisions.** Versioning / ABI handshake (reuse the D-CAP4 `api: stable` freeze?) and export-surface spelling (`#Plugin` marker vs manifest `entry:` + `pub` contract) are decided in follow-up cards once this substrate choice lands.

## User derives & reflection — board card c155

### D-METAREFLECT1 — Reflection read-API surface (rec B)

**Gist:** How build-time code asks a type for its fields, their types, and their markers.

**Story.** Doris maintains a small in-house table-store library. Every team that uses it writes a `struct` for a row, then by hand types out the column names, the column types, and which fields to skip — and the hand-list drifts out of sync with the struct the moment someone adds a field. She wants the library to read the struct's shape itself at build time and generate the column list, so the struct stays the single source of truth. To do that, her derive body needs a way to walk a type: ordered fields, each field's type, and the `@skip`-style markers on each one.

**In the wild:**
```jet
// table-store: build the column list from the row struct itself, at build time.
@Row
struct Account {
    id: Int
    email: Text
    @skip cached_token: Text   // not a real column
}

// inside the library's derive body — read Account's shape:
comptime {
    let cols = T.reflect().fields
        .where(|f| !f.has_marker("skip"))
        .map(|f| (f.name, sql_type(f.ty)))
    // cols == [("id","INTEGER"), ("email","TEXT")]
}
```

**Other languages:**
```swift
// Swift: Mirror is a reflected handle over a value/type — one entry point, walk children.
let m = Mirror(reflecting: account)
for child in m.children { print(child.label, type(of: child.value)) }
```
```zig
// Zig: @typeInfo returns a comptime value you index into.
inline for (@typeInfo(Account).Struct.fields) |f| {
    @compileLog(f.name, f.type);
}
```

**Tradeoffs:** (subagent-reviewed)

| Option | Entry points | LSP-discoverable | One-path (I8) fit | Iteration ergonomics | New surface |
|---|---|---|---|---|---|
| A free functions | many builtins | poor (loose bag) | weak (scattered) | manual | low |
| B reflected handle | one (`T.reflect()`) | strong | strong | good (`.fields` is a list) | one `Type` value |
| C `comptime for` | one keyword form | medium | medium | best for walk-only | a 2nd comptime control form |

- **Option A — free functions.** A set of comptime builtins: `fields_of(T)`, `type_name(T)`, `attrs_of(T, field)`. Familiar and cheap; composes with ordinary comptime bindings. But it grows into a loose bag of globals with no single place to discover what reflection can do, and each new query is another top-level name.
```jet
comptime {
    let names = fields_of(T)                       // ["id","email","cached_token"]
    for f in names {
        let t = field_type(T, f)                   // Int / Text / Text
        let skip = attrs_of(T, f).contains("skip")
        if !skip { emit_column(f, sql_type(t)) }
    }
}
```
- **Option B — reflected `Type` handle (recommended).** `T.reflect()` returns one reflected value; everything hangs off it (`.name`, `.fields`, each field's `.name`/`.ty`/`.markers`). One discoverable, LSP-completable entry point — fits the Blueprint north-star — and keeps reflection from scattering into globals. Costs one first-class comptime `Type` value. Matches Swift's `Mirror` and Zig's `@typeInfo`-as-value.
```jet
comptime {
    let info = T.reflect()
    for f in info.fields {                         // ordered, typed
        if f.has_marker("skip") { continue }
        emit_column(f.name, sql_type(f.ty))        // f.ty is a reflected type
    }
}
```
- **Option C — `comptime for field in T`.** A dedicated iteration construct that binds each field's shape per iteration. Reads most naturally for the dominant case (walk fields to build an impl). But it is a second comptime control form, and it is narrower than a general read-API — you still need some handle for "the type's name" or "this field's markers", so it tends to need B underneath anyway.
```jet
comptime for field in T {                          // field: name, ty, markers
    if field.has_marker("skip") { continue }
    emit_column(field.name, sql_type(field.ty))
}
```

**Recommendation:** B — one discoverable, LSP-completable handle keeps reflection coherent (Blueprint/I8) and mirrors Swift `Mirror` / Zig `@typeInfo`; adopt C later as pure sugar over B if field-walking dominates real derive bodies.

### D-METADERIVE1 — User derive authoring + output mechanism (rec A)

**Gist:** How a library author writes a `derive`, and whether its output is Jet text the compiler re-reads or a tree it hands the compiler directly.

**Story.** Hank ships a small networking library. He wants users to tag a `struct` with `@Wire` and get a hand-free binary encoder, the same way the built-in `#[Codable]` works — but `Wire` is his trait, not the compiler's, so he has to *author* the derive. The hard question isn't the keyword; it's the output mechanism. When a user puts `@Wire` on a struct with a field his encoder can't handle, the error has to land on that user's `@Wire` line in plain Jet — not as a wall of rustc output about generated code. That only holds if the derive's output goes back through the front end the same way hand-written code does (D-CTCODEGEN1=A: never inject pre-parsed AST; errors pin at the trigger).

**In the wild:**
```jet
// library author defines the derive once:
derive Wire for T {                                  // T = the triggering type's reflected handle
    let puts = T.reflect().fields
        .map(|f| `    self.$f.name.write_wire(buf)`)  // one line per field; $-splice computed names
    emit `
        impl Wire for $T.name {
            fn write_wire(self, buf: Buffer) { $puts }
        }
    `                                                  // emitted Jet TEXT → lexer→parser→sema
}

// a consumer, in another file, just tags the type:
@Wire
struct Packet { seq: Int, payload: Bytes }

let buf = Buffer.new()
Packet{seq: 1, payload: b"hi"}.write_wire(buf)        // works: both fields are Wire
```

**Other languages:**
```rust
// Rust: a derive macro returns a TokenStream — source-level tokens the compiler
// then PARSES and type-checks. Errors point into the generated code, at the call site.
#[proc_macro_derive(Wire)]
fn derive_wire(input: TokenStream) -> TokenStream { /* quote!{ impl Wire … } */ }
// Swift macros are the same shape: emit SwiftSyntax source, recompiled & rechecked.
```

**Tradeoffs:** (subagent-reviewed)

| Option | Declaration | Output mechanism | Errors pin at `@Marker` site | Honors D-CTCODEGEN1=A | Trigger / coherence |
|---|---|---|---|---|---|
| A `derive T for …` | impl-like block | Jet **source fragment** re-enters lexer→parser→sema | yes (parsed like hand code) | yes | `@Marker` router · local-only |
| B `derive fn(T: Type)` | comptime fn | returns **typed AST** sema re-validates | only with hand-threaded spans | **no — reopens it** | `@Marker` router · local-only |
| C `@derive(T) impl …` | attribute + skeleton | Jet source fragment (same as A) | yes | yes | attribute · local-only |

- **Option A — `derive Trait for T { … }`, source-fragment re-entry (recommended).** Reads like an impl block; the body uses reflection (D-METAREFLECT1) and `$name` splices (D-CTMARKER1=C) to build Jet **text**, which re-enters the front end exactly like hand-written code. Triggered by the existing `@Marker` router (`split_type_markers` already routes unknown names to derives — zero new trigger syntax), local-only coherence (derive only where the trait or the type is defined). Errors pin at the trigger for free, because sema sees ordinary parsed code carrying the trigger span. Matches Rust/Swift macros.
```jet
derive Wire for T {
    let puts = T.reflect().fields.map(|f| `    self.$f.name.write_wire(buf)`)
    emit `impl Wire for $T.name { fn write_wire(self, buf: Buffer) { $puts } }`
}

@Wire
struct Frame { seq: Int, when: Instant }    // Instant is not Wire

// error pins at the user's trigger, in plain Jet (rustc never speaks — I2):
//   error[E-DERIVE-FRAGMENT]: derived `Wire` impl does not type-check
//     --> frame.jet:1:1
//      |
//    1 | @Wire
//      | ^^^^^ this derive generated code that failed sema
//      = field `when: Instant` has no `write_wire` — `Wire` needs every field to be Wire
```
- **Option B — `derive fn Trait(T: Type) { … }`, typed-AST return.** Declares the derive as an explicit comptime function that builds and returns AST nodes; sema then re-validates the tree. Reads clearly as "a function the compiler calls." But the compiler receives **pre-built AST**, so spans don't map to any user text — pinning an error at the trigger means threading synthetic spans by hand, and D-CTCODEGEN1=A already ratified "never inject pre-parsed AST past the sema gatekeeper." Picking B reopens that decision.
```jet
derive fn Wire(T: Type) -> Impl {
    let body = T.reflect().fields
        .map(|f| Stmt.call(field(self_(), f.name), "write_wire", [ident("buf")]))
    Impl.new(trait: "Wire", ty: T, method: Method("write_wire", body))
}   // returns a tree, not text → conflicts with the ratified re-entry rule
```
- **Option C — `@derive(Trait) impl … { … }`, attribute + source fragment.** Closest to today's `#[…]` marker world: an attribute on an `impl` skeleton whose body emits a Jet source fragment (same sound mechanism as A). Familiar to anyone coming from attribute systems, but it splits the declaration across an attribute and a half-written `impl`, and the `impl … for T` header duplicates what the attribute already says — more ceremony than A for the same result.
```jet
@derive(Wire)
impl Wire for T {
    fn write_wire(self, buf: Buffer) {
        emit T.reflect().fields.map(|f| `self.$f.name.write_wire(buf)`)
    }
}
```

**Recommendation:** A — declaration `derive Trait for T` reads as an impl, output is a Jet **source fragment** re-entering lexer→parser→sema (the literal D-CTCODEGEN1=A path, so errors pin at the `@Marker` trigger and rustc never speaks); reuse the existing `@Marker` router and a Rust-style local-only orphan rule.

## Monorepo workspace — board card c156

### D-WORKSPACE2 — Workspace surface keyword + filename (rec B)

**Gist:** Pick the keyword and filename for the Jet-grammar file that indexes every package in a multi-package repo.

**Story.** Earl runs a 40-package repo: a ranker service, a logging crate, shared protobufs, a half-dozen CLIs. He deletes the old `jetpack.toml` index and writes one Jet file at the repo root that lists the members. He wants its first line to read like what it is — "this is the set of packages in this repo" — and to sit next to `env.jet`/`config.jet` without a beat of confusion about which file does what.

**In the wild:**
```jet
// fleet.jet
module fleet {
    members: find("./packages")
}
```

**Tradeoffs:** (subagent-reviewed)

| Option | Reads as | Collision risk | Theme fit |
|---|---|---|---|
| A `workspace` | industry-standard, generic, long | none | off-theme |
| B `fleet` | "all the packages we operate" | none | strong |
| C `roster` | "the enrolled list of members" | none | mild (crew) |
| D `wing` | "a formation of packages" | none | strong |
| E `squadron` | "a unit of grouped packages" | none | strong, long |
| F `hangar` | "where the packages are kept" | **store name** | strong |
| G `manifest` | "the cargo list" | **overloaded vs pkg.jet** | mild |

- **Option A — `module workspace` / `workspace.jet`.** Familiar from Cargo/npm workspaces, so a transplant guesses it instantly. But it's the longest, most generic spelling, off the jet/jetpack/jetos aviation canon, and carries baggage from other ecosystems' workspace semantics that Jet's surface doesn't share.
```jet
// workspace.jet
module workspace { members: find("./packages") }
```
- **Option B — `module fleet` / `fleet.jet` (recommended).** A fleet is the set of aircraft an operator runs — so "the fleet" reads cleanly as "all the packages in this repo." Short, on-canon with jet/jetpack/jetos, no collision with `pkg.jet`/`env.jet`/`config.jet`, and a beginner needs no prior workspace vocabulary to get it.
```jet
// fleet.jet
module fleet { members: find("./packages") }
```
- **Option C — `module roster` / `roster.jet`.** A roster is literally an enrolled list of members, which is exactly the field's job — the keyword and the `members:` field reinforce each other. Slightly more crew/personnel than aircraft, but no collision and very legible.
```jet
// roster.jet
module roster { members: find("./packages") }
```
- **Option D — `module wing` / `wing.jet`.** A wing is a formation of aircraft flying together — distinctive, short, on-theme. Reads a touch more poetic than literal ("the wing" is less obviously "the package list" than "the fleet"), and "wing" has an unrelated everyday meaning that may briefly mislead.
```jet
// wing.jet
module wing { members: find("./packages") }
```
- **Option E — `module squadron` / `squadron.jet`.** A squadron is a named operational unit of aircraft — strongly on-theme and unambiguous as "a managed group." Longer to type than `fleet`/`wing` and a less common English word for a beginner.
```jet
// squadron.jet
module squadron { members: find("./packages") }
```
- **Option F — `module hangar` / `hangar.jet`.** Evocative and on-theme, but **collides**: "hangar" already names the Jetpack package store (`Source/Jetpack/Store.rs`). Two unrelated meanings of one word in the same package system is exactly the footgun to avoid — reject.
```jet
// hangar.jet
module hangar { members: find("./packages") }
```
- **Option G — `module manifest` / `manifest.jet`.** "Manifest" = the cargo list, which fits, but it's **overloaded** against the per-package manifest concept (`pkg.jet`), so "the manifest" becomes ambiguous about which level it means. Avoid.
```jet
// manifest.jet
module manifest { members: find("./packages") }
```

**Recommendation:** B — `fleet` is short, on-canon, collision-free, and reads as "all the packages in this repo" with zero prior vocabulary; falls back cleanly to the generic A if the owner prefers the industry term.

## Dot-inferred construction — board card c158

### D-DOTCTOR2 — Does `T.{ }` retire the dotless `T { }` struct literal? (rec A)

**Gist:** Now that named construction is `T.{ … }`, decide whether the old dotless `T { … }` goes away or lives on beside it.

**Story.** Floyd is writing the config types for a small service. He names a struct two ways in the same file — once the way the tutorial showed him last year (`Config { … }`) and once the way the new docs show (`Config.{ … }`) — and both compile. He stalls: which is "right," and will a reviewer flag the other? Two spellings for the exact same act of building a struct is the thing the language is supposed to spare him.

**In the wild:**
```jet
let server = Server.{
  host: "0.0.0.0",
  port: 8080,
  tls: .Enabled(Cert.{ path: "/etc/ssl/site.pem" }),
  log: .Json,
}
```

**Other languages:**
```rust
// Rust: one struct-literal spelling, always dotless, no enum-dot pressure
let server = Server { host: "0.0.0.0".into(), port: 8080 };
```
```swift
// Swift: construction is a call, T(...); enum cases use a leading dot (.json)
// but struct init never does — so Swift feels no symmetry pull on structs
let server = Server(host: "0.0.0.0", port: 8080)
```

**Tradeoffs:** (subagent-reviewed)

| Option | One spelling (I8) | Enum-dot symmetry | Migration surface |
|---|---|---|---|
| A retire S29 | Yes — only `T.{ }` | Full — matches `.Variant` | Wide (all struct literals + S74 patterns) |
| B coexist | No — two spellings | Partial — dot optional | None |
| C inference-only dot | Yes for named (`T { }` only) | Broken — named struct dotless, named enum dotted | None |

- **Option A — Retire S29 fully (recommended).** `T.{ … }` becomes the one named-construction spelling; the dotless `T { … }` is removed and typing it teaches the fix-it. One spelling per job (I8), and named struct construction reads exactly like named enum construction — `T.{ … }` next to `T.Variant`, `.{ … }` next to `.Variant`. A beginner learns a single rule: a leading dot means "construct," named or inferred, struct or enum.
```jet
let server = Server.{ host: "0.0.0.0", port: 8080 }

// typing the old dotless form:
let server = Server { host: "0.0.0.0", port: 8080 }
//                  ^ E0320: named construction uses a dot: `Server.{ … }`
//                    fix: insert `.` before `{`  →  Server.{ host: …, port: … }
```
- **Option B — Coexist.** Keep dotless `T { … }` for named construction and also accept `T.{ … }`. Nothing migrates, but the shipped language then has two spellings for one job — a direct I8 violation. Floyd's two-spellings-in-one-file confusion is the permanent state, and every style guide has to legislate which to use. The card asked for symmetry with the enum dot; coexistence only makes the dot optional, so the symmetry is a suggestion rather than a rule.
```jet
let a = Server { host: "0.0.0.0", port: 8080 }   // legal
let b = Server.{ host: "0.0.0.0", port: 8080 }   // also legal — same thing, I8 tension
```
- **Option C — Inference-only dot.** The dot exists only for context-inferred construction (`.{ … }`, `.Variant`); named construction stays dotless `T { … }` and `T.{ … }` is never offered for structs. One spelling per job is preserved, but it breaks symmetry the other way: a named enum is `T.Variant` (dotted) while a named struct is `T { … }` (dotless), and an inferred struct is `.{ … }` (dotted) while its own named form drops the dot. The leading dot would mean "construct" for enums and "inferred only" for structs — two rules, not one.
```jet
let server = Server { host: "0.0.0.0", port: 8080 }   // named struct: no dot
let mode   = .Json                                     // inferred enum: dot
let cert: Cert = .{ path: "/etc/ssl/site.pem" }        // inferred struct: dot
// named enum is Server.Variant (dot) but named struct is Server { } (no dot) — asymmetric
```

**Recommendation:** A — one spelling per construction job (I8), and a leading dot uniformly means "construct," so named/inferred and struct/enum all read the same.

## Recently ratified — context (no action)

_Most recent batch (ratified 2026-06-25): **D-CTMARKER1** (C — `$` for the comptime
splice site only + a `comptime { … }` execution block) · **D-WORKSPACE1** (B — fully
computable `workspace.jet` index) · **D-METADEPTH1** (A — reflection/derives only;
full Jai → frozen c154) · **D-CTEFFECT1**, **D-DOTCTOR1**, **D-MONOREF1**,
**D-BUILDPROFILE1**, **D-CTCODEGEN1**, **D-COMPILERLIB1** · plus **D-ENC-DYN1** (A+)
and **D-ENC-YAML1** (A) — build c152, shipped. Tracking cards: c154–c161._


_Background: **D-ASSOC-NOW** was decided **C** (fund both streams: complete
associated types → c149/c72 layer 2, and D-PARSE-1 → c111) and recorded in
[`syntax-decisions.md`](syntax-decisions.md)._

---

**Still deferred (not blocking; expand to a card when needed):**
- **D-SERDE-ACCESS — dynamic-tree accessor API.** How a user reads an untyped
  `Json`/agnostic `DataTree` by hand: pattern-match (shipped today) vs a fluent accessor
  (`tree.field("x").int()?`, `.text()`, `.bool()`, indexing). Only matters for the
  hand-impl / dynamic path (D-SERDE2), not the typed derive. Recommend: keep
  pattern-match as the floor; add minimal fluent accessors if hand-impl ergonomics demand it.

---

> **Drained 2026-06-24 (batch 5).** Owner decided the last open cards: **D-EFF4 = B**
> (ship the closed ten effects now — Net/Fs/Io/Db/Time/Rand/Env/Exec/Log/Gpu — and reserve a
> future `effect <Name>` user-declaration form), **D-EFF5 = A** (flat effect lattice; `#(Io)`
> = console only, no umbrella; `Io`→`Console` rename left as optional polish), and
> **D-JITDEP1 = approve Cranelift** for JIT tier-1 (runtime-side only, I6 holds; the own
> bytecode-VM and own native-JIT progression are frozen board cards so they're not lost).
> All recorded in `syntax-decisions.md`; the effect-system cluster (c62) is now unblocked.

> **Drained 2026-06-24 (batch 4).** The owner ratified all 11 remaining open full cards:
> **D-SIMD2 = A** (method-reduce SIMD surface; operator overloading on built-in lane types
> only), **D-SERDE2 = A** (Swift-plain hand-impl: `encode`/`decode`, `DataTree`, `DecodeError`),
> **D-SERDE3 = C** (typed `RenameAll` menu camel/snake/pascal/kebab/screaming),
> **D-SERDE4 = B, owner-modified** (umbrella `#[Codable]`; one-way `#[Encode]`/`#[Decode]`),
> **D-SERDE5 = A** (per-field bracket markers `#[Rename]`/`#[Skip]`/`#[Default(expr)?]`/`#[Flatten]`,
> absent-optional omitted, struct-flatten now), **D-SERDE6 = C** (typed `decode<T>` turbofish +
> expected-type; turbofish blessed as general grammar), **D-SERDE7 = A + ship chooser now**
> (externally tagged default; `#[Tag("type")]`/`#[Untagged]` container chooser — distinct from
> D-SERDE5 field attrs), **D-SERDE8 = A** (lenient default + `#[DenyUnknownFields]`),
> **D-NOSTD1 = A** (platform-implied std opt-out), **D-IF3 = A** (`if x == { … }` required
> dispatch marker; E0992/E0993), **D-FMT1 = A** (author-intent single-line bodies). The two
> **clarification corrections** were confirmed: **C-CASING** (plan tags → D-CASING1 PascalCase)
> and **C-MANIFEST** (`pkg.jet` → `pack.jet`). All recorded in `syntax-decisions.md`, cards
> stripped. Serde increment-2 implementation unblocked end-to-end (sidequests/serde-model.md).


> **Drained 2026-06-24 (batch 3).** Two follow-on cards ratified: **D-JSONVERB1 = A**
> (`json.to_string(v)` + `json.to_string_pretty(v)`, 2-space indent — renames/retires
> `json.render`; keeps Jet's one `to_`-prefixed conversion idiom, matching ratified `to_float`
> S42; bare `json.string`/`json.stringify` rejected) and **D-TXN4 = A** (`#Transact(order) { …
> order.on_commit(…) }` — the scope's name *is* the handle, mirroring ratified `region r { …
> r.alloc(…) }`; refines D-TXN3's `scope.on_commit` → `<name>.on_commit`, semantics unchanged;
> the D-TXN2 fix-it string is updated to match). The `.Type()`-conversion idea (`x.Float()`)
> was discussed and **declined** — `x.to_float()` (S42) stays as ratified and shipping; no
> reopen. Recorded in `syntax-decisions.md`, cards stripped.

---

> **Drained 2026-06-24 (batch 2).** The owner ratified six cards from the missing-decision
> audit: **D-DBG3 = A** (`jet debug` interactive surface — `step`/`next`/`continue`/`finish`
> + `s`/`n`/`c`/`f` aliases, `(jet)` prompt, `<- here`/`locals:` layout); **D-LINALG1 = A**
> (`jet.linalg` names `Vec2/3/4`/`Mat3/4`, `.dot`/`.cross`/`.matmul` — A names as aliases over
> a `Vec<N>`/`Matrix<M,N>` generic substrate, per owner); **D-SUPPLY1 = A** (dedicated
> `jet vendor` / `jet audit` verbs + `--vendor-dir`, SBOM as a `--sbom` flag); **D-TXN3 = A**
> (`scope.on_commit(() => {…})` library form, no new keyword — the D-TXN2 fix-it string is
> updated to match; the "name the transact scope" follow-on is now open as **D-TXN4**);
> **D-NUMOPS2 = A** (sized/unsigned integers inherit the D-NUMOPS1 trap-on-overflow default;
> `wrapping(…)` is the opt-in); **D-QUAL3 = C** (a `#UnitFamily` mints one distinct type per
> member — `usd`→`Usd` — so signatures read `price: Usd`; the family tag is PascalCase
> `#UnitFamily`). All recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (dap-debugger, math-linalg, package-ecosystem-trust, transact-rollback, dsg9, units; c68
> unblocked by D-QUAL3).

---

> **Drained 2026-06-24.** The owner ratified the last two open cards: **D-BENCH1 = A**
> (`#Bench "name" { … }` region-benchmark block, sibling of `#Test`, run by the existing
> `jet bench` verb) and **D-PKGSIGN1 = B + A opt-in** (SHA-256 checksum is the always-on
> integrity floor; Ed25519 author signing is an opt-in, non-blocking layer — `require_signed`
> off by default). Both recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (epoch-3/testing-docs-ergonomics.md §4; sidequests/package-ecosystem-trust.md §4).

---

> **Memory-model gate CLOSED — ratified 2026-06-23.** The owner decided all three gate
> cards: **D-CAP8 = C** (infer in bodies, freeze at `api: explicit`), **D-CAP9 = D** (`*x`
> = raw-of, dereference becomes postfix `p.*`, `*T` replaces `Ptr<T>`), **D-CAP10 = A**
> (overloads out of scope; call-site-sigil disambiguation on a single definition). Recorded
> in `syntax-decisions.md`; cards stripped. The whole access-capability model
> (`docs/prompt-memory-model-final.md`) is now unblocked — see
> `docs/research/memory-model-implementation-plan.md` for the build order.

---

> **Drained 2026-06-22.** The owner's 2026-06-22 batch ratified every open full card —
> D-UNSAFE2, D-FIXARR1, D-CAP2/3, D-EFF2/3, D-MIGRATE2A/B/C/D/E/F, D-JSONOUT1, D-ARGS1,
> D-MATHLIB1, D-SIMD1, D-REACT1, D-FANOUT2, D-STRPARSE1, D-CTCORE1, D-JIT1, D-HOTSWAP1,
> D-DEVMODE1, D-SOA2A/B/C/D, D-TEST1, D-TEST4, D-BIND2, D-NUMOPS1, D-SERDE1, D-ITER1 (plus
> the earlier batch D-EFF1/D-QUAL1/D-TXN1/D-MIGRATE1/D-SOA1 and D-DBG2). All are recorded
> in `syntax-decisions.md` and their cards stripped from this file. The effect-system
> surface is now fully decided (D-EFF1+D-QUAL1+D-EFF2+D-EFF3). **D-MUTSELF1** (self-mutation
> in `mut self` methods) was opened and ratified 2026-06-23 (option A) — recorded in
> `syntax-decisions.md`, card stripped. The memory-model gate (D-CAP8/9/10) was opened and
> ratified 2026-06-23 — see the note above. **No full decision cards remain open.** What's left
> below is informational only: the **deferred-ballots list**
> (stubs to promote when their prerequisites land), the **B6 `defer`** note, and the
> **Coverage / D-COV1** tooling note. Cards **c25** (range sugar) and **c55** (REPL v2) are
> implement-only. Submitting a decision records it in `syntax-decisions.md` and removes it
> from this file.

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PUBLISH1 — `jet publish` command shape + semver/resolver policy (board card c96).**
*User story:* Saoirse cuts a release of her Jet library and Amara pins a semver range to it.
*Decision (when promoted):* the `jet publish` command surface, version-immutability /
re-publish-refusal policy, and the resolver default (highest-compatible vs exact pins +
explicit update; lockfile default). *Why deferred:* rides **c50** (build-from-source) and
**c56** (registry upload) infra, both unverified/soft-blocked on dep approvals. Promote to a
full card with worked `jet publish` shell examples once M12.2 infra is verified.
Rec direction: `jet publish` infers version from `pkg.jet`, refuses re-publish + a dirty
tree, resolver defaults to highest-compatible with a committed lockfile. From the 2026-06-20
persona run (Saoirse, Amara).

---

**D-JITDEP1 — DECIDED 2026-06-24: approve Cranelift** (runtime-side JIT tier-1, I6 holds).
Recorded in `syntax-decisions.md`. Active work = board card for the Cranelift backend over
the `JitBackend` seam; the own-bytecode-VM and own-native-JIT progression are frozen cards.

---

**D-QUAL4 — Plain marker-tag type-position spelling (prefix vs postfix).**
*User story:* A web dev marks a value `#Tainted` at its source and needs to write
the *type* of a tainted string in a function signature — `flagged: #Tainted String`
vs `String #Tainted`. Same question for `#SingleUse`, `#NoCopy`, and the typestate
markers — the plain (non-parameterized) value-tags that attach to an existing type
rather than minting a new one (so D-QUAL3's "mint a type" Option C doesn't apply).
*Decision (when promoted):* prefix `#Tag Type` (matches every other Jet `#Marker`:
`#Test fn`, `#Numeric distinct`) vs postfix `Type #Tag`. Rec direction: **prefix**, for
one consistent marker idiom. *Why deferred:* no ready consumer — units (c68) ride D-QUAL3
and mint types; the first plain value-tag consumer is taint (D-TAINT1, gated on D-EFF1)
or single-use (D-LIN1, c71). Promote to a full card when c71 or the taint work starts.
Split from D-QUAL3 on 2026-06-24 (a single card can't pick both axes).

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* This is **D-TAINT1 Option B** (full information-flow control —
security-label lattice, principals, `declassify`), which the **owner explicitly
deferred to post-Epoch-3 on 2026-06-21** when ratifying D-TAINT1 Option A
(`#tainted` + sanitizers). Captured here so it is not lost. Generalizes D-TAINT1
and requires the full effect/tag propagation model from D-EFF1 and D-QUAL1 to be
ratified first; the compliance dimension (what counts as a legal sink) is a policy
question that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---

