---
name: project-memory-model-gate
description: Memory/access-capability redesign — D-CAP7/8/9/10 all ratified 2026-06-23; implementation unblocked; plan doc location
metadata:
  type: project
---

The access-capability redesign (`docs/prompt-memory-model-final.md`) replaces capability
keywords with prefix sigils `T`/`~T`/`^T`/`&T`/`*T`. **Gate FULLY RATIFIED 2026-06-23** — all
decisions logged in `syntax-decisions.md`, ballot cards stripped, board c124–c131 moved
`deciding → ready`. Implementation is unblocked:

- **D-CAP7** — sigil spelling frozen.
- **D-CAP8 = C** — unmarked `T` infers in bodies (elevates by usage), frozen into the public signature at an `api: explicit` boundary. Call-site sigil still required for inferred params; overgrant warns; inferred `&`-share freezes like `~`/`^`. Repoints today's fixed-read default + E0202/E0205.
- **D-CAP9 = D** — `*x` = raw-of (`#Unsafe`-only); dereference is now POSTFIX `p.*` (Jai precedent), retiring prefix `*p`; `*T` replaces `Ptr<T>` (deprecated alias); E0208 reworded to teach `p.*`. `~x`/`^x`/`&x` are free position-disambiguated prefixes. Amends S58 prose (`&x`=address-of never shipped).
- **D-CAP10 = A** — overloads out of scope (S14); single-fn call-site-sigil disambiguation, not overload resolution; perf flag dropped.

Implementation plan: `docs/research/memory-model-implementation-plan.md` — maps the prompt's
9 phases to real code (file:line), splits BUILD-NOW (non-gated) vs WAITS-ON-GATE (now all
unblocked). Capability spine already exists: `AccessConvention {Read,Mutate,Move}` at
`Source/AST.rs:7`, single producer `parse_access_prefix` (`Source/Parser/Expressions.rs:1569`,
unmarked→Read at :1631) — grow it into `AccessCapability {Infer,Read,Write,Move,Share,Raw}`.

**Implementation progress (all green + pushed to origin/master):** Phase 3 (AccessConvention →
`{Infer,Read,Write,Move,Share,Raw}`), Phase 2 (sigils `~T`/`^T`/`&T` parse at type/call/receiver
positions; `~`/Tilde lexed; E0029 two-markers), Phase 4 (D-CAP8 solver `Sema/Capability.rs` —
unmarked params parse as `Infer`, resolved before checks/codegen by a deterministic body scan
elevating over `Read<Share<Write<Move`; call args normalize `Infer`→`Read`). Marquee works:
`fn heal(player: Player){ player.hp += amount }` infers `~Player`. Board c125/c126 done, c124
building. **Remaining:** Phase 6 keyword migration (mut/take/view → teaching errors, formatter →
sigils); D-CAP9 (`*x` raw-of, postfix `p.*` deref, `*T` replaces `Ptr<T>`); richer Move/Share
inference (receiver-method mutation not yet detected); c129 api-freeze, c130 region-compose,
c131 raw reconcile; diagnostics re-voiced to capability language.

Sequencing: the memory-model build touches Parser/Sema/AST/TIR/diagnostics — the same shared
files as the stdlib burn-down. **Burn-down DONE 2026-06-23**: all 7 agents (c87/c88/c91/c97/
c105/c106-107/c116) integrated to local master, full suite green (876 tests). Three integration
bugs were found+fixed at merge (none catchable in isolated worktrees): c97 `CtValue::Ok/Err`
shadowed `Result` (→ResOk/ResErr); c87 term FFI prelude leaked `unsafe`/dangling refs (strip
wired to only 2/4 codegen exits + incomplete) ; example renumber shifted fuzz corpus onto a
regex FFI variant (skip bare-rustc link check for `extern crate jet_ffi_`). Nothing pushed yet
(awaiting owner go-ahead on push). Memory-model implementation starts now on this green base
as a coherent sequenced effort. See [[project-tower-pm-pipeline]] and
docs/research/memory-model-implementation-plan.md.
