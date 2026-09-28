# Jet vocabulary

This page is the single definition home for **collecting loop**, **event**,
**reader**, and **stream**. It is for language authors and documentation
writers; the governing decisions are linked beside each definition, and
`tests/truthfulness.rs::vocabulary_page_has_one_definition_and_no_retired_senses`
checks the home and its cross-page link rule.

## Collecting loop

A collecting loop is an eager `loop ... -> ...` expression that evaluates now
and returns one `[T]` in iteration order.

Authority: D-LOOPEVAL1 and D-COMPREHENSION1 in [syntax decisions](syntax-decisions.md).

A collecting loop does not yield; use `next` to omit an item from its result.

## Event

An event is one typed occurrence or one item in an event algebra, such as
`Event<T>`, `DataEvent`, or an XML reader event.

Authority: D-EVENT1 and D-EVENT2 in [syntax decisions](syntax-decisions.md),
and [D-ENCSTREAM-SURFACE1](encoding-decisions.md#d-encstream-surface1--public-streaming-encoding-surface).

An event is not a stream: name a sequence's values as events, and name its
producer or input handle a reader or stream according to its mechanism.

## Reader

A reader is a codec-owned input handle that consumes input and returns its
declared item type, clean end, or an encoding error.

Authority: [D-ENCSTREAM-SURFACE1](encoding-decisions.md#d-encstream-surface1--public-streaming-encoding-surface)
and the [encoding decision law](encoding-decisions.md).

A reader is not a `Stream<T>`; pull control does not change that a codec reader
reads a format and returns codec items.

## Stream

A `Stream<T>` is a lazy, pull-driven sequence whose producer suspends at `yield`
and ends when it is exhausted.

Authority: D-STREAMYIELD1 and D-CONC-STREAM1 in
[syntax decisions](syntax-decisions.md).

A stream is a producer mechanism, not the name for a codec handle or an event;
those are a reader and an event respectively.

## Effect roles

An effect projection has four separate roles at the application boundary:

| Role | Meaning |
|---|---|
| `required_effects` | Effects reached by the selected entry, computed by sema. |
| `granted_effects` | Effects allowed by the effective application or dependency policy. |
| `denied_effects` | Effects explicitly refused by that policy. |
| `authority` | The identity of the policy source, such as `package.jet authority.holds`. |

These roles are facts, not synonyms: a required effect can be granted, denied,
or undecided, while an authority name identifies the policy source and is not an
effect. Expert source may use `#FX`; beginner source omits routine effect rows.
Inspect, hover, JSON, receipts, lock provenance, and Canvas use the same four
labels so a policy decision cannot be mistaken for a sema requirement.

## Retired senses

| Retired wording | Write this |
|---|---|
| a codec mode, codec adapter, or format handle called a stream | reader or writer |
| an event or event sequence called a stream | event or event sequence |
| an eager `loop ... -> ...` expression called yielding | collecting loop |

Decision IDs and historical file names that contain old wording stay exact. New
prose uses the current term.

## Link and lint rule

Every Markdown page that uses one of the four terms links to this page and uses
the definitions above. The doc lint rejects these retired senses:

1. a codec mode called a stream;
2. an event called a stream;
3. a collecting loop called yielding.

The vocabulary page and this rule are checked by
`vocabulary_page_has_one_definition_and_no_retired_senses` in
`tests/truthfulness.rs`; the truth row is registered in the corpus table.

## Corpus truth row

| Truth | Home | Renderers | Guard |
|---|---|---|---|
| D-ONCE-WORD1 vocabulary | this page | Markdown pages that use these terms | doc lint and truthfulness test |
