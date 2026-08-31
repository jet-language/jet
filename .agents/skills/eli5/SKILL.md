---
name: eli5
description: "Explain any topic in accurate beginner-friendly terms without baby talk, hidden prerequisites, or misleading simplification. Use when the user says ELI5, “explain like I’m five,” “in simple terms,” “plain English,” or asks for a beginner explanation."
---

# ELI5

Explain the idea so a curious beginner can build a correct mental model on the first read. Simplify the path, not the truth.

## Persistence

Active for the current explanation unless the user asks to keep ELI5 active. If kept active, apply it to later explanations until the user says `stop ELI5`, `normal detail`, or asks for an expert treatment.

## Default reader

Assume an intelligent beginner who can follow ordinary cause and effect but does not know the topic’s jargon, conventions, or hidden prerequisites. Do not imitate a child or use baby talk.

The user may name a different starting point, such as “ELI5 for a Python developer” or “ELI5 for someone who knows accounting.” Treat that knowledge as available and explain everything else.

## Method

1. **Lead with the point.** Start with one plain sentence that answers “what is it?” or “why does it matter?”
2. **Build from known things.** Introduce one new idea at a time. Show cause before effect and purpose before mechanism.
3. **Define unavoidable terms immediately.** Use the exact term, then explain it in ordinary words on first use. Reuse the term consistently after that.
4. **Give one concrete example.** Prefer a small everyday scenario, tiny input/output, or short sequence over an abstract definition.
5. **Use analogies only when they earn their keep.** State what maps to what. Name the analogy’s limit if carrying it further would teach the wrong model.
6. **Layer precision.** Give the beginner model first. Add a short “More precisely” section only when omitted detail changes decisions, safety, or likely follow-up understanding.
7. **Check the chain.** Every sentence must follow from something already explained. If a sentence requires outside knowledge, define that knowledge or remove the sentence.

## Output shape

Use the smallest shape that explains the topic:

1. **In one sentence:** the core idea.
2. **How it works:** two to five short steps or paragraphs.
3. **Example:** one concrete case.
4. **More precisely:** optional; include only necessary caveats or the exact technical model.

Do not force headings for a one-paragraph answer. Match depth to the question.

## Rules

- Preserve facts, uncertainty, numbers, units, names, code, commands, quoted text, and error messages exactly.
- Prefer common words and active voice. Use short sentences, but keep the logical connectors that show why something happens.
- Explain jargon; do not merely replace it with different jargon.
- Never say “just,” “simply,” or “obviously” when the missing step is the explanation.
- Never hide an important exception to make the story cleaner.
- Never turn probabilities into certainties.
- Never confuse an analogy with the mechanism.
- Never pad with history, taxonomy, edge cases, or implementation detail unless they answer the question.
- Never talk down to the reader, perform a childish voice, or use cartoon examples when a real one is clearer.
- For ordered procedures, safety warnings, medical/legal/financial limits, and irreversible actions, clarity and completeness override brevity.
- If the request is ambiguous, explain the most common meaning and briefly name the alternative meanings instead of interrogating the user.

## Before and after examples

### Technical definition

Before:

> A cache is a high-speed data storage layer that stores a subset of data, typically transient in nature, so future requests are served faster than accessing the primary storage location.

After:

> A cache keeps a nearby copy of data you are likely to need again, so you can get it faster. For example, a browser saves a site’s logo instead of downloading the same image on every visit. The saved copy can become outdated, so caches need rules for when to refresh or discard it.

### Networking

Before:

> DNS performs hierarchical, distributed resolution of domain names into IP addresses.

After:

> DNS is the internet’s address book. You give it a name such as `example.com`, and it returns the numeric address computers use to reach that site. “Address book” is only an analogy: DNS is a distributed system of many servers, not one central list.

### Programming

Before:

> A closure is a function bundled with references to its lexical environment.

After:

> A closure is a function that remembers values from where it was created. If you create a function while `taxRate` is 0.2, that function can still use `taxRate` later, even after the surrounding code has finished. More precisely, it keeps access to the variables it captured, not necessarily frozen copies of their values.

### Probability

Before:

> A 20% probability does not imply the event occurs once in every five trials due to variance in finite samples.

After:

> A 20% chance means the event is expected about 20 times across many similar tries. It does not promise one success in each group of five. You could get no successes in five tries, or several; the results tend to approach 20% only over many tries.

## Final check

Before sending, ask:

- Can a beginner state the main idea after the first sentence?
- Is every necessary term defined before it is used?
- Does the example demonstrate the mechanism rather than decorate it?
- Did simplification change any fact, condition, or level of certainty?
- Can any sentence disappear without breaking understanding?
