# Plan: Structured flag / argument parsing (D-ARGS1)

**Status: plan — awaiting owner decision D-ARGS1.**

Unblocks: **Amara** (automation scripts with `--option value` flags), and every
CLI persona (Priya) beyond the raw `io.args()` list.

---

## Goal

`io.args()` returns a raw `[String]` (confirmed `64_cli_args.jet`). Any tool with
flags (`--verbose`, `--output path`, `-n 5`) parses them by hand. Give a
declarative arg-spec: declare the flags/options/positionals a program accepts,
get them parsed into typed values, with auto-generated `--help` and a clear error
on bad input.

Verified: `core.io` exposes `args` (`CheckerStdlib.rs:1507`); there is no arg
parser (`grep arg.*parse|flag Source/` → nothing in stdlib surface). Note
**D-CLI1** (c11) governs how `jet run` passes `--flags` *through* to the program —
this plan is the program-side parsing of those flags, a different layer.

## Pipeline touch points

- **stdlib** (`core.args` or `jet.args`): an arg-spec builder/struct, the parse
  loop, typed coercion, `--help`/usage generation, error reporting.
- **sema / comptime**: a struct-driven spec (`#[Args] struct Cli { … }`) would
  need field reflection — same S56-vs-comptime tension as D-CSVROW1/D-JSONOUT1.
  A builder form needs no compiler work.
- **diagnostics**: bad-flag / missing-required / wrong-type errors. These are
  *user-program* runtime errors (the user's CLI rejecting input), not compiler
  diagnostics — define their voice/format.

## Invariants in play

- **I8** one parsing story; don't ship both a builder and a derive that diverge.
- **One-path / beginner-experience**: the default should auto-generate `--help`
  and good error messages so a beginner gets a polished CLI for free.
- **I5** example: a tool with a flag, an option-with-value, and a positional.
- Does **not** depend on S56 if a builder/spec-value form is chosen.

## Open questions (need owner decision — D-ARGS1)

1. **Spec surface** — (a) a struct annotated `#[Args]` whose fields are flags
   (clap-derive style; needs comptime reflection or S56); (b) a builder value
   (`args.flag("verbose").option("output", String).positional("input")`); (c) a
   declarative table value. Pick one; avoid blocking on S56.
2. **Short vs long flags** — support `-v`/`--verbose` pairing, `-n 5`/`--num 5`,
   `--key=value` and `--key value` both? Bundled short flags (`-rf`)?
3. **Typed coercion + required/optional/default** — how are `Int`/`Bool`/`String`
   options typed and how is a missing required option reported?
4. **Auto `--help`** — generated from the spec automatically (yes), and a `--version`
   convention? What does the generated usage look like (the product copy)?
5. **Subcommands** — in v1 scope (`tool add …` / `tool remove …`) or deferred?

## Test plan

1. `examples/features/cli_args_parsed.jet` — declare a spec with a flag, a
   value-option, and a positional; print parsed values; golden output (I5).
2. `--help` golden snapshot (the generated usage text is product copy).
3. Error cases: unknown flag, missing required, wrong type → each a golden error.
4. `--key=value` vs `--key value` equivalence test.
