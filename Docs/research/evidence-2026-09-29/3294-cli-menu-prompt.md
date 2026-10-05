# #3294 — metadata-driven interactive CLI selection (D-CORE-CLI-MENU1=A)

Date: 2026-09-29. Binary: jet-debug-snapshot14 via `~/.cache/jet-dev/safe-jet.sh`.

## Question

Can an ordinary interactive selection job reuse the canonical command schema
(ArgsSpec / `#CLI` metadata) instead of a second menu tree, with explicit
cancel/EOF/invalid/secret/accessibility behaviour and no hidden dispatch? What
exists today, and what remains to build?

## Sources read

- Tower decision D-CORE-CLI-MENU1, ratified **A** (2026-09-12):
  `ArgsSpec.prompt(input, output) -> ParsedArgs?` plus a stdin/stdout
  convenience; one schema owns names/help/validation/navigation; None on
  explicit cancel or clean EOF before a complete selection; malformed input is a
  structured ArgsError; IO failure keeps IOError; one selection per call; the
  caller dispatches; no handler runs, no scheduler, no screen clear.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/Args.rs:555` `jet_args_parse_guided`
  (parse with missing fields allowed) and `:1049-1310` guided field listing,
  validation and argv append. Guided fields are derived from the same
  `JetArgsSpec` the argv parser uses (`jet_args_guided_fields(target)`), and
  the final argv is re-parsed by `jet_args_parse`.
- `crates/jet-codegen/src/Prelude/Core/ArgsProjection.rs:221-359`: TUI prompt
  (`jet_args_guided_tui_prompt`), line prompt (`jet_args_guided_line_prompt`),
  `jet_args_guided_interactive` (stdin and stderr are terminals, no machine
  output, no `CI`), `jet_args_guided_accessible` (`NO_COLOR`,
  `JET_ACCESSIBLE`, `JET_CLI_ACCESSIBLE`, `TERM=dumb` select the line prompt).
  EOF is read from `JetTermRead::EndOfInput` / `JetKey::Unknown`, not from an
  empty string; both become the error "guided input ended before the form was
  submitted". Escape/interrupt map to the `escape` key.
- `crates/jet-codegen/src/Codegen/MIRRust.rs:10718-10725`: generated `#CLI`
  entry calls `jet_args_guided_argv_tui(&__spec, &__argv)` before
  `jet_args_parse`; a guided error prints a banner and exits 2.
- `Core/args/args.jet`: the CoreLib `core.args` module exposes `spec()`,
  `decode`, `decode_argv`, `merge` over `DataTree`; `ArgsSpec` there has only
  `flags` and no `option_int` or `prompt` method.

## Evidence (run)

- `safe-jet.sh check ~/.cache/jet-dev/scratch/Closer08/prompt.jet` (the
  ballot's own proposed snippet `args.spec().option_int(...)`, `spec.prompt()`):
  `Error [E0102]: ... ArgsSpec has no method option_int`. There is no public
  `prompt` operation (no `ArgsSpecPrompt` op in
  `Compiler/JetSema/Source/Sema/Calls/ProcessHandles.jet`).

No PTY session was run: the ratified `prompt` operation does not exist, so
criterion 7's command fixture cannot be driven. The existing guided field-fill
path was read in source only; it was not exercised under a terminal here.

## Verdict per criterion

1. Separate interactive input from nonexecuting completion extraction — **met
   as design** by D-CORE-CLI-MENU1=A: prompt returns `ParsedArgs?` and never
   dispatches; completion extraction (D-SHAPE-CLI-COMPLETE1) stays a
   nonexecuting read of the same spec. Today's guided path already only
   produces argv that is re-parsed; it does not run handlers.
2. Cover cancel/EOF/invalid choice/accessibility — **met as design**
   (ratified contract above); today's guided path covers EOF (explicit error,
   not empty string), invalid values (re-prompt with a Correction line) and
   accessibility (line prompt under NO_COLOR/JET_ACCESSIBLE/TERM=dumb), but
   maps EOF to an error, not to `None`, and has no cancel-as-None.
3. Avoid a second command schema — **met as design**: guided fields and the
   ratified prompt both derive from `JetArgsSpec`.
4. Ballot any missing Core menu API or placement — **met**: D-CORE-CLI-MENU1
   ratified A; no further ballot needed.
5-9. Require the implemented `ArgsSpec.prompt` and PTY tests
   (`cli_surface args_prompt_*`) — **unmet**: the API does not exist on the
   current tree. This card is an implementation card (compiler op + Prelude
   `jet_args_prompt`), outside a closer's no-compiler-change scope.

## Missing implementation (from the card plan, confirmed absent)

- `ArgsSpecPrompt` op (arities 0 and 2) in ProcessHandles.jet + HandleMethods.jet
  route + Rust TIR mirror.
- `jet_args_prompt` in Args.rs promoting the guided loop to subcommand
  selection with nested navigation/back, secret options through the existing
  secret input, EOF/cancel → None, non-TTY → capability error.
- `Examples/features/cli/args_prompt.jet` and the two PTY tests.
