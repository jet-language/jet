# Reactive UI stack — Tower #134

**Status:** ready — ALL gates ratified 2026-06-30; every phase buildable
**Primary gates:** D-REACTCORE1=D, D-RENDERTGT2=A, D-SIGNAL1=A, D-TYPEDSTYLE1=A, D-MOTION1=A
**Phase gates (ratified):** D-UITREE1=A (typed constructors), D-STYLESHAPE1=A (flat Style record), D-MOTIONTIME1=A (injectable Clock), D-A11YGATE1=B (`jet lint --a11y`)

## What exists already (do not rebuild)

- `JetSignal<T>`, `JetDerived<T>`, `jet_reactive_effect` in `Prelude/CoreLib.rs`
- `#Reactive fn` sema (E2914) + `#Reactive {}` block (TStmt::Reactive)
- `reactive.signal`, `.derived`, `.computed`, `.effect` in sema+codegen
- `JetBackend` trait + `JetNullBackend` + `JetTuiBackend` in `Prelude/Ui.rs`
- `jet_ui_reactive_render` in `Prelude/Ui.rs`
- Geometry: Point, Size, Rect, SizeConstraint, UiNode, JetAriaRole
- Events: JetInputEvent, JetEventResult; Paint: JetPaintCmd
- `core.ui` sema module (CheckerCoreLib.rs:3368) + codegen dispatch
- JS DOM runtime shim (`Prelude/DomRuntime.js`) — complete (measure/layout/paint/onEvent/reactive signals/WASM loader)
- Web codegen (`Codegen/Web.rs`) — complete (measure/layout/paint/on_event wired to `jetDom.*`, WASM bridge, JS app emission)
- Examples: examples/features/ui/reactive_scope.jet, ui_null_backend.jet, ui_tui_reactive.jet; examples/features/web/web_hello.jet, ui_web_reactive.jet, ui_web_click.jet, web_compute.jet, ui_showcase.jet (examples tree moved to topic dirs 2026-07-02, old numbered stems retired)
- Diagnostics E2910–E2914

## Build order

### Phase 1 — computed alias fix (DONE, verified 2026-07-02)
`"computed"` is present in the `jet.reactive` export list (`core_module_items` in `CheckerCoreLib.rs`, `"jet.reactive" => &["signal", "derived", "computed", "effect"]`) and fully wired in sema (`CheckerCoreLib.rs:1120`) and codegen (`TIR/lower.rs:5247`, `TIR/subset.rs:3773`). Covered end-to-end by `examples/features/ui/reactive_scope.jet` (golden) + `tests/tir.rs::reactive_scope_marker`. Landed as part of the c134 Phase 7 checkpoint commit (`edfa5f57`); this plan's "buildable now" note was stale.

### Phase 2 — View model layer (BLOCKED: D-UITREE1)
Typed node tree over `core.reactive`. Composable, diff-based subtree rerenders.

### Phase 3 — Typed styles (BLOCKED: D-STYLESHAPE1, D-STYLEUNIT1)
`core.ui.style` module: typed `Style`, `Length`, `Color` value types. Replace raw String color in `JetPaintCmd::FillRect`. Diagnostic range E2920-E2924.

### Phase 4 — Ownable component kit
`jetpack add <component>` copies `.jet` source into user tree. Starter: Button, Label, Input, Container. Requires Phase 2 (View type) + Phase 3 (Style).

### Phase 5 — Motion (BLOCKED: D-MOTIONTIME1)
`core.motion`: spring/tween/keyframes as `Computed<T>` derived from reactive state + Clock. No imperative frame mutation.

### Phase 6 — A11y (BLOCKED: D-A11YGATE1)
`JetUiNode` gains `role: Option<JetAriaRole>` and accessible label. Keyboard focus routing. Release-gated lint diagnostics E2930-E2931.

