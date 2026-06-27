---
name: tower
description: Act on what the owner recorded in Tower: implement ratified decisions, answer open card questions, advance agent-lane cards, and raise ballot-ready decisions. Use after the owner records decisions or notes in Tower, or when asked to process Tower, act on decisions, work the board, sweep the board, or do Tower work.
---

# Tower

Tower (`tools/Tower/`) holds the project board in `tools/Tower/tower.json`. The
owner records decisions and notes there. Your job is to do the work those unblock,
and nothing that is still the owner's.

## Rule

The owner's decisions are the only allowed bottleneck. He must never wait on an
agent to write a plan or identify a decision, and he must never receive a plan or
decision that no agent has reviewed. Do planning and decision development eagerly;
he only picks.

## Board Model

Every card has a computed lane. Read `tools/Tower/app/store.mjs` and its `laneOf`
logic before mutating the board.

Owner lanes:

- `decide`
- `activate`
- `frozen`

Do not touch those lanes unless the owner explicitly instructs it.

Agent lanes:

- `plan` - write a thorough plan and raise any needed decisions.
- `implement` - the plan is vetted and linked decisions are ratified; build it.
- `building` - continue in-progress work to completion.
- `verify` - verify a claimed-done card end to end, then close it.

Also answer any `questions[]` item with `status:"open"`.

## Steps

1. Read `tools/Tower/tower.json`, or run `node tools/Tower/Tower.mjs status` and
   read the agent sections. Build work lists for ratified decisions needing work,
   cards in agent lanes, and open questions.
2. Answer questions first. If the owner asked to change a ballot, edit the decision
   in `tower.json`. Mark the question `answered` and set `answer`.
3. Do each card's work under the repo workflow in `AGENTS.md`: failing test first,
   spec, parser, sema, codegen, tests green, docs updated. Respect I1-I8.
4. Advance cards as work changes: append a dated log entry and move phase forward:
   `planning` to `deciding` or `ready`; `ready` to `building`; `building` to
   `verify`; `verify` to `done` only after real verification.
5. Prefer the running server API. Start it if needed with
   `node tools/Tower/Tower.mjs serve`, then use endpoints such as
   `POST /api/card/update`, `POST /api/question/answer`, and
   `POST /api/clearance` only if the owner explicitly delegated a decision.
   If there is no server, edit `tower.json` directly using shapes in
   `tools/Tower/app/store.mjs` and keep valid JSON.
6. Report what was implemented, what was answered, which cards advanced, and what
   is newly blocked on the owner.

## Implementation Standard

"Implemented" means a complete, reachable vertical slice, never a stub:

- parser, sema, and codegen wired from real `.jet` source;
- each new diagnostic has a code in `diagnostics.md` and a `tests/ui` snapshot;
- a runnable example with golden-tested output where user-visible;
- unit or integration tests for new paths;
- `nix develop -c cargo test` green;
- docs updated to match behavior.

A decision may remain ratified but unbuilt only if still gated on an unratified
upstream decision. Otherwise the owner's ratification is the go signal.

## Raising Decisions

Any owner-facing choice becomes a `decision` on its card. Fill the Focus Mode
fields completely:

- `gist` - one very short plain-language sentence.
- `story` - one short paragraph with a realistic person and reason.
- `inWild` - realistic Jet code where the choice matters.
- `comparisons` - cross-language comparison when relevant.
- `options[]` - `{key, name, detail, code}` for every option, each with worked
  code and user-visible result. Mark the recommended name.
- `rec` - recommended option key plus a one-line reason.

Rules:

- Give a rich menu of original syntax candidates.
- Never contradict a ratified decision.
- Decision IDs must be Tower-parseable (`D-...` or `S<digits>-...`) and must not
  collide with existing IDs.
- Never rank on effort or implementation difficulty. Rank on safety, beginner
  experience, performance, one-path design, and long-term correctness.
- A plan-writer proposes syntax; only the owner ratifies it.

## After Ratification

1. Honor every word the owner wrote, including questions and requests.
2. Add the decision to `docs/spec/syntax-decisions.md`; set `status:"ratified"`
   and `outcome` in `tower.json`.
3. Reconcile the card. If unblocked, move to `building` and build now; if gated,
   keep `deciding` and name the gate.
4. Implement end to end. When green, move to `done`.

## Codex Notes

Use Codex sub-agents when available for independent cards, plan review, or design
review. If sub-agents are unavailable, do the work directly. Keep delegation one
layer deep. Do not create git worktrees unless the owner asks.
