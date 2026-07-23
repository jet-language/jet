# Simple — agent reference

Official STE Issue 9: https://www.asd-ste100.org/
This repo ships an agent subset only. Orwell: [orwell.md](orwell.md). Priority: [priority.md](priority.md).

## Writing-rule map

| Area | Do | Do not |
|------|----|--------|
| Words | One meaning / part of speech | Synonyms for style |
| Nouns | ≤ 3 words in a cluster | Long stacked titles |
| Verbs | Simple tenses; imperative for steps | Progressive as main verb; heavy auxiliaries |
| Voice | Active | Passive when agent is known |
| Length | ≤20 procedure / ≤25 description | Long multi-clause sentences |
| Density | One instruction / one topic | Packed paragraphs |
| Completeness | Keep articles and subjects | Telegraphic fragments |
| Structure | Vertical lists for complexity | Dense one-line lists |

## Procedure template

```text
To <goal>:
1. <Imperative sentence ≤20 words.>
2. <Imperative sentence ≤20 words.>
```

## Description template

```text
<Subject> <simple-present verb> <object>.
```

## Safe rewrites

| Habit | Rewrite |
|-------|---------|
| “Prior to initiating the build, ensure dependencies are present.” | “Before you start the build, make sure the dependencies are present.” |
| “The diagnostic is displayed when validation fails.” | “The compiler shows the diagnostic when validation fails.” |
| “Utilization of `.new()` is recommended for containers.” | “Use `.new()` for containers.” |
| “This functionality allows users to…” | “Use this function to…” |
| “It should be noted that…” | Delete. State the fact. |
| “In order to…” | “To…” |
| “A total of three files were modified.” | “The change modifies three files.” |

## Jet technical nouns

Keep `sema`, `struct`, decision IDs, codes, paths exact. Simple words glue them:

> The `sema` pass checks the `struct`. Then codegen writes the TIR.

## When stuck

1. Name the technical terms exactly.
2. State one fact or one command.
3. Split the sentence.
4. Turn abstract nouns into verbs where you can.
5. If meaning would suffer, stop compressing (Orwell #6).
