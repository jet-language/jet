---
name: simple
description: >-
  Write clear controlled prose for a governed Jet artifact such as a doc, spec,
  ballot, Tower card, report, commit, or PR, or when the user explicitly asks
  for simple, STE, or STE100. Preserve meaning, exact technical tokens, frozen
  copy, and safety. Do not trigger for ordinary prose or agent status chatter.
---

# Simple

Use `simple` to transform new user-facing prose. It owns the wording, not the artifact's meaning or the calling workflow. Return the prose without opening a review, ballot, plan, or implementation workflow.

## Core rules

1. **Be clear.** State the result first. Use concrete words, short sentences, active voice, and one idea per sentence.
2. **Keep meaning.** Preserve facts, caveats, uncertainty, order, scope, and safety conditions. Cut ceremony, not information.
3. **Keep exact tokens.** Never rewrite code, identifiers, paths, commands, Jet syntax, error strings, decision IDs, diagnostic codes, or quoted text.
4. **Keep frozen copy.** Do not restyle registered diagnostics, UI snapshots, ratified Tower or syntax-decision wording, or user-supplied quotes unless the caller asks.
5. **Keep grammar.** Do not drop required articles, subjects, or verbs to sound terse. When clarity and style conflict, clarity wins.

Apply these rules to new docs, specs, comments meant for people, Tower ballots and card text, owner-facing reports, commit messages, PR bodies, and product or UI copy when that artifact is in scope. Apply them when the user invokes `simple`, `ste`, `STE`, `STE100`, or `ASD-STE100`. Do not restyle agent-to-agent status chatter.

This skill is an agent working subset, not a claim of full ASD-STE100 compliance. The calling workflow owns the artifact's final acceptance.

## Contextual references

- For an explicit `STE` / `STE100` request, or an artifact contract that requires controlled language, read [reference.md](reference.md) and [orwell.md](orwell.md). They hold the strict sentence, grammar, length, word, and precedence rules.
- Read [priority.md](priority.md) only when a real rule conflict needs its precedence stack.
- Read [examples.md](examples.md) when a before/after pattern helps. Do not copy an example when it changes the source meaning.
