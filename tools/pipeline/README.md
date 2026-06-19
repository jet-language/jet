# `pipeline` — task pipeline dashboard

A tiny, dependency-free tool over the owner workflow:

```
inbox  →  plan  →  ballot  →  ratified  →  implemented
```

It reads only the canonical docs and never invents state of its own, so it can
never drift from the docs that are the source of truth.

## Use

```sh
# Dashboard + decision ballot in the browser (the main way to work):
nix develop -c node tools/pipeline/pipeline.mjs serve --open

# Same view in the console:
nix develop -c node tools/pipeline/pipeline.mjs status

# Scaffold a new sidequest plan:
nix develop -c node tools/pipeline/pipeline.mjs new <slug> "Title"
```

Opening `docs/spec/decision-ballots.html` is a shortcut: if the server is
running it redirects to the dashboard; otherwise it shows the command above.

## The dashboard

`serve` starts a local server (default `http://127.0.0.1:4173`) that:

- shows the pipeline at a glance — inbox tasks, sidequest plans, open ballots,
  ratified count, last submission, and any queued example-improvement requests;
- **renders the ballot from `docs/spec/decision-ballots.md`** — the cards are
  parsed out of the markdown, so there is exactly one source of truth and no
  duplicated card data to keep in sync;
- lets you **pick** an option, **undo** it (click again or "✕ clear"), and add a
  per-decision comment;
- on **Submit**, writes your decisions to `docs/spec/ballot-results.md` — no
  copy/paste. Then tell Claude **"go"** and it ratifies them into
  `syntax-decisions.md`, strips the decided cards, and implements the plans;
- **↻ improve examples** on a card appends a request to
  `tools/pipeline/regen-queue.md`; Claude reviews that card's examples against the
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
