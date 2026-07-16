# AGENTS.md — agent operating manual

Canonical manual for EVERY coding agent (Claude Code, Cursor, Codex, …).
`CLAUDE.md` is a symlink to this file — edit here, never fork per-tool copies.

You are building a dual-facet, memory-safe compiled language: magic
out-of-the-box for beginners, full expert control behind explicit opt-in.
The long-run goal is jack-of-all-trades, master-of-ALL — no reason to
reach for another language. The front end owns all semantics and every
error message; rustc is a hidden verifier/optimizer. A human **owner**
has final say on all user-facing syntax.

## Consult these files as needed

1. docs/spec/philosophy.md — ranked priorities; settles all arguments
2. docs/spec/syntax-decisions.md — what syntax you may use; never invent any
3. docs/spec/architecture.md — pipeline + rules R1–R12
4. docs/spec/diagnostics.md — error voice + format; snapshot-pinned

## Command environment

Run project commands through `scripts/agent/jet-env` every time. It uses the
cached nix-direnv environment when available and otherwise enters the Nix shell;
all agents still use the same Rust, C toolchain, Node, Jet wrapper, and repo
utilities:

```
scripts/agent/jet-env cargo build
scripts/agent/jet-env cargo test
scripts/agent/jet-env jet run examples/features/basics/hello.jet
scripts/agent/jet-env rg "pattern" docs Source crates tests
```

`direnv allow` enables the cache for interactive work. Default shell is the
fast core compiler shell. Use `scripts/agent/jet-env full <command>` only for
FFI, Canvas/browser, graphics, VM/image, or full verification work. Do not rely
on host-installed `cargo`, `rustc`, `jet`, `node`, or search tools unless you
are explicitly testing host-shell independence. Avoid parallel shell launches;
group dependent checks in one `jet-env sh -c '…'` invocation where practical.

Known traps:
- Shell launches can leave large `/tmp/nix-shell.*` dirs; the launcher clears
  exited ones before starting. A full `/tmp`
  causes phantom ENOSPC test failures. Check `df -h /tmp` before trusting
  a weird failure; `rm -rf /tmp/nix-shell.*` clears it (a SessionStart
  hook does this for Claude Code).
- The dev-shell `jet` is a wrapper that execs `target/debug/jet` — rebuild
  (`scripts/agent/jet-env cargo build`) before smoke-testing compiler changes.

## Deeper guides — read the one that matches your task

Plain markdown, tool-neutral despite the `.claude/` path; Claude loads them
as skills, every other agent reads them as files:

- `.claude/skills/verify/SKILL.md` — verification checklist + this repo's
  traps (stale binaries, snapshot/golden/formatter gotchas). Read before
  claiming anything done.
- `.agents/skills/tower/SKILL.md` — the board workflow (cards, lanes,
  ballots, questions).
- `.agents/skills/tower-ballot/SKILL.md` — ballot standard + how the owner
  decides. Read before raising any owner-facing choice.

The two agent-config directories are intentional. `.agents/` is the
tool-neutral home for shared prompts and skills. `.claude/agents/` contains
Claude Code's three harness-specific agent definitions (`jet-impl`,
`jet-verify`, `jet-ballot`); they adapt this manual and do not duplicate the
shared skill implementations.

For this repo, launch Tower with
`scripts/agent/jet-env node Tower/tower.mjs serve --open`. All other Tower
commands use the same prefix; `.agents/skills/tower/SKILL.md` owns the workflow.

## Invariants (violating one = stop and fix)

- **I1** Safe by default, expert tier first-class: all Jet code is memory-safe
  and type-safe unless the user explicitly opts in. `#Unsafe("reason") { … }` /
  `#Unsafe("reason") fn` (E2-M13/D-LL1/D-UNSAFE2) is the supported expert tier,
  gated by user-written audited regions. Generated Rust `unsafe` may appear only
  inside those gate regions or vetted std/mem internals.
- **I2** rustc never speaks to users. rustc rejecting generated code is an
  internal compiler error (exit 101, banner owned by `Source/CmdCompile.rs`)
  and a P0 bug.
- **I3** Codegen is dumb. All checking lives in sema. Never "try rustc and
  see" as a checking strategy.
- **I4** Every diagnostic has a code in docs/spec/diagnostics.md, what/why/fix, and a
  tests/ui snapshot. No snapshot → the diagnostic doesn't exist.
