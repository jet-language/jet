# Jet decision ballot — owner responses (open items only)

Your ratified answers have been incorporated into `docs/spec/syntax-decisions.md`
and `docs/plans/epoch-2/` and removed from here — this file now holds **only the
decisions still waiting on you**. The briefing (examples, comparisons, the
attribute-shape syntax mockup) is in `docs/spec/decision-ballots.md`; the section
numbers below (§0–§11) point into it. Write your answer under each item.

## §0 — Attributes shape (ATTR-SHAPE / D-LL2 / D-JSON1)
- **Decision:** (pending — α or β? confirm the two-shape rule)
- **Your earlier notes:**
  - *ATTR-SHAPE:* "let's do `#[attribute(s)]` so we can support a list of attributes instead of just one. Then we can use a block for scoped effects"
  - *D-LL2:* "include the list of attributes in a `#[...]` - rust style, for a list or a single attribute; allow scoping with blocks, i.e. `async {... async code ...}`. Before locking in show me what you think I mean for the syntax so we are clear."
  - *D-JSON1:* "treat 'serialize' as an attribute, like transact — a `#[Serialize]` block right before `struct Profile {...}`; explicitly defined automatically with a single word but overridable."

## §1 — D-ERR2: name the concrete error carrier
- **Decision:** option 1 ratified (the capability is the `Error` trait). **Carrier name still pending** — pick from the menu in §1 (lean `Fault`).

## §2 — D-DEV2: JIT / Cranelift
- **Decision:** (pending — open the Epoch-3 JIT-runtime-type-server design doc?)
- **Your note:** "Give me more information - i dont know what cranelift is & I want a JIT runtime type server system so we can try replacing typescript/javascript with high performance safe apps" → answered in §2.

## §3 — D-DX5: external subcommands
- **Decision:** (pending — confirm A)
- **Your note:** "I don't understand, give me a more clear, slightly more verbose real world example. A plugin api sounds nice but i dont know what that means" → answered in §3.

## §4 — D-FP2: expression-body functions
- **Decision:** (pending — A / B / C)
- **Your note:** "The options are not clear here. Clear them up for me to decide" → clarified in §4.

## §5 — D-PAT5: multiple function bodies by pattern
- **Decision:** (pending — A decline / B accept)
- **Your note:** "Need better comparison between options & better explanation. Give me an example plus an example from a persona perspective for each case" → in §5.

## §6 — D-PURE1 & D-PURE2: pure eval + sandbox
- **Decision:** (pending — confirm A + A)
- **Your note:** "The visual presentation is unclear, the examples are too terse & not well explained. Separate D-PURE 1 & 2 then represent them to me" → done in §6.

## §7 — E2-V12: JetOS / pure eval / layer-3 boundary
- **Decision:** (pending — OK to retire as redundant?)
- **Your note:** "This makes no sense to me. Present it more clearly with examples." → in §7.

## §8 — D-TOOL4: snapshot testing
- **Decision:** (pending — A / B)
- **Your note:** "I need an actual example, I don't understand this" → in §8.

## §9 — D-CFFI2: finding C libraries
- **Decision:** (pending — confirm layered answer)
- **Your note:** "This seems like it could be messy if not using jetpack. What if a user doesnt have the lib already? Shouldn't it be pulled into the jet hangar?" → answered in §9.

## §10 — D-NET2: server concurrency model
- **Decision:** (pending — confirm A)

## §11 — D-REF3: inlay hints beyond clone
- **Decision:** (pending — confirm A)
