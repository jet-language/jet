# c161 — Encode D-CTCODEGEN1=A as a standing architecture rule

**Status:** Ratified. Small: a doc task (new architecture rule) + a guard so future derives/build steps can't quietly reintroduce AST-injection. One conformance nuance to confirm with the owner (see Open Owner-Q).

## Goal

Make D-CTCODEGEN1 a permanent, written architecture rule (not a feature) so every future code-generation path — derives, comptime, any build step — is bound by it, and add a guard that fails if a path tries to slip generated constructs past the sema gatekeeper.

## Current state (verified, file:line)

**How `#[Codable]` works today — the conformance evidence:**

1. **Markers are validated in sema, at the user's site.** `validate_serde_items` runs in `Source/Sema/Bundle.rs:423` (D-SERDE), *after* the trait registry resolves field/variant types, and emits `E2407`–`E2412` (`Source/Sema/CheckerCoreLib.rs:2458-2587`) pinned at the user's `#[Codable]`/field/variant spans. So a malformed derive request is a real sema diagnostic at the trigger site — never a rustc error (I2 held).
2. **Codegen for the impls is mechanical.** The `impl user_Encode` / `impl user_Decode` blocks are emitted in `Source/Codegen/Items.rs:316-790`, walking the already-sema-approved fields/variants with zero decisions (I3/R1). They are codegen *output* (Rust), like any other emitted body.
3. **AST synthesis happens pre-gatekeeper.** `synthesize_impls` (`Source/Sema/Registration.rs:1561`, called at `:262`) builds `Func` AST nodes (delegation/trait defaults) and inserts them *before* `register_type_methods` / `register_impl_methods` run — so the synthesized nodes are registered and checked by sema exactly like hand-written methods. Injection is **before** the gatekeeper, never past it.

**Key finding — there is no source round-trip seam today.** Grep for `Lexer::lex` / `Parser` across `Source/` shows every caller is a pipeline **entry point** — `Source/lib.rs:204,469,556,600`, `Source/Loader.rs:431`, `Source/REPL.rs`, `Source/LSP/*`, `Source/CFFI.rs:565` (binding cache), `Source/Jetpack/ModuleEval/Source.rs:28` (env eval). **Nothing in `Source/Sema/` or `Source/Codegen/` re-lexes or re-parses a generated Jet source fragment mid-pipeline.**

So `#[Codable]` conforms to the rule's **guarantee** — generated constructs are sema-checked, errors surface at the user trigger site, nothing is injected past the gatekeeper (I3/R2/I2 all hold) — but it does **not** literally "emit a source fragment that re-enters lexer→parser→sema." It validates-in-sema, then dumb-emits Rust; the only synthesis (`synthesize_impls`) injects AST *pre*-gatekeeper. This nuance shapes both the rule text and the guard (Open Owner-Q below).

**Existing R-rules:** `docs/spec/architecture.md:54-114` lists `R1`–`R10`. `R1` (codegen dumb), `R2` (sema gatekeeper), `R5` (ICE policy) are the relevant siblings. The new rule slots as **R11**.

## Decision (ratified, verbatim)

D-CTCODEGEN1=A is a STANDING RULE, not a feature: every build-time code generation step (derives, comptime) emits a typed SOURCE fragment that re-enters lexer→parser→sema like hand-written code — no generation path may inject pre-parsed AST past the sema gatekeeper. Keeps I3 (codegen dumb), R2 (sema gatekeeper), I2 (rustc never speaks); errors in generated code surface as real sema diagnostics pinned to the user trigger site. The existing `#[Codable]` derive already follows this.

## Implementation (staged)

### 1. Proposed architecture-rule text (to insert in `docs/spec/architecture.md` after R10 — NOT edited here; for the owner/implementer)

