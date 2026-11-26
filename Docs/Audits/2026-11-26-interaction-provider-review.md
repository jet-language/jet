# Design Review: Interaction Is a Provider (`Interaction` Trait and `#Context(interaction:)`)

- **Date:** 2026-11-26
- **Scope:** Proposal item P16 — interaction is a provider (`ask`, `choose`, `confirm`, `secret`, and `progress` call an installed handler: terminal, GUI, agent, or test script) across `Core/Term/`, task context (`#Context(interaction:)` / `context.interaction`), and `#CLI` guided input.
- **Verdict:** **PASS WITH CUTOVER CONDITIONS** — Jet already routes task-scoped policies and providers through `#Context` (`allocator`, `logger`, `on_stop`, `print`, `dir`, `env`), and already implements terminal prompts and progress updates in [`Core/Term/Term.jet`](../../Core/Term/Term.jet). However, today every interactive prompt (`readline`, `input_secret`, `confirm`, `choose`, `progress`) calls `jet.runtime.term` directly, leaving no way to install a test script, GUI dialog, or agent tool-call bridge without spawning a PTY or mocking file descriptors. Declaring one `Interaction` provider trait in `core.term` with `TerminalInteraction` as the default, carrying the active provider on the task context (`#Context(interaction:)` / `context.interaction`), routing all `core.term` prompt/progress functions and `#CLI` guided input through it, and deleting duplicate terminal-only implementations solves the problem cleanly with zero runtime ABI changes.

---

## 1. Current Inventory: Every Prompt, Progress, and Guided-Input Surface

### 1.1 Core Library (`Core/Term/Term.jet`, `Core/Args/Args.jet`, `Core/Builtins/Builtins.jet`)

| Surface | File & Lines | Symbol(s) | Current Behavior | Gap / Cutover Action |
| :--- | :--- | :--- | :--- | :--- |
| Line prompt | `Core/Term/Term.jet:228-232` | `readline(prompt)` | Calls `terminal_prompt(prompt, false)` + `terminal_line()` directly against stdio. No `ask` spelling exists. | Add `ask(prompt: String{""})`, move terminal logic into `TerminalInteraction.ask`, and route both `ask` and `readline` through `context.interaction.ask(prompt)`. |
| Secret prompt | `Core/Term/Term.jet:234-251` | `input_secret(prompt)` | Checks `terminal8_stdin_tty` / `terminal8_stdout_tty`, enters no-echo terminal mode, reads one line directly. No `secret` spelling exists. | Add `secret(prompt: String)`, move terminal logic into `TerminalInteraction.secret`, and route both `secret` and `input_secret` through `context.interaction.secret(prompt)`. |
| Confirmation prompt | `Core/Term/Term.jet:314-318` | `confirm(prompt)` | Writes `"{prompt} [y/N] "` and parses `terminal_line()` via `confirm_answer` directly. | Move terminal logic into `TerminalInteraction.confirm` and route `confirm` through `context.interaction.confirm(prompt)`. |
| Menu selection | `Core/Term/Term.jet:320-335` | `choose(prompt, options)` | Writes `choose_menu` and retries up to 10 invalid answers on `terminal_line()` directly. | Move terminal logic into `TerminalInteraction.choose` and route `choose` through `context.interaction.choose(prompt, options)`. |
| Progress update | `Core/Term/Term.jet:441-456` | `progress(label)` | Suppresses on `JET_MACHINE_OUTPUT=1`, writes stdout line when stderr is not a TTY, or `\r{label}` to stderr when TTY. | Move terminal logic into `TerminalInteraction.progress` and route `progress` through `context.interaction.progress(label)`. |
| Iterator progress adapter | `Core/Term/Term.jet:437-439`, `Compiler/JetRuntime/Source/Iter.jet:1402-1425`, `ProgressState.jet:1-276` | `__progress_items` (`io.progress(items, desc, tpl)`) | Pure `-[IO]>` runtime iterator wrapper that renders formatted bars to TTY stderr only (silent on pipes). | Retain as the lazy iterator progress adapter (different effect contract `-[IO]>` and TTY-only silence invariant preserved by `Examples/Systems/Terminal/Progress.jet`). |
| Guided CLI prompt seam | `Core/Term/Term.jet` (new), `Core/Args/Args.jet:977-991` | `prompt_guided(name, help, choices)` | No guided-input prompt seam exists in `Core/Term` or `Core/Args`; missing required CLI inputs always fail immediately. | Add `core.term.prompt_guided` delegating to `context.interaction` (`choose` when choices are fixed, else `ask`) whenever a custom `Interaction` provider is installed or on an interactive TTY; return `Ok(None)` on non-TTY default terminals. |
| Core builtin role | `Core/Builtins/Builtins.jet:387` | `BuiltinRole.ContextPrint` | Declares `ContextPrint` for `context.print`, no role for `context.interaction`. | Add `BuiltinRole.ContextInteraction` filled by `priv __interaction_provider: fn() -> Interaction Never!` in `Core/Term/Term.jet`. |