### Phase 7 — Web/DOM backend (DONE, verified 2026-07-02)
`DomRuntime.js` DOM ops complete (measure/layout/paint/onEvent/reactive signals, real DOM mounting when `document` exists, scope-keyed element identity). `Codegen/Web.rs` measure/layout/paint/on_event fully wired to `jetDom.*`. WASM loader (`instantiateWasm` in `DomRuntime.js`, `loadWasm`/`bridge_*` in `Web.rs`'s `emit_js_app`) implemented and exercised by `web_compute_wasm_bridge_roundtrip`. `jet dev --target=web` orchestration in `Source/CmdDevWeb.rs`. Reactive-signal-drives-DOM example: `examples/features/web/ui_web_reactive.jet` (a `reactive.signal` re-painting on `.set()`, the counter/click case) — golden-tested natively (`tests/golden.rs`) and end-to-end under `node` via `tests/web_build.rs::web_reactive_dom_snapshot_roundtrip`; `ui_web_click.jet`/`.html` cover the click-driven DOM-mount variant. Landed as part of the c134 Phase 7 checkpoint commit (`edfa5f57`) plus follow-on dogfooding; this plan's "buildable now" note and the `168_ui_web_hello.jet` filename were stale (examples tree moved to topic dirs, no numbered stems).

### Phase 8 — Native backends
AppKit/Win32/GTK via `#Extern` C FFI. Each implements `JetBackend`. Platform-gated with `#Target(os)`.

## Open ballots needed

- **D-UITREE1**: typed JetNodeKind enum vs tagged dynamic record (blocks Phase 2)
- **D-STYLESHAPE1**: flat Style record vs builder chain (blocks Phase 3)
- **D-MOTIONTIME1**: injectable Clock param vs global time signal (blocks Phase 5)
- **D-A11YGATE1**: --release flag vs jet lint --a11y profile vs #A11yStrict marker (blocks Phase 6)

Recommended: A for all four (enum, flat record, injectable clock, lint profile).

### Phase 9 — c134 ballot batch: web-default target, companion HTML, native OS gating (ratified 2026-07-01)

Three ballots on card c2qj06uq, all outcome=A. First two were already built ahead of formal ratification (dogfooded on Phase 7 extension work); this phase is mostly cleanup + closing the gap on the third, which unblocks Phase 8.

**Housekeeping first (applies to all three):** every touched file still carries `D-WEBDEFAULT1 (open, c134)` / `D-HTMLPAIR1 (open, c134)` doc-comments — `crates/jet-foundation/src/Syntax.rs:241,247`, `crates/jet-foundation/src/AST.rs:419,427,560,562`, `crates/jet-codegen/src/Codegen/Web.rs:19,23`, `crates/jet-parser/src/Parser/Items.rs:3,1852,1859,1900`, `crates/jet-driver/src/Jetpack/PackageManifest/mod.rs:119`, `Source/main.rs:1212,1250,1275`. Reword to `(ratified 2026-07-01, c134)` — grep `open, c134` to find every site. Also: neither decision has a row in `docs/spec/syntax-decisions.md` yet (I7/syntax-decision-protocol requirement) — add both, dated 2026-07-01, `Ratified — implemented` status, next to the existing `D-WASM1` row (`docs/spec/syntax-decisions.md:3272`).

#### 9.1 — D-WEBDEFAULT1=A: file/manifest default build target

Ratified semantics: `#Target(Web)` — a third value on the *existing* `#Target(...)` marker, same lexical form as `#Target(Wasm)`/`#Target(Js)` but a different axis (default CLI backend, not a partition ceiling) — makes `jet build/dev/check` default to `--target=web` without the flag. `pkg.jet`'s `target: "web"` field does the same for a managed package. Precedence: `--target=<x>` CLI flag > `pkg.jet` `target:` > file `#Target(Web)` > native default. `jet run` never infers (native-execution only; a web build has no runnable console binary).

Already implemented, verified working:
- `crates/jet-foundation/src/Syntax.rs:245` `WEB_TARGET_DEFAULT_WEB = "Web"`
- `crates/jet-parser/src/Parser/Items.rs:1900-1929` `parse_web_target_marker` returns `TargetMarker::DefaultWeb` for `Web`, `TargetMarker::Bucket(...)` for `Wasm`/`Js`
- `crates/jet-foundation/src/AST.rs:419-426` `Program.default_target`, `AST.rs:560-562` `LoadedModule` mirror
- `crates/jet-driver/src/Jetpack/PackageManifest/mod.rs:119-122` `PackageMeta.target: Option<String>`
- `Source/main.rs:1231-1249` `effective_target(cmd, file, explicit)` — the precedence chain; `cmd == "run"` short-circuits to `None`; `Source/main.rs:1276-1283` `manifest_default_target` walks to `pkg.jet` via `Loader::find_manifest_root`
- Diagnostics: reuses `E0003` for a `#Target(Web)` placed on a `module {}` block (`crates/jet-parser/src/Parser/Items.rs:377-395`, fixture `tests/ui/web_target_web_on_module.jet`/`.stderr`) and for a duplicate `#Target(Web)` marker (same block). No new E-code needed — this is the same generic "marker in the wrong position" family `#Target(Wasm)`/`#Target(Js)` already use.
- Tests: `tests/web_build.rs::jet_cli_run_never_infers_web_target_from_marker`, plus the manifest-precedence and file-marker-precedence integration tests referenced in the c134 card log (2026-07-01 entry, "4 new tests total this round" — locate via `grep -n "default_target\|manifest_default_target" tests/*.rs`)
- Formatter: **gap.** No formatter handling for `#Target(Web)`/`#Target(Wasm)`/`#Target(Js)` markers exists in `crates/jet-parser/src/Formatter/Items.rs` or `mod.rs` (grep confirms zero hits), and `tests/fmt.rs` has no stability test for any `#Target(...)` marker. Per the house lesson (formatter round-trip is required for new syntax, not optional — a past miss here silently corrupted syntax for months), add a `fmt_target_web_marker_stability` test asserting `jet fmt` on a `#Target(Web)` file is byte-stable, and fix formatter emission if it isn't. Do this before closing the phase, not after.

#### 9.2 — D-HTMLPAIR1=A: explicit companion-HTML marker (option A only — option B not ratified)

Ratified semantics: `#Html("path.html")`, same marker family as `#Target(Web)`, names this program's companion host page explicitly. Precedence when writing web build output: explicit `#Html(...)` > legacy `<stem>.html` sibling convention (kept, not deprecated) > generic auto-generated page. A referenced `#Html(...)` path that doesn't resolve on disk is a hard build error naming the missing file. Note: the ballot's amendment added an option B (`pkg.jet` `packages: { … executable { html: "..." } }` field for managed packages, composing with the file marker) — the recorded `outcome` is `"A"` only, so option B is **not** ratified; do not build the manifest-level `html:` field.

Already implemented, verified working:
- `crates/jet-foundation/src/Syntax.rs:250` `ATTR_HTML = "Html"`
- `crates/jet-parser/src/Parser/Items.rs:1852-1898` `at_html_marker`/`parse_html_marker` — dup-marker check (E0003, "only one `#Html(…)` marker"), non-string-literal arg (E0003), multi-part/interpolated path (E0003)
- `crates/jet-foundation/src/AST.rs:427-431` `Program.html_path`, `AST.rs:560-562` `LoadedModule.html_path`
- `crates/jet-codegen/src/Codegen/Web.rs:19-25,52` `WebArtifacts.explicit_html_path`
- `Source/CmdCompile.rs:847-889` `write_web_artifacts` — the three-way precedence chain, missing-file error text: `` error: `#Html("{rel}")` names a file that doesn't exist: {path} ({io_err}) `` (a raw CLI error string, not a sema diagnostic with an E-code — matches the house pattern for other file-not-found CLI errors like the missing-`main.jet` message in `Source/main.rs`; do not force an E-code onto it)
- Tests: `tests/web_build.rs::jet_cli_uses_explicit_html_marker`, `::jet_cli_html_marker_missing_file_is_an_error`
- Formatter: same gap as 9.1 — no `#Html(...)` formatter/stability coverage. Add `fmt_html_marker_stability` alongside the `#Target(Web)` one.

No new example needed — dogfooded on the flagship showcase already (`topic/ui-showcase.jet` + companion `.html`, per the examples-tree reorg in flight; do not cite the current numbered path).

#### 9.3 — D-OSTARGET1=A: native OS platform gating for `#Extern` backends (unblocks Phase 8)

Ratified semantics: extend the *same* `#Target(...)` marker family with an `Os.*` namespace — `#Target(Os.Linux)`, `#Target(Os.Macos)`, `#Target(Os.Windows)` — as a second, mutually-exclusive axis from the web bucket/default values (`Wasm`/`Js`/`Web`). Unlike the web values, `#Target(Os.*)` attaches at item level (an `impl` block, per the ballot's worked example) rather than file/module level. `jet build --target=<triple>` only emits/links `#Target(Os.X)`-gated items whose `X` matches the triple's OS (native default triple when `--target` is omitted); an `#Extern` call site reachable from code that isn't itself gated to match is a compile error, not a link failure. This is genuinely unbuilt — Phase 8 was blocked on exactly this decision (c134 card log, 2026-07-01: "Phase 8 blocked... Raised D-OSTARGET1... Card stays in deciding for Phase 8 only"). It is now unblocked but not implemented.

Design notes for the implementing agent (none of this is separately ratified — it's the mechanical follow-through of the ratified text, flag anything that feels like a new user-facing choice as its own ballot rather than guessing):

- **Parser.** `crates/jet-parser/src/Parser/Items.rs:1903-1929` `parse_web_target_marker` currently expects one bare `Ident` inside `#Target(...)` (`expect_ident`). `Os.Linux` needs `Ident . Ident` — extend to peek for a `.` after the first ident and, when present, require the first segment to equal a new `Syntax::TARGET_OS_NAMESPACE = "Os"` constant, then parse the second segment against `Linux`/`Macos`/`Windows`. Reuse the existing "not a known web partition" `E0003` error shape for an unrecognized second segment (extend its message/fix text to mention `Os.Linux`/`Os.Macos`/`Os.Windows` too — same code, don't mint a new one, matching how `Wasm`/`Js` share `E0003` today).
- **Attachment point.** Today's `at_web_target`/`#Target(...)` dispatch lives in the top-level file-parsing loop (`Items.rs:372`, module-block variant at `Items.rs:379` and the sibling handling in `crates/jet-parser/src/Parser/Modules.rs:154`) — file/module scope only. `#Target(Os.X)` needs to attach to `impl` blocks (confirmed by the ballot's worked example) and plausibly plain `fn` items (the ballot gist says "function/module"). This is new parser surface, not a variant of the existing file-scope check — locate the `impl` item parse path (`Items.rs:945`, `2975`, `3012`, `Item::Impl(ImplDef {...})`) and add an optional leading-marker check there, storing the result on a new `ImplDef.os_target: Option<OsTarget>` field (mirror for `FuncDef` if function-level gating is needed — check the ballot's two example call sites again before committing to both).
- **AST.** New `OsTarget` enum (`Linux`/`Macos`/`Windows`) in `jet-foundation`, sibling to `WebPartition::WebBucket`. New optional field on `ImplDef` (and maybe `FuncDef`) in `crates/jet-foundation/src/AST.rs`.
- **Sema — new file `crates/jet-sema/src/Sema/OsTarget.rs`, mirroring the existing `crates/jet-sema/src/Sema/WebPartition.rs` pattern (same shape of problem: two diagnostics, one for a structural conflict, one for a cross-boundary call):**
  - `E-OSTARGET-MIXED-AXIS` — a `#Target(Os.*)` marker and a web-axis `#Target(Wasm|Js|Web)` marker both apply to the same item (D-OSTARGET1's "two different axes, one marker, mutually exclusive values"). Model the diagnostic constructor on `web_cross_partition`/`web_target_browser` in `crates/jet-foundation/src/WebPartition.rs:79-102` (string E-code, not numeric — matches the existing `E-WEB-CROSS-PARTITION`/`E-WEB-TARGET-BROWSER` family this decision extends). Draft: "`#Target(Os.{os})` can't combine with `#Target({web})` on `{item}` — pick one axis." Fixture: `tests/ui/os_target_mixed_axis.jet`/`.stderr`.
  - `E-OSTARGET-UNMATCHED-CALL` — a call site not itself gated to match reaches a `#Target(Os.X)`-gated item (the ballot's "Linux-only #Extern call site not behind a matching #Target is a compile error, not a link failure"). Model directly on `check_web_partition`'s caller/callee bucket-mismatch walk in `WebPartition.rs:379-456` — same reachability-analysis shape, new axis. Fixture: `tests/ui/os_target_unmatched_call.jet`/`.stderr`.
  - Register both rows in `docs/spec/diagnostics.md` next to `E-WEB-CROSS-PARTITION`/`E-WEB-TARGET-BROWSER` (line ~295-296 table, ~853-855 detail).
- **Codegen.** Native backend selection: only emit/link the `impl` matching the active OS bucket, mirroring how `Codegen/Web.rs` already filters function membership by `WebBucket`. The active OS bucket comes from the existing `--target=<triple>` flag (`Source/main.rs:590-592`, `E2-M15`, already threaded through `cross_target`/`effective_target`) — map triple → `OsTarget` (host OS when `--target` is absent), do not add a second, competing flag.
- **Syntax.rs registration (I7):** `TARGET_OS_NAMESPACE = "Os"`, `TARGET_OS_LINUX = "Linux"`, `TARGET_OS_MACOS = "Macos"`, `TARGET_OS_WINDOWS = "Windows"`, doc-commented `D-OSTARGET1=A (ratified 2026-07-01, c134)`, next to the existing `ATTR_TARGET`/`WEB_TARGET_DEFAULT_WEB` block (`Syntax.rs:219-245`).
- **Example.** `topic/os-target-gating.jet` (examples tree is mid-reorg into topic dirs — do not cite a numbered path). Sandbox has no GTK/AppKit/Win32 dev headers or pkg-config (verified during the original Phase 8 attempt, c134 card log 2026-07-01) — do not gate the example or its golden test on a real linked native library. Demonstrate the *gating mechanism* with two dummy structs implementing `JetBackend` under `#Target(Os.Linux)` / `#Target(Os.Macos)`, no real `#Extern` C calls, proving (a) only the host-OS impl is emitted/linked, (b) an ungated call into an OS-gated item is rejected. A real GTK/AppKit/Win32-backed native backend needs its own devShell/flake toolkit-dependency change first — that is a separate gate the owner should see before it's assumed (already flagged once in the card log; don't re-litigate, just don't build past it here).
- **Formatter/fmt-stability:** new syntax (`#Target(Os.Linux)` on an `impl`) — add formatter emission and an `fmt_os_target_marker_stability` test in `tests/fmt.rs` per the house rule, same as 9.1/9.2's gap.
- **Tests:** `tests/cross.rs` (existing E33xx/cross-compile pattern, `check_freestanding_src`-style harness — add an OS-target analog) plus unit tests in the new `OsTarget.rs` sema file mirroring `WebPartition.rs`'s own test module.

Exit criteria for Phase 9: 9.1/9.2 doc-comments reworded + syntax-decisions.md rows added + formatter stability tests added and green; 9.3 parses `#Target(Os.*)` on `impl`, both new diagnostics fire with fixtures, codegen emits/links only the matching-OS impl, one dummy (non-GTK) example golden-tested, `tests/cross.rs` covers the gating, formatter round-trips. Phase 8 (real native backend) stays open behind the separate devShell/toolkit-dependency gate.

## Acceptance
Phases 1+7 closed 2026-07-02. Full card done when: view model example with diff rerenders; typed Style usable; component added via jetpack; motion spring with deterministic fake-clock tests; a11y example with keyboard nav golden; web DOM snapshot (done — Phase 7); one native backend behind #Target guard; no second reactive model anywhere.
