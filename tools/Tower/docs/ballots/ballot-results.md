# Owner ballot results

_submitted 2026-06-25 21:27 · **all ratified 2026-06-25**_

Every decision below is ratified into
[`syntax-decisions.md`](../../../../docs/spec/syntax-decisions.md) (decision-log
entries dated 2026-06-25), cards stripped from
[`decision-ballots.md`](decision-ballots.md), board cards advanced. Nothing is
pending — kept as the submission record only.

## Decisions (ratified)

- **D-DOTCTOR2 = A** — retire the dotless `T { }`; `T.{ … }` is the sole
  named-construction spelling, old form → E0320. Card c158 → ready.
- **D-METAREFLECT1 = B** — reflection read-API = one `T.reflect()` handle.
- **D-METADERIVE1 = A** — `derive Trait for T { … }` emits a source fragment
  that re-enters lexer→parser→sema; errors pin at the `#[…]` trigger. Card c155 → ready.
- **D-PLUGIN1 = B** — `target: plugin` = sandboxed WASM, safe by default.
- **D-WORKSPACE2 = A** — `workspace` / `workspace.jet`. Card c156 → ready.
- **D-DEP-WASM1 = A** — **wasmtime + Component Model** backs the D-PLUGIN1
  sandbox (reuses already-approved Cranelift; runtime-side only, I6 holds). This
  was the dependency *gate* for D-PLUGIN1, surfaced as its own ballot rather than
  left as prose. Card c81 → ready.

---

_No open decisions remain. `decision-ballots.md` shows zero open cards._