### 1.2 Compiler & Tooling (`Compiler/JetSema/`, `Compiler/JetFoundation/`, `Compiler/JetCLI/`, `Jetpack/`)

| Surface | File & Lines | Symbol(s) | Current Behavior | Gap / Cutover Action |
| :--- | :--- | :--- | :--- | :--- |
| `#CLI` required-field resolution | `Compiler/JetSema/Source/Sema/Registration/CLIEntry.jet:753-785` | `sema_cli_entry_direct` | Collects missing required positional arguments (`__missing`) and required options (`__v{slot} == None`) and immediately fails with usage. Guided interactive input is not yet wired. | Route missing required non-flag fields (when `!__asked`) through `__jet_cli_term.prompt_guided(...)` before collecting `__missing` / failing, so custom `Interaction` providers (scripted, GUI, agent) and interactive terminals fill missing fields through `context.interaction` while non-TTY default runs keep exact exit-2 refusals. |
| Context field registry | `Compiler/JetSema/Source/Sema/ContextKeys.jet:42,58,71-80,112-121` | `SEMA_CONTEXT_RESERVED_NAMES`, `SEMA_CONTEXT_CORE_FIELDS`, `sema_context_field_module`, `sema_context_field_key` | Carries `allocator`, `logger`, `on_stop`, `print`, `trace`, `dir`, `env`. | Register `interaction` carried by `core.term` via `ContextInteraction` and `CONTEXT_INTERACTION_KEY`. |
| Context key constant | `Compiler/JetFoundation/Source/Facts/Body.jet:320` | `CONTEXT_PRINT_KEY` | Defines fixed key strings for Core-carried context fields. | Add `CONTEXT_INTERACTION_KEY: "context:interaction"`. |
| Core effect declarations | `Compiler/JetFoundation/Source/CoreDeclarations/Calls.jet:455-458, 530-533` | `core_call_io_effect`, `core_call_time_wait_effect` | Lists `readline`, `input_secret`, `confirm`, `choose`, `progress` under `core.term` with `IO` + `Time.Wait`. | Add `ask`, `secret`, `prompt_guided` to `core.term`'s `Time.Wait` row (`IO` already covers all non-`style_force` `core.term` exports). |
| Replay capture sensitivity | `Compiler/JetCLI/Source/CLI/ProveReplay.jet:122` | `JET_CLI_REPLAY_TERM_READS` | Lists `readline` and `read_all_input`. | Add `ask` alongside `readline`. |
| Compiler & Jetpack host CLIs | `Compiler/JetCLI/Source/CLI/{Budget,EffectNotice,RemoteRun,Picker}.jet`, `Jetpack/Foundation/Source/Terminal.jet`, `Jetpack/CLI/Source/Commands/UpdateCommand.jet` | Compiler/Jetpack internal CLI prompts and TUI picker | Host compiler/Jetpack interactive controls and raw-mode TUI picker governed by [`tui-interaction.md`](../Spec/Reference/tui-interaction.md). | Outside user-program Core API; raw-mode TUI picker keeps direct terminal control per `tui-interaction.md`, while user programs and `#CLI` apps use `core.term.Interaction`. |

---

## 2. Adversarial Design Attacks & Resolutions

