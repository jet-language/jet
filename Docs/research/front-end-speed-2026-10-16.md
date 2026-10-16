# Front-end speed: native lexer and parser (2026-10-16)

This note measures the Jet-authored lexer and parser (`Compiler/JetLexer`,
`Compiler/JetParser`) as the native compiler runs them. It records what was
changed on `perf-next` and ranks the remaining architectural work by
estimated gain.

## Method

- **Driver.** A scratch unit contains every `Compiler/JetFoundation` source
  except `Target/TargetMachine.jet` (that file needs Jetpack Authority), plus
  JetLexer, JetParser and a small `run` driver, in `sources.list` order. The
  package imports are blanked as `Compiler/Bootstrap/assemble.mjs` blanks
  them. The driver times `lex(bytes)` and then
  `parser_parse_lexed_lines(^lexed, …)`. In dump mode it writes three
  projections:
  - every token, with its kind, span, text, decoded name, literal facts and
    string parts (recursively), plus the lexer diagnostics;
  - the parse diagnostics and the span of every expression, walked through
    all function, method and module bodies with `ast_visit_body`;
  - the `jfmt_format_program_with_tokens` rendering of the program.
- **Build.** The p26 factory
  (`source-34c0…/regression/Main-DevGreen-sharded-stage0-p26/retained/jetc0`)
  built the driver through `pack-r17zh/run-factory.sh selfhost`, with the
  candidate's runtime pack. That is the O0 native code the self-hosted
  compiler runs. One driver build takes about 15 to 19 minutes and peaks at
  8.7 GB RSS plus about 10 GB of swap.
- **Inputs.** The inputs are prefixes of the assembled compiler unit
  (`p26/assembly/compiler/selfhost.jet`, 25.4 MB), cut at source-file
  boundaries:
  - s1: 2.27 MB, 434,786 tokens;
  - s3: 3.17 MB, 520,171 tokens;
  - the full unit.

  Each run used a 1 GiB stack and its own memory-capped systemd scope.
- **Profile.** `perf record --call-graph fp` was run on s3. The executable
  has no symbol table, so sample addresses were mapped back through the
  `native.o` symbols.

## Measurements

Wall times below are the driver's own `lex_ms` and `parse_ms`, three runs per
cell. Peak RSS comes from GNU `time`.

| Input | Build | Lex (ms) | Parse (ms) | Total (ms) | Peak RSS |
|---|---|---|---|---|---|
| s1 2.27 MB | before | 1164–1234 | 3458–3776 | 4669–4941 | 937 MB |
| s1 2.27 MB | after | 1063–1340 | 2107–2352 | 3416–3454 | 937 MB |
| s3 3.17 MB | before | 1573–1825 | 3979–4513 | 5653–6086 | 1026 MB |
| s3 3.17 MB | after | 1375–1514 | 2511–2660 | 3893–4078 | 1026 MB |
| full 25.4 MB | before | — | — | killed by OOM at the 12 GB cap after 188 s (179 s CPU) | >12 GB |
| full 25.4 MB | after | — | — | killed by OOM at the 16 GB cap after 100 s (59 s CPU) | >16 GB |

- **Parse.** Parse time fell by about 38% on both slices (s1: 3.6 s to
  2.2 s; s3: 4.2 s to 2.6 s). Total lex+parse time fell by about 30% on s1
  and 33% on s3.
- **Lex.** Lexer time did not change measurably. The s1 ranges overlap, and
  on s3 the after range is about 10% lower, which is near the noise.
- **Memory.** Peak RSS did not change. It is about 410 bytes of RSS per
  source byte, or about 2.1 KB per token, and memory is now the binding
  constraint. The full compiler unit cannot be lexed and parsed under a
  16 GB cap. At the observed rate it needs roughly 10 to 11 GB just to hold
  the tokens and the AST, plus whatever the run peaks at above that.
- **Full unit.** The after build reached the 16 GB cap in a third of the
  CPU time the before build used to reach 12 GB. A full-unit wall time needs
  the shared big-job lock (more than 16 GB) and was not run tonight.
- **Allocations.** Allocation counts were not measured: the runtime exposes
  no allocation counter to the driver.

Speed is about 100–130 k tokens/s, or 0.5–0.8 MB/s. The 1M LOC/s target is
roughly 40 MB/s, so a 50–80× gap remains.

### Where the time went (before, s3, 10,151 samples)

| Share | Site |
|---|---|
| 21.8% | `parser_parse_interpolation`: `source: state.source` copied the whole source `[U8]` into the nested state of **every** string interpolation (this cost grows with source size × interpolation count) |
| 10.6% | `diagnostic_registry_row`: reading the constant `DIAGNOSTIC_ROWS` cloned the whole registry list on every `diagnostic_from_registry` call, mostly from speculative parses that create an error and then rewind. Fixed separately by #5001 (static constants), which is not yet on perf-next |
| 13.7% | `name_intern_bytes`: FNV hash (5.3%) and byte-equality probe (5.5%), called per token |
| ≈13% | implicit `Expr`/`ExprField` clones: field reads such as `fields.fields`, `left.expr`, and pattern bindings moved into new nodes (`parser_parse_typed_literal_body`, `parser_parse_postfix`, `parser_parse_binding_statement`, `ast_binding`, `ast_const`) |
| 63% (self) | runtime allocator and copy code reached from the sites above |

## What changed

1. **Parser: interpolations borrow the source text** (`JetParser`). This
   landed on dev as 704578171/ce65d90a7. On the rebase onto c2704c81f,
   perf-next dropped its own equivalent commit (e6464902d). The
   parser state's `source` is now one lent row (`[[U8]]`), like `tables`.
   `parser_lend_source` and `parser_return_source` move the row into an
   interpolation's nested state and back, so the text is never copied. All
   readers go through `parser_source_len`, `parser_source_byte` and
   `parser_source_slice`, and "no row" reads as the empty source exactly as
   before. Pattern-hole nested states still parse without source text, as
   they did. This removed the 21.8% hot spot and the per-interpolation O(n)
   copy.
