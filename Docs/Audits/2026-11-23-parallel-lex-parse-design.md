# Parallel Lexing and Parsing Inside One File — Design Note (`#6796`, Rev 2)

**Date:** `2026-11-23` · **Base commit:** `fb1cb69a62` · **Card:** `#6796` (`c1`–`c5`) · **Revised to address all findings of Adversarial Review 1 (`1355890c`)**

## 1. Problem & Measured Live Baselines (`fb1cb69a62`)

Jet's largest modules are single files where module-level parallelism (`#6794`) cannot split work across threads. Live `JET_TRACE_FILE` measurements on `fb1cb69a62` prove that existing chunked lexing (`Compiler/JetLexer/Source/Lexer/Chunks.jet`) and chunked parsing (`Compiler/JetParser/Source/Parser/Entry.jet`) achieve **zero effective intra-file scaling** on the largest `#5479 c1` modules (`Core/Math/Math.jet` at `1.33 MB`, `Statements.jet` at `850 KB`, `CheckProgram.jet` at `787 KB`):

| Target File | Size (bytes) | `strict(\n}\n)` Cuts (`Chunks.jet:123`) | Real Top-Level Item Cuts | `1` Thread Parse Wall | `16` Threads Parse Wall | Observed Speedup |
|---|---:|---:|---:|---:|---:|---:|
| **`Core/Math/Math.jet`** *(largest `#5479 c1` module)* | `1,332,325` | **`0`** | `900` | `78.31 ms` (`58.8 ns/B`) | `79.08 ms` (`59.4 ns/B`) | **`0.99×`** |
| **`Compiler/JetSema/Source/Sema/Statements.jet`** | `849,512` | **`0`** | `452` | `51.41 ms` (`60.5 ns/B`) | `47.07 ms` (`55.4 ns/B`) | **`1.09×`** |
| **`Compiler/JetSema/Source/Sema/CheckProgram.jet`** | `786,584` | **`0`** | `619` | `38.96 ms` (`49.5 ns/B`) | `39.52 ms` (`50.2 ns/B`) | **`0.99×`** |

### Complete Root-Cause Audit in Live Code
1. **Broken boundary predicate (`Chunks.jet:123`):** `lexer_chunk_head_at` checks `source[at - 1] != 10 || source[at - 2] != 125 || source[at - 3] != 10` (`\n}\n` immediately followed by an item head). Because `jet fmt` always emits a blank line (`\n\n`) and/or `//` doc comments after `}`, `lexer_chunk_cut` returns `0` cuts on every formatted file and silently falls back to `lexed_threads_single`.
2. **Four hard worker caps:**
   - `Chunks.jet:25`: `LEXER_CHUNK_MIN_BYTES: 262144` (caps `0.85–1.33 MB` files at 3–5 lexer workers).
   - `Entry.jet:374`: `PARSER_CHUNK_MIN_TOKENS: 65536` (caps `0.85–1.33 MB` files at 2–5 parser workers).
   - `Loader.jet:4822`: `JET_DRIVER_DEMAND_MIN_TOKENS: 65536` (caps core-identifier demand scan at 2–5 workers).
   - `Loader.jet:5684`: `jet_driver_parse_wave` calls `jet_driver_parse_unit` with `compiler_threads_single()`, disabling intra-file threading completely inside multi-file builds (`jet check`/`jet build Compiler`).