### Attack 1: Trait-Object Default Cannot Be a Comptime `#ContextKey` Constant
- **Attack:** Declaring `#ContextKey interaction: Interaction{TerminalInteraction{}}` fails at compile time (`Jet.ComptimeConstructNotSupported`: materializing a trait-object IR value in a module constant). Using an optional `#ContextKey interaction: Interaction?{None}` forces callers to box concrete providers manually (`Val(box(p))`) instead of passing a concrete struct directly to `#Context(interaction: my_provider)`.
- **Resolution:** Follow the exact Core-carried context-field pattern already established by `context.print` (`ContextPrint` -> `CONTEXT_PRINT_KEY`, `SemaContextDefault.Call`):
  - `Core/Term/Term.jet` declares `#Builtin.Role(.ContextInteraction) priv __interaction_provider: fn() -> Interaction Never! { TerminalInteraction{} }`.
  - `ContextKeys.jet` resolves `context.interaction` and `#Context(interaction: value)` to type `Interaction` with key `CONTEXT_INTERACTION_KEY` (`"context:interaction"`) and runtime default call `__interaction_provider()`.
  - Callers write `#Context(interaction: my_provider) { ... }` with any concrete struct implementing `Interaction`; `sema_expr_expected_owned` boxes it directly to `Interaction` with zero wrapper ceremony, and `ir_context_key_word` hashes `CONTEXT_INTERACTION_KEY` into a unique 62-bit runtime key word automatically.

### Attack 2: Stateful Providers Under Copy-on-Read `context.interaction` and Spawned Tasks
- **Attack:** `context.get` / `context.interaction` returns an owned copy of the value stored in the task's context chain, and `task { ... }` inherits the parent's context chain concurrently. If `Interaction` methods required `^self`, callers could not mutate a copied trait value across consecutive prompts or share a script cursor / transcript across spawned tasks.
- **Resolution:** Declare all five `Interaction` methods with an immutable `self` receiver (`fn(self, ...)`). Stateless providers like `TerminalInteraction` carry zero fields; stateful providers (scripted test runners, GUI event bridges, agent tool-call dispatchers) hold a `Shared<State>` cell (`guard_edit()` / `guard_read()`), making every prompt sequence deterministic, visible to the installing caller after `#Context` exits, and race-free across spawned child tasks.

### Attack 3: Effect Polymorphism Across Terminal, Scripted, GUI, and Agent Providers
- **Attack:** `TerminalInteraction` performs `-[IO, Time.Wait]>`, whereas a scripted test harness is pure (only locking `Shared<State>`), and an agent or GUI provider may perform `IO` and `Time.Wait` waiting for a tool-call response or dialog dismissal.
- **Resolution:** Declare `-[IO, Time.Wait]>` on all five `Interaction` trait methods (matching the existing `core.term` signatures of `readline`, `choose`, `confirm`, `input_secret`, and `progress`). Jet's sub-effect checking permits pure `impl Interaction` methods (such as deterministic in-memory scripted providers) as well as blocking `-[IO, Time.Wait]>` providers (terminal, IPC/tool-call agent bridges) with zero effect breakage for existing callers.

### Attack 4: `#CLI` Guided Input Must Not Break Non-TTY Refusals or CI Scripts
- **Attack:** Routing missing required `#CLI` fields into `context.interaction` unconditionally would hang or prompt on piped CI runs and break existing `#CLI` tests that expect exit code 2 and `"missing required argument..."` / `"missing required option..."` on stderr.
- **Resolution:** `core.term.prompt_guided(name, help, choices)` inspects the active provider via a type arm on `context.interaction`:
  - When `context.interaction` holds `TerminalInteraction`, prompt only when both stdin and stdout are TTYs (`rt.terminal8_stdin_tty() && rt.terminal8_stdout_tty()`); otherwise return `Ok(None)` so `#CLI` emits its exact non-TTY refusal error and exits 2.
  - When `context.interaction` holds any custom provider (scripted, GUI, or agent), prompt unconditionally through `context.interaction.choose(label, choices)` (when fixed choices exist) or `context.interaction.ask(label)` (otherwise), then pass the answer through `#CLI`'s existing type validation (`Int`, `Float`, `Bool`, `String`, `Path`).

