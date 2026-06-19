# `Tower` — task pipeline dashboard

A tiny, dependency-free tool over the owner workflow:

```
inbox  →  plan  →  ballot  →  ratified  →  implemented
```

It reads only the canonical docs and never invents state of its own, so it can
never drift from the docs that are the source of truth.

## Use

```sh
# Dashboard + decision ballot in the browser (the main way to work):
nix develop -c node tools/Tower/Tower.mjs serve --open

# Same view in the console:
nix develop -c node tools/Tower/Tower.mjs status

# Scaffold a new sidequest plan:
nix develop -c node tools/Tower/Tower.mjs new <slug> "Title"
```

Opening `tools/Tower/docs/ballots/decision-ballots.html` is a shortcut: if the server is
running it redirects to the dashboard; otherwise it shows the command above.

## The dashboard

A dark mission-control surface. The hero is a **pipeline ribbon** — the seven
workflow stages flow across the top with live counts; click a stage to jump to
it. Sections start **collapsed** but stay informative: each header shows a count
and a preview of what's inside, so you open only what you want.

`serve` starts a local server (default `http://127.0.0.1:4173`) that:

- shows the whole pipeline at a glance in the ribbon, plus ratified count and
  last submission in the status line;
- **Board / Bugs:** each card moves down the pipeline with inline **◀ ▶**
  buttons (no dropdown) and carries its plan link, notes, and delete;
- **Decisions:** **renders the ballot from `tools/Tower/docs/ballots/decision-ballots.md`** —
  cards are parsed out of the markdown, so there is one source of truth and no
  duplicated card data. Every open decision is a full card with selectable
  options, grouped by section. A sticky meter shows **how many are decided**,
  each group header shows its **decided / total**, and **Next undecided** jumps
  to the first unanswered one;
- you **tick** an option, **undo** it (click again or "✕ clear"), and add a
  per-decision comment;
- on **Sign & file**, writes your decisions to `tools/Tower/docs/ballots/ballot-results.md` —
  no copy/paste, and it **merges**: a new submission adds to or replaces by id,
  never wiping earlier decisions. Then tell Claude **"go"** and it ratifies them
  into `syntax-decisions.md`, strips the decided cards, and implements the plans;
- **↻ improve examples** on a card appends a request to
  `tools/Tower/regen-queue.md`; Claude reviews that card's examples against the
  house criteria (human voice, plain language, a user-story scenario, inline
  cross-language comparison) and improves it before you re-read.

The boundary is deliberate: the server **records and queues**; it never edits
code or ratifies. Ratifying and implementing stay Claude steps, gated on your
word — so a stray submit can't change the language.

## Why it's markdown-driven

The owner drops tasks in one inbox, agents lift them into reviewed plans,
decisions surface to the ballot with worked examples, the owner decides on the
dashboard, agents implement, and a reviewing agent verifies. The tool is a lens
and an input surface over that flow — the docs remain the record.
