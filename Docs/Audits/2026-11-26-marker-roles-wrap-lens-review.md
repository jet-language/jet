# Adversarial Design Review: Marker Roles Ladder, `wrap` Role, and `jet expand --lens source` (#7004 / P2)

- **Date:** 2026-11-26
- **Scope:** Proposal item P2 (`Docs/Proposals/2026-11-24-greenfield-language.md` Part 2 P2, §3.11) and issue #7004 — (1) unifying marker power into one 5-role ladder (`note`, `check`, `generate`, `wrap`, `rewrite`) with per-dependency role denial; (2) adding a `wrap` marker role that replaces `policy ... { wrap(call) { ... } }` declarations and deletes `policy`/`wrap` from the grammar (also unblocking `#7011` c4 wrapping block policies); and (3) adding a `source` lens to `jet expand` with `jet debug` stepping through expanded code.
- **Verdict:** **NEEDS BALLOT** (5 bounded owner ballots **Q1–Q5** below before starting implementation).

---

## 1. Executive Summary

Jet already has most of the infrastructure for P2: typed marker structs (`Compiler/Prelude/Markers.jet`), `impl Marker { sites:, expand:, check:, row: }` (`Compiler/JetParser/Source/Parser/Declarations/Markers.jet:122-218`), `Meta.Generate` enforcement on marker templates (`Compiler/JetSema/Source/Sema/Registration/UserTemplates.jet:511-692`, E0774/E0775), expert `Meta.Code.Rewrite` authority + `meta.allow_rewrite(package:)` (`Compiler/JetSema/Source/Sema/Calls/Build.jet:777-821`, E0777/E3514), 10 `jet expand` fact lenses (`Compiler/JetCLI/Source/CLI/Expand.jet:33-44`), and a native DWARF + `.debug_jetline` + `.debug_jetsource` debugger (`Compiler/JetBackend/Source/Image/Dwarf.jet:1-56`, `Jetline.jet:16-120`, `Compiler/JetDebug/Source/Native.jet:1-83`).

However, inspecting the live codebase at `41e06744a41` reveals **six critical architectural seams and hidden defects** that must be decided before writing code:

1. **Today's `policy ... { wrap(call) { ... } }` is tiny (only 8 declarations across 6 repo files, 0 in `Core/` or `Compiler/` source) — and fundamentally broken for functions with parameters.** `CallablePolicies.jet:104-118` clones `policy.body` into a per-target function whose `call` parameter has the target's full function type, so `call()` fails to compile on any function taking parameters; `CallablePolicies.jet:213-228` wraps at **call sites** (recreating Spring AOP's self-invocation bypass and leaving target effect rows ignorant of wrapper effects); and `CallablePolicies.jet:5-7, 25, 247` silently turns built-in `retry`/`trace`/`cache` into no-ops.
2. **Proposal §3.11's `wrap` signature does not match Jet's live `impl Marker` placement or `via` effect polymorphism.** Live Jet places `expand`, `check`, and `row` inside `impl Marker { ... }` (`Declarations/Markers.jet:122-136`) and writes effect polymorphism with `-[via call]>` rather than a type-parameter effect variable `-[E]>`. Furthermore, `wrap` must handle unbounded `F` unifying with `Never` (for infallible functions), refuse multi-call wrapping of `^^` move-consuming bodies, and clarify whether bare `apply(f)` unwrapping (a security bypass on `#Audit`/`#Auth`) is retired.
3. **The 5-role ladder does not yet match live `Meta.*` effects or per-dependency denial.** Today `authority.deny` (`Compiler/JetDriver/Source/Driver/Loader.jet:562-580`) is a **flat root-wide effect list**, NOT a per-dependency map; `Meta.Generate` is non-expert (allowed for dependencies by default); `note`, `check` (`expand -[]>` and `check: fn(self, call: CompilerCall)`), and `wrap` have **no `Meta.*` effect at all**; and `UserTemplates.jet:632-635` forbids `Meta.Code.Rewrite` on `Marker.expand` (keeping rewrites strictly on `build: fn`'s `Compiler.rewrite_body`).
4. **`jet expand` currently accepts `--facts <lens>` (not `--lens`), refuses bare `jet expand FILE`, and has no source-rendering pipeline.** `CLI/Expand.jet:454-465` parses only `--facts`, `CLI/Inspect.jet:2116` returns `not_ported_output` when no lens is passed, and none of the 10 lenses prints formatted, provenance-labeled generated Jet source.
5. **Naive `jet debug` stepping through expanded text would shift every user line number and break `.debug_jetsource` verification.** Replacing `main.jet` with expanded text in `.debug_line` / `.debug_jetline` shifts every breakpoint and stack frame in hand-written code. A clean **dual-source model** (hand-written code keeps `(main.jet, original_line)`; generated/derived/wrapped/rewritten bodies point to a deterministic `.jet/expanded/<module>.expanded.jet` companion verified by `.debug_jetsource` SHA-256) solves both without breaking normal debugging.
6. **Keyword cleanup is clean and complete.** `policy` and `wrap` are contextual identifiers (`Parser/Items.jet:183-215`), NOT lexer tokens, so deleting `UserPolicyDecl`, `parser_user_policy_ahead` (which currently hijacks top-level identifiers named `policy`!), `SyntaxCodec` rows, `Formatter/Items.jet:883`, and 4 editor grammar entries is a single atomic cutover.

---

## 2. Strongest Existing Solutions & Primary-Source Lessons

| System | Primary Source | What It Does Best | Concrete Lesson for Jet #7004 |
| :--- | :--- | :--- | :--- |
| **Python Decorators & `functools.wraps`** | PEP 318 (*Decorators for Functions and Methods*); PEP 3129; PEP 362; Python `functools.wraps` | Stacking order top-to-bottom = outer-to-inner; uniform syntax for wrapping functions; `__wrapped__` preserves introspection metadata. | Naive runtime wrapper replacement hides added side effects, breaks static signature/effect checking, and dumps debugger users into generic `*args, **kwargs` trampolines unless source provenance and types are preserved statically. |
| **Swift Macros & Xcode Macro Expansion** | Swift Evolution SE-0382 (*Expression Macros*), SE-0389 (*Attached Macros*), SE-0415 (*Function Body Macros*) | Explicit role separation (`@attached(peer \| member \| accessor \| conformance \| body)`); body macros wrap a function body while preserving its signature; Xcode renders inline macro expansions and lets LLDB step into virtual `@__swiftmacro_*.swift` buffers. | Swift's dual-buffer debugging proves the exact right coordinate model: **user code keeps its original `.jet` file and line numbers**, while generated/wrapped bodies carry DWARF file entries pointing to a deterministic expanded buffer so stepping into generated code works without shifting user lines. |
| **Rust Proc Macros & `cargo-expand`** | The Rust Reference (*Procedural Macros*); RFC 1566; `cargo-expand` (`rustc -Zunpretty=expanded`) | `cargo-expand` gives developers one command to inspect all derived/macro-generated code as readable Rust source. | Two weaknesses Jet must surpass: (1) `cargo-expand` strips per-marker provenance comments, and (2) `rustc` maps every generated instruction back to the single line of `#[derive(...)]`, making line-by-line debugger stepping through derived code impossible. |
| **AspectJ & Spring AOP Around Advice** | Kiczales et al., ECOOP 1997 (*Aspect-Oriented Programming*); AspectJ `ProceedingJoinPoint.proceed()`; Spring AOP Proxy Documentation | Expressive around-advice wrapping (`proceed()`) for retry, tracing, transactions, and caching. | **Two classic pitfalls to avoid:** (1) invisible pointcut action-at-a-distance broke reasoning — Jet keeps wrapping strictly opt-in via visible markers; (2) proxy-based call-site wrapping suffers the **self-invocation bypass bug** (internal calls bypass `@Retryable`/`@Transactional`), which is the exact bug in Jet's current `CallablePolicies.jet:213-228`! Body-wrapping eliminates it at the root. |
| **TC39 JS / TypeScript 5.0 Decorators** | TC39 Stage 3 Decorators Specification; TypeScript 5.0 Release Notes | Strictly typed signature-preserving wrappers with explicit target kinds and deterministic application order; deliberately rejects arbitrary AST body rewriting inside decorators. | Reinforces Jet's role ladder: signature-preserving `wrap` is safe without root consent, whereas arbitrary body `rewrite` sits at the top of the ladder behind explicit per-package root consent. |

---

## 3. Complete Live Codebase Inventory (at `41e06744a41`)

### 3.1 Every `policy ... { wrap(call) { ... } }` Site Across the Repository

A full `git grep` across the repository finds **zero** uses in `Core/` or `Compiler/` implementation code and **8 `policy` declarations across 6 `.jet` files** plus **1 inline test string**:

| Category | File & Line(s) | Count / Role |
| :--- | :--- | :--- |
| **Example** | `Examples/Basics/Functions/CallablePolicies.jet:3, 32, 40` | 3 declarations (`audit`, `pass_through`, `labeled`) + `apply(...)` demos (note: `load_user` uses `#Policy(trace(...))` / `apply(retry(3), ...)`, both silent no-ops today) |
| **Spec surface sample** | `Docs/Spec/Reference/syntax-surface.jet:115-116` | 1 declaration (`audit`) |
| **Case tests** | `Tests/Cases/UserPolicies/UserPolicyApplyKeepsACallableBinding.fixture/main.jet:1`, `UserPolicyDeclarationAndCustomChainParseTogether.fixture/main.jet:2`, `user_policy_declaration_and_custom_chain_parse_together.case:8` | 2 declarations (`audit`) + 1 formatted-source assertion |
| **UI tests** | `Tests/UI/user_policy_valid.jet:1`, `Tests/UI/user_policy_bad_args.jet:1` | 2 declarations (`audit`) |
| **Compiler unit fixture** | `Compiler/JetSema/Tests/BurnDownFixtures.fixture/BurnDownFixtures.jet:65` | 1 inline string (`policy audit(...) { wrap(call) { return call() } }`) |
| **Ledgers & surface docs** | `Compiler/JetFoundation/Data/Syntax/CoreSurface.jet:479`, `Jetpack/Foundation/Source/SyntaxRegistry.jet:71`, `Tests/Guards/core_surface_ledger.jet:13`, `Tools/Agent/CoreSurfaceLedger.jet:146` | Surface documentation & ledger rows for `policy`/`wrap(call)` |
| **Editor grammars** | `Tools/Editors/TreeSitter/grammar.js:18, 276`, `Tools/Editors/Vscode/Syntaxes/jet.tmLanguage.json:277`, `Tools/Editors/jet.tmGrammar:277`, `Tools/Editors/Zed/languages/jet/highlights.scm:54` | Keyword highlighting for `policy` and `wrap` |

### 3.2 Compiler Pipeline Ownership Today

| Subsystem | File & Lines | Current Mechanism | Gap / Cutover Needed |
| :--- | :--- | :--- | :--- |
| **AST & Codec** | `Compiler/JetFoundation/Source/AST/Items.jet:41, 264, 1529`; `Facts/SyntaxCodec.jet:71, 76, 7747-7763` | `UserPolicyDecl` + `Program.user_policy_declarations` | Delete completely once `wrap` moves to `MarkerDecl`. |
| **Parser & Formatter** | `Compiler/JetParser/Source/Parser/Items.jet:183-215`; `Entry.jet:1701-1704`; `Formatter/Items.jet:883`; `Declarations/Markers.jet:122-218` | Contextual `policy`/`wrap` parser (`parser_user_policy_ahead` matches ANY top-level `TOKEN_IDENT == "policy"`!). `impl Marker` parses only `expand`, `check`, `row`. | Delete `parser_parse_user_policy` & `jfmt_fmt_user_policy_decl`; add `wrap` method parsing/formatting to `impl Marker`. |
| **Sema Wrapping & IR** | `Compiler/JetSema/Source/Sema/CallablePolicies.jet:1-255`; `BodyFlatten.jet:833, 1415`; `Compiler/JetIR/Source/BuildRoutes.jet:2154-2215` | Clones `policy.body` per target (`__jet_policy_<p>_<t>`), wraps at direct call sites via `TFnValueKind.Policy` -> `BodyFnValueKind.Policy` -> synthetic `BodyLambda`. Skips built-in names (`cache`, `retry`, `trace`, `registration`, `route`). | Replace with body-wrapping (`Marker{...}.wrap(fn() { ... })`) + closure-based `apply(Name(args), f)`; delete `TFnValueKind.Policy` / `BodyFnValueKind.Policy` and the built-in no-op skip list. |
| **Role Ladder & Authority** | `Compiler/JetSema/Source/Sema/Registration/UserTemplates.jet:511-692`; `Calls/Build.jet:777-821`; `Effects/Checks.jet:1483-1548`; `Compiler/JetFoundation/Source/Policy/Authority.jet:163-225`; `Compiler/JetDriver/Source/Driver/Loader.jet:543-625` | `expand -[]>` -> check (E0775 if generating); `expand -[Meta.Generate]>` -> generate; `check: fn(self, call)` -> call rule; `meta.rewrite_body` -> `Meta.Code.Rewrite` (expert, E0777) + `meta.allow_rewrite(package:)` (E3514). `authority.deny` is a flat root list `[String]?`. | Add `wrap` to ladder; add per-dependency role denial in `package.jet` & Sema marker application/expansion checks so a root can deny any role (`note`, `check`, `generate`, `wrap`, `rewrite`) per dependency. |
| **`jet expand` CLI** | `Compiler/JetCLI/Source/CLI/Expand.jet:33-44, 448-482`; `CLI/Inspect.jet:2112-2143`; `CLI/Front.jet:440`; `CLI/InspectB.jet:48, 940` | 10 fact lenses via `--facts <lens>`; bare `jet expand FILE` returns `not_ported_output`. | Add `source` lens (inlining derives, templates, wraps, build rewrites with provenance comments), support `--lens` (aliasing `--facts`) and optional `--marker` filter. |
| **Debug Line Mapping** | `Compiler/JetBackend/Source/Image/Dwarf.jet:1-56, 1846-1925`; `Image/Jetline.jet:16-120`; `Compiler/JetDebug/Source/Native.jet:63-83`; `NativeFacts.jet:169-226`; `Registration/Derives.jet:148-163` | `.debug_line` + `.debug_jetline` map compiled functions to source file/line; `.debug_jetsource` verifies SHA-256 of compiled files. `SemaTemplateOrigin` only tracks compile-time `jet debug --build` breakpoints inside template sources. | Emit companion expanded source artifact (`.jet/expanded/<module>.expanded.jet`) hashed into `.debug_jetsource` and point generated/derived/wrapped/rewritten function line rows there while preserving user file line numbers untouched. |

---

## 4. Adversarial Attacks & Edge Cases

### Attack 1: `wrap` Method Placement, Signature Polymorphism, Arity, and `^^` Move Captures
- **Claim attacked:** "A marker struct may implement a `wrap` method; then `#Name(args)` on a function or block wraps it, and `apply(Name(args), f)` keeps working" (`Docs/Proposals/2026-11-24-greenfield-language.md:267-268, 1892-1915`).
- **Attacks:**
  1. **Placement collision (`impl Marker` vs struct method):** Proposal §3.11 writes `wrap: fn...` directly on the struct body outside `impl Marker { sites: [...] }`, whereas live Jet (`Declarations/Markers.jet:122-136`) requires `expand`, `check`, and `row` *inside* `impl Marker { ... }`. Putting `wrap` outside `impl Marker` means any ordinary helper method named `wrap` on a note marker struct accidentally promotes it to the `wrap` role!
  2. **Effect & failure polymorphism mismatch with live Jet Sema:** Proposal §3.11 writes `wrap: fn<R, E, F: Describe>(self, call: fn() -[E]> R F!) -[E, Clock]> R F!`. Live Jet expresses higher-order effect polymorphism via `-[via call]>` (not an effect type parameter `E`), and an infallible target (`fn() -> Int`, i.e. `F = Never`) CANNOT satisfy `F: Describe` (`Never` does not implement `Describe`). Thus:
     - General wrappers (`Trace`, `Audit`, `Time`) MUST use unbounded `F` (`fn<R, F>(self, call: fn() -[via call]> R F!) -[via call, Log]> R F!`) so `F` unifies with `Never` (infallible) AND with any concrete error type (`Err`, `NetError`, etc.)!
     - Failure-inspecting wrappers (`Retry`) MAY bound `F: Describe` — which cleanly **rejects `#Retry` on infallible functions at compile time**!
     - **Requirement for Sema:** Verify that generic `R F!` unifies with both infallible `R` (`F = Never`) and fallible `R E!` (`F = E`) in generic method instantiation!
  3. **Nullary thunk `call` solves arity, labels, defaults, variadics, and generics — EXCEPT move-once `^^` captures:** Wrapping `f(a: Int, b: String{"x"}, ...rest: [Int])` as `Marker{...}.wrap(fn() { body })` works uniformly for every parameter shape without variadic generics, and `apply(Name(args), f)` works for any arity `f` by synthesizing `fn(a, b, ...) { Name(args).wrap(fn() { f(a, b, ...) }) }`. **HOWEVER**, if `body` consumes a `^^` move parameter (`payload: ^^Bytes`), the closure `fn() { body }` is a move-once closure — calling `call()` in a loop (`Retry`) would use-after-move on attempt 2! Sema's capture/ownership pass MUST reject calling a consuming thunk inside a repeatable `fn()` parameter (or distinguish `call` called at most once vs repeatedly).
  4. **Proposal snippet return-path bug:** In §3.11 lines 1896-1903, `loop attempt in 1..self.times { ... }` can exit without returning if `self.times <= 0`. Core `Retry` must enforce `times >= 1` (e.g. `attempt 1` unconditionally or `#Pre(self.times >= 1, ...)` + final `return call()`).

### Attack 2: Body-Wrapping vs. Call-Site Proxying (and `apply(f)` Unwrapping Bypass)
- **Claim attacked:** `apply(Name(args), f)` keeps working and wrapping replaces `CallablePolicies.jet`.
- **Attack:** Today `CallablePolicies.jet:213-228` wraps at **direct call sites** (`sema_policy_direct_value`), which causes **four defects**:
  1. **Self-invocation bypass (Spring AOP bug):** Recursive or indirect calls through function pointers miss or duplicate wrappers inconsistently.
  2. **Hidden wrapper effects:** `fetch_rates: fn() -[Net]> Rates NetError!` does not show `Clock` from `#Retry` in its checked effect row (`BuildRoutes.jet:2186`).
  3. **Per-call closure allocation:** Every call constructs a fresh `BodyFnValueKind.Policy` lambda (`BuildRoutes.jet:2154-2200`).
  4. **Security bypass via `apply(f)`:** `Examples/Basics/Functions/CallablePolicies.jet:18, 20` tests `unwrapped: apply(audited_load)` stripping `#Policy(audit(...))` from `audited_load`. If a function is marked `#Audit` or `#Auth`, allowing an external caller to strip its wrapper with `apply(f)` breaks encapsulation and security!
- **Resolution:** Lower `#WrapMarker` directly **inside the target function's body** (`fetch_rates`'s checked body becomes `Retry{times: 3}.wrap(fn() { ... })`), joining the wrapper's effects into the target function's effect row and deleting `TFnValueKind.Policy` / `BodyFnValueKind.Policy` completely. Keep `apply(Outer, Inner, f)` for composing wrap markers onto an existing function value (producing a normal closure), and **retire bare `apply(f)` unwrapping** (Ballot Q2).

### Attack 3: Stacking Order, `#Policy(...)` Coexistence, and `#7011` Block Seam
- **Attack:** Three orderings interact: (a) stacked wrap markers `#A #B f: fn()`, (b) marker groups `#[A, B] f: fn()`, (c) `apply(A, B, f)`, and (d) `#Policy(retry(3), deadline(2s)) { ... }` from `#7011` Q5.
- **Resolution:** One universal law across all four: **top-to-bottom / left-to-right = outer-to-inner**.
  - `#A #B f: fn() { body }` == `#[A, B] f: fn() { body }` -> `A.wrap(fn() { B.wrap(fn() { body }) })`.
  - `apply(A, B, f)` -> `fn(args...) { A.wrap(fn() { B.wrap(fn() { f(args...) }) }) }`.
  - On blocks (unblocking `#7011` c4): `#A #B { body }` -> `A.wrap(fn() { B.wrap(fn() { body }) })`, obeying `#7011` Q2's escaping control-flow rule.
  - Under `#7011` Q5 Option A, `#Policy(retry(3), deadline(2s))` on a function or block desugars directly to the same wrap-marker chain (removing the silent no-op `sema_policy_builtin` list in `CallablePolicies.jet:5-7`).

### Attack 4: The 5-Role Ladder vs. Live `Meta.*` Effects and Per-Dependency Role Denial
- **Claim attacked:** "The roles form a ladder; only the last needs consent... A root can deny any role per dependency" (`Docs/Proposals/2026-11-24-greenfield-language.md:243-255`).
- **Attacks:**
  1. **Missing `check: fn` / `row: fn` from the P2 table:** Live Jet has **two** check-role mechanisms: (a) `expand -[]>` (declaration-time read-only lint, `ReadOnlyLint.jet`), and (b) `check: fn(self, call: CompilerCall)` + optional `row: fn` (call-site rule, `Declarations/Markers.jet:154-218`, used by `Core/HTTP/Server.jet`, `Core/App/App.jet`, `Core/Service/Service.jet`). Both belong to the **`check`** rung (read-only, emits registered diagnostics, cannot generate or alter code).
  2. **Rewrite rung mismatch (`expand -[Meta.Rewrite]>` vs `Compiler.rewrite_body`):** Proposal P2 lists `rewrite` as `expand -[Meta.Rewrite]>`, whereas live Jet (`UserTemplates.jet:514, 632-635`, `Calls/Build.jet:688, 777-821`) forbids `Meta.Code.Rewrite` on `Marker.expand` (E0774) and restricts body rewriting to `build: fn`'s `meta.rewrite_body(f, #code { ... })` gated by `Meta.Code.Rewrite` (expert, E0777) + `meta.allow_rewrite(package:)` (E3514). Either `Marker.expand` stays read/generate-only while `rewrite` stays in `build: fn` (documented accurately in the ladder), OR `expand -[Meta.Code.Rewrite]>` is explicitly added and gated by both E0777 and E3514.
  3. **Per-dependency role denial does NOT exist today:** `Loader.jet:562-580` parses `authority.deny` ONLY as a root-wide effect list `[Effect]`, and `note`, `check`, and `wrap` carry **no `Meta.*` effect**. A root package today has **zero way** to deny a dependency from using `wrap`, `check`, `note`, or `generate` without banning `Meta.Generate` for the entire build!
  4. **Transitive dependency hole:** Suppose root depends on `pkg_a`, which uses a `#Wrap` or `#Generate` marker defined in `pkg_b` (or re-exported by `pkg_a`). Whose package identity is checked by per-dependency denial? **Rule:** Check the **declaring package** of the marker (the package that authored the `expand`/`check`/`wrap` code running inside the build or around calls), AND if a marker group re-exports a marker from `pkg_b`, expanding the group checks each member's declaring package so a dependency cannot launder a denied marker through a group! Denial MUST always win over grants and emit a registered diagnostic naming the denied package, marker, and role (Ballot Q3).

### Attack 5: `jet expand --lens source` Completeness, CLI Surface, and Determinism
- **Claim attacked:** "`jet expand --lens source FILE` prints the file as the compiler checks it, with every marker's output inlined and labeled with its marker" (P2 lines 253-255, 269-272).
- **Attacks:**
  1. **Flag & default mismatch:** Today `Expand.jet:454-465` parses `--facts <lens>` (not `--lens <lens>`), and bare `jet expand FILE` returns `not_ported_output` (`Inspect.jet:2116`). Users following P2 (`jet expand FILE`, `jet expand --lens source FILE`, `jet expand --marker Name FILE`) would hit errors unless `--lens` is supported alongside `--facts` and bare `jet expand FILE` defaults to `source` (Ballot Q4).
  2. **Five distinct generation/transformation producers must feed one source projection:**
     - Compiler derives (`#Codable`, `#Equatable`, `#Comparable`, `#Debug`, `#Printable`, `#Patchable`, `#CLI`, distinct forwards — note `#Codable` bodies are generated on demand in `Registration/SerdeItems.jet:2158-2167`!)
     - User marker `expand` templates & `#loop` expansions (`Registration/UserTemplates.jet`, `Derives.jet`)
     - `wrap` marker body rewrites (`Marker{...}.wrap(fn() { ... })`)
     - Build-script additions & rewrites (`meta.add`, `meta.add_struct`, `meta.rewrite_body`)
     - Named marker group expansions (`#[Group]` -> member markers)
  3. **Syntactic validity & formatting determinism:** Naive string concatenation produces ugly, non-parseable, or non-deterministic output. The `source` lens MUST format generated AST items/bodies through `JetFormatter` (`Compiler/JetParser/Source/Formatter/`), insert deterministic provenance comments (`// generated by #Name (file:line)`, `// wrapped by #Name (file:line)`, `// rewritten by <pkg> (build.jet:line)`), and produce valid Jet source whose line numbers match the debug artifact 1-to-1!

### Attack 6: `jet debug` Stepping Through Expanded Source Without Breaking Normal Debugging
- **Claim attacked:** "The debugger steps through that text" (P2 lines 254-255, 271).
- **Attack:** Look at `Dwarf.jet:32-56` and `NativeFacts.jet:169-226`:
  - Every function in `.debug_line` and `.debug_jetline` stores `(file, decl_line, line_rows)`.
  - `.debug_jetsource` stores `(path, sha256)` for every source file so `jet debug` verifies the on-disk file matches the compiled binary before binding breakpoints or printing source listings.
  - **The line-shift hazard:** If the compiler remapped `main.jet`'s entire line table to the expanded text of `main.jet`, then adding a `#Codable` struct at line 5 would shift every function below line 5 by +40 lines — breaking `break main.jet:25`, breaking editor breakpoints, breaking crash stack traces, and breaking `.debug_jetsource` SHA-256 verification against `main.jet`!
- **Resolution (Dual-Source Companion Model — Ballot Q5):**
  1. **Hand-written user functions/statements** keep their exact `(main.jet, original_line)` coordinates in `.debug_line`, `.debug_jetline`, and `.debug_jetsource`. Zero line shift!
  2. **Generated items, derived impls, wrapped bodies, and rewritten functions** receive exact line coordinates inside a deterministic companion file written during debug builds (e.g. `.jet/expanded/<relpath>.expanded.jet`), generated by the **exact same `source` lens renderer** as `jet expand --lens source`, and recorded in `.debug_jetsource` with its SHA-256!
  3. For wrapped functions (`#Retry(times: 3) fetch_rates`), the generated wrapper thunk maps to the expanded companion (`// wrapped by #Retry`), while the original inner body statements keep their exact `(main.jet, original_line)` rows (or step seamlessly from the wrapper line in `.expanded.jet` into `main.jet`'s original body lines!).
  4. `jet debug` (`Native.jet`, `NativeListing.jet`, `NativeFacts.jet`) already resolves files by `X64JetlineFunction.file` and verifies `.debug_jetsource` — so once the companion file is written to `.jet/expanded/` and hashed into `.debug_jetsource`, `jet debug` lists, breaks on, and steps through expanded code automatically with zero special-case hacks!

### Attack 7: Removing `policy` and `wrap` Keywords Cleanly
- **Attack:** `Entry.jet:1701` (`parser_user_policy_ahead`) currently hijacks ANY top-level statement beginning with identifier `policy` (`token.kind == TOKEN_IDENT && value == "policy"`), even before checking for a parameter list! Meanwhile `policy` and `wrap` appear in 4 editor grammars, 2 core-surface ledgers, `SyntaxCodec`, `Formatter`, and 8 repo declarations.
- **Resolution:** Remove `UserPolicyDecl`, `parser_user_policy_ahead`, `parser_parse_user_policy`, `jfmt_fmt_user_policy_decl`, `SyntaxCodec` `userpolicydecl` rows, `CoreSurface.jet:479`, `SyntaxRegistry.jet:71`, ledger rows, and `policy`/`wrap` from all 4 editor grammars in one atomic cutover, migrating all 8 repo `policy` declarations (and `BurnDownFixtures.jet:65`) to `struct { ..., impl Marker { sites: [.Function], wrap: ... } }`. If old `policy name(...) { wrap(call) { ... } }` source appears, emit a clean migration diagnostic pointing to `impl Marker { wrap: ... }` (or ordinary syntax error once retired).

---

## 5. Owner Ballots Required (`NEEDS BALLOT`)

### Q1 — `wrap` Placement, Signature Syntax, and Marker-Level `Rewrite`
1. **Placement & signature:**
   - **Option A (Recommended):** Declare `wrap` **inside `impl Marker { ... }`** (matching live `expand`, `check`, and `row` in `Declarations/Markers.jet:122-218`), using Jet's live `via` effect polymorphism:
     ```jet
     Retry: struct {
         times: Int,
         impl Marker {
             sites: [.Function, .Block],
             wrap: fn<R, F: Describe>(self, call: fn() -[via call]> R F!) -[via call, Time]> R F! { ... }
         }
     }
     ```
     (General wrappers like `Trace`/`Audit` omit the `Describe` bound on `F` so `F` unifies with `Never` for infallible functions.) Keep `Meta.Code.Rewrite` strictly on `build: fn`'s `Compiler.rewrite_body` (documented as rung 5 of the ladder in `metaprogramming.md`).
   - **Option B:** Allow `wrap` directly as a struct method outside `impl Marker` (matching proposal §3.11 syntax verbatim), and/or also allow `expand -[Meta.Code.Rewrite]>` directly on markers (requiring both E0777 and E3514).

### Q2 — Body-Wrap Lowering & Retiring Bare `apply(f)` Unwrapping
- **Option A (Recommended):** Lower `#WrapMarker` directly inside the target function's body (fixing self-invocation bypass, joining wrapper effects into the target signature, and deleting `TFnValueKind.Policy` / `BodyFnValueKind.Policy`). Keep `apply(Name(args), ... , f)` for wrapping any function value of any arity via an ordinary closure, and **retire bare `apply(f)` unwrapping** (callers must not silently strip `#Audit`/`#Auth` wrappers from a function).
- **Option B:** Synthesize a separate unwrapped inner symbol so bare `apply(f)` continues to return the unwrapped target function (preserving `Examples/Basics/Functions/CallablePolicies.jet:18, 20` verbatim, at the cost of allowing any caller to bypass a function's wrap markers).

### Q3 — Per-Dependency Marker-Role Denial Syntax in `package.jet`
How does a root package deny any of the 5 roles (`note`, `check`, `generate`, `wrap`, `rewrite`) per dependency?
- **Option A (Recommended):** Add a per-dependency role denial map under `authority:` (e.g. `authority: { deny_roles: { "dep": [.Wrap, .Generate] } }` or `markers: { deny: { "dep": [.Wrap, .Generate] } }`) using an explicit closed `MarkerRole` enum (`Note`, `Check`, `Generate`, `Wrap`, `Rewrite`, `All`). Checked at every marker application/expansion/wrap/rewrite site against the marker's **declaring package** (including members of expanded marker groups); deny always wins over grants and emits a dedicated registered diagnostic.
- **Option B:** Overload `authority.deny` in `package.jet` to accept either a list `[Effect]` or a map `{ "dep": [...] }`, minting synthetic `Meta.Note`, `Meta.Check`, `Meta.Wrap` effect names alongside `Meta.Generate` and `Meta.Code.Rewrite`. (Mixes runtime/compile-time marker roles into the machine effect tree.)

### Q4 — `jet expand` CLI Surface (`--lens source`, `--facts`, Default Behavior, `--marker`)
- **Option A (Recommended):** Add `--lens <name>` (keeping `--facts <name>` as a compatible alias for all 10 existing lenses), add `source` to `JET_CLI_EXPAND_LENSES`, make bare `jet expand FILE` default to `--lens source` (replacing `not_ported_output` at `Inspect.jet:2116`), and support `--marker <Name>` to filter expanded output to one marker (matching §3.11 line 1934).
- **Option B:** Require explicit `--lens source` (or `--facts source`) and keep bare `jet expand FILE` returning an error demanding an explicit lens name.

### Q5 — `jet debug` Expanded-Source Coordinate Model
- **Option A (Recommended):** **Dual-source model** — user-written functions and statements keep their exact `(file.jet, original_line)` rows in `.debug_line` / `.debug_jetline` / `.debug_jetsource` (zero line-shift on user breakpoints and stack traces), while generated items, derived impls, wrapped thunks, and rewritten bodies emit line rows pointing to a deterministic `.jet/expanded/<relpath>.expanded.jet` companion written by the `source` lens renderer and hashed into `.debug_jetsource`.
- **Option B:** Single expanded-file model — rewrite `.debug_line` / `.debug_jetline` for the entire compilation unit to point at `.jet/expanded/<relpath>.expanded.jet` (one unified line coordinate space, but shifts every hand-written line number in breakpoints and stack traces).

---

## 6. Binding Cutover List (Pending Ballots Q1–Q5)

| Slice | Target Files | Exact Changes |
| :--- | :--- | :--- |
| **c1 (`wrap` Marker Role & Sema/IR Lowering)** | `Compiler/JetFoundation/Source/AST/Items.jet`, `Compiler/JetParser/Source/Parser/Declarations/Markers.jet`, `Compiler/JetParser/Source/Formatter/Items.jet`, `Compiler/JetSema/Source/Sema/CallablePolicies.jet`, `Compiler/JetSema/Source/Sema/Registration/`, `Compiler/JetSema/Source/Sema/Calls/`, `Compiler/JetIR/Source/BuildRoutes.jet` | 1. Parse and format `wrap` on `MarkerDecl` per Q1.<br>2. Verify `R F!` generic failure polymorphism unifies with `Never` (infallible) and concrete error types, and verify `via` effect propagation joins wrapper effects into the target's effect row.<br>3. Lower `#WrapMarker` on functions (and blocks via `#7011` c4) as body-wrapping `Marker{...}.wrap(fn() { ... })`, rejecting repeatable multi-call wrapping of `^^` move-consuming bodies.<br>4. Lower `apply(Name(args), ..., f)` to an ordinary forwarding closure for any arity; handle bare `apply(f)` per Q2; delete `TFnValueKind.Policy` / `BodyFnValueKind.Policy` (`BuildRoutes.jet:2154-2200`) and the `sema_policy_builtin` skip list (`CallablePolicies.jet:5-7`). |
| **c2 (5-Role Ladder & Per-Dependency Role Denial)** | `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`, `Compiler/JetDriver/Source/Driver/Loader.jet`, `Compiler/JetDriver/Source/Driver/Identity.jet`, `Compiler/JetDriver/Source/Driver/Pipeline.jet`, `Compiler/JetSema/Source/Sema/MarkerModules.jet`, `Compiler/JetSema/Source/Sema/MarkerRules.jet`, `Compiler/JetSema/Source/Sema/Registration/UserTemplates.jet`, `Jetpack/PackageModel/` | 1. Classify every marker into its exact highest role in the 5-role ladder (`Note` < `Check` < `Generate` < `Wrap` < `Rewrite`), explicitly covering both `expand -[]>` and `check: fn`/`row: fn` under `Check`.<br>2. Parse and propagate per-dependency role denial from `package.jet` per Q3.<br>3. Enforce per-dependency role denial at marker use/expansion/wrap/rewrite sites against the marker's declaring package (including group members), with deny always beating grants and emitting a registered diagnostic. |
| **c3 (`policy`/`wrap` Grammar Deletion & Repo Migration)** | `Compiler/JetFoundation/Source/AST/Items.jet`, `Facts/SyntaxCodec.jet`, `Data/Syntax/CoreSurface.jet`, `Compiler/JetParser/Source/Parser/{Entry,Items}.jet`, `Formatter/Items.jet`, `Jetpack/Foundation/Source/SyntaxRegistry.jet`, `Tests/Guards/core_surface_ledger.jet`, `Tools/Agent/CoreSurfaceLedger.jet`, `Tools/Editors/{TreeSitter/grammar.js,Vscode/Syntaxes/jet.tmLanguage.json,jet.tmGrammar,Zed/languages/jet/highlights.scm}`, `Examples/Basics/Functions/CallablePolicies.jet`, `Docs/Spec/Reference/syntax-surface.jet`, `Tests/Cases/UserPolicies/`, `Tests/UI/user_policy_*.jet`, `Compiler/JetSema/Tests/BurnDownFixtures.fixture/BurnDownFixtures.jet` | 1. Delete `UserPolicyDecl`, `parser_user_policy_ahead`, `parser_parse_user_policy`, `jfmt_fmt_user_policy_decl`, and `SyntaxCodec` rows.<br>2. Remove `policy`/`wrap` from editor grammars and core-surface ledgers.<br>3. Migrate all 8 repo `policy` declarations + 1 inline fixture to `impl Marker { wrap: ... }` and verify O0/O1/O2 parity. |
| **c4 (`jet expand --lens source` & `jet debug` Expanded Stepping)** | `Compiler/JetCLI/Source/CLI/{Expand.jet,Inspect.jet,InspectB.jet}`, `Compiler/JetDriver/Source/Driver/Pipeline.jet`, `Compiler/JetBackend/Source/Image/{Dwarf.jet,Jetline.jet}`, `Compiler/JetDebug/Source/{Native.jet,NativeFacts.jet,NativeListing.jet}` | 1. Implement deterministic formatted `source` lens rendering derives (`#Codable`, `#Equatable`, `#Comparable`, `#Debug`, `#Printable`, `#Patchable`, `#CLI`, distinct forwards), user marker templates/`#loop`, `wrap` expansions, and build `meta.add`/`meta.rewrite_body` outputs with provenance comments (`// generated by #Name (file:line)`).<br>2. Wire `--lens` (aliasing `--facts`), default lens per Q4, `--marker` filter, and `--json` provenance records.<br>3. Implement Q5's debug coordinate model (`.jet/expanded/<relpath>.expanded.jet` hashed in `.debug_jetsource`) and add end-to-end `jet debug` stepping case tests across derived, templated, and wrapped functions. |
| **c5 (Docs, Spec, Examples, Goldens)** | `Docs/Spec/metaprogramming.md`, `Docs/Spec/spec.md`, `Docs/Spec/Reference/`, `Examples/` | Document the unified 5-role ladder, `wrap` marker contract, per-dependency denial, `jet expand --lens source`, and `jet debug` expanded stepping; update example `.out` goldens across O0/O1/O2. |