- **I5** Examples are the executable spec. Every feature ships with an
  example + expected output that golden tests enforce.
- **I6** Zero external crates in the compiler, ever. This covers the root
  `Source/` crate and compiler seam crates listed by
  `tests/truthfulness.rs::compiler_seam_crates_have_only_path_dependencies`;
  runtime/tool siblings with separately ratified dependencies are not a way to
  smuggle dependencies into those seams. Stdlib sub-libraries and modules may
  use external crates to bootstrap until the end of Epoch 3; after that, all
  external deps must be replaced with native Jet/Rust implementations.
  Existing ratified bootstrap dependencies are recorded in the owning
  architecture and syntax law. Any new stdlib external dependency requires
  owner approval.
- **I7** Every user-typeable keyword/sigil lives in
  `crates/jet-foundation/src/Syntax.rs` with a decision ID.
- **I8** One way to mean it, many ways to write it. There is exactly one
  canonical mechanism for any given semantic job — reject a second feature that
  does the same thing a different way with a great error + the existing path.
  But organizing/writing code flexibly is fine and encouraged: code layout, file
  structure, and formatting are the user's choice, and policy-driven
  customization (lints, profiles, opt-in strictness a team requires) is welcome.
  The bar: the default surface stays small enough to be easy to find and easy to
  learn. Beginners get a magic, batteries-included experience with safe defaults
  and no footguns; experts get full control through explicit opt-in escape
  hatches — never make the footgun the default, never deny the expert the reach.
  New mechanisms need a roadmap slot or owner sign-off.

## Workflow loop

Pick the next roadmap item → write the failing test first (ui fixture or
example) → spec it in docs/spec/spec.md → implement parser → sema → codegen →
JIT/dev parity through the same executable TIR (R12) → scoped tests green →
independent review → update docs touched → done means: tests pass, docs match
behavior, no invariant bent.

Use scoped targeted tests (`scripts/agent/jet-env cargo test --test <name>`)
plus independent review for each card. Never trust a builder's "green" — the
reviewer re-runs the relevant proof. Only the orchestrator runs
`scripts/agent/jet-env full scripts/agent/verify-full.sh`, once after a major
push on its closeout or blocking card; CI also runs the full suite. Keep normal
test parallelism; use global `-- --test-threads=1` only to reproduce a specific
race.

## Syntax decision protocol

**BALLOT FIRST.** Before writing any code on a card, enumerate every
owner-gate it contains — new user-facing syntax, a new stdlib external dep
(I6), an invariant carve-out, any owner-only call. Queue EVERY gate as a
ballot-ready decision in Tower immediately (see the **tower** and
**tower-ballot** skills; data lives in `.tower/`, written only via the Tower
CLI — never hand-edit the JSON). Then **stop work on the gated feature**
until the owner decides; build something ungated meanwhile. When the owner
ratifies: update `crates/jet-foundation/src/Syntax.rs` / parser, re-bless
snapshots, log it in syntax-decisions.md. If the ratification adds or removes
user-typeable syntax, run `scripts/agent/jet-env jet self devtools grammars`
and commit the regenerated editor sections.

The owner is CEO/CTO; his decisions are the only allowed bottleneck — he
never waits on you for a plan or a decision, and nothing reaches him that an
agent hasn't already reviewed.

Before any plan or ballot reaches the owner, run the **two-facet pass**:

- **Beginner pass:** assume the reader is learning Jet for the first time and
  wants magic out of the box. Defaults should require no ceremony, no policy
  jargon, and no build-system knowledge unless the user opts in.
- **Expert pass:** assume the reader knows exactly what they want and needs
  full control over targets, effects, generated code, toolchains, scheduling,
  caching, and audit output.

After both passes, rewrite each viable option so it is internally cohesive.
Do not require a separate hybrid option or harvest field. When one canonical
mechanism can serve both audiences, make that mechanism a normal worked option
with ergonomic defaults and explicit expert control.

## Owner decisions and frontend acceptance

Owner choices live as Tower ballots and verdicts, never only in prose or logs.
Honor the complete ruling, including attached questions and acceptance terms;
the detailed authoring law lives in `.agents/skills/tower-ballot/SKILL.md`.
Reject an option before ballot when it hollows out a useful default, forces a
file or project shape, or weakens a safety invariant.

