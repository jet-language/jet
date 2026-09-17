# Syntax, grammar, and examples

Load this branch when a change touches user-typeable syntax, grammar, the
lexical registry, or executable examples. The source and decision record are
authoritative; this reference replaces a generic in-skill checklist.

- Before changing a keyword or sigil, confirm the owner-ratified decision in
  `docs/spec/syntax-decisions.md`. Register every user-typeable spelling in
  `crates/jet-foundation/src/Syntax.rs` under I7; do not add a provisional
  entry without an approved decision ID.
- For grammar or parser changes, inspect the relevant grammar and parser source,
  then run the named narrow grammar/devtool command:
  `scripts/agent/jet-env jet self devtools grammars`. Do not treat that command
  as runtime or tier evidence.
- For diagnostics caused by syntax, use the canonical row in
  `crates/jet-codegen/src/Prelude/Diagnostics.jet`, matching UI snapshot, and
  `docs/spec/diagnostics.md`; preserve registered code and What/Why/Fix meaning.
- For an example, read `docs/spec/contributing/examples.md` and
  `examples/README.md`, exercise the exact example path after a fresh binary,
  and update only its matching golden under `examples/features/expected/`.
  Trace embedded example paths, snapshots, harness lists, and generated uses
  before moving a file.
- Record only evidence for the changed acceptance criterion. Never claim a
  runtime, snapshot, golden, generated-artifact, or I9 tier pass from syntax,
  type-check, or source inspection alone.