2. **Lexer: ASCII fast paths** (`JetLexer`).
   - The raw scanner tests ASCII whitespace and identifier starts inline
     before its punctuation chain.
   - `scan_identifier_end` tests ASCII identifier bytes inline.
   - Line comments and plain string bodies (`scan_plain_string_end`) advance
     over plain ASCII bytes inline.
   - The payload pass's `gap_diagnostic` skips ASCII whitespace inline, and
     `decode_string_content` copies plain ASCII text bytes inline.

   Non-ASCII bytes keep the scalar path. These changes cut per-byte calls,
   but the lexer's time is dominated by interning and by the payload and
   terminator passes, so they do not show at this noise level.

**Byte identity.** The before and after dumps are identical: 5,504 files
compared, 0 differences. Dump files were compared byte for byte. Driver
stdout was compared after removing the timing fields and the driver's own
panic line numbers. The dumps cover token streams with literal facts and
string parts, lexer and parser diagnostics, the expression spans of every
body, and the formatter output. The inputs were s1, s3 and all 1,398
`Examples/**/*.jet` files, each lexed and parsed alone. Both builds behave
the same way on the problem inputs:

- `features/comptime/prep_loop.jet` and `features/reflection/derive_loop.jet`
  hang past 60 s, before and after. This is a pre-existing lexer or parser
  non-termination and needs its own card.
- Directory-shaped `.jet` names fail to read in both builds.

**SIMD.** Core exposes no byte-vector type and no unaligned word load from
`[U8]`. `F64x4`/`Vec*` are plain fixed arrays, and integers have
`trailing_zeros`, `leading_zeros` and `count_ones`. A word-at-a-time
(SWAR) scan needs a `[U8]` → `U64` little-endian load at an index; without
it, building a word costs eight checked byte loads, which is slower than the
byte loop. **Finding: missing primitive `[U8].load_u64_le(index)` (or an
equivalent byte-vector view).** With it, whitespace, comment, string and
identifier runs could advance 8 bytes per step using
`(w - 0x0101…) & ~w & 0x8080…`-style masks and `trailing_zeros`.

## Ranked further opportunities

1. **Compact AST and token storage (memory, then time). Estimated 5–10×
   less RSS and 2–3× faster parsing.** About 2.1 KB of RSS per token is the
   limit today: the full compiler cannot be parsed in 16 GB. `Expr` is one
   wide enum whose nodes are value trees, so each subtree is a separate heap
   graph and implicit copies duplicate it. Move to arena or struct-of-arrays
   nodes: an `ExprId` index into per-kind columns, spans as two `U32`
   columns, and names as interned `Name` ids instead of `String` in the AST.
   Sema then reads ids. This is the biggest lever for the 0.5–1 GB goal.
2. **Remove implicit subtree clones in the parser (≈13% of parse
   now).**
   - Move fields out with `^` where the source is dead, for example
     `^fields.fields` in `parser_parse_typed_literal_body`, and move
     `left.expr` / `right.expr` into new binary and postfix nodes.
   - Have `ast_binding` and `ast_const` take their payloads.

   `jet audit copies --json` lists every implicit copy site: 1,724 in the
   parser (2,331 with the formatter) and 66 in the lexer on this tree. With
   item 1, most of these disappear.
3. **Static constant reads (10.6%, landed elsewhere as #5001).** After
   perf-next takes #5001, re-measure on the f2 factory. Separately, stop
   building registry diagnostics inside speculative parses that are rewound
   anyway: probe first, then report.
4. **Interning (13.7%).**
   - The scanner already knows each identifier's bytes, so fold the FNV hash
     into `scan_identifier_end` (one pass).
   - Compare a stored 32-bit hash before the byte compare.
   - Size the name table from the source length to avoid `name_table_grow`.

   Keywords are already classified before interning. Estimated: halves the
   lexer's interning cost.
5. **Fuse lexer passes.** The lexer walks the bytes three times: the raw
   scan (`raw_token_facts`), the payload pass (which re-walks every gap in
   `gap_diagnostic`) and the terminator pass (`newline_between`). It then
   rebuilds the token list (`lexed_with_terminators` reverses and pops).
   - The scanner can record whether each gap held a newline, and whether it
     held only whitespace, as fact bits.
   - The payload pass can then skip clean gaps.
   - The terminator events can be produced in the same loop.

   Estimated: 30–40% of lex time.
6. **SWAR / SIMD scanning.** This needs the word-load primitive above
   (owner-gated as new surface; it needs a paired performance cell).
   Estimated: 2–4× on comment, string and whitespace runs once items 4 and
   5 have removed the per-token overheads that dominate today.
7. **Post-parse token passes.**
   - `parser_used_import_names` and `parser_debug_trace_sites` each re-walk
     every token, including interpolation tokens.
   - `parser_collect_debug_trace_sites` builds a `closes` list as long as
     the whole token stream for every call, interpolations included.

   Fold both into one walk, and allocate `closes` only for `(`.
   Estimated: about 3% of parse.
8. **Optimization level.** The native compiler is built at O0 (`plan.json`
   `optimization: O0`). Small leaf helpers such as `byte_at`,
   `parser_current_kind`, `kind_at` and `parser_is_trivia` stay real calls.
   Building the compiler at O1 with leaf inlining would likely give 1.5–2×
   across the front end with no source change, once the backend's O1 is
   trusted for the fixed point.