Frontend acceptance requires the full mock and state matrix in the owner's real
terminal or browser: relevant archetypes, viewports and states, keyboard/focus
paths, and terminal ANSI/`NO_COLOR` behavior. A prose claim or selected
screenshot is not acceptance evidence.

## Canonical maintainer runbooks

- Bless snapshots, add syntax, and triage ICEs:
  `.claude/skills/verify/SKILL.md`.
- Add a diagnostic: `docs/spec/diagnostics.md` → “Adding a diagnostic”.
- Add an FFI bridge: `docs/spec/architecture.md` → “Adding an FFI bridge”.

## Communication — caveman default

All agent chatter — status updates, analysis, plans, sub-agent briefs and
reports — is written ultra-terse: drop articles/filler/pleasantries/hedging,
fragments OK, technical terms exact. Under Claude Code, invoke the
`caveman:caveman` skill (full) at the start of every sub-agent brief; other
harnesses follow the rule as prose. Write NORMAL prose only where the text
is product or durable copy: user-facing docs, ballots/decision cards,
diagnostics text, commit messages, README/spec files. Disable only when the
owner says "stop caveman" / "normal mode".

## Sub-agent delegation

Spawn sub-agents for parallelisable or context-heavy work rather than doing
everything in one context window. Match the model effort to the task.

Rules:
- **Checkpoint-commit before delegating.** A sub-agent `git restore` has
  wiped uncommitted parent work before. `git add -A && git commit -m "wip:
  checkpoint"` first (a Claude Code hook enforces this for write-capable
  agents).
- Every sub-agent brief starts with the caveman invocation (see
  Communication) and states: goal, relevant file paths, invariants that
  apply, and "targeted tests only — orchestrator owns major-push closeout".
- Prefer the baked project agents in `.claude/agents/`: `jet-impl` for builds,
  `jet-verify` for independent verification, and `jet-ballot` for decisions.
- **Adversarial review gate.** Before a meaningful change is integrated or
  called complete, assign a different agent a fresh-context review. Give it the
  diff, acceptance criteria, invariants, and test evidence — not the
  implementer's reasoning — and tell it to assume the change is wrong. Its only
  job is to find concrete bugs, missing paths, invariant violations, false-green
  tests, and accidental scope changes; it does not implement. The implementer
  fixes findings, then the reviewer re-checks material fixes. Parent inspects
  both reviews and evidence. Meaningful means any change to
  compiler semantics, safety/ownership/FFI, runtime behavior, public contract,
  generated output, or more than one coherent implementation file. A one-file
  mechanical edit with an exact, locally verified transformation is exempt;
  parent records why. Never waive this gate because code compiles or tests pass.
- One layer deep — sub-agents never spawn sub-agents.
- Never spawn a sub-agent just to run a single shell command — use Bash directly.
- Sub-agents must still follow all invariants (I1–I8) and the Nix command environment.

## Git workflow

Work directly on the current branch. Do not create new branches, worktrees,
or forks unless the owner explicitly asks for one. If it would be genuinely useful to speed up work efficacy & efficiency, you may request the owner's approval.

## Style

- Write terse. Use natural, minimal, plain language; reach for a technical
  term only when it's the precise word. No filler, no hedging, no throat-
  clearing. This goes double for Markdown: don't pad docs with restated
  headings, bullet lists that repeat the prose, "comprehensive"/"robust"/
  "seamless" adjectives, or summary paragraphs that add nothing. Say the
  thing once, plainly, and stop. Bloated LLM prose is tiring to read —
  cut it.
- Plain std-only Rust; small modules; no cleverness codegen-side.
- Error message text is product copy: write it like docs/spec/diagnostics.md, get it
  snapshot-tested, never tweak casually.
- When in doubt, the ranked priorities in docs/spec/philosophy.md decide. Effort is the
  resource you spend; safety and beginner experience are the ones you
  don't.
- **Difficulty is never a deterrent, and never an argument** (philosophy.md →
  "Effort is never a deterrent"). Never let "this is hard / a lot of work / would
  take a long time" influence a recommendation, an option ranking, or how much you
  scope. Hard work up front is the chosen currency; a hard path is often the right
  one. **Do it right the first time** — full, end-to-end, the first time. Never
  ship a stub or "milestone-pending" placeholder meaning to revisit, unless
  genuinely blocked on an unratified upstream decision (name the gate). Don't even
  *mention* implementation difficulty as a factor; weigh only the ranked priorities.