### Attack 5: Eliminating Duplicate Terminal-Only Paths
- **Attack:** Adding `Interaction` beside existing `readline`, `input_secret`, `confirm`, `choose`, and `progress` bodies would leave two copies of terminal prompting logic to drift.
- **Resolution:** Move the single canonical implementation of terminal line reading, numbered choice menus, `[y/N]` confirmation, no-echo secret entry, and progress output into `TerminalInteraction`'s `impl Interaction`. Delete the standalone terminal bodies from `readline`, `input_secret`, `confirm`, `choose`, and `progress`; every top-level `core.term` prompt/progress function (`ask`, `readline`, `choose`, `confirm`, `secret`, `input_secret`, `progress`) becomes a direct one-line delegation to `context.interaction`.

---

## 3. Exact Cutover List (2-Stage `seed: s1` Rollout)

### Stage 1 (`seed: s1` — Audit, Core Builtin Role, `Core.rows`, Context Wiring, `Interaction` Trait & `TerminalInteraction`)
1. `Docs/Audits/2026-11-26-interaction-provider-review.md` — this design review.
2. `Core/Builtins/Builtins.jet` — add `BuiltinRole.ContextInteraction`.
3. `Compiler/Prelude/Core.rows` — export `Interaction`, `TerminalInteraction`, `ask`, `secret`, `prompt_guided` from `core.term`.
4. `Compiler/JetFoundation/Source/Facts/Body.jet` — add `CONTEXT_INTERACTION_KEY: "context:interaction"`.
5. `Compiler/JetFoundation/Source/CoreDeclarations/Calls.jet` — register `ask`, `secret`, `prompt_guided` in `core.term`'s `Time.Wait` effect table.
6. `Compiler/JetSema/Source/Sema/ContextKeys.jet` — wire `interaction` in `SEMA_CONTEXT_RESERVED_NAMES`, `SEMA_CONTEXT_CORE_FIELDS`, `sema_context_field_module`, and `sema_context_field_key`.
7. `Compiler/JetCLI/Source/CLI/ProveReplay.jet` — include `ask` in `JET_CLI_REPLAY_TERM_READS`.
8. `Core/Term/Term.jet` — declare `Interaction` trait, `TerminalInteraction` default implementation (deleting duplicate terminal-only bodies), `#Builtin.Role(.ContextInteraction) priv __interaction_provider`, `ask`, `secret`, and `prompt_guided` (delegating to `TerminalInteraction{}` during Stage 1 bootstrap).

### Stage 2 (Verified Live on Rebuilt S1 Compiler — Full Cutover, `#CLI` Guided Input, Tests, Example, Docs)
1. `Core/Term/Term.jet` — flip `ask`, `readline`, `choose`, `confirm`, `secret`, `input_secret`, `progress`, and `prompt_guided` to dispatch through `context.interaction`.
2. `Compiler/JetSema/Source/Sema/Registration/CLIEntry.jet` — route missing required non-flag `#CLI` fields (when `!__asked`) through `__jet_cli_term.prompt_guided` before missing-argument refusal.
3. Live tests covering:
   - Scripted `Interaction` provider driving `ask`, `readline`, `choose`, `confirm`, `secret`, `input_secret`, `progress`, nested `#Context(interaction:)` shadowing/restore, spawned `task { ... }` inheritance, and `#CLI` `args.decode<T>()` guided input.
   - Agent-runner `Interaction` provider turning every `ask`, `choose`, `confirm`, `secret`, and `progress` call (including `#CLI` guided input) into a structured JSON tool call and returning structured responses.
4. Executable example `Examples/Systems/Terminal/InteractionProvider.jet` + `.out` golden verified across `O0`, `O1`, and `O2`.
5. Documentation & editor grammar verification:
   - `Docs/Spec/spec.md` (`#Context` section for `context.interaction` / `#Context(interaction:)`).
   - `Docs/Spec/Reference/tui-interaction.md` and `Docs/Spec/Reference/core-library.md` (documenting `Interaction`, `TerminalInteraction`, `ask`, `choose`, `confirm`, `secret`, `progress`, and `prompt_guided`).
   - Run `editor_grammars_have_generated_sections` and `tree_sitter_parses_every_example_without_error_or_missing_nodes` guards.
