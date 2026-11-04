FFIDesign-1: #CLI emits a parser written for the Args fields (#6497 size root 1)

Base: dev 45ab3b5ef5. The patch also applies to 6995079ddb: `git apply --check -p1` is OK on an export of
the 8 files at 6995079ddb, and none of the 8 files changed between the two commits. Eight files:
Compiler/JetSema/Source/Sema/Registration/CliEntry.jet, Core/Args/Args.jet, Compiler/Prelude/Core.jet,
Compiler/JetCli/Source/Cli/StructureRegistry.jet, and the generated tables
Compiler/JetCli/Source/Cli/CoreSources.jet and
Compiler/JetFoundation/Source/Registry/{CoreCallRows,CoreEnumRows,CoreSourceOwned}.jet. The tables were
rewritten by `node Tools/agent/gen-core-tables.mjs --write`, and `--check` passes. Chain s1 (seed a0_022) built
with rc=0, rows=0: s1 jet bed7096657bfe75d, pack1 04516b9cca3fd029. The runtime native key is unchanged
(f00985fa44cda6a9).

What changed

1. Generated parser (CliEntry.jet). A #CLI struct without commands now gets a parser written for its own
   fields. The code does the same steps as core.args cli_parse / cli_parse_level, in the same order:
   - `--`;
   - long names: help and version, `--name=value`, bare flags, options that take the next word, and unknown
     names with the "did you mean" hint;
   - short clusters;
   - positionals;
   - then positional fill (a named value wins), surplus, env and default fallbacks, missing arguments,
     missing required options, help, version, and the value checks.

   Each field has its own locals: `__v{slot}` holds its text, `__f{slot}` its flag, and `__x{slot}` whether
   it was given explicitly. There is no map, no spec string and no table walk. The generated branches use
   no `else` chains, because parse_generated_source counts the L0507 style lint as a diagnostic.
2. Help as data. The help and usage text is one string constant built at compile time.
3. Per-kind value helpers. Values are decoded by helpers that take the field's text: `__cli_int_of`,
   `__cli_float_of`, `__cli_bool_of`, `__cli_text_of`, and their `_opt_of` forms. The parser calls only the
   kinds the struct declares, so a program with no Float field links no Float parser. In nm of collatz, fib
   and isum, the base has 1 float-parse symbol and 2 map symbols; the patched builds have 0 and 0.
4. Shared refusal texts (Core/Args/Args.jet). Each refusal text is now one function that the table
   interpreter, the generated parser and `args_store` (the ArgsSpec path) all call, so the texts cannot
   drift apart: `__cli_int_refusal`, `__cli_float_refusal`, `__cli_choice_refusal`, `__cli_unknown`,
   `__cli_surplus`, `__cli_missing`, `__cli_usage`, `__cli_with_usage`. Also in Args.jet:
   - the table decoder reads choices by count instead of splitting on `|`;
   - the map getters `__cli_has`, `__cli_text`, `__cli_int`, `__cli_float`, `__cli_bool` and their `_opt`
     forms are removed, because nothing calls them any more.

   The Core.jet and StructureRegistry.jet name lists and the generated tables are updated to match.
5. Entry shape for a struct without commands. The entry is now two functions:
   - `__jet_cli_parse() -> T` does the parsing. Help and version print and then call `process.exit(0)`
     there. Output and exit code stay the same; the projections already ended help this way.
   - `__jet_cli_run()` is `__p :: __jet_cli_parse()` followed by `run(T{f: __p.f, ...})`.

   Why the struct literal: with `run(__jet_cli_parse())`, the inlined `run` read `args.n` through the
   address of the call result. That load took r13 in the collatz hot loop. X64Select copies an r13 lea
   base, so the loop gained `mov %r13,%rbx; lea (%rbx,%rbx,2),%rbx`: +4.5% instructions and +5.4% cycles.
   With the literal, the loop is `lea (%r14,%r14,2),%r12` with no copy, the same shape as the base.
6. Projections (`args.decode<T>()` and `T.merge`). Each projection is now a short function that calls a
   generated parser, `__jet_cli_decode_raw` or `__jet_cli_merge`. The parser returns `T?` and passes the
   refusal text through `&String`; the short function turns a refusal into the single `FieldError`. The
   short functions are placed first in the generated text.

   Root cause: spans in generated code are offsets into the generated text, but the IR error journey
   (ir_journey_site, BuildExpr.jet) reads an `Err` site as a position in the user file. With the larger
   written parser, those sites fell past the end of Examples/features/serde/shape_projection.jet
   (2138 bytes), and the IR stopped with "journey site span falls outside its source".

   This fault is already in the base: the base compiler hits the same ICE on any small file that calls
   decode or merge. The patch moves the sites earlier, so every file the base builds still builds, and
   some that the base fails on now build. The fault itself is not removed (see Not fixed).

Evidence

