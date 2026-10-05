# #3277 — Standard JMESPath adapter (D-CORE-JMESPATH1=A): evidence check

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-dev/safe-jet.sh`).

## Question

Can #3277 close? Its criteria 5–9 require a running `core.data.jmespath`
adapter (full official compliance corpus, all 26 built-ins, bounded limits,
reusable compiled `Expression`) proven on AOT, `jet run` and the interpreter.

## Method

1. Searched the whole tree (Core, Compiler, crates, tests, Examples) for the
   substring `jmespath` in file names and in `.jet/.rs/.json/.toml` text
   (Python walk; results below).
2. Checked the named proof harness: `tests/corelib_encoding_surface.rs` and
   `tests/corelib_parts/`.

## Evidence

- Tree search: **0** files named or containing `jmespath`
  (Core/, Compiler/, tests/, Examples/, crates/).
- `tests/corelib_parts/` holds only `compile.rs` and `derives.rs`; no
  `jmespath.rs`, and no vendored compliance corpus under `tests/fixtures/`.
- Registry: `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet` module
  export table has no `core.data.jmespath` module.

## Verdict

**FAIL (not implemented).** #3277 is an implementation card for the ratified
option A (named, bounded adapter); there is nothing to prove yet. The
statement-only criteria stand as design constraints for the implementer and
are already satisfied by the ratified ballot text, not by code:

- c1 (no "full JMESPath" claim from the peer page): no Jet artifact makes
  such a claim; D-CORE-JMESPATH1 option A text requires the full corpus.
- c3 (static predicates vs external query interchange): the typed
  `Query`/iterator path (`Compiler/JetFoundation/Source/Collections.jet`)
  is untouched; an adapter would be a separate module over `DataTree`.
- c4 (ballot before new grammar/API): D-CORE-JMESPATH1 is ratified (A).

c2, c5–c9 need the adapter itself (lexer/parser, evaluator, limits,
typed `JmesPathError`, corpus driver on three tiers). Route to an
implementation worker; no closer evidence can substitute.
