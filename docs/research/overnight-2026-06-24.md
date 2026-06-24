# Overnight autonomous run — 2026-06-24

Summary of the unattended burn-down for morning planning. Everything below is on
`origin/master`, every commit green (full-suite or flake-tolerant gate; see Env notes).

## Post-run sweep updates (later 2026-06-24 — supersede where noted)
- **CI reverted (owner directive):** the c113 subagent wrongly created an active
  `.github/workflows/ci.yml` that ran twice. CI must **not** be enabled until post-Epoch-3
  (no Epoch-3 work has actually started). Removed; **c113 → frozen** (the flake.nix version
  sync + truthfulness version-check stay). Supersedes the "c113 done" line below.
- **Tower planning sweep done:** every implementable card now has a plan. 8 plans written +
  agent-vetted (c51 testing-ergonomics, c77 three-mode-execution, c104 serde-model, c119
  split-compiler-modules, c120 beginner-expert-mode-audit, c121 perf-compiletime-dashboards,
  c122 package-ecosystem-trust, c123 flagship-vertical-slices). Board reconciled: **31 done,
  19 ready, 2 deciding, 13 frozen, 4 building/backlog**. Queues drained.
- **Two ballots drafted (ready for owner review):** **D-BENCH1** (c121 — benchmark-block
  syntax, rec `#Bench`) and **D-PKGSIGN1** (c122 — package signing, rec Ed25519). c121/c122 →
  **deciding**. The ballot is otherwise correctly drained (all other cards' decisions ratified).
- **c62** (qualifier system) is **ready**, not "needs ballot": its decisions (D-QUAL1/D-QUAL2)
  are ratified and it has a plan; effect-specific parts gate on D-EFF2/D-EFF3 (ratified).

## Done — memory/access-capability model (the headline work)

The core is implemented and shipping:
- **Phase 3** — `AccessConvention` → full `{Infer,Read,Write,Move,Share,Raw}` vocabulary.
- **Phase 2** — sigils `~T`/`^T`/`&T` parse at type, call, and receiver positions; `~` lexed; E0029.
- **Phase 4 (D-CAP8)** — capability inference: unmarked params resolve from body usage over the
  `Read<Share<Write<Move` lattice, deterministically, before checks/codegen. Marquee works:
  `fn heal(player: Player){ player.hp += amount }` infers `~Player`. Receiver-method mutation
  inference too (`p.damage(5)` on a `~self` method → `~`).
- **D-UNSAFE2** — `#Unsafe("reason")` takes the safety reason; `#Audit` retired → E0055.
- **Diagnostics** — E0120/E0201/E0202/E0205/E0631/E1102/E0208 re-voiced to capability language.
- **Determinism test**, **docs** (sigil spec + migration), 9 golden examples (90–98, +99–101).

### Memory-model — remaining ATTENDED implement-tasks (decided, NOT safe unattended)
Captured on the board (c124/c127/c129/c130/c131); precise scope there:
- **D-CAP9 (c127/c131)** — `*x`=raw-of, postfix `p.*` deref, `*T` replaces `Ptr<T>`. Not a
  deref-swap: a surface redesign of the unsafe-pointer tier (reconcile `mem.Ptr<T>`/`address_of`/
  `volatile_read`), with FFI/codegen implications. Decided (D), needs careful attended work.
- **Keyword retirement (c124, Phase 6)** — `mut`/`take`/`view` → teaching errors; formatter →
  sigils; migrate ~37 `.jet` files; preserve non-capability `take(…)` lambda captures; update
  decisions/grammar tests. High-churn; needs care.
- **c129** — freeze resolved capability sigs into `api=stable|explicit` metadata (no emission today).
- **c130** — `&T` share composes with `region{}`/arena/view; map escape errors to E0631/E0632/E1102.

## Done — Tower backlog (15 cards)

c84 (task.detach + E1106) · c85 (repr(C) verified) · c82 (`[T#N]` + JetShow array fix) ·
c93 (F32 precision math) · c98 (comptime Core calls + E0958) · c43 (FFI U32 tests) ·
c115 (warning-clean + `#![deny(warnings)]`) · c108 (backend-story docs verified) ·
c112 (ring-package docs) · c41 (Zed grammar sync) · c42 (`@test`→`#Test` docs) ·
c75 (embed_file verified) · c65 (fan-out — done by D-FANOUT2=B ratification) ·
c113 (CI workflow + flake version) · c114 (truthfulness gate test).

## Remaining backlog — morning planning queue (categorized)

**Implement-ready (ratified, no open decision — sizeable, do attended):**
- **c51** — testing ergonomics: property tests + doctests + coverage (D-TEST1/D-TEST4). Substantial.
- **c64** — reactive/dataflow (D-REACT1=B = *library*, not core). Verify whether B defers it to a
  library (possibly done-by-ratification like c65) or means build the lib.
- **c68** — units as a first-class tag (D-UNIT1). Verify ratified scope, then implement.
- **c77** — three-mode execution + JIT dev runtime / hot-reload (D-JIT1/HOTSWAP1/DEVMODE1). Big.
- **c78** — cache-friendly data layout / SOA (D-SOA2). Owner wanted a better name than "SOA" —
  confirm the slot name, then implement the `#layout(<name>)` member.
- **c94** — linear algebra + SIMD (D-MATHLIB1/D-SIMD1). Big.

**Gated (blocked on upstream, not startable):**
- **c104** — serde Serialize/Deserialize (D-SERDE1) — gated on user-defined derives (S56, Epoch 3).
- **c96** — M12.2 registry + `jet publish` UX — rides registry infra (c50/c56), D-PUBLISH1 deferred.
- **c26** — arena allocator compiler inference — far-horizon.

**Audit / refactor / tooling (no decision, but judgment-heavy — better attended):**
- **c111** — replace hand-rolled parsers where correctness matters. Big, correctness-sensitive.
- **c119** — split the largest compiler modules by responsibility (refactor).
- **c120** — beginner/expert mode separation audit.
- **c121** — performance + compile-time dashboards.
- **c122** — strengthen package/ecosystem trust.
- **c123** — ship flagship vertical slices per domain.
- **c52** — DAP step-through debugger + adoption docs.
- **cmqp3895z4j0** — back-fill open decisions to the v2 ballot schema.

**Likely needs a ballot (design/scope decision):**
- **c62** — qualifier system: traits vs attributes vs tags. Surface-design; draft a decision card
  before implementing.

## Environment notes (important for the next session)
- **Isolated git worktrees SIGBUS** in this sandbox (reproducible on bulk checkout, not disk-space) —
  so subagents ran **sequentially in the shared tree**, not in parallel. Parallelism returns if the
  worktree defect clears.
- **Full-suite OOM flake**: `cargo test` intermittently SIGKILL/SIGABRTs one random test binary under
  load (memory pressure from many concurrent rustc subprocesses); each binary passes in isolation.
  Validation gate used: **green = zero real assertion failures**; a SIGKILL/SIGABRT on an
  otherwise-passing binary is tolerated. The full suite is ~885 tests when it doesn't flake.
- **`#![deny(warnings)]`** now active (c115) — any new warning is a hard build error.