> - **R11 — Generated code goes through the gatekeeper.** Every build-time
>   code-generation step (built-in derives like `#[Codable]`, user-authored
>   derives, comptime) produces only constructs that sema has checked before
>   codegen sees them. A generation path either (a) emits a typed source/AST
>   fragment that is registered and checked by sema *before* the codegen
>   gatekeeper — exactly like hand-written code — or (b) is validated entirely
>   in sema (diagnostics pinned at the user trigger site) and then lowered by a
>   dumb, decision-free emitter. **No path may inject pre-parsed AST past the
>   sema gatekeeper, and codegen never re-parses or re-checks generated code.**
>   This keeps R1 (codegen dumb), R2 (sema is the gatekeeper) and I2 (rustc
>   never speaks): an error in generated code is a real sema diagnostic at the
>   user's trigger site, never a rustc error. `#[Codable]` follows this —
>   markers are checked in sema (`validate_serde_items`, E2407–E2412) before its
>   impls are mechanically emitted.

(If the owner wants the stronger literal-mechanism wording — *all* generation must round-trip as Jet source text — see Open Owner-Q; that wording does not describe today's derive and would require re-architecture.)

### 2. The guard / test

The enforceable invariant is "codegen does not re-parse or re-check, and nothing reaches codegen unchecked." Two complementary guards:

- **Structural guard (cheap, durable):** a `tests/` check (or `compile_fail`-style source scan) asserting that no module under `Source/Codegen/` references `Lexer::lex` / `Parser::` — codegen must never re-enter the front end. Pair with an explicit allowlist of the legitimate `Lexer::lex` entry points (lib/Loader/REPL/LSP/CFFI/jetpack) so a *new* mid-pipeline re-lex in sema/codegen is a deliberate, reviewed change rather than a silent one. This is the closest thing to a mechanical "AST-injection can't sneak back in" tripwire.
- **Behavioral guard (proves the guarantee):** the existing `E2407`–`E2412` ui snapshots already prove a broken derive request surfaces as a sema diagnostic at the user site, not a rustc/ICE error. Add one focused regression: a deliberately-malformed `#[Codable]` type that asserts a sema diagnostic (E24xx) is produced — never exit 101 (ICE) or a rustc message — locking in I2 for the derive path. If a future derive is added, it must ship the same kind of "bad input → sema diagnostic at trigger site" test (state this as a checklist line in the rule's docs cross-ref).

No new diagnostic code is needed — this card defines a rule + guard, not a user-facing feature.

### 3. Docs cross-refs

- `docs/spec/architecture.md`: add R11 (text above).
- `docs/spec/diagnostics.md`: note (near E2407–E2412) that these are the R11 enforcement codes for `#[Codable]`.
- Cross-ref from the metaprogramming surface (D-METADEPTH1=A / c155, S56 reflection+derives) and D-CTMARKER1 (`comptime { }` block / `$splice`): any new generation path added there is bound by R11. When user-authored derives land (c155), their output must re-enter sema with diagnostics at the user trigger site — R11 is the contract.

## Sequencing/gates

None blocking. This is a doc + guard task on the current tree. It *constrains* the upcoming generation features — c155 (user derives / reflection) and c625/D-CTMARKER1 (`comptime { }`, `$`-splice) — so land R11 before or alongside those so they are built against the rule.

## Open Owner-Q

1. **Rule wording: guarantee vs. literal mechanism.** The verbatim says generation "emits a typed SOURCE fragment that re-enters lexer→parser→sema." Verified reality: `#[Codable]` does **not** round-trip as Jet source — it validates in sema, then dumb-emits Rust, and the only synthesis (`synthesize_impls`) injects AST *pre*-gatekeeper. Both satisfy the *guarantee* (nothing past the gatekeeper; errors as sema diagnostics). Should R11 be stated as that **guarantee** (recommended — matches the code, covers both derive styles, and is what "`#[Codable]` already follows this" is true of), or as the **strict mechanism** (all generation must literally re-lex Jet source)? The strict reading would force re-architecting the derive to emit Jet source instead of Rust impls and conflicts with pre-gatekeeper AST synthesis — flagging so the proposed R11 text above (guarantee-form) is confirmed, not assumed.