3. **16× per-worker deep clone + ~10 serial full-file passes (violating Amdahl's law for `>= 8×` at 16t, which requires serial fraction `< 6.7%`):**
   - `Entry.jet:376–378, 749–752`: `parser_parse_chunk_part` captures whole-file `TokenTables`, `source`, and `line_starts`, which Jet task read-captures deep-copy per worker (16× serial copy at dispatch!).
   - `Entry.jet:249`: `parser_colon_declarations` runs 100% serially over the whole token stream.
   - `Chunks.jet:193–216, 316–318`: serial `lexer_chunk_name_map`, serial `compiler_transfer_rows` across 5 vectors, and `reverse(); pop(); reverse()` double-reverse per chunk.
   - `Loader.jet:4973–4974`: serial `name_pool_adopt` and `jet_driver_unit_spellings` (`CoreReach.jet:832–884`).
   - `Entry.jet:652, 684–710, 179`: serial `parser_marker_scopes`, 11× `compiler_transfer_rows`, `parser_used_import_names` (3 token scans), `parser_reexport_declarations`, `parser_debug_trace_sites` (2 scans whenever `"debug"` appears), `parser_state_marker_migration`, and `parser_join_effect_impls`.
4. **Scalar byte classification (`Scan.jet`):** Byte-by-byte scalar scanning (`49.5–60.5 ns/B` lex+parse combined) rather than vectorized block classification (`<= 1.0 ns/B` per `#6796 c5`).

## 2. Strongest Existing Implementations (Fair Best Case & Gaps)

| System & Primary Source | Mechanism & Best-Case Performance | What It Does That This Design Does Not (Trade-Off) |
|---|---|---|
| **simdjson** (Langdale & Lemire, *Parsing Gigabytes of JSON per Second*, VLDB 2019; `src/generic/stage1/`) | 64-byte AVX2/AVX-512 `vpshufb` nibble-LUT classification + `pclmulqdq` prefix-XOR in-string masking + structural bit-index extraction (`2.5–6 GB/s` single core). | JSON has a trivial regular lexical grammar with no nested `/* ... */` comments, no triple-quoted/backtick raw strings, no `#FFI` raw bodies, and no synthetic semicolon terminator rules; Jet must combine SIMD stage-1 classification with speculative chunk verification for unbounded multi-line constructs. |
| **Clang / LLVM** (`clang/lib/Lex/Lexer.cpp`, `SkipWhitespace`/`SkipLineComment` SSE2/NEON) | Vectorized whitespace/comment skipping on contiguous `MemoryBuffer` slices; zero-copy `StringRef` token views into source buffer. | Clang operates on raw C++ pointers without RC or bounds checks and parses translation units serially per file (module/file parallelism only). |
| **Zig** (`lib/std/zig/Tokenizer.zig`, `lib/std/zig/Ast.zig`) | Flat SoA `TokenList` + flat `NodeList` storing `u32` byte offsets directly into source with zero per-token heap allocation or `NameTable` copies. | Zig does not need per-chunk AST-node `Name` remapping because tokens stay flat indices into source until Sema; Jet keeps current AST node layout in `#6796` until `#6793 c3` replaces AST nodes with flat SoA columns. |
| **Oxc** (`crates/oxc_parser/src/lexer/`, Rust) | Arena-allocated bump AST + SIMD whitespace/identifier/string fast-skipping (`~1–2 GB/s` lexing single core) with zero RC drops. | Rust's bumpalo arena + `&'a str` borrows avoid RC and task-capture copies; Jet relies on lexical `task.group` read borrows (`TaskGroups.jet:648–655`) and per-chunk local tables here until `#6793` arenas land. |
| **Jet `X64/Count.jet`** (`Compiler/JetBackend/Source/X64/Count.jet`, `Encoder.jet:1086–1387`) | Runtime CPUID/XGETBV cached dispatch: AVX-512 (`256 B/step`), AVX2 (`128 B/step`), SSE2 (`64 B/step`), hitting `1.00×` asm ceiling on `byte_count`. | Proves Jet's encoder already supports `vpcmpeqb`/`vpmovmskb`/`evex_vpcmpeqb_mem`, **but** `Encoder.jet` still lacks `vpshufb`/`pshufb`/`pclmulqdq` and `x64_simd_dispatch_probe` (`Encoder.jet:1325–1387`) does not yet probe `SSSE3` or `PCLMULQDQ` (addressed in Slice 2). |

## 3. Runnable Probe (Cold-Streamed 64 MiB Full Structural Classifier)

To test hardware feasibility of `#6796 c5` (`<= 1.0 ns/byte`) honestly on **cold memory larger than LLC** AND with real classification work (not a cache-hot `popcnt` toy), we compiled and executed a full 32-byte AVX2 structural block classifier via `jet cc -O3` (`__attribute__((target("avx2,pclmul,popcnt")))`) performing:
- Two `vpshufb` nibble-LUT lookups + bitmask extraction (`ws_mask`, `struct_mask`),
- Exact `vpcmpeqb` + `vpmovmskb` masks for `\n`, `"`, `\`, and non-ASCII UTF-8 high-bit (`>= 0x80`),
- Escape + quote `pclmulqdq` prefix-XOR carry (`in_string_mask`) across 32-byte blocks,
- **24 bytes written per 32-byte input block** (`6 × uint32_t` bitmasks written to pre-faulted output memory),
measured on a single **64 MiB cold streaming pass** (`> LLC`, tiled from real Jet source) AND a 200-iteration cache-hot pass:

| Source Input | Cold 64 MiB Stream Wall | Cold `ns/byte` | Cold Throughput | Cache-Hot `ns/byte` | Margin vs `<= 1.0 ns/B` (`c5`) |
|---|---:|---:|---:|---:|---:|
| `Statements.jet` (`66.26 MB` cold / `850 KB` hot) | `7.32 ms` | **`0.110 ns/B`** | **`9.05 GB/s`** | `0.074 ns/B` (`13.53 GB/s`) | **`9.1×` under budget** |
| `Core/Math/Math.jet` (`66.62 MB` cold / `1.33 MB` hot) | `7.44 ms` | **`0.112 ns/B`** | **`8.95 GB/s`** | `0.072 ns/B` (`13.97 GB/s`) | **`8.9×` under budget** |

**Explicit caveat:** This probe proves the **hardware + algorithm ceiling** in C (`jet cc -O3`) leaves a `~9×` safety margin below `1.0 ns/byte`. It does **not** yet prove Jet's own `-O2` codegen achieves `<= 1.0 ns/byte` on `Lanes<U8, 32>` + `#Multiversion` + `core.arch`; closing that codegen proof on the self-host unit is the explicit exit gate of Slice 3 (`c5`).

## 4. Target Architecture (`c2`–`c5`)

### 4.1 Sound Chunk-Boundary & Multi-Line Fallback Protocol (`c2`, Zero Ballot Required)
No language grammar change is needed because speculative chunking verifies exact equivalence to single-threaded lex+parse:

1. **Exact cut byte offset (`Chunks.jet:123–153, 294–302`):**
   - SIMD pre-scan locates candidate column-0 `\n}` positions (`m_rb & (m_nl << 1)` with 1-bit cross-block carry).
   - For each target split point, scan forward from `\n}\n` across zero or more blank lines (`[ \t\r]*\n`) and zero or more column-0 `//...\n` comment lines; accept the candidate when the next non-blank/non-comment line starts a **parseable** top-level item head (`#` marker/prep, `LEXER_CHUNK_HEAD_WORDS`, or `identifier:` followed by a valid declaration head).
   - **Critical invariant:** Place the chunk cut offset `cut` **immediately after `}\n`** (so `source[cut - 2] == '}'` and `source[cut - 1] == '\n'`, preserving `Chunks.jet:294–302`'s seed `}` position, exact `Terminators.jet` `{rbrace_end, rbrace_end}` `TOKEN_SEMI` span, and `gap_start` for `E0088`!).
   - Update `Entry.jet:630–636` (`parser_scan_run_part`) to skip leading `TOKEN_LINE_COMMENT` trivia before asserting `TOKEN_SEMI` + valid item head, and align `Entry.jet:417–419` (`parser_chunk_head_kind`) with `Chunks.jet:127–145` so top-level `TOKEN_IDENT` followed by `TOKEN_COLON`/`TOKEN_COLON_EQ` (matching `Entry.jet:1209`, never bare script statements at `Entry.jet:1230–1236`) and `TOKEN_PREP` (`#run`/`#if`/`#loop`, matching `Entry.jet:1186–1236`) are accepted without falling back to `whole = true`.
2. **All 5 multi-line lexical constructs & contiguous-run merge fallback:**
   - Live `Scan.jet` has five multi-line constructs that can contain `\n}\n` inside them:
     1. Triple-quoted strings `"""..."""` (`Scan.jet:482–491`)
     2. Multi-line raw backtick strings `` `...` `` / `` ``...`` `` (`Scan.jet:370–384, 1106–1115`)
     3. Nested block comments `/* ... */` (`Scan.jet:18–24, 523–541, 771–775`)
     4. Unclosed char literals `'...` (`Scan.jet:505–518`)
     5. `#FFI(lang)` raw foreign bodies (`Scan.jet:728–730`, `Payload.jet:475–484`, guarded by `bodies_local` at `Chunks.jet:298`)
   - **Contiguous-run merge fallback (sound for spans across `>= 3` chunks):** Chunk `k` is clean iff (a) its delimiters nest to depth 0, (b) its final code token is `TOKEN_RBRACE` at `rbrace_pos`, (c) `unclosed_block_comment == false`, and (d) `bodies_local == true`. Whenever chunk `k` is not clean, merge `[k, k+1]` and re-lex; if the merged span STILL fails end-of-chunk verification (e.g., a `/* ... */` or `"""..."""` spanning 3+ chunks), extend the merged run through `k+2, ...` until it realigns at a clean depth-0 `TOKEN_RBRACE` or EOF. All unaffected chunks outside that run stay untouched.
   - Preserve exact diagnostic ordering (`Chunks.jet:59–60, 218`: payload diagnostics before terminator diagnostics, in source order) and replace `Chunks.jet:316–318`'s `reverse(); pop(); reverse()` by omitting the seed `}` from emitted tokens directly.

### 4.2 Map-Reduce `parser_colon_declarations` & Eliminating All Serial Bottlenecks (`c2` + `c4`: `>= 8×` at 16t)
1. **Map-reduce `parser_colon_declarations` (`ColonDeclarations.jet:418–454, 620–634`):**
   - *Parallel per-chunk map:* Rotate unambiguous local colon heads (`fn`/`struct`/`enum`/`trait`/`module`), collect tiny per-chunk lists `(local_modules, local_others, local_candidates)` of top-level names.
   - *Microsecond whole-file merge:* Union `modules` and `others` across all chunks, run the exact whole-file `parser_colon_mark_instances` fixed point (`ColonDeclarations.jet:418–439`) over the combined candidate records.
   - *Parallel per-chunk apply:* Apply marked module-instance token rotations inside each chunk before parsing.
2. **Eliminate 16× `TokenTables`/`source` deep copy & lower all 4 thread caps:**
   - Lower `LEXER_CHUNK_MIN_BYTES` (`Chunks.jet:25`) `262144 → 32768`, `PARSER_CHUNK_MIN_TOKENS` (`Entry.jet:374`) `65536 → 8192`, and `JET_DRIVER_DEMAND_MIN_TOKENS` (`Loader.jet:4822`) `65536 → 8192`.
   - Keep **per-chunk `TokenTables` and token slices** directly into each chunk's parser worker instead of flattening tokens and copying whole-file `TokenTables` 16× (`Entry.jet:376–378, 749–752`). Share `source` across workers via zero-copy lexical `task.group` read borrow (`TaskGroups.jet:648–655`, already supported!) with per-chunk byte offsets.
   - Remap local chunk `Name` IDs onto the canonical chunk-order-merged `NameTable` deterministically (preserving byte-offset `NodeId`s from `State.jet:153–157` and identical `Name` IDs across 1 and 16 threads).
   - **Multi-file wave adaptive threading (`Loader.jet:5684`):** In `jet_driver_parse_wave`, grant intra-file chunk workers (`min(free_threads, max_chunks)`) to any file `>= 64 KiB` when queued files `< workers` (eliminating the single-file tail stall in multi-file builds).
3. **Parallelize the 9+ serial post-passes & fix pre-existing soundness bugs:**
   - Map-reduce `jet_driver_unit_spellings` (`CoreReach.jet:832–884`), `parser_marker_scopes` (`Entry.jet:652`), `parser_used_import_names` (`Entry.jet:763–993`), `parser_reexport_declarations` (`Entry.jet:708`), `parser_debug_trace_sites` (`Entry.jet:999–1055`), and `parser_state_marker_migration` (`Entry.jet:710`) inside each chunk worker, reducing only compact summaries on join.
   - **Pre-existing `#FX` bug fix (`Entry.jet:444, 675–711`):** Include `MARKER_FX` handling and merge `program.file_cap` from chunk 0 so chunked parsing never drops file-level capabilities.
   - **`FencedNames` (`<: ... :>`):** If any chunk sets `has_fences == true`, route through whole-file `FencedNames` expansion so `ParserFenceBudget` (`FencedNames.jet:89–94`) remains exact.
   - **`impl Name.Effect` / `impl Trait.Derive`:** Preserve `parser_join_effect_impls` (`Entry.jet:179, 190–205`) post-join over merged `items` in source order.
   - **Scope note on single-item generated tables (`DiagnosticRows.jet`):** We explicitly reject formatting-specific `    },` hacks. `c4` is proved on multi-item files (`Core/Math/Math.jet` at 1.33 MB — the largest single module of `#5479 c1` — and `Statements.jet` / `CheckProgram.jet` as the second large file).

### 4.3 Public Vector Tools for `<= 1.0 ns/byte` (`c5`)
Per ratified **`D-CPU-DISPATCH1=A`** (`#4168`: `#Multiversion(...)`, `core.arch`, `#if #build.cpu.has(.AVX2)`) and **`D-LANES1=A`** (`#4167`: `Lanes<T, N>` retiring legacy `U8x16`/`U8x32` under greenfield law):
- **Trace instrumentation (`Compiler/JetFoundation/Source/Trace.jet:32–39`):** Emit `sink.begin("lex.classify", "", -1, -1)` / `sink.end("lex.classify", source.len())` and `sink.progress("counters.lex.classify.bytes", source.len(), source.len())` (using existing `CompilerTraceSink` schema; reporter prints `ns/byte = dur_ns / bytes`).
- **Missing x86-64 encoder + CPUID support (`Compiler/JetBackend/Source/X64/Encoder.jet:1069–1387`):** Add `pshufb` (SSSE3), `vpshufb` (AVX2/AVX-512BW), `pclmulqdq` (optional for prefix-XOR), and CPUID bit checks for `SSSE3` and `PCLMULQDQ` in `x64_simd_dispatch_probe`.
- **Implementation slice order:**
  1. **Slice 1 (`c2` + `c3` + `c4`):** Fix boundary protocol (`}\n` cut + comment-skipping `Entry.jet:630–636` + aligned head kinds), contiguous-run fallback for all 5 multi-line constructs, map-reduce `ColonDeclarations`, lower all 4 thread caps + adaptive `Loader.jet:5684`, per-chunk `TokenTables` + zero-copy lexical `task.group` read borrow of `source`, map-reduce the 9+ serial passes, fix `#FX` `file_cap` + `FencedNames` fallback, verify exact `S2 == S3` + `golden`/`ui`/`cases` on all tiers + **`>= 8×` speedup at 16t vs 1t** on `Core/Math/Math.jet` and `Statements.jet` (committed receipt).
  2. **Slice 2 (`#4167` + `#4168` ratified vector prerequisites):** Ship `Lanes<U8, 16>` / `Lanes<U8, 32>` (`D-LANES1=A`, retiring legacy `U8x16`/`U8x32`), `#Multiversion(x86_64, x86_64_v3, x86_64_v4)` + `#if #build.cpu.has(...)` + `core.arch.x86` block-classifier intrinsics (`D-CPU-DISPATCH1=A`), plus `Encoder.jet` `vpshufb`/`pshufb`/`pclmulqdq` + CPUID probes — identical bits across CPU levels and `O0`/`O1`/`O2` (I9).
  3. **Slice 3 (`c5`):** Implement the 32-byte/64-byte `#Multiversion` `Lanes<U8, 32>` + `core.arch` block classifier in `Compiler/JetLexer/Source/Lexer/Classify.jet`, prove `<= 1.0 ns/byte` in `JET_TRACE_FILE` on the self-host unit (`jet check`), and re-verify full `c3` parity (`S2 == S3` + `golden`/`ui`/`cases`).
