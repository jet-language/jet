---
title: Lessons learned: peer-language lineage audit
---
# Lessons learned — peer-language lineage (2026-07-23)

Format: peer failure -> Jet risk -> guard. Board surveyed first (cards, decisions, ideas);
anything already tracked is cited as the guard, not re-proposed. Only the last section
names work that is genuinely not on the board.

## Systems family

- **C++ feature accretion** (5+ initialization spellings, N mechanisms per job, committee can never delete). Jet risk: the idea backlog is full of second spellings (increment ops, alt marker sigils, fan-out variants). Guard: I8 one-mechanism + I7 ratified-syntax registry; ideas stay ideas until balloted. Covered.
- **Rust compile times** (regretted publicly by core team; drove users to Go/Zig). Jet risk: same rustc backend today. Guard: tracked — #666 beat-cargo, #669 compile-speed bets, #676/#677 latency budgets, D-VERDICT-666-1 lens split. Covered.
- **Rust borrow-checker learnability wall** (biggest documented churn reason in Rust surveys). Jet risk: memory safety pricing out beginners. Guard: ratified memory model v5 (D-MEM1=A: bare/read, &/write, ^/take, second-class borrows) exists precisely for this. Covered.
- **Rust async coloring** (async/await split the ecosystem: tokio vs async-std, duplicate trait worlds; C# has the same two-color regret). Jet risk: adding an `async` marker later would fork every stdlib API. Guard today: structured concurrency (TaskGroup + parallel adapters, D-PARCAPTURE1=D) is the one mechanism and there are no async keywords in Syntax.rs. Watch item: any future async-marker proposal is an I8 violation requiring a ballot, not an ergonomic tweak.
- **Zig perpetual pre-1.0 breakage / D Phobos-Tango stdlib schism**. Jet risk: shipping users onto a surface with no stability promise, or letting expert opt-outs harden into a dialect. Guard: dialects covered by I8; stability promise NOT covered — see gaps below.
- **Jai closed development, no ecosystem**. Guard: E4 jetpack arc + registry cards. Covered.

## Managed family

- **Java null** (billion-dollar mistake) and **checked exceptions** (universally routed around). Jet risk: error-handling ergonomics driving users to swallow errors. Guard: typed values/optionals direction ratified; or_return ergonomics already an idea (b5wsdrk) awaiting ballot. Covered, pending that idea's ballot.
- **Go generics shipped 10 years late** (a decade of interface{} casts calcified into APIs). Lesson: core type-system capabilities must precede stdlib breadth or the stdlib fossilizes the workaround. Jet risk: corelib audit wave (#288-#307, #706-#720) racing ahead of unresolved surface questions (union/any type idea b4eclxq, Jai Any b86bdeq). Guard: partially — flag: settle the union/any-type decision before core.encoding/core.data APIs bake casts in. Ballot the existing idea rather than a new item.
- **Go module system arrived late** (GOPATH decade). Guard: package model ratified up front (#587, #609, #610, D-ECO-*). Covered.
- **Swift pre-ABI-stability churn** burned early adopters; **expression type-checker timeouts** became a meme. Jet risk: inference blowup on real code. Guard: latency budgets D-PERFBUDGET-COMPILE1=C + #677 (frozen, exists). Churn -> stability-law gap below.
- **.NET Framework->Core split-world migration**. Same stability-law gap.

## Functional family

- **Haskell LANGUAGE-pragma dialect explosion** (no two codebases speak the same Haskell). Jet risk: #549 compiler-extension plugin API (done) breeding dialects. Guard: I7 — plugins cannot mint user-typeable syntax without a decision ID. Covered; keep I7 enforcement in plugin-API review scope.
- **Haskell lazy-by-default space leaks**: wrong default for the beginner facet even when powerful. Guard: philosophy (safe useful defaults) + I8. Covered.
- **Scala implicit conversions** (readability collapse, then painfully removed in 3). Jet risk: invisible rewriting magic in the beginner facet. Guard: philosophy magic-with-audit stance + owner kill criteria. Covered.
- **Elm 0.19 kernel-code lockdown** (stranded experts, killed ecosystem trust overnight). Jet risk: beginner facet hollowing out expert control. Guard: dual-facet mission + mandatory expert pass in AGENTS.md. Covered — this is the strongest external validation of the dual-facet bet; do not compromise it for beginner polish.
- **OCaml multicore took 25 years** because the memory model came after the ecosystem. Guard: concurrency semantics land inside JIT/AOT parity law (#688, #727-#730) while the surface is young. Covered.

## Scripting family

- **Python 2->3**: breaking change without a compelling carrot or a real migration tool; decade lost. Success counter-pattern: Rust editions + go fix (opt-in epochs, auto-migrator ships WITH the break). Jet risk: real — see gaps below.
- **Python packaging fragmentation** (pip/poetry/conda/uv, 15 tools). Guard: jetpack as the one tool, E4 arc + I8. Covered.
- **npm left-pad + unpublish; PyPI typosquatting; postinstall attacks**. Guard: tracked across #6 (registry UX), #423 (live registry/delivery), #429 (reproducibility cert), #431 (advisory/SBOM/provenance), #434 (trust/compromise), #398 (build sandbox), D-JPK-SERVICEAUTH1. One suggestion, not a new card: make immutable-publish/yank-tombstone policy an explicit acceptance criterion on #423 or #431 when they unblock, so left-pad is impossible by construction.
- **PHP stdlib naming incoherence** (needle/haystack). Guard: corelib audit cards + surface-audit skill + pascal-case idea b0qwt5pc. Covered.
- **Perl TIMTOWTDI -> write-only code; Ruby monkey-patching**. Guard: I8 is the anti-TIMTOWTDI law; no open-class mechanism exists. Covered.

## Config/OS family

- **Nix: right model, hostile surface** — stringly-typed everything, error messages from the guts, flakes stuck experimental for years causing community schism. Jet risk: this IS Jet's chosen battlefield (jetos/jetpack). Guards: typed manifests (owner U13 typed-values-over-strings), I2/I4 diagnostics as products, and the ratification workflow prevents perma-experimental features. Covered by the whole E4/e7 arc.
- **NixOS module system: mkForce/priority mystery-meat merging**. Guard: #653 Config merge law + explain provenance, #470 option-priority explain. Covered — these two cards are the direct answer; prioritize accordingly.
- **YAML Norway problem / Helm-Ansible text-templating of structured data**. Guard: typed config direction ratified; Jet templates data, not text. Covered.
- **Bash as glue**: untyped word-splitting footguns at the OS boundary. Guard: jet run + typed core.os arc (#523-#525). Covered.

## Proof-oriented family

- **Ada/SPARK: right about safety, lost on ergonomics and tooling access**. Safety that costs approachability loses the market. Guard: philosophy.md is exactly this bet. Covered.
- **Idris/Dafny: upfront proof burden kills adoption; gradual verification wins**. Guard: #240 progressive proof and replay mode is precisely the gradual path. Covered.
- **Lean's success lesson** (tooling + stdlib as community product): docs/examples as first-class. Guard: I5 executable examples + #86 jetdoc (frozen — acceptable for now). Covered.

## Do-not-ballot (law already covers)

- Second syntax spellings / dialects / TIMTOWTDI: I8 + I7.
- unsafe leakage: I1 audited @Unsafe regions.
- rustc error passthrough: I2.
- Diagnostics quality: I4 + diagnostics spec.
- One package tool: I8 + E4 arc.
- Beginner-default hollowing / expert lockout: dual-facet mission + mandatory two-pass rule.
- Borrow-model learnability: D-MEM1 ratified; do not reopen.
- JIT/AOT feature drift: D-VERDICT-666-1 / D-VERDICT-687-1; do not reopen.

## Genuinely untracked gaps (candidates, no board duplicate found)

1. **Language stability & migration law** — the one lesson with no card, no decision, no idea on the board. Python 2->3, Swift churn, Zig no-promise, Scala 2->3, .NET split all failed here; Rust editions + go fix is the proven pattern (opt-in epochs, auto-migrator ships with every break, one compiler supports N epochs). Jet already does clean breaks freely (pack.jet->pkg.jet) — fine pre-users, fatal post-users. Recommend: one planning card now, ballot before the first external-user stability promise (natural anchor: bootstrap gate #217 or first registry publish #6). Decides: when the promise starts, epoch/edition mechanism yes/no, and the rule that every post-promise break ships its migrator.
2. **Toolchain telemetry/trust policy** — Go's telemetry episode and Audacity show even well-intentioned opt-out telemetry burns trust permanently. No card or decision mentions telemetry. Small single ballot, owner-gated, cheap to decide early: opt-in only vs none, and what jet build may phone home (registry queries included). Low urgency, high cost if decided implicitly by code later.

Nothing else qualified: every other candidate gap found during the sweep mapped to an existing card, ratified decision, invariant, or logged idea, and is cited above instead of duplicated.
