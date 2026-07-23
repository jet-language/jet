---
name: simple
description: >-
  Controlled clear prose for Jet: ASD-STE100-inspired rules plus Orwell’s six
  writing rules for all agent and documentation text. Use only when the user
  invokes simple / ste / STE / STE100. Do not apply unless invoked.
disable-model-invocation: true
---

# Simple writing (opt-in)

**Opt-in only.** Apply only when the user invokes this skill (`simple`, `ste`,
`STE`, `STE100`, `ASD-STE100`).

While active, all **natural-language** output follows this skill:

- user replies
- commit messages and PR bodies you draft
- docs, specs, comments, ballots, and example prose you **newly write**

**Never rewrite these into “simple English” synonyms:** code, identifiers, paths,
commands, Jet syntax, error strings, decision IDs, diagnostic codes.

## Honesty bound

This is an **agent working subset**, not a certified ASD-STE100 checker. Jet does
not ship the official dictionary. When unsure:

1. Prefer short common words from the tables below.
2. Prefer one short sentence over a clever one.
3. Do not invent fake “approved word” claims.

Official STE (copyright ASD): https://www.asd-ste100.org/

## Do not touch unless the task says so

- Registered diagnostic **what/why/fix** text and UI snapshots (I4)
- Ratified Tower / syntax-decision wording already in-tree
- User-supplied quotes and error paste-backs

Explain in new prose. Do not silently restyle frozen product copy.

## Authority order (prose)

1. Exact technical tokens (code, IDs, paths, errors)
2. Orwell #6 — do not write barbarous or misleading text
3. STE grammar completeness (articles, subject, verb present)
4. STE length / one-idea rules
5. Orwell 1–5 (clichés, short words, cut dead weight, active, no decorative jargon)
6. Everyday word table in this skill

Full conflict notes: [priority.md](priority.md). Orwell detail: [orwell.md](orwell.md).

## Orwell’s rules

From “Politics and the English Language” (1946):

1. No stale metaphor / printed figure of speech.
2. Short word over long word.
3. Cut a needless word.
4. Active over passive.
5. Everyday word over decorative jargon.
6. Break a lesser rule sooner than write anything barbarous.

**With this skill:** #3 does **not** remove required articles/subjects/verbs.
#5 does **not** rename Jet/API terms. Prefer active (#4); unknown-agent
description may stay passive.

## STE hard requirements

1. One everyday word → one meaning → one part of speech. No synonym stacking.
2. Active voice (unknown agent in a description may be passive).
3. Procedures ≤ 20 words/sentence. Descriptions ≤ 25.
4. One instruction per sentence. One topic per paragraph. ≤ 6 sentences/paragraph.
5. Noun clusters ≤ 3 words (else use `of` / `for` / `that`).
6. Verb forms: infinitive, imperative, simple present/past/future; past participle
   only as adjective. Avoid heavy auxiliary stacks.
7. No `-ing` as the main verb in running prose (ok in technical nouns).
8. Do not drop articles, subjects, or verbs.
9. Use vertical lists for steps and complex items.
10. Prefer concrete verbs: `set`, `remove`, `install`, `make sure`, `do`, `stop`,
    `start`, `show`, `give`, `put`, `get`.

## Jet technical terms

Allowed as technical nouns/verbs even if absent from everyday tables:

- Compiler/language terms (`struct`, `sema`, `codegen`, `JIT`, …)
- Decision IDs, diagnostic codes, crate/file/CLI names
- API method names as written in code

Glue words around them stay simple.

## Everyday word table

| Prefer | Avoid in prose |
|--------|----------------|
| start | begin, commence, initiate |
| stop | terminate, cease (unless API name) |
| make sure | ensure; “verify that” in casual prose (`verify` ok as Jet skill/command) |
| show | display, exhibit, illustrate |
| give | provide, supply |
| get | obtain, acquire, retrieve (prose) |
| put | place, position |
| remove | eliminate (prose); `delete` ok as API |
| set | configure (prose); `config` ok as technical noun |
| do | perform, execute, carry out |
| use | utilize, employ, leverage |
| help | facilitate, assist |
| change | modify, alter, mutate (prose) |
| because | due to the fact that |
| also | furthermore, moreover, additionally |
| but | however, nevertheless |
| must | should (when the rule is mandatory) |
| can | is able to, is capable of |

## Procedures vs descriptions

- **Procedure:** imperative steps, one action each, list form.
- **Description:** simple present, one topic per paragraph.

## Commits and PRs

- Subject: short imperative (Conventional Commit type ok: `fix:`, `docs:`).
- Body: this skill’s prose rules.
- Do not “simplify” code fences or file paths in the PR.

## Checklist

- [ ] Tokens exact (code/IDs/paths/errors)
- [ ] Orwell 1–6; #6 clarity wins over style theater
- [ ] STE length / active / articles kept
- [ ] No clichés or synonym stacking
- [ ] No silent rewrite of diagnostics or ratified law
- [ ] Meaning preserved

## Family

| Skill | Adds |
|-------|------|
| `simple` | Prose law (this file) |
| `simple-caveman` | Anti-filler talk |
| `simple-ponytail` | Lazy code ladder |
| `simple-ponytail-caveman` | Code ladder + anti-filler talk |

## More

- [priority.md](priority.md) · [orwell.md](orwell.md) · [reference.md](reference.md) · [examples.md](examples.md)
