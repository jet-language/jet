# Jet language-shape work

Status: design rules for open Tower ballots. No new syntax here is ratified.

## Goal

Jet should be easy to read, easy to write, and easy to reason about. A short
form is good only when its full meaning is unique and easy to reveal.

Beginners should write intent without ceremony. Experts should be able to add
the exact type, effect, provider, target, ownership, or build choice. Companies
should be able to require those facts at selected boundaries and audit where
each fact came from.

These are three views of one program. They are not three languages.

## What stays

Several existing forms already achieve that goal:

```jet
point :: Point.{ x: 2, y: 3 }
point: Point :: .{ x: 2, y: 3 }

role :: Role.Admin
role: Role :: .Admin
```

The explicit and inferred forms have the same value shape. A leading dot says
that the expected type supplies the missing qualifier. If there is no unique
expected type, Jet must ask for the explicit form.

The language-wide rule is:

> Users may omit a fact only when one answer is already forced. Jet must show
> that answer, explain why it was chosen, and let the user write it explicitly.

This rule does not reopen `Type.{...}`, `.{...}`, `Type.Variant`, `.Variant`,
or `fn run()`.

## Evaluation model: one truth, several views

This is a model for judging ballots and a candidate tooling direction, not a
ratified three-view feature. Source text remaining complete and portable is the
constraint. Tools may eventually offer three lossless views:

- **Compact:** hide facts that are uniquely known.
- **Exact:** show inferred types, effects, ownership, providers, targets, and
  defaults beside the source that caused them.
- **Audit:** add policy results, provenance, hashes, and proof status.

Revealing a fact does not invent another syntax. It shows the legal explicit
form. Pinning that fact writes the explicit form into source. Folding it is
allowed only when the compiler can prove the short form still has one meaning.

Views must preserve comments, formatting choices, names, order, and behavior.
Plain-text editing, copying, diffing, and building must never require an editor
database.

## Punctuation map under review

The owner has given the intended high-level roles. They are conformance rules,
not a reason to create a ballot for every punctuation mark. A ballot is needed
only where two credible designs still disagree. Any such ballot decides one
row and cannot silently choose another.

| Shape | One question |
| --- | --- |
| `{ ... }` | What kind of body or scope may braces contain? |
| `[ ... ]` | What plural values or requirements may brackets group? |
| `( ... )` | What inputs, signatures, or payload parts may parentheses group? |
| `< ... >` | What type specialization may angles contain? |
| `.` | When may the expected type supply a missing qualifier? |
| `|` | Which peer alternatives does a form accept? |
| `|>` | Should one value move through ordinary calls from left to right? |
| `_name` | What promise does an internal-looking name make? |
| `@` | What may this prefix apply to one declaration? |
| `#` | What may this prefix change about a nested source region? |

The old phrase “attribute versus compiler instruction” is rejected as a user
model. It requires knowledge of compiler internals. The open marker ballots
instead compare concrete user-visible boundaries, including retiring one of the
two prefix families.

## Records, variants, and other construction

Plain record construction is already decided:

```jet
User.{ name: "Ada", role: .Admin }
```

The remaining construction questions are narrower:

- how a type creates fresh hidden state;
- how an existing value converts to another type;
- how code creates a non-owning view;
- whether expected type may shorten opaque construction.

These jobs are separate ballots. A vote on fresh state must not also choose
conversion or view spelling.

## Calls and flow

Jet should have one declared operation. The existing UFCS decision does not
allow an ordinary free function to become a receiver call. The open pipe ballot
asks only whether a left-to-right lens may call that same operation.

```jet
parse(raw)
raw |> parse
```

The second line is still only a ballot option. If adopted, both lines must
resolve the same symbol. There is no fallback search. The pipe cannot add
separate error, ownership, effect, or scheduling behavior.

Whether Jet admits `|>` and which input receives its value are separate votes.

## Effects and resources

The effect model, its source location, omission, denial, and generic rows are
separate questions. Ballots must not treat this as one punctuation choice.

The beginner may omit an effect set only when inference has one answer. The
expert can pin it. A company can require it at public or audited boundaries.
Policy may narrow authority; policy may not silently add authority or change
behavior.

Owned resources already clean up at scope exit. Remaining decisions cover
early release, cleanup failure, and asynchronous cleanup separately.

## Packages are typed information

Package identity, sources, outputs, dependencies, environments, and policy must
form one closed typed graph. Unknown fields fail. File order never silently
wins. Every contribution keeps its source location.

This does not decide the source spelling. Separate ballots choose:

- one package role's shape;
- whether moving it between files changes meaning;
- how repeated field contributions combine;
- how an expert replaces a value;
- what provenance the build keeps;
- how one output is represented;
- how aliases select outputs;
- how an output points to an ordinary callable.

The same `Greeter` package appears in every option. A package ballot may not
quietly choose effect syntax, entry conventions, or record construction.

## Internal names

The useful part of Python's leading underscore is discoverability, not privacy.
Jet is already private by default. The open decisions separately ask:

- what `_name` tells a reader;
- who may explicitly access it;
- who may replace it;
- how the current underscore module-disable feature is replaced.

Hard access control remains a real access-control rule. A naming convention is
not a security boundary.

## Decision order

Work follows this dependency order:

1. Enforce the owner-directed delimiter roles and find real conflicts.
2. Apply the atomic admission checks below to each concrete syntax question.
3. Decide one prefix-marker role at a time.
4. Declaration and internal-name rules.
5. Fresh state, conversions, views, flow, and ownership transfer.
6. Effects and resources.
7. Runtime units and optional dimensional numbers.
8. Package roles, composition, outputs, aliases, and entry links.
9. Command-line and cross-surface exposure.
10. Conformance audit and implementation.

An open dependency is named as an assumption, not silently preselected.

## Ballot quality gate

Every ballot must pass all of these checks:

1. Its result fits one enforceable sentence.
2. Every option changes the same property.
3. The owner may postpone every sibling and still leave coherent law.
4. Reversing this result does not require reversing a sibling.
5. Every option solves the same fixed example.
6. Every option is a design an informed language author could honestly choose.
7. The recommendation names the human mistake it prevents.
8. The beginner path has no expert ceremony.
9. The expert can reveal and pin every hidden fact.
10. Enterprise policy can narrow, require, and audit the same facts.
11. No option is ranked by implementation effort.
12. The prose uses common words and shows code before formal detail.

If the owner can like an option's main idea but reject one punctuation detail
inside it, the ballot is bundled and must be split.

## Evidence

The shared language examples, mechanism research, primary sources, and
psychology audit live in
[`language-shape-research.md`](language-shape-research.md).
