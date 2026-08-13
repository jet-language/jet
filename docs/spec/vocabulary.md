# Jet vocabulary

Status: ratified reference for **D-ONCE-WORD1=A** (card #1732).

This page is the one home for Jet's meanings of **stream**, **reader**, **event**, and
**collecting loop**. Other pages may use these words, but they must link here and use
the meanings below.

## Stream

Definition: A `Stream<T>` is a lazy, pull-driven sequence whose producer suspends at
`yield` and ends when it is exhausted.

Authority: [D-STREAMYIELD1](syntax-decisions.md#d-streamyield1--generators) and
[D-CONC-STREAM1](syntax-decisions.md#d-conc-stream1--a-stream-is-a-task).

## Reader

Definition: A reader is a codec-owned input handle that consumes input and returns its
declared item type, clean end, or an encoding error.

Authority: [D-ENCSTREAM-SURFACE1](encoding-decisions.md#d-encstream-surface1--public-streaming-encoding-surface)
and the [encoding decision law](encoding-decisions.md).

A reader is not a `Stream<T>`. A codec reader may use pull control, but its name states
that it reads a format and returns codec items.

## Event

Definition: An event is one typed occurrence or one item in an event algebra, such as
`Event<T>`, `DataEvent`, or an XML reader event.

Authority: [D-EVENT1](syntax-decisions.md#d-event1--first-party-typed-eventhook-family),
[D-EVENT2](syntax-decisions.md#d-event2--typed-async-events-scheduler-tranche),
and [D-ENCSTREAM-SURFACE1](encoding-decisions.md#d-encstream-surface1--public-streaming-encoding-surface).

An event is not a stream. A sequence of events is still a sequence; call the values
events and call the producer or input handle a reader or a stream according to its
mechanism.

## Collecting loop

Definition: A collecting loop is an eager `loop ... -> ...` expression that runs now
and returns one `List<T>` in iteration order.

Authority: [D-LOOPEVAL1 and D-COMPREHENSION1](syntax-decisions.md#s19--loops).

A collecting loop does not yield. The word `yield` belongs to a `Stream<T>` producer;
use `next` to omit an item from a collecting loop.

## Retired senses

| Retired wording | Write this |
|---|---|
| a codec mode, codec adapter, or format handle called a stream | reader or writer |
| an event or event sequence called a stream | event or event sequence |
| an eager `loop ... -> ...` expression called yielding | collecting loop |

The decision IDs and historical file names that contain old wording stay exact. New
prose must use the right term.

## Link and lint rule

Every Markdown page that uses one of these four words links to this page. A doc lint
must reject the three retired senses in the table. Its hostile fixtures are:

1. a codec mode called a stream;
2. an event called a stream;
3. a collecting loop called yielding.

The vocabulary page and this rule are checked by
`vocabulary_page_has_one_definition_and_no_retired_senses` in
`tests/truthfulness.rs`; the truth row is registered in the corpus table.

## Corpus truth row

| Truth | Home | Renderers | Guard |
|---|---|---|---|
| D-ONCE-WORD1 vocabulary | this page | Markdown pages that use these words | doc lint and truthfulness test |
