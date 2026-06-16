# Implementation kickoff — the Jet ecosystem

This is the canonical prompt for an implementing agent. "Everything" is **not**
one run: the repo protocol (docs/plans/README.md) is **one chunk per run, then
stop and report**, and most Epoch 2 milestones are gated on owner ballots. Paste
the block below to start; the agent does one chunk, commits `<chunk> verified`,
and stops. Paste it again (or say "continue") for the next chunk.

> **⚠ Live status, read first:** Chunks 0–5 and provider stages R0–R2 have
> **shipped** (two binaries; `payload.jet` manifest; typed `module {}` `env.jet`
> surface; `core`+`nix` providers; hangar store; `.jet/lock`; merge engine). The
> authoritative, current list of what is built vs. pending is
> [`active-task.md`](active-task.md) — consult it before picking up a chunk. The
> remaining live work is **R3 (tvix)** and the **still-open jetos surface**
> (Chunk 5 tail + Chunk 6). This file remains the chunk protocol + kickoff prompt.

**Owner gates to remember:** Chunk 0 recorded the S52 amendment (`jet.toml` →
`payload.jet` per U10, unified `.jet/lock`). Chunk 6 (Epoch 2 milestones)
requires the relevant ballot group in docs/spec/decision-ballots.md to be
ratified first — the agent will stop and ask when one is open.

---

```text
You are implementing the Jet ecosystem we have planned. Work strictly by the
repo's own protocol — one chunk per run, test-first, stop and report at the end.
Do NOT try to do everything in one run.

# Read first, in this order
1. CLAUDE.md (operating manual + invariants I1–I8)
2. docs/spec/philosophy.md, syntax-decisions.md, architecture.md, diagnostics.md
3. docs/plans/README.md  ← the implementing-agent protocol; follow it exactly
4. docs/plans/jetpack-jetos/unified-ecosystem.md  ← the owner-RATIFIED design-of-record
5. docs/plans/jetpack-jetos/README.md (sequencing, milestones, provider roadmap, jetos parity)
6. docs/plans/active-task.md (LIVE status — what is built vs. pending right now)
7. docs/spec/decision-ballots.md (what is still OPEN — never implement an open ID)

# Non-negotiable rules
- Invariants I1–I8. In particular: NO `unsafe` in generated code (golden-tested);
  rustc never speaks to users (I2); codegen is dumb, all checks in sema (I3);
  every diagnostic has a code + what/why/fix + a tests/ui snapshot (I4); zero new
  compiler crates without owner approval (I6).
- Syntax gate (docs/plans/README.md step 2): only implement syntax that is
  Ratified in docs/spec/syntax-decisions.md. The unified-ecosystem names
  (U1–U10) are now recorded there and enforced by tests/decisions.rs. If ANY
  decision a chunk needs is still open, STOP and report to the owner; do not
  pick an option.
- Test-first: write the failing ui fixture / example BEFORE the code. Build in
  pipeline order: src/syntax.rs → lexer → parser → sema → codegen. Never skip
  sema into codegen.
- Run everything through the Nix shell: `nix develop -c cargo build`,
  `nix develop -c cargo test`. Bless snapshots only when output matches
  diagnostics.md voice (`nix develop -c env UPDATE_EXPECT=1 cargo test`).
- DEAD CODE WARNING: unused fields/methods are usually scaffolding for a LATER
  chunk. Never delete a dead-code warning without verifying it isn't an intended
  unbuilt feature.
- Definition of done PER CHUNK: exit criteria pass as tests; cargo test fully
  green; new diagnostics have snapshots + a `jet explain` entry; new behavior has
  an example with expected output; docs/spec/spec.md, diagnostics.md, and
  roadmap.md updated; no invariant bent. Then commit `<chunk> verified` and STOP.
  Do not start the next chunk in the same run.

# Implementation sequence (one chunk per run, in order)
0. RECORD DECISIONS. Add the owner-ratified unified-ecosystem decisions to
   docs/spec/syntax-decisions.md and src/syntax.rs (keywords/sigils: `module`,
   leading `_` disable, `find`, namespaces `env`/`system`/`image`, types
   `Env`/`System`/`Image`/`Pkg`, `provider@target` refs) and the S52 amendment
   (`payload.jet` replaces `jet.toml` per U10; single `.jet/lock` replaces
   `jet.lock`; `.jet/` managed folder; `/etc/jet/hangar` store). Update
   tests/decisions.rs so the ratification test stays green. No behavior yet —
   registry + tests only.
1. JPK-0 foundation (jetpack README §3.5): jetpack entrypoint, command parser,
   source-ref classifier (`github@…`, `path@…`, `nixpkgs@…`), `.jet/` + hangar
   store roots. Unit-test the classifier.
2. Manifest reshape (U1/U2/U10): parse `payload.jet` (Jet-syntax package
   manifest: `payload:`/`deps:`/`packages:`) and the unified `.jet/lock`; migrate
   the jet.toml path.
3. Module surface: parse `module name {}` (+ `_`-disable) and top-level
   `sources:` / `imports: find("./modules")`; sema for the `env`/`system`/`image`
   namespaces with `Env`/`System`/`Image`/`Pkg`; the merge engine per
   unified-ecosystem.md §6; evaluation via the pure-eval/interpreter path.
4. `env.jet` dev environments end-to-end (`jetpack enter <name>`, `jet dev`
   shell) against the hangar store. Realistic example + transcript test.
5. `config.jet` / jetos scaffolding (`system`/`image` namespaces) — ONLY the
   parts whose decisions are ratified. STOP at anything gated on jetos D-OS
   options or layer-3 / pure-eval (E2-M16).
6. Then Epoch 2 milestones per docs/plans/epoch-2/ (E2-M2 … E2-M17), in the
   dependency order in that README — but for EACH, first confirm its ballot group
   (Part 2/3 of decision-ballots.md) is ratified; if it is open, STOP and report.

Start with Chunk 0. When it is green and committed, stop and tell the owner what
the next chunk is and whether it is unblocked.
```
