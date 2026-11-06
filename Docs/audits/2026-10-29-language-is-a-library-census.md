# "The Language Is A Library": census, 2026-10-29

Card #6128. Pillar 5 in Docs/spec/philosophy.md has two meanings. First, the
compiler is a small kernel and a public, versioned library that the CLI, the
language server, `fn build`, compile-time code and outside tools all call.
Second, every capability that can be a library is one. Core is ordinary Jet
that uses only mechanisms any package can use.

This census lists every place the code at dev `25f9dee58f` breaks the pillar.
It is a read-only audit; nothing here changes behavior. Module-private helpers
inside Core (`__` internals, #5978) are ordinary package privacy and are not
findings. A finding is a privilege a user package cannot get, or a second copy
of a language rule.

Each finding names its owner in the last column:

- `card Fnn` / `card Tnn` / `card Dnn` is a proposed fix card. Its JSON is in
  `$JET_SCRATCH_ROOT/lanes/Main/q/cards/6128/` under the same key; Main files it
  and the filed number replaces the key here.
- `ballot Bnn` is a proposed owner ballot, in the same folder.
- `#NNNN` is an existing card that already owns the finding.

Paths are repo-relative. Line numbers are at `25f9dee58f`.

## How Core is told apart

One predicate marks Core: a module identity that starts with `<corelib>/`
(`Compiler/JetSema/Source/Sema/Diagnostics.jet:20-22`,
`sema_module_is_core_source`). The driver has its own twin,
`jet_driver_unit_is_core` (`Compiler/JetDriver/Source/Driver/Loader.jet:2659`).
Core sources reach a compile as a separate list beside the package graph
(`JetDriverCompileRequest.core_sources`,
`Compiler/JetDriver/Source/Driver/Identity.jet:180-218`): the host embeds them
or reads them from the toolchain tree, never from a resolved package. Most
findings below are a branch on one of these two predicates, or on a Core
module name such as `"core.web"`.

## C1. Core-only privileges

| Key | Privilege | Evidence | Owner |
|---|---|---|---|
| F01 | **The compiler keeps a twin of Core's API.** Core members are typed and dispatched by compiler tables, not by their Core declarations. A package can reach the host only through C FFI. | `Compiler/Prelude/Core.jet:1-4` ("the one readable Core declaration twin", 2,582 lines) and `Compiler/Prelude/CoreCallExtensions.jet`; generated `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet:10` (`CORE_CALLS`, 1,998 rows); hand-written tables `Registry/CoreSignatures.jet:88`, `Registry/CorePlatformSignatures.jet:229`, `Registry/CoreHostSignatures.jet:22`, `Registry/CoreValueSignatures.jet` (1,046 lines). A native row wins over the Core declaration: `Compiler/JetSema/Source/Sema/Calls/Core.jet:264-270`. | card F01 |
| F02 | **Host leaves only named Core modules may call.** Sema refuses these calls unless the caller is a specific Core module. This is an intrinsic gate, not visibility: the leaves are public names in the twin. | `Sema/Calls/Core.jet:1143-1147` (`core.term` `__stdio_*`), `:1149-1154` (`core.mem.__loader_new`, only from `core.data`), `:1155-1160` (`__coerce_*`, only from `core.encoding.json`), `:1161-1166` (CSV leaves), `:1170` (`__core_encoding_stream`), `:1268-1272` (global `Shared` constructor, E0067 outside Core); `Sema/Calls/Data.jet:777`. | card F02 |
| F03 | **Sema has a hand-written checker per Core domain.** These rules live in the compiler, so a package with the same shape of API cannot get the same checks. | `Compiler/JetSema/Source/Sema/Calls/`: `Http.jet` (920 lines), `HttpRouting.jet` (1,167), `Services.jet` (785), `TaskGroups.jet` (1,116), `StreamHandles.jet` (571), `ProcessHandles.jet` (518), `TimeHandles.jet` (517), `UiHandles.jet` (515), `Hardware.jet` (468), `Data.jet` (1,564), `Geometry.jet` (308); `Static.jet:871` (`ExpiringSecret.new` with 3 arguments, Core only). | card F03 |
| F04 | **IR lowers Core members straight to runtime symbols.** The route tables name Core members and runtime functions; no package can add a route. | `Compiler/JetIR/Source/RouteHandle.jet` (1,035 lines), `RouteBuiltin.jet` (660), `RouteBrowser.jet` (509); runtime side `Compiler/JetRuntime/Source/RouteAFont.jet`, `RouteAGame.jet`, `RouteBrowser.jet`, `RouteTerm.jet`, `RouteWeb.jet`. | card F04 |
| F05 | **Language features call named Core helpers by file path.** String format selectors, `Dyn` decoding, precise math and `loop line in handle.lines()` lower to Core functions found by file and name. A user type cannot take part. | `Compiler/JetIR/Source/CoreReach.jet:35-66` (format to `Core/Text/Fmt.jet`), `:68-82` (`__decode_*` in `Core/Encoding/Encoding.jet`), `:90-104` (`Core/Math/Math.jet` helpers), `:117-138` (`read_line`, `__stdout_next_line`). | card F05 |
| F06 | **Optimizations only Core gets.** Unreached Core bodies are not lowered; other packages' bodies always are. Core types get runtime handle layouts by identity. | `Compiler/JetDriver/Source/Driver/Pipeline.jet:2643-2661` (`jet_driver_prunable_core_modules`); `Compiler/JetIR/Source/BuildTypes.jet:347`, `:494-504`, `:538`. | card F06 |
| F07 | **Constructs only Core may write.** Some record literals, annotations, private native types and protected type names are legal only in Core. Core-to-Core calls also resolve members that are not exported. | `Sema/Expressions/Constructors.jet:402` with `Compiler/JetFoundation/Source/Registry/CoreModules.jet:431`; `Sema/FailureLaw.jet:549-555` (`IOOperation`, `IOContext`, owner annotations); `Sema/Registration/Items.jet:3509-3510`; `Sema/Calls/Methods.jet:1345`; `Sema/TypeRelations.jet:888` (`core.email` `Limits`); `Sema/Calls/Core.jet:271` (`core_private_seam`). | card F07 |
| F08 | **Unsafe and secret rules Core skips.** Core reads secret bytes without `#Unsafe`; packages need it. Core may also wrap a generic formal in `ExpiringSecret`. | `Sema/Expressions/Places.jet:752-760` (any `core.*` caller skips E0510); `Sema/FailureLaw.jet:631`; `Sema/Calls/Static.jet:871`. | card F08 |
| F09 | **Diagnostics Core does not receive.** Core skips the marker vocabulary and site checks, the unbuilt-feature checks, budgets, typestate validation and a serde pass. | `Sema/MarkerModules.jet:439`, `:447` (E0927, E0355, unbuilt rules); `Sema/CheckProgram.jet:5005` (E2104 generators), `:5037` (unbuilt runtime uses), `:3189-3192` (typestate declarations); `Sema/Calls/Core.jet:218-224` (unbuilt Core call refused only for user code); `Sema/BudgetSpecs.jet:158-165`; `Sema/Typestate.jet:64`, `:70`; `Sema/Registration/SerdeItems.jet:1513`. | card F09 |
| F10 | **Target facts Core ignores.** Core's generic bodies are checked without the selected target's `no_os` fact; user bodies are checked with it. | `Sema/Scopes.jet:492-498`. | ballot B10 |
| F11 | **Type facts keyed by Core type names.** Copy, resource and typestate facts come from name lists, not from the types' declarations. | `Sema/Ownership/Copy.jet:298-307`, `:597`, `:697`; `Sema/Ownership/Crossings.jet:225`; `Sema/Typestate.jet:67-68` (`Task` `join` and `detach`). | card F11 |
| F12 | **Effects and credential sinks keyed by Core module names.** A package's function gets its effects from its signature; a Core call gets them from a name table. | `Sema/Taint.jet:240-288` (`sema_taint_core_effect`), `:290-300` (`sema_taint_credential_sink`). | card F12 |
| F13 | **The effect vocabulary is closed, and it holds domain effects.** A package cannot declare an effect. `GPU`, `Browser`, `DB` and `Secret` are kernel effects. | `Compiler/Prelude/Effects.jet:13-58` (`GPU` :30, `Browser` :53, `DB` :18-20, `Secret` :54). | #6137 (ballot C3 there) |
| F14 | **Core lexes and parses with rights packages lack.** Core is lexed with reserved identifiers allowed and parsed by its own job with owned members. | `Compiler/JetDriver/Source/Driver/MarkerGroups.jet:222`; `Compiler/JetLexer/Source/Lexer/Terminators.jet:558-560`; `Loader.jet:3489-3497`, `:3525`. | card F14 (amends #5978) |
| F15 | **Core ships with the compiler, not as a versioned package.** Core comes from the toolchain beside the package graph, is keyed by `<corelib>/`, and has a private `core.<area>.runtime` namespace (E1001) that packages cannot mirror. | `Identity.jet:180-218`; `Loader.jet:2659`, `:3616-3625`; `Sema/Diagnostics.jet:20-22`. | ballot B15 |
| F16 | **`core.build` is implemented inside the compiler.** Build and BuildPlan methods are dispatched by member-name strings into a private compiler service. | `Compiler/JetBuildSession/Source/Session.jet:1-4` ("Private compiler service linkage"); `Compiler/JetEval/Source/BuildServices.jet:1-3`; `Compiler/JetDriver/Source/Driver/BuildAdapter.jet:36-81`. | card F16 (relates to #6139) |

## C2. Tools that keep their own copy of a language rule

| Key | Tool | Copy | Evidence | Owner |
|---|---|---|---|---|
| T1 | Language server | Its own identifier lexing and keyword-context guesses over raw text. The two identifier rules disagree: one is ASCII-only, the other accepts any alphabetic character. | `Compiler/JetLSP/Source/LSP/Hover.jet:145-148`; `Navigation.jet:361-367`; `ReferenceResponses.jet:268-290`; `CompletionContext.jet:54-91` (binding start, keyword context, `Service{`/`Env{` byte scans). Keywords come from `jet_foundation.Syntax` (`Navigation.jet:11`), which is correct. | card T1 |
| T2 | Formatter | Calls the parser's AST, with no second parser. It does spell literals itself and recognizes `__jet_` names by prefix. | `Compiler/JetParser/Source/Formatter/Formatter.jet:342`, `:650-660`; `Formatter/Expressions.jet:2091-2134`. | card T2 |
| T3 | REPL | Classifies input and refuses features by substring, ahead of the parser. | `Compiler/JetRepl/Source/Input.jet:86-113`, `:116-118`. | card T3 |
| T4 | Debugger | Two copies of a watch-expression grammar, plus Rust/C type names and Rust name mangling left from the Rust backend. | `Compiler/JetDebug/Source/Dap03.jet:235-245`; `Debug/Snapshot.jet:154-192`; `Inferior.jet:531-566`. | card T4 |
| T5 | Editors | Tree-sitter has a hand-written grammar. Only its token block is generated (`Tools/agent/editor-grammars.mjs`). TextMate scopes and Zed queries are hand-written too. VS Code runs `jet self lsp` and calls the library. | `Tools/editors/tree-sitter/grammar.js:15-26` (generated), `:27-548` (hand-written); `Tools/editors/jet.tmGrammar`; `Tools/editors/vscode/syntaxes/jet.tmLanguage.json`; `Tools/editors/zed/languages/jet/*.scm`; `Tools/editors/vscode/extension.js:781`. | ballot T5 |
| T6 | `fn build` host | Keeps its own session, key and declaration contracts ("Rust v2 length-prefixed byte contract", "Rust runtime_bridge positional contract"). | `Compiler/JetBuildSession/Source/Keys.jet:1-5`; `Declarations.jet:1-2`; `BuildAdapter.jet:36-81`. | card F16 |
| T7 | Canvas / dev server | Finds functions and `use` lines by scanning text lines, and finds `#Unsafe` by substring. | `Compiler/JetDevserver/Source/Canvas/GraphJson.jet:108-119`; `Canvas/EditActions.jet:579-621`, `:782`. | card T7 |
| — | Tower lint | No copy found. `Tools/tower/app/lint.mjs` checks card hygiene only (rules listed in `Tools/tower/test/lint.test.mjs:13-14`). | — | none needed |

## C3. Domain stacks that should be libraries

| Key | Stack | Where it lives now | Owner |
|---|---|---|---|
| D1 | Web | `Core/Web/` (11 files, 4,408 lines); sema `Calls/Http.jet`, `Calls/HttpRouting.jet`; IR `RouteBrowser.jet`; runtime `RouteBrowser.jet`, `RouteWeb.jet`, `UiBrowserError.jet`; effect map `Sema/Taint.jet:260-267`, `:285`; kernel effect `Browser` (`Effects.jet:53`); the dev server `Compiler/JetDevserver/` (45 files, 25,776 lines) inside the compiler. | ballot D1 |
| D2 | Terminal UI | `Core/Tui/Tui.jet` (709 lines), `Core/Term/Term.jet` (665); the `#Live` marker in the syntax registry (`Compiler/JetFoundation/Data/Syntax/CoreSurface.jet:871-876`); runtime `RouteTerm.jet`, `UiCarriersTui.jet`, `TerminalInput.jet`. | ballot D2 |
| D3 | Graphical UI | `Core/Ui/` (703 lines); sema `Calls/UiHandles.jet`; 25 `Ui*.jet` runtime files; `#Unsafe`-gated transfer leaves (`Sema/Calls/Core.jet:1189-1190`). | ballot D3 |
| D4 | GPU compute | `Core/Compute/` (253 lines); kernel effect `GPU` (`Effects.jet:30`); `Sema/Taint.jet:270`; `Sema/ComputeValidation.jet` (240), `Sema/Kernel.jet` (163). | #5964 |
| D5 | Games and fonts | `Core/Game/Raylib.jet` (205 lines), `Core/Font/Font.jet` (168); `Sema/Taint.jet:277` (raylib is `GPU`); runtime `RouteAGame.jet` (291), `RouteAFont.jet` (190), `UiRaylibMetadata.jet`. | ballot D5 |
| D6 | Database and accounts | `Core/Db/` (2,808 lines), `Core/Auth/Auth.jet` (1,351); kernel effect `DB`; `Sema/Taint.jet:272`, `:283`. | ballot D6 |
| — | AI | No AI stack in Core or the compiler: no `core.ai`, LLM or provider module. The MCP protocol is in `Jetpack/Integrations/Source/Mcp/`, package tooling outside Core. The AI-out-of-Core direction (q2no5uew) already holds. | none needed |

## Order of work

F01 is the root. F02, F04, F05 and F12 fall out of it once Core declares its
own host leaves with a mechanism any package can use. F03, F07, F09 and F11
then move one domain at a time. The ballots (B10, B15, T5, D1-D3, D5, D6) need
an owner answer before their cards can be written.
