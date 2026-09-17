# Simple — contextual STE reference

Read this file for a governed artifact or an explicit `STE` / `STE100` request. This is an agent working subset, not the official ASD-STE100 dictionary.

Official STE: https://www.asd-ste100.org/

## Strict rules

| Area | Do | Do not |
|---|---|---|
| Words | Give one meaning and one part of speech to each everyday word. | Stack synonyms for style. |
| Nouns | Keep noun clusters to three words or fewer; use `of` or `for` when needed. | Use long stacked titles. |
| Verbs | Use infinitive, imperative, simple present, simple past, or simple future. | Use a progressive main verb or heavy auxiliary stack. |
| Voice | Use active voice when the agent is known. | Use passive voice to hide a known agent. |
| Length | Keep procedures to 20 words or fewer per sentence; descriptions to 25 words or fewer. | Use long multi-clause sentences. |
| Density | Put one instruction in each sentence and one topic in each paragraph. | Pack unrelated instructions into a paragraph. |
| Completeness | Keep articles, subjects, and verbs. | Use telegraphic fragments in governed prose. |
| Structure | Use vertical lists for steps and complex items. | Compress complex items into one line. |

Use no more than six sentences in a paragraph. Do not use `-ing` as the main verb in running prose, except in technical nouns. Prefer concrete verbs such as `set`, `remove`, `install`, `make sure`, `do`, `stop`, `start`, `show`, `give`, `put`, and `get`.

## Authority order

1. Exact technical tokens: code, IDs, paths, errors, Jet syntax, and quoted text.
2. Orwell #6: do not write barbarous or misleading text.
3. STE grammar: keep articles, subjects, and verbs.
4. STE sentence, paragraph, list, noun, and verb limits.
5. Orwell #1–5.
6. The everyday word table below.

## Everyday word table

| Prefer | Avoid in prose |
|---|---|
| start | begin, commence, initiate |
| stop | terminate, cease (unless an API name) |
| make sure | ensure; “verify that” in casual prose (`verify` is fine as a Jet skill or command) |
| show | display, exhibit, illustrate |
| give | provide, supply |
| get | obtain, acquire, retrieve |
| put | place, position |
| remove | eliminate (`delete` is fine as an API) |
| set | configure (`config` is fine as a technical noun) |
| do | perform, execute, carry out |
| use | utilize, employ, leverage |
| help | facilitate, assist |
| change | modify, alter, mutate |
| because | due to the fact that |
| also | furthermore, moreover, additionally |
| but | however, nevertheless |
| must | should when the rule is mandatory |
| can | is able to, is capable of |

## Shape

**Procedure**

```text
To <goal>:
1. <Imperative sentence of 20 words or fewer.>
2. <Imperative sentence of 20 words or fewer.>
```

**Description**

<Subject> <present-tense verb> <object>.
```

## When stuck

1. Name the technical terms exactly.
2. State one fact or one command.
3. Split the sentence.
4. Turn an abstract noun into a verb when meaning stays exact.
5. If meaning would suffer, stop compressing.
