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

Sequencing: the memory-model build touches Parser/Sema/AST/TIR/diagnostics — the same shared
files as the in-flight stdlib burn-down (c87/c88/c91/c97/c105). Integrate those worktrees onto
a clean green master FIRST, then start the memory-model implementation on the stable base.
See [[project-tower-pm-pipeline]].