- Output identical. Seven #CLI programs were built with the base compiler and with the patched compiler and
  run on the same argument lists, covering help, version, errors, env, defaults, choices, short clusters,
  `--`, and the decode/merge refusals. Script: $JET_SCRATCH_ROOT/lanes/FFIDesign/cli/smoke.sh (gone by 2026-11-04).
  - wordfreq, cmds, rich, tensormap and collatz: stdout, stderr and exit codes are IDENTICAL.
  - proj and projcmd (decode and merge, with padding so the base builds them): identical except the
    journey "failed here:" location line on a projection refusal. That line points into generated code in
    both builds, so it is wrong in the base too; it now names a different wrong line.
  - Sizes, base -> patched: wordfreq 341328 -> 257864, proj 329960 -> 280976, projcmd 296936 -> 280024,
    cmds 274888 -> 266224, rich 308032 -> 249976, tensormap 397000 -> 296584, collatz 294448 -> 207640.
- Asm-parity census (measure.sh, base 45ab vs patched). Bytes:

    program     before   after    delta
    hello        27928    27928        0   (no #CLI; machine code identical)
    collatz     294480   207672   -86808
    fib         277920   195208   -82712
    isum        286424   203696   -82728
    sieve       290504   207808   -82696
    wordfreq    341352   257896   -83456
    nbody       300952   218152   -82800
    bulkrename  911312   824008   -87304
    tensormap   397024   296608  -100416

  No cell grows.
- Cycles. Interleaved before/after runs under perf stat cycles:u,instructions:u, pinned to one CPU; the
  figure is the median (bench.mjs).

    program     cycles %   rounds/CPU   instructions before -> after
    collatz       -8.95     11 / 8       9552891192 -> 9552839312
    fib          -10.91     11 / 8       6789121832 -> 6789070108
    isum          -1.30     21 / 24      1000366251 -> 1000314025
    sieve         -8.43     21 / 24        46281924 ->   46229583
    wordfreq      -2.45     11 / 8       3801110490 -> 3801062288
    nbody         -0.05     21 / 24        91242949 ->   91244906
    bulkrename    +0.49     41 / 24        28923895 ->   28876674
    tensormap     -5.39     21 / 24      8390747661 -> 8390735575
    hello      (+2.45, +6.71)  21, 41    3143 -> 3143 (same machine code: this is the noise floor)

  Noise controls on the same CPU, same binary on both sides (A/A): isum +2.49%, bulkrename -0.16%,
  hello +1.79%.
  - bulkrename +0.49% is inside that noise. It runs 47k fewer instructions.
  - An earlier 11-round isum run on the loaded machine read +3.69%. Its hot sum loops are instruction-for-
    instruction the same as the base, at the same 64-byte alignment. The fill loop has the same 11
    instructions; one slot load moved from `n` to the list base. The quiet 21-round run reads -1.30%.
  - nbody runs 1957 more instructions (+0.002%) because `run` is now inlined into the small entry; its
    cycles read -0.05%.
- collatz hot loop. Base: `lea (%r12,%r12,2),%r15`. Patched: `lea (%r14,%r14,2),%r12`. There is no copy
  in front of either.
- Lane check: Tools/agent/lane-check.sh with the 45ab compiler printed CHECK OK.
- Goldens, ui and cases: per Main, the wave's goldens, ui and cases are the oracle. My own CLI oracle was
  cancelled before it ran. On the old base (b1eb, an earlier version of this change), the goldens census
  passed 1202/1202 (io/process_run_all newly passing). The one regression that run found,
  serde/shape_projection, is the ICE fixed in item 6. With the final compiler,
  `APP_PORT=9000 jet run Examples/features/serde/shape_projection.jet -- --listen-port 7000` prints
  9000 / example / 7000 / localhost / 9100 / db-host.

Not fixed / deviations

- A struct with commands still uses the table interpreter (cmds: -8.7 KB only, from the shared refusal
  functions and the removed getters). Command specs are the next step.
- Env values are read before the parse, for every field that names an env key. The base read them after
  the parse. The values and the precedence are unchanged.
- Generated-code spans: in a small file that calls `args.decode<T>()` or `T.merge`, the journey ICE above
  can still occur. Probe: $JET_SCRATCH_ROOT/lanes/FFIDesign/cli/projcmd (gone by 2026-11-04) without its padding (426 bytes). The
  base ICEs on both decode and merge; the patched compiler ICEs on merge only.
  Suggested follow-up: in the IR, treat generated CLI projections as no-hop, as `ir_cli_entry_function`
  already does for the entry, or give generated code its own source file.
- The journey "failed here:" line for a projection refusal is wrong in the base and still wrong, now at a
  different line.
- Core/Args/Args.jet keeps its existing `use core as core`.
- RuntimeStopExports was not regenerated; the runtime is untouched.
- Lane-check on a single Core file gives only E0067 (`__` names in user code), which is expected.
- Census and bench ran unslotted and one process at a time. Cycle numbers are from a shared machine; the
  A/A rows above give the noise.

#5459 (capability diff, planning deliverable)
- Plan: $JET_SCRATCH_ROOT/lanes/FFIDesign/capdiff/PLAN.md
- Ballot: $JET_SCRATCH_ROOT/lanes/FFIDesign/capdiff/D-CAPDIFF1.json (gone by 2026-11-04), built by build-capdiff1.mjs. Implementation is
  blocked on: the `jet update` port for registry packages; `pkg_update_lock_provenance` having no caller; there
  being no record of #Unsafe sites; and the owner's decision.
