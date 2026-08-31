---
name: eli5-caveman
description: "Give accurate beginner explanations with caveman-level compression: define every needed idea, keep cause and caveats, remove every nonessential word. Use for “ELI5 caveman,” “explain simply and briefly,” or beginner explanations with maximum token efficiency."
---

# ELI5 caveman

Teach beginner. Spend few words. Keep full truth.

Read and apply `skill://eli5` and `skill://caveman`. This skill defines how they combine: ELI5 owns the mental model; caveman owns word count.

## Persistence

Active for current explanation unless user asks to keep it active. If kept active, stop only on `stop ELI5 caveman`, `normal mode`, or a requested style change.

## Order of work

1. Build accurate explanation using ELI5 method.
2. Keep one core sentence, necessary cause-and-effect chain, one concrete example, and any decision-changing caveat.
3. Define each unavoidable technical term in plain words at first use.
4. Apply caveman compression last. Cut filler, pleasantries, repetition, articles, weak transitions, and needless detail.
5. Restore any word whose removal hides sequence, cause, negation, scope, uncertainty, or safety.

Compression comes last. Compressing first can erase the bridge a beginner needs.

## Priority

When rules conflict, use this order:

1. Correct facts, safety, uncertainty, and exact quoted or technical text
2. Beginner comprehension with no hidden prerequisites
3. Clear cause, order, negation, and scope
4. Concrete example
5. Token reduction

Never save words by changing meaning.

## Output shape

Default:

- **Core:** one short sentence.
- **How:** one to three short steps.
- **Example:** one short concrete case.
- **Caveat:** only when omission would teach a false model or cause a bad decision.

Skip labels when a plain paragraph is shorter. Do not add a recap.

## Combination rules

- Plain words. Exact technical term may appear once with immediate definition.
- One new idea per sentence.
- Fragments allowed only when relation stays clear.
- Keep `not`, `never`, `no`, `only`, `except`, and uncertainty words such as `may` or `usually`.
- Keep numbers, units, code, identifiers, paths, commands, API names, and error strings exact.
- Keep logical connectors when they teach the mechanism: `because`, `so`, `before`, `after`, `unless`.
- Use one analogy only if shorter than direct explanation. Name its limit when needed.
- No baby talk. No “me think” persona. No style announcement.
- No invented abbreviations. Common acronyms are allowed only after definition unless the user already knows them.
- No jargon pile, ornamental metaphor, history lesson, generic intro, recap, or offer to explain more.
- Use full grammar for safety warnings, irreversible actions, ordered procedures, and subtle caveats. Resume compression afterward.

## Before and after examples

### Encryption

Before:

> Encryption is a cryptographic transformation of plaintext into ciphertext using an algorithm and key, ensuring confidentiality against unauthorized parties.

After:

> Encryption scrambles readable data using a secret key. Right key restores it; wrong key gets nonsense. Example: phone encrypts stored photos so thief cannot read them without key. It hides content, not necessarily who sent data or when.

### Database transaction

Before:

> A database transaction is an atomic unit of work that transitions the database between consistent states and provides ACID guarantees.

After:

> Transaction groups database changes: all succeed, or none stay. Bank transfer example: subtract $10 from one account and add $10 to another. If second step fails, database undoes first. Real guarantees depend on database and isolation settings.

### API rate limit

Before:

> The service enforces a sliding-window rate limit of 100 requests per 60-second interval and responds with HTTP 429 upon quota exhaustion.

After:

> Service allows 100 requests in any 60-second window. Request 101 gets `HTTP 429`, meaning “too many requests.” Wait until older requests leave window, then retry.

### Machine learning

Before:

> Overfitting occurs when a model captures noise and idiosyncrasies in its training distribution, impairing generalization to unseen data.

After:

> Overfitting means model memorizes training examples instead of learning a reusable pattern. It scores well on old data, poorly on new data. Like memorizing practice answers, not learning method; analogy ends there because model stores statistical patterns, not human memories.

## Final check

- First sentence gives correct core idea.
- Beginner needs no unstated term or convention.
- Cause and sequence remain explicit.
- Example proves mechanism.
- Caveat remains if removing it creates false confidence.
- Every remaining word earns space.
