# Per-function code size in the tool twins (2026-11-13, #6328 criterion 29)

Card #6328 criterion 29 asks that no Core or generated function carries code its callers
cannot reach, or expansion a hand-written version would not. This audit is the dated
breakdown that criterion asks for. It lists every function of 4 KB or more in the
tool-twin binaries, says what its bytes are, and names the general rule that shrinks
them. It is read-only; nothing here changes behavior.

## Build

- **Sources:** dev `ea1caf0d11`.
- **Compiler:** seed-b14 (`jet@a0.0.45#1e8010c1e134c5a9a1244b282b59998ff2a343a8ea0df88729a0a6bf69453b26`).
- **Target:** x86-64 Linux, default `jet build`.

The tool twins are the gauntlet entries with `"mode": "tool"`
(`Tools/gauntlet/entries/*/entry.json`). Their `tool.jet` paths name 63 programs. The five
fixture entries under `Tools/gauntlet/entries/*/fixtures/entries/` (`tool-entry`, `bare`,
`broken`, `greeter`, `plain`) are left out.

Each twin was built once, one at a time. `nm -S` gave the function sizes, and
`objdump -d` gave the classes below; the binaries were then deleted. 62 of the 63 twins
built. `Tools/perf/jetc-profile/startup/run.jet` did not: the seed refuses it with
`jet.UseAfterMove` at `run.jet:209`. The lambda on line 208 captures the loop's `name`,
and a lambda that needs no `FnMut` moves a non-copy local it captures
(`Checks.jet` `sema_ownership_collect_lambda_requirement_events`), so lines 209 and 216
read it after the move. That is a bug in the twin's source, not a size finding. Its fix
lands separately.

A Core or runtime function (`core_*`, `__core_*`, `unit_*`, `jet_*`) is listed once, with
its largest copy and the number of twins that carry it. A tool's own function is listed
once per name and size, so a module that two twins share (`Report_report` in
`scaling-report` and `scaling-run-gate`) is one row.

## Byte classes

Each function's bytes are split into these classes. Each one is a mechanism with its
own rule.

| class | what the bytes are | how it is counted | rule |
|---|---|---|---|
| **E** stop edges | Cleanup regions laid out after the epilogue. Each one drops every live owned variable, latest first, between `jet_rt_cleanup_enter` and `jet_rt_cleanup_leave`. | Exact: bytes from each `cleanup_enter` call to its `cleanup_leave` call, plus 20 B for the enter/leave sequence. | **c29x2 + c29x3** (submitted, `X64Select.jet` `ir_x64_pads`). An edge whose tail equals a tail already laid out runs only its own drops, then jumps into that tail. c29x3 lets a tail start at a Move or Copy. A tail that branches back into the body is shared only where it disturbs no value the body keeps (`ir_x64_share_safe`). |
| **S** xmm spills | `movsd`/`movss` between an xmm register and a frame slot. | Count × 6 B (estimate). | **c29x1b** (submitted, `X64Alloc.jet`). Chooses which Float slot spills by loop heat, so a hot loop's Float values stay in xmm registers. |
| **P** cold-arm poll tests | The inline task-control test that a stop poll in a loop runs: `mov rax, gs:[0]`, two loads, three tests (44 B). Counted only where the poll sits in an arm that ends in Unreachable (right after a `*_stop`, panic or trap call). | Count × 44 B. | **c29x4** (submitted, `X64Select.jet` `ir_x64_in_stop_arm`). Such an arm runs at most once, so it calls the poll directly. |
| bounds stops | A failed list index's site arguments and call: file, length, index, line, `jet_rt_list_bounds_stop`. | Count × 30 B (estimate). | No rule yet. These are the check's own report; P shortens the poll after them. |
| **I** owned Int drops | `jet_rt_int_drop` of an exact `Int`: tag test, then call (about 20 B). | Count × 20 B (estimate). | OptRange already removes the drop of an Int proven inline (`OptRange.jet` `ir_range_rewrite`, `.Drop`). The drops left are of values with no proven range. |
| **U** Unicode tables | `unit_unicode_*`: inline case, grapheme and width table lookups. | Whole function. | **c29r4** (G11Speed-4, submitted). |
| **C** typed CLI parser | `__jet_cli_parse`, generated per `#CLI` struct (`CliEntry.jet` `sema_cli_entry_direct`). | Whole function. | Proposed: store each input once (see "What the rest is"). |
| other | Everything else: the source's own calls, their arguments, results, and the drops of their temporaries. | Size minus the classes above. | See "What the rest is". |

The rule column of the table names a class when it holds at least 10% of the function
(E, I), 5% (S) or 2% (P). U and C mark whole functions. `-` means no class reaches its
share, so the bytes are the source's own calls (see "What the rest is").

## Totals

1,069 functions of 4 KB or more hold 15,417,842 B. 472 of them (5,252,286 B, 34.1%) are Core or runtime functions; the rest are the tools' own.

| class | bytes | share | rule |
|---|---:|---:|---|
| E stop edges | 2,428,360 | 15.8% | c29x2 + c29x3 |
| S xmm spills (estimate) | 204,858 | 1.3% | c29x1b |
| P cold-arm poll tests | 115,632 | 0.7% | c29x4 |
| bounds stops (estimate) | 110,490 | 0.7% | none yet |
| I owned Int drops (estimate) | 575,320 | 3.7% | OptRange, where a range is proven |
| U Unicode table functions | 165,213 | 1.1% | c29r4 |
| other | 11,817,969 | 76.7% | see below |

Rows by rule: E 473, `-` 289, E+P 86, E+I 65, P 42, I 35, E+S 23, E+P+I 19, S 15, U 10, P+I 7, E+S+I 3, C 1, E+C 1. The functions make 359,024 calls. Outside the stop edges, 89,060 of those calls are drops, releases or frees, and the functions build 3,395 interpolated strings.

## What the rest is

The "other" bytes are the code each function's source asks for: calls, their arguments,
the results they write, and the drops of their temporaries. A function's size follows
its call count. Two mechanisms inside "other" could still shrink, and four functions are
large for reasons specific to them.

Inlined callees are not split out. Their code carries the caller's line in DWARF, so the
disassembly cannot tell a callee's bytes from the caller's. Only the IR counts below
(from `--opt-dump`) see across that line.

**Early-return arms (proposed rule R).** A `?`, a `?? return` or an early `return`
drops every live owned variable latest first and then returns (`Build.jet`
`ir_emit_return`, `ir_drop_bindings`). That is the same order a stop edge uses, but the
drops are emitted inline in each arm and are never shared. The O2 IR of three functions
(`jet inspect decisions --opt-dump`) shows how much this is:

- `Report_report` (scaling-report): 67 return arms hold 4,961 of 22,136 ops (22.4%),
  including 1,362 drop ops. 173 stop edges hold another 5,595 ops (25.3%).
- `core_regex_scan` (a regex probe): 8 return arms, 944 of 3,458 ops, 106 drops.
- `__core_encoding_stream_jcs_number` (a JSON probe): 3 return arms, 838 of 3,893 ops,
  78 drops.

The rule: lay these arms out after the epilogue, as stop edges are, and share their drop
tails by the same test (c29x2, c29x3). The returned value is stored to its home before
the shared tail, since the tail ends at a common Return. No submission covers this yet.

**Owned Int drops (I).** About 20 B each, and about 28,800 of them. OptRange removes a
drop only when it proves the Int inline. The largest remaining counts are in:

- the byte parsers (`__core_net_ssl_*` 11 to 14 KB each, `core_archive_inflate*` 6 KB);
- the exponent arithmetic of `core_math___pow_accurate` (16 KB) and `__pow_cr` (6 KB).

The rule that reaches them (proposed, no submission yet) is to give OptRange the range
of an Int converted from a fixed-width integer, such as a byte read.

**Large for reasons of their own:**

- `Integrate10_run`: 249 KB from 172 source lines.
  - 68 stop edges hold 55,890 B.
  - The rest is the source's own work: 5,440 calls, 2,285 drops outside the edges, and
    102 interpolated strings.
  - It is a tool function, outside criterion 29's Core-and-generated scope, but it is the
    largest single place where E applies.
- `core_time_civil_format` (37.7 KB). Every call builds four name lists, 20 field values
  and 45 token pairs (`Time.jet:1859`). It then replaces each token over the pattern in
  order, because ordered replacement is the stated contract (`Time.jet:1888`): an inserted
  name is subject to later replacements. A formatter that scans the pattern once would
  change that contract, so this belongs to a Core ballot, not a codegen rule. 36 stop
  edges (16 KB) are E.
- `core_math___pow_accurate` (78 KB) and `__pow_cr` (58 KB). These are double-double
  kernels: E 31 KB and 16 KB, S 26 KB and 23 KB, I 16 KB and 6 KB. Correctly rounded pow
  is large by nature, and the classes above hold most of the rest.
- `run___jet_cli_parse` (35 KB for the 16 inputs of `enum-codec-bench`, about 2.2 KB per
  input). This is the C row. The generated direct parser
  (`CliEntry.jet` `sema_cli_entry_direct`) emits a value input's conversion and refusal
  (`sema_cli_entry_store`) in every arm that can set it: `--name=value`, `--name value`,
  the `-x` cluster, and the positional fill. That is up to four copies per input. A
  hand-written parser resolves the name to a slot first and stores once. The rule is to
  route every `#CLI` struct through the compact spec table, as a struct with commands
  already does, or to store once per input after the arms resolve the slot.

**Unicode (U).** `unit_unicode_grapheme_class` (34.7 KB) and
`unit_unicode_upper_mapping` / `lower_mapping` (28.8 / 26.9 KB) are inline table lookups.
c29r4 covers them.

**The card's named rows** (seed-b13 on 2026-11-12, then seed-b14 here):

- `__core_encoding_stream_jcs_number`: 37,759 B, now 38,771 B. E 12.5 KB, S 3.8 KB.
- `core_regex_scan`: 35,189 B, now 35,889 B. E 7.0 KB, P 1.1 KB, I 5.6 KB.
- `core_encoding_json_parse_string`: 21,323 B, now 22,799 B (37 twins). E 1.9 KB; the rest
  is its own 538 calls.
- `MemorySafetyFuzz_run_memory_safety_fuzz_axis`: 58,080 B, now 58,320 B. E 8.3 KB; the
  rest is its own 1,446 calls.
- `unit_unicode_upper_mapping` and `lower_mapping`: 28,789 B and 26,909 B (U, above).

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| E stop edges | card | #6328 criterion 29: c29x2 and c29x3 submitted |
| S xmm spills | card | #6328 criterion 29: c29x1b submitted |
| P cold-arm poll tests | card | #6328 criterion 29: c29x4 submitted |
| U Unicode table functions | card | #6328 criterion 29: c29r4 submitted |
| R early-return arms | card | #6328 criterion 29: proposed rule, no submission yet |
| I owned Int drops | card | #6328 criterion 29: proposed range rule, no submission yet |
| C typed CLI parser | card | #6328 criterion 29: proposed store-once rule, no submission yet |
| bounds-stop site arguments | no-action | the bytes are the bounds check's own failure report, which the check needs; P shortens the poll after it |
| `core_time_civil_format` ordered replacement | no-action | ordered replacement is Core's stated contract (`Time.jet:1888`); a single-scan formatter changes it and needs a Core ballot, which this report-only run does not open |
| jetc-profile-startup not built | card | #5072 criterion 7 (the twin's card): the source reads `name` after a lambda moved it; fixed separately |
<!-- /audit-dispositions -->

## Functions of 4 KB or more

Columns:

- **edges:** the count, then the bytes (class E).
- **spill, cold polls, bounds, Int drops:** bytes of each class (estimates as above).
- **other:** the rest.
- **calls:** all `call` instructions.
- **drops:** drop, release and free calls outside the stop edges.
- **interp:** string interpolations (`jet_rt_string_builder_new`).
- **rule:** the classes that hold their share.

| function | twin | bytes | edges | spill | cold polls | bounds | Int drops | other | calls | drops | interp | rule |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| `Integrate10_run` | integrate10 | 249,039 | 68: 55,890 | 0 | 704 | 150 | 3,040 | 189,255 | 5,440 | 2,285 | 102 | E |
| `Report_report` | scaling-report +1 | 135,945 | 21: 8,015 | 264 | 0 | 0 | 60 | 127,606 | 2,689 | 1,267 | 20 | - |
| `IntegApply6_apply_sub` | integ-apply6 | 80,799 | 27: 9,039 | 0 | 528 | 60 | 0 | 71,172 | 1,865 | 729 | 63 | E |
| `core_math___pow_accurate` | scaling-harness +2 | 78,115 | 66: 31,497 | 25,824 | 0 | 0 | 16,020 | 4,774 | 1,277 | 217 | 0 | E S I |
| `TowerSpecApply_apply` | tower-spec-apply | 71,887 | 32: 6,188 | 0 | 1,188 | 570 | 740 | 63,201 | 1,822 | 514 | 43 | - |
| `PerformanceSurface_measure_pair` | run-performance-surface | 71,095 | 37: 8,800 | 108 | 836 | 600 | 280 | 60,471 | 1,910 | 386 | 4 | E |
| `CensusSnap2_run` | census-snap2 | 70,282 | 30: 12,069 | 0 | 88 | 0 | 2,160 | 55,965 | 1,725 | 657 | 44 | E |
| `run_run` | runtime-allocator | 68,006 | 32: 20,822 | 108 | 440 | 720 | 280 | 45,636 | 1,408 | 602 | 6 | E |
| `Scoreboard_entry_record` | run-scoreboard | 62,396 | 33: 13,230 | 0 | 616 | 0 | 0 | 48,550 | 1,568 | 334 | 3 | E |
| `Scoreboard_scoreboard` | run-scoreboard | 58,681 | 27: 7,733 | 0 | 88 | 0 | 800 | 50,060 | 1,502 | 254 | 0 | E |
| `core_math___pow_cr` | enum-codec-bench +3 | 58,420 | 51: 15,523 | 23,490 | 0 | 210 | 5,980 | 13,217 | 740 | 178 | 0 | E S I |
| `MemorySafetyFuzz_run_memory_safety_fuzz_axis__84077983dccad0fd` | memory-safety-fuzz | 58,320 | 25: 8,341 | 0 | 44 | 30 | 660 | 49,245 | 1,446 | 324 | 1 | E |
| `Stage_stage_row` | run-stage-entry | 57,846 | 20: 3,385 | 0 | 0 | 0 | 40 | 54,421 | 1,459 | 490 | 14 | - |
| `Publication_publication_state` | run-publication | 57,667 | 52: 13,364 | 72 | 0 | 0 | 300 | 43,931 | 1,618 | 271 | 21 | E |
| `run_command` | scaling-report | 57,124 | 22: 6,716 | 0 | 0 | 0 | 320 | 50,088 | 1,241 | 537 | 7 | E |
| `run_run` | jetc-profile-live | 56,123 | 12: 11,066 | 0 | 0 | 60 | 620 | 44,377 | 807 | 355 | 9 | E |
| `Gate___read__f7228aa22a359784` | scaling-harness +2 | 54,787 | 5: 8,079 | 0 | 0 | 30 | 320 | 46,358 | 1,125 | 358 | 0 | E |
| `ToolCases_run_tool_entry` | run-tool-entry | 53,353 | 29: 8,859 | 0 | 572 | 90 | 400 | 43,432 | 1,456 | 322 | 13 | E |
| `MemorySafetyFuzz_run_stages__db60ac47d21ce264` | memory-safety-fuzz | 53,181 | 21: 1,893 | 0 | 0 | 0 | 40 | 51,248 | 1,335 | 350 | 5 | - |
| `run_run` | hot-text | 52,620 | 30: 11,306 | 0 | 132 | 120 | 660 | 40,402 | 1,365 | 574 | 15 | E |
| `PerformanceSurface_pair_row` | run-performance-surface | 51,480 | 12: 4,263 | 36 | 44 | 0 | 0 | 47,137 | 1,141 | 206 | 0 | - |
| `Gate___read__73ff6eb4b0bba54f` | scaling-harness +2 | 51,112 | 6: 8,738 | 0 | 0 | 30 | 0 | 42,344 | 990 | 306 | 0 | E |
| `Gate___read__6ac676dcb1f663aa` | scaling-harness +2 | 51,006 | 6: 8,929 | 0 | 0 | 0 | 0 | 42,077 | 989 | 310 | 0 | E |
| `IntegLand4_run` | integ-land4 | 50,368 | 27: 8,322 | 0 | 44 | 30 | 520 | 41,452 | 1,259 | 439 | 41 | E |
| `Comparison_peer_comparisons` | run-comparison +2 | 48,569 | 25: 8,067 | 0 | 1,144 | 0 | 0 | 39,358 | 1,268 | 293 | 2 | E P |
| `Stage_run_service` | run-stage-entry | 48,044 | 21: 6,752 | 6,324 | 0 | 30 | 380 | 34,558 | 1,021 | 286 | 9 | E S |
| `Stage_stage_entry` | run-stage-entry | 44,990 | 39: 83 | 0 | 44 | 0 | 300 | 44,563 | 1,188 | 215 | 13 | - |
| `Mixed_mixed_unit` | scaling-corpora +1 | 43,956 | 28: 15,295 | 0 | 44 | 30 | 5,100 | 23,487 | 1,057 | 279 | 23 | E I |
| `PerformanceSurface_validate_performance_surface` | run-performance-surface | 43,875 | 47: 4,705 | 504 | 0 | 0 | 180 | 38,486 | 1,232 | 199 | 19 | E |
| `Scoreboard_peer_record` | run-scoreboard | 42,884 | 18: 3,384 | 0 | 396 | 0 | 0 | 39,104 | 1,034 | 212 | 4 | - |
| `Batch10_run` | batch10 | 42,196 | 33: 8,070 | 0 | 88 | 90 | 100 | 33,848 | 1,044 | 341 | 23 | E |
| `Result_result_shape_issues` | run-publication +2 | 41,686 | 31: 8,163 | 0 | 0 | 0 | 0 | 33,523 | 980 | 226 | 17 | E |
| `run_run` | scaling-corpora | 41,451 | 29: 5,488 | 0 | 220 | 870 | 540 | 34,333 | 1,019 | 213 | 31 | E |
| `__core_net_ssl_srv12_ssl_srv12_parse_client_hello__ea1d52118000fce6` | run-cold-context +2 | 41,337 | 43: 7,603 | 0 | 0 | 240 | 13,880 | 19,614 | 1,304 | 488 | 0 | E I |
| `Probe_http_probe` | run-probe +1 | 41,116 | 10: 2,170 | 24 | 0 | 0 | 140 | 38,782 | 498 | 101 | 4 | - |
| `run_classify_variant` | optimization | 41,056 | 18: 4,263 | 888 | 44 | 0 | 0 | 35,861 | 1,034 | 241 | 2 | E |
| `run_run` | enum-codec-bench | 40,393 | 35: 6,275 | 8,514 | 0 | 120 | 80 | 25,404 | 741 | 200 | 12 | E S |
| `Memory_measure` | read-into-memory | 40,291 | 20: 8,910 | 7,734 | 132 | 90 | 400 | 23,025 | 599 | 196 | 7 | E S |
| `PerformanceSurface_performance_surface_main` | run-performance-surface | 39,943 | 19: 8,499 | 0 | 44 | 0 | 40 | 31,360 | 951 | 291 | 9 | E |
| `run_execute` | optimization | 39,430 | 18: 4,137 | 0 | 352 | 0 | 20 | 34,921 | 973 | 226 | 5 | E |
| `run_analyze` | jetc-profile | 39,414 | 20: 7,665 | 0 | 176 | 30 | 840 | 30,703 | 835 | 271 | 7 | E |
| `run_evaluate` | selfhost-run | 39,152 | 27: 9,252 | 0 | 132 | 60 | 40 | 29,668 | 931 | 354 | 1 | E |
| `run_sample` | enum-codec-bench | 38,855 | 18: 5,912 | 0 | 0 | 0 | 1,200 | 31,743 | 1,012 | 266 | 12 | E |
| `__core_encoding_stream_jcs_number` | asm-parity +32 | 38,771 | 52: 12,464 | 3,762 | 484 | 660 | 1,300 | 20,101 | 815 | 226 | 4 | E S |
| `run_run` | enumset-bench | 38,629 | 20: 9,056 | 0 | 0 | 0 | 440 | 29,133 | 850 | 313 | 6 | E |
| `run_measure_arm` | core-activation | 38,364 | 29: 10,846 | 0 | 176 | 90 | 720 | 26,532 | 825 | 250 | 14 | E |
| `run_measure` | corelib-compare | 37,953 | 19: 9,119 | 696 | 88 | 180 | 380 | 27,490 | 820 | 357 | 6 | E |
| `core_time_civil_format` | optimization +3 | 37,677 | 36: 16,093 | 0 | 0 | 120 | 1,400 | 20,064 | 882 | 315 | 15 | E |
| `run___read__3cccfe9ef8ffbf52` | selfhost-run | 37,625 | 5: 6,477 | 0 | 0 | 0 | 0 | 31,148 | 772 | 254 | 0 | E |
| `Summarize_render` | jetc-trace | 36,666 | 29: 7,482 | 468 | 660 | 450 | 3,020 | 24,586 | 876 | 218 | 12 | E |
| `Gate_phase_table` | scaling-harness +2 | 36,189 | 26: 5,514 | 7,152 | 44 | 180 | 140 | 23,159 | 506 | 154 | 3 | E S |
| `Measure_phase_table` | perf-measure | 35,972 | 38: 7,379 | 8,784 | 440 | 630 | 360 | 18,379 | 557 | 128 | 4 | E S |
| `core_regex_scan` | batch10 +31 | 35,889 | 45: 6,951 | 0 | 1,100 | 750 | 5,560 | 21,528 | 794 | 240 | 0 | E P I |
| `PerformanceSurface_run_arm` | run-performance-surface | 35,759 | 9: 1,935 | 2,712 | 0 | 0 | 340 | 30,772 | 743 | 150 | 2 | S |
| `run___jet_cli_parse` | enum-codec-bench | 35,345 | 17: 3,447 | 588 | 704 | 480 | 800 | 29,326 | 932 | 336 | 6 | C |
| `run_run` | scaling-enqueue | 35,238 | 13: 1,190 | 0 | 0 | 0 | 200 | 33,848 | 840 | 79 | 25 | - |
| `__core_net_ssl_srv13_ssl_srv13_parse_client_hello` | run-cold-context +2 | 34,707 | 27: 8,056 | 0 | 0 | 270 | 14,020 | 12,361 | 1,235 | 539 | 0 | E I |
| `unit_unicode_grapheme_class` | comptime-items +10 | 34,692 | 0: 0 | 0 | 0 | 0 | 0 | 34,692 | 0 | 0 | 0 | U |
| `Measure_run_measured` | perf-measure | 34,689 | 21: 6,816 | 270 | 44 | 0 | 1,560 | 25,999 | 874 | 216 | 14 | E |
| `run_measure_all` | enum-codec-bench | 34,617 | 16: 10,091 | 0 | 132 | 0 | 1,360 | 23,034 | 623 | 231 | 10 | E |
| `run_run` | selfhost-run | 34,518 | 21: 8,076 | 0 | 0 | 0 | 140 | 26,302 | 662 | 206 | 14 | E |
| `core_math_log2` | enum-codec-bench +3 | 33,977 | 52: 13,563 | 13,098 | 0 | 1,110 | 2,100 | 4,106 | 494 | 38 | 0 | E S |
| `Gate___read__e43c63a00f6a26d9` | scaling-harness +2 | 33,779 | 5: 4,737 | 0 | 0 | 0 | 620 | 28,422 | 703 | 224 | 0 | E |
| `Gate_finish` | scaling-harness +2 | 33,222 | 40: 9,413 | 564 | 0 | 30 | 760 | 22,455 | 789 | 82 | 12 | E |
| `Gate___read__de2210fa14750d42` | scaling-harness +2 | 33,077 | 4: 3,761 | 0 | 0 | 30 | 540 | 28,746 | 678 | 220 | 0 | E |
| `run_run` | core-activation | 32,752 | 22: 8,563 | 0 | 308 | 180 | 480 | 23,221 | 782 | 230 | 15 | E |
| `MemorySafetyFuzz_hardening_payload` | memory-safety-fuzz | 32,519 | 7: 267 | 0 | 0 | 0 | 0 | 32,252 | 829 | 128 | 5 | - |
| `run_load_family` | optimization | 31,990 | 17: 3,066 | 0 | 396 | 90 | 0 | 28,438 | 848 | 239 | 16 | - |
| `core_net_url_parse_recipe` | run-cold-context +2 | 31,506 | 29: 4,043 | 0 | 440 | 420 | 1,860 | 24,743 | 660 | 110 | 0 | E |
| `core_http_client___send` | run-cold-context +2 | 31,447 | 39: 6,309 | 0 | 220 | 150 | 2,720 | 22,048 | 748 | 243 | 8 | E |
| `run_optimization` | optimization | 31,094 | 54: 8,265 | 0 | 44 | 0 | 0 | 22,785 | 720 | 149 | 2 | E |
| `run_run` | function-boundary | 31,029 | 21: 6,356 | 0 | 88 | 30 | 0 | 24,555 | 769 | 223 | 21 | E |
| `run_jet_build` | selfhost-run | 30,917 | 10: 4,418 | 0 | 0 | 0 | 220 | 26,279 | 612 | 152 | 0 | E |
| `Gate_run_gate` | scaling-harness +2 | 30,303 | 15: 1,542 | 0 | 836 | 570 | 340 | 27,015 | 681 | 127 | 6 | P |
| `core_encoding_json_patch_with_limits` | run-performance-surface | 30,204 | 31: 5,885 | 0 | 88 | 60 | 300 | 23,871 | 712 | 78 | 15 | E |
| `core_args_cli_parse_level` | build-system-adapter +2 | 30,172 | 40: 4,038 | 0 | 528 | 360 | 2,480 | 22,766 | 803 | 296 | 9 | E |
| `__core_net_ssl_cli_ssl_write_client_hello_body__8fd41d14b45c9d92` | run-cold-context +2 | 29,512 | 35: 4,915 | 0 | 44 | 210 | 4,080 | 20,263 | 809 | 259 | 0 | E I |
| `BuildSystem_run_build_system_adapter` | build-system-adapter | 29,449 | 31: 10,222 | 0 | 0 | 0 | 180 | 19,047 | 583 | 142 | 3 | E |
| `run___jet_cli_run` | perf-measure | 29,077 | 47: 5,337 | 216 | 88 | 150 | 260 | 23,026 | 756 | 223 | 8 | E |
| `unit_unicode_upper_mapping` | batch10 +32 | 28,789 | 0: 0 | 0 | 0 | 0 | 0 | 28,789 | 0 | 0 | 0 | U |
| `RunColdContext_evaluate` | run-cold-context | 28,702 | 16: 5,054 | 0 | 44 | 0 | 0 | 23,604 | 800 | 270 | 13 | E |
| `Corpora_corpus` | scaling-harness +2 | 28,630 | 22: 4,225 | 0 | 44 | 0 | 40 | 24,321 | 402 | 88 | 5 | E |
| `run_parity` | asm-parity | 28,552 | 16: 4,245 | 4,644 | 44 | 300 | 160 | 19,159 | 506 | 152 | 7 | E S |
| `Coverage_build_census` | coverage | 28,507 | 21: 3,927 | 1,272 | 0 | 0 | 420 | 22,888 | 547 | 146 | 9 | E |
| `__core_net_ssl_srv12_ssl_srv12_write_server_hello__383ea79539e9dc1a` | run-cold-context +2 | 28,484 | 50: 3,323 | 0 | 0 | 630 | 3,960 | 20,571 | 821 | 204 | 0 | E I |
| `Gate___read__0c489c73f5dd67fe` | scaling-harness +2 | 28,482 | 30: 7,947 | 0 | 88 | 90 | 580 | 19,777 | 572 | 143 | 3 | E |
| `StreamB10_run` | streamb10 | 28,443 | 24: 3,062 | 0 | 176 | 150 | 80 | 24,975 | 647 | 195 | 18 | E |
| `run_gains` | asm-parity | 28,205 | 15: 6,130 | 0 | 0 | 0 | 40 | 22,035 | 550 | 160 | 8 | E |
| `run_gain_row` | asm-parity | 28,146 | 29: 6,494 | 0 | 528 | 390 | 1,520 | 19,214 | 618 | 179 | 0 | E |
| `run_jet_worker` | stream-bench | 27,718 | 34: 4,100 | 0 | 264 | 180 | 420 | 22,754 | 677 | 160 | 11 | E |
| `run___jet_cli_run` | compile-throughput | 27,418 | 24: 3,313 | 0 | 88 | 150 | 320 | 23,547 | 631 | 180 | 11 | E |
| `CompileThroughput_run_compile_throughput_adapter` | compile-throughput | 27,403 | 31: 9,474 | 0 | 44 | 0 | 180 | 17,705 | 561 | 151 | 3 | E |
| `Stage_verify_service` | run-stage-entry | 27,374 | 48: 4,878 | 5,580 | 0 | 0 | 100 | 16,816 | 631 | 94 | 12 | E S |
| `run_run` | enum-codec-jet-adapter | 27,274 | 27: 3,708 | 0 | 132 | 90 | 500 | 22,844 | 751 | 250 | 10 | E |
| `Project_project` | build-system-generate | 27,261 | 25: 2,346 | 0 | 0 | 0 | 20 | 24,895 | 795 | 238 | 26 | - |
| `run_draw` | jetc-profile | 27,056 | 30: 4,032 | 7,212 | 0 | 390 | 500 | 14,922 | 394 | 75 | 7 | E S |
| `unit_unicode_lower_mapping` | batch10 +35 | 26,909 | 0: 0 | 0 | 0 | 0 | 0 | 26,909 | 0 | 0 | 0 | U |
| `core_archive_inflate_dynamic` | run-cold-context +2 | 26,859 | 24: 2,650 | 0 | 528 | 390 | 5,780 | 17,511 | 733 | 277 | 0 | I |
| `run_stage_entries` | optimization | 26,851 | 4: 80 | 0 | 0 | 0 | 0 | 26,771 | 583 | 17 | 17 | - |
| `run_receipt_for` | selfhost-run | 26,765 | 16: 3,000 | 0 | 0 | 0 | 400 | 23,365 | 592 | 187 | 9 | E |
| `Gate_prepare` | scaling-harness +2 | 26,682 | 22: 4,593 | 0 | 0 | 30 | 40 | 22,019 | 603 | 214 | 7 | E |
| `Profile_index_functions` | scaling-profile | 26,662 | 30: 6,143 | 0 | 440 | 270 | 2,200 | 17,609 | 603 | 144 | 4 | E |
| `Result_comparison_issues` | run-publication +1 | 26,567 | 31: 4,630 | 0 | 44 | 0 | 0 | 21,893 | 686 | 187 | 13 | E |
| `run_print_scoreboards` | run-scoreboard | 26,546 | 32: 4,064 | 0 | 440 | 300 | 420 | 21,322 | 558 | 167 | 6 | E |
| `run_normalize` | jetc-profile | 26,381 | 18: 3,134 | 0 | 176 | 240 | 140 | 22,691 | 452 | 48 | 3 | E |
| `run_stub_sample` | run-performance-surface | 26,371 | 7: 866 | 0 | 0 | 90 | 140 | 25,275 | 700 | 139 | 2 | - |
| `RunColdContext_prepare` | run-cold-context | 26,351 | 10: 1,147 | 0 | 0 | 0 | 0 | 25,204 | 671 | 175 | 13 | - |
| `run_run` | comptime-items | 26,294 | 27: 4,717 | 48 | 132 | 90 | 1,080 | 20,227 | 612 | 152 | 14 | E |
| `core_archive_inflate` | run-cold-context +2 | 26,242 | 30: 3,656 | 0 | 704 | 510 | 6,200 | 15,172 | 694 | 186 | 0 | E P I |
| `core_http_client___client_exchange` | run-cold-context +2 | 26,204 | 22: 5,199 | 0 | 0 | 30 | 1,460 | 19,515 | 664 | 252 | 5 | E |
| `Shape_load_entries` | run-shape | 26,097 | 21: 4,307 | 0 | 132 | 90 | 280 | 21,288 | 569 | 147 | 9 | E |
| `core_net_socket_addr_parse` | run-stage-entry | 26,093 | 29: 3,836 | 0 | 440 | 420 | 940 | 20,457 | 618 | 100 | 7 | E |
| `VerifyRows_run` | verify-rows | 26,043 | 32: 5,770 | 0 | 352 | 300 | 20 | 19,601 | 600 | 168 | 5 | E |
| `Gate_resume_receipt` | scaling-harness +2 | 25,842 | 23: 2,100 | 12 | 176 | 150 | 140 | 23,264 | 459 | 52 | 2 | - |
| `run_gains_table` | asm-parity | 25,774 | 29: 2,657 | 588 | 176 | 120 | 0 | 22,233 | 576 | 103 | 15 | E |
| `run_run` | hello-compile | 25,529 | 19: 4,426 | 12 | 220 | 30 | 0 | 20,841 | 659 | 237 | 5 | E |
| `run_report` | optimization | 25,493 | 11: 2,659 | 0 | 308 | 0 | 0 | 22,526 | 660 | 115 | 1 | E |
| `__core_net_ssl_cli13_ssl_tls13_parse_server_hello` | run-cold-context +2 | 25,430 | 38: 2,509 | 0 | 0 | 150 | 5,600 | 17,171 | 833 | 320 | 0 | I |
| `RuntimeCCensus_run` | runtime-c-census | 25,385 | 25: 6,079 | 0 | 132 | 90 | 780 | 18,304 | 611 | 122 | 9 | E |
| `__core_net_ssl_srv13_ssl_srv13_parse_pre_shared_key_ext` | run-cold-context +2 | 25,306 | 20: 8,042 | 0 | 132 | 90 | 11,080 | 5,962 | 911 | 359 | 0 | E I |
| `run_measure_from_manifest` | build-system | 25,286 | 24: 2,735 | 72 | 0 | 0 | 80 | 22,399 | 567 | 175 | 8 | E |
| `__core_crypto_cipher_layer_cipher_crypt` | run-cold-context +2 | 25,231 | 55: 3,788 | 0 | 572 | 420 | 1,620 | 18,831 | 572 | 79 | 0 | E P |
| `run_measure_build_system` | build-system | 25,134 | 30: 10,551 | 978 | 0 | 0 | 660 | 12,945 | 510 | 191 | 6 | E |
| `__core_crypto_x509crt_x509_crt_get_ext` | run-cold-context +2 | 25,119 | 30: 2,371 | 0 | 0 | 0 | 4,340 | 18,408 | 688 | 241 | 0 | I |
| `RunColdContext_make_report` | run-cold-context | 25,104 | 8: 2,103 | 0 | 0 | 0 | 40 | 22,961 | 494 | 107 | 0 | - |
| `Corpora_shapes__lambda_26f172b376f1bba6` | scaling-corpora | 24,986 | 33: 9,347 | 0 | 0 | 0 | 1,240 | 14,399 | 442 | 89 | 3 | E |
| `Corpora_shapes__lambda_c71beca401312911` | scaling-enqueue +4 | 24,986 | 33: 9,347 | 0 | 0 | 0 | 1,240 | 14,399 | 442 | 89 | 3 | E |
| `Profile_profile` | scaling-profile | 24,936 | 29: 4,497 | 114 | 264 | 150 | 100 | 19,811 | 629 | 161 | 6 | E |
| `RunColdContext_validate_baseline` | run-cold-context | 24,925 | 17: 2,635 | 72 | 0 | 0 | 780 | 21,438 | 702 | 227 | 1 | E |
| `run___read__2f3c29fe98d439ef` | selfhost-run | 24,855 | 15: 5,356 | 0 | 0 | 0 | 440 | 19,059 | 498 | 147 | 6 | E |
| `Gate_options` | scaling-enqueue | 24,713 | 10: 1,196 | 138 | 0 | 0 | 200 | 23,179 | 572 | 91 | 5 | - |
| `RunColdContext_load_adapters` | run-cold-context | 24,656 | 19: 4,314 | 0 | 0 | 0 | 0 | 20,342 | 536 | 190 | 6 | E |
| `core_http_client___client_follow` | run-cold-context +2 | 24,610 | 20: 5,665 | 0 | 88 | 60 | 560 | 18,237 | 618 | 257 | 5 | E |
| `core_text_collation_elements` | jetc-profile +2 | 24,551 | 25: 4,305 | 0 | 484 | 330 | 2,780 | 16,652 | 541 | 109 | 0 | E I |
| `run_pinned_nixpkgs` | canvas-driver-lifecycle | 24,373 | 42: 4,586 | 0 | 132 | 240 | 120 | 19,295 | 599 | 140 | 9 | E |
| `Gate_options` | scaling-harness +2 | 24,213 | 10: 1,196 | 138 | 0 | 0 | 200 | 22,679 | 555 | 86 | 5 | - |
| `Profile_ranked` | scaling-profile | 24,200 | 21: 6,610 | 3,480 | 0 | 60 | 200 | 13,850 | 372 | 121 | 7 | E S |
| `__core_net_ssl_srv13_ssl_tls13_handshake_server_step__d41da886b19cc148` | run-cold-context +2 | 24,188 | 48: 3,271 | 0 | 176 | 270 | 3,100 | 17,371 | 783 | 200 | 0 | E I |
| `IntegLand4_same_tree` | integ-land4 | 24,101 | 27: 4,317 | 0 | 484 | 300 | 2,040 | 16,960 | 572 | 132 | 7 | E P |
| `Profile_ranked` | scaling-harness +2 | 24,041 | 21: 6,610 | 3,456 | 0 | 60 | 180 | 13,735 | 370 | 121 | 7 | E S |
| `__core_net_ssl_cli13_ssl_tls13_handshake_client_step__89c3f07642a0f9e1` | run-cold-context +2 | 23,985 | 47: 4,867 | 0 | 44 | 150 | 5,600 | 13,324 | 845 | 238 | 0 | E I |
| `RunColdContext_validate_task` | run-cold-context | 23,976 | 14: 4,155 | 0 | 44 | 30 | 40 | 19,707 | 594 | 264 | 7 | E |
| `run_validate_capture` | selfhost-run | 23,938 | 18: 2,765 | 0 | 44 | 30 | 20 | 21,079 | 484 | 105 | 7 | E |
| `Stage_cache_states` | run-stage-entry | 23,897 | 28: 4,115 | 0 | 176 | 30 | 40 | 19,536 | 604 | 174 | 4 | E |
| `run_arm_cell` | enumset-bench | 23,794 | 16: 2,552 | 0 | 0 | 0 | 420 | 20,822 | 653 | 172 | 23 | E |
| `run_print_probes` | run-tools | 23,734 | 55: 3,836 | 0 | 352 | 240 | 120 | 19,186 | 658 | 155 | 12 | E |
| `core_encoding_binary_unpack` | runtime-c-census +2 | 23,683 | 45: 4,879 | 0 | 440 | 330 | 2,260 | 15,774 | 514 | 124 | 0 | E |
| `__core_net_ssl_srv12_ssl_handshake_server_step__ce3ab7544c2aca2f` | run-cold-context +2 | 23,680 | 36: 6,089 | 0 | 0 | 240 | 3,400 | 13,951 | 730 | 141 | 0 | E I |
| `run___read__4ade2d14353b6662` | asm-parity | 23,648 | 5: 3,906 | 0 | 0 | 30 | 0 | 19,712 | 473 | 148 | 0 | E |
| `core_net_tls___own_identity` | run-cold-context +2 | 23,619 | 36: 3,490 | 0 | 0 | 30 | 200 | 19,899 | 745 | 109 | 0 | E |
| `run___read__556005d547dcac39` | asm-parity | 23,569 | 5: 4,008 | 0 | 0 | 0 | 0 | 19,561 | 473 | 152 | 0 | E |
| `LaneCommit_run` | lane-commit | 23,451 | 10: 1,811 | 0 | 132 | 90 | 220 | 21,198 | 721 | 244 | 19 | - |
| `Domains_domain_files` | scaling-enqueue +4 | 23,445 | 15: 4,323 | 0 | 0 | 0 | 960 | 18,162 | 557 | 84 | 15 | E |
| `__core_crypto_pem_pem_read_buffer` | run-cold-context +2 | 23,253 | 22: 1,857 | 0 | 88 | 270 | 2,360 | 18,678 | 605 | 213 | 0 | I |
| `PerformanceSurface_context_issues` | run-performance-surface | 23,200 | 20: 1,181 | 0 | 0 | 0 | 160 | 21,859 | 529 | 140 | 2 | - |
| `__core_crypto_x509crt_x509_crt_parse_der_core` | run-cold-context +2 | 23,180 | 14: 2,947 | 0 | 0 | 0 | 3,840 | 16,393 | 829 | 302 | 0 | E I |
| `MemorySafetyFuzz_base_runner_result` | memory-safety-fuzz | 23,172 | 55: 2,208 | 0 | 0 | 30 | 0 | 20,934 | 675 | 82 | 0 | - |
| `Result_comparison_issues` | run-scoreboard | 23,143 | 20: 4,069 | 0 | 0 | 0 | 0 | 19,074 | 585 | 167 | 13 | E |
| `Mixed_records_worker__lambda_79af5e321eb1b59f` | scaling-generate | 23,076 | 38: 11,586 | 0 | 176 | 540 | 2,180 | 8,594 | 470 | 76 | 1 | E |
| `Mixed_records_worker__lambda_83d4675fe943330c` | scaling-corpora | 23,076 | 38: 11,586 | 0 | 176 | 540 | 2,180 | 8,594 | 470 | 76 | 1 | E |
| `__core_crypto_cipher_layer_cipher_update` | run-cold-context +2 | 23,054 | 24: 1,845 | 0 | 528 | 360 | 1,440 | 18,881 | 469 | 71 | 0 | P |
| `__core_crypto_pkparse_group_id_from_specified` | run-cold-context +2 | 22,997 | 19: 5,548 | 0 | 0 | 90 | 7,380 | 9,979 | 864 | 279 | 0 | E I |
| `core_regex_replace_n` | function-boundary +18 | 22,997 | 28: 6,719 | 0 | 616 | 420 | 2,100 | 13,142 | 504 | 145 | 0 | E P |
| `Measure_check_completed` | perf-measure | 22,975 | 18: 3,423 | 168 | 176 | 120 | 1,980 | 17,108 | 605 | 244 | 1 | E |
| `Timer_run` | timer | 22,886 | 16: 3,089 | 0 | 44 | 30 | 1,380 | 18,343 | 570 | 101 | 7 | E |
| `run_hot_text_declared` | hot-text | 22,859 | 28: 5,254 | 0 | 440 | 270 | 2,060 | 14,835 | 554 | 118 | 3 | E |
| `__core_net_ssl_cli12_ssl_handshake_client_step__86e6ca7d1b9a0036` | run-cold-context +2 | 22,807 | 47: 4,700 | 0 | 0 | 240 | 4,400 | 13,467 | 775 | 202 | 0 | E I |
| `core_encoding_json_parse_string` | canvas-driver-lifecycle +36 | 22,799 | 27: 1,869 | 0 | 220 | 180 | 1,040 | 19,490 | 538 | 81 | 8 | - |
| `core_math___pow_ziv` | enum-codec-bench | 22,785 | 32: 5,981 | 564 | 0 | 0 | 8,520 | 7,720 | 641 | 118 | 0 | E I |
| `__core_net_ssl_cli12_ssl_cli12_parse_server_hello__937cb3ae8eb30174` | run-cold-context +2 | 22,736 | 28: 1,653 | 0 | 0 | 390 | 5,780 | 14,913 | 784 | 270 | 0 | I |
| `core_math_log10` | enum-codec-bench | 22,702 | 45: 5,962 | 4,476 | 0 | 120 | 3,420 | 8,724 | 488 | 86 | 0 | E S I |
| `Scoreboard_cell_record` | run-scoreboard | 22,694 | 16: 2,530 | 0 | 44 | 60 | 200 | 19,860 | 531 | 116 | 1 | E |
| `Policy_strict_performance_policy_issues` | run-policy +2 | 22,527 | 20: 2,805 | 0 | 0 | 0 | 0 | 19,722 | 559 | 131 | 9 | E |
| `__core_crypto_rsa_private_core__abe7dafa8c20de14` | run-cold-context +2 | 22,413 | 28: 5,360 | 0 | 176 | 120 | 900 | 15,857 | 577 | 175 | 0 | E |
| `core_text_runtime_kernel_text_normal_normalize` | jetc-profile +2 | 22,402 | 35: 5,927 | 0 | 924 | 630 | 1,980 | 12,941 | 540 | 53 | 4 | E P |
| `run_stage` | run-stage-entry | 22,330 | 29: 4,250 | 0 | 0 | 30 | 300 | 17,750 | 629 | 142 | 13 | E |
| `__core_crypto_cipher_layer_keyed_base` | run-cold-context +2 | 22,296 | 17: 699 | 0 | 0 | 360 | 0 | 21,237 | 438 | 94 | 0 | - |
| `core_http_client___client_url` | run-cold-context +2 | 22,275 | 24: 5,924 | 0 | 660 | 570 | 1,960 | 13,161 | 611 | 233 | 0 | E P |
| `core_encoding_json_write_canonical` | enum-codec-bench +7 | 22,234 | 12: 2,461 | 438 | 616 | 420 | 1,060 | 17,239 | 490 | 113 | 4 | E P |
| `core_sys_env_text` | batch10 +15 | 22,177 | 26: 5,357 | 0 | 572 | 390 | 1,460 | 14,398 | 513 | 184 | 4 | E P |
| `__core_crypto_cipher_layer_cipher_auth_decrypt_ext` | run-cold-context +2 | 22,130 | 61: 5,940 | 0 | 0 | 0 | 1,480 | 14,710 | 685 | 99 | 0 | E |
| `RunColdContext_compare_baseline` | run-cold-context | 22,129 | 9: 212 | 192 | 0 | 0 | 80 | 21,645 | 538 | 109 | 8 | - |
| `__core_net_ssl_msg_ssl_buffer_message` | run-cold-context +2 | 22,046 | 32: 2,217 | 0 | 792 | 930 | 3,480 | 14,627 | 538 | 110 | 0 | E P I |
| `__core_crypto_ecp_point_read_binary` | run-cold-context +2 | 21,982 | 20: 6,662 | 0 | 0 | 150 | 600 | 14,570 | 408 | 86 | 0 | E |
| `Comparison_metric_cell` | run-comparison +2 | 21,974 | 16: 1,088 | 48 | 0 | 0 | 0 | 20,838 | 554 | 99 | 3 | - |
| `__core_crypto_md_compress` | core-activation +18 | 21,846 | 50: 3,642 | 0 | 3,036 | 2,070 | 1,280 | 11,818 | 411 | 40 | 0 | E P |
| `Stage_run_tier` | run-stage-entry | 21,845 | 7: 970 | 0 | 0 | 30 | 0 | 20,845 | 565 | 132 | 0 | - |
| `Shape_entry_shape_issues` | run-shape | 21,717 | 25: 979 | 0 | 0 | 0 | 0 | 20,738 | 451 | 74 | 10 | - |
| `Profile_index_functions` | scaling-harness +2 | 21,689 | 30: 6,143 | 0 | 440 | 270 | 2,200 | 12,636 | 555 | 133 | 3 | E P I |
| `Stage_visit_module` | run-stage-entry | 21,635 | 20: 3,230 | 0 | 176 | 120 | 820 | 17,289 | 437 | 40 | 6 | E |
| `__core_crypto_rsa_rsa_complete` | run-cold-context +2 | 21,507 | 29: 6,601 | 0 | 44 | 60 | 2,420 | 12,382 | 942 | 222 | 0 | E I |
| `__core_net_ssl_cli12_ssl_cli12_parse_server_key_exchange__4ad4b2e4843d01d1` | run-cold-context +2 | 21,492 | 39: 1,620 | 0 | 0 | 300 | 2,460 | 17,112 | 722 | 197 | 0 | I |
| `__core_crypto_x509_x509_get_subject_alt_name_ext` | run-cold-context +2 | 21,482 | 28: 3,824 | 0 | 44 | 30 | 2,600 | 14,984 | 510 | 115 | 0 | E I |
| `Stage_stage_service` | run-stage-entry | 21,457 | 2: 39 | 0 | 0 | 0 | 0 | 21,418 | 639 | 136 | 0 | - |
| `Corpora_wide_enums` | scaling-corpora +5 | 21,430 | 9: 3,136 | 0 | 132 | 90 | 60 | 18,012 | 507 | 83 | 27 | E |
| `Corpora_corpus` | scaling-generate | 21,387 | 14: 3,146 | 0 | 0 | 0 | 40 | 18,201 | 264 | 53 | 4 | E |
| `Measure_sample_tree` | perf-measure | 21,299 | 28: 5,090 | 102 | 132 | 90 | 1,020 | 14,865 | 428 | 137 | 3 | E |
| `Scoreboard_ceiling_cell` | run-scoreboard | 21,295 | 15: 3,260 | 0 | 264 | 0 | 0 | 17,771 | 541 | 121 | 0 | E |
| `__core_net_ssl_tls_ssl_tls12_populate_transform` | run-cold-context +2 | 21,290 | 20: 1,565 | 0 | 0 | 780 | 2,100 | 16,845 | 542 | 92 | 0 | - |
| `Gate_encode` | scaling-harness +30 | 21,229 | 0: 0 | 0 | 0 | 0 | 0 | 21,229 | 51 | 12 | 0 | - |
| `__core_crypto_rsa_prepare_blinding__eaa06aa0b54a3986` | run-cold-context +2 | 21,210 | 21: 6,411 | 0 | 0 | 0 | 120 | 14,679 | 392 | 82 | 0 | E |
| `Corpora_shapes__lambda_29784dc5ab356469` | scaling-corpora | 21,154 | 27: 7,742 | 0 | 0 | 0 | 1,060 | 12,352 | 378 | 76 | 7 | E |
| `Corpora_shapes__lambda_8f60e23dfe331156` | scaling-enqueue +4 | 21,154 | 27: 7,742 | 0 | 0 | 0 | 1,060 | 12,352 | 378 | 76 | 7 | E |
| `__core_crypto_ecp_mul_internal__406ea89d48e331cf` | run-cold-context +2 | 21,139 | 53: 7,143 | 0 | 0 | 60 | 1,360 | 12,576 | 819 | 124 | 0 | E |
| `__core_crypto_ecp_mul_internal__eebdaf2cb2684e8e` | run-cold-context +2 | 21,139 | 53: 7,143 | 0 | 0 | 60 | 1,360 | 12,576 | 819 | 124 | 0 | E |
| `run_measure` | enumset-bench | 21,044 | 9: 2,666 | 372 | 132 | 150 | 200 | 17,524 | 414 | 123 | 1 | E |
| `Axis_probe_axis_tool` | run-tools | 20,941 | 6: 84 | 0 | 0 | 0 | 60 | 20,797 | 565 | 80 | 5 | - |
| `Gate_decode` | scaling-harness +14 | 20,889 | 19: 5,388 | 0 | 0 | 0 | 400 | 15,101 | 285 | 74 | 0 | E |
| `Shape_load_entries` | run-tool-entry | 20,868 | 17: 3,542 | 0 | 44 | 0 | 0 | 17,282 | 485 | 115 | 8 | E |
| `Summarize_run` | jetc-trace | 20,846 | 14: 1,493 | 2,964 | 0 | 0 | 60 | 16,329 | 380 | 43 | 7 | S |
| `core_encoding_read_list__b8dc075672ca8c97` | jetc-profile | 20,775 | 23: 6,708 | 0 | 44 | 60 | 120 | 13,843 | 400 | 100 | 3 | E |
| `__core_crypto_rsa_rsa_parse_key` | run-cold-context +2 | 20,668 | 12: 4,635 | 0 | 0 | 0 | 1,440 | 14,593 | 726 | 208 | 0 | E |
| `core_regex_look_tables` | batch10 +31 | 20,659 | 21: 6,934 | 0 | 704 | 480 | 1,420 | 11,121 | 341 | 70 | 0 | E P |
| `Roots_roots` | roots-sh | 20,651 | 28: 3,154 | 0 | 132 | 90 | 440 | 16,835 | 549 | 166 | 4 | E |
| `core_regex_build_dfa` | batch10 +22 | 20,619 | 17: 4,254 | 0 | 1,144 | 810 | 2,780 | 11,631 | 465 | 85 | 0 | E P I |
| `__core_crypto_bignum_exp_mod_from` | run-cold-context +2 | 20,604 | 38: 6,414 | 0 | 836 | 630 | 1,980 | 10,744 | 587 | 73 | 0 | E P |
| `run_run_command` | selfhost-run | 20,588 | 12: 6,854 | 2,496 | 0 | 0 | 460 | 10,778 | 302 | 56 | 2 | E S |
| `core_time___civil_tzif_footer` | jetc-profile-live +1 | 20,535 | 18: 4,214 | 0 | 176 | 240 | 1,100 | 14,805 | 393 | 110 | 3 | E |
| `__core_net_ssl_msg_ssl_read__f3322bc3f1fc6d02` | run-cold-context +2 | 20,487 | 11: 613 | 0 | 88 | 60 | 660 | 19,066 | 245 | 47 | 0 | - |
| `__core_crypto_cipher_blocks_portable` | run-cold-context +2 | 20,469 | 31: 2,833 | 0 | 1,320 | 900 | 2,460 | 12,956 | 465 | 38 | 0 | E P I |
| `RunColdContext_run` | run-cold-context | 20,457 | 21: 2,855 | 0 | 0 | 0 | 100 | 17,502 | 475 | 41 | 17 | E |
| `core_encoding_read_list__0ba0193dfdf757a3` | asm-parity | 20,449 | 22: 6,420 | 0 | 44 | 60 | 120 | 13,805 | 385 | 98 | 3 | E |
| `run_reachable_core_modules` | core-activation | 20,445 | 23: 1,640 | 0 | 0 | 510 | 380 | 17,915 | 416 | 48 | 1 | - |
| `run_run` | scaling-module-lookups-proof | 20,445 | 14: 3,359 | 0 | 88 | 120 | 0 | 16,878 | 516 | 132 | 10 | E |
| `Axis_probe_jet_binary` | run-tools | 20,412 | 25: 1,708 | 0 | 44 | 540 | 340 | 17,780 | 603 | 54 | 10 | - |
| `__core_net_ssl_gen13_ssl_tls13_process_certificate__aff398ba533287c4` | run-cold-context +2 | 20,392 | 40: 5,651 | 0 | 0 | 150 | 4,740 | 9,851 | 707 | 148 | 0 | E I |
| `Gate_judge` | scaling-harness +2 | 20,370 | 22: 5,086 | 984 | 88 | 0 | 0 | 14,212 | 436 | 102 | 8 | E |
| `run___read__6755fc82b135810c` | stream-bench | 20,359 | 4: 1,708 | 0 | 0 | 30 | 440 | 18,181 | 414 | 124 | 3 | - |
| `Profile_profile` | scaling-harness +2 | 20,341 | 22: 4,547 | 186 | 308 | 210 | 80 | 15,010 | 494 | 130 | 1 | E |
| `PerformanceSurface_collect_identity` | run-performance-surface | 20,322 | 20: 2,416 | 0 | 0 | 0 | 100 | 17,806 | 600 | 91 | 2 | E |
| `Measure_parse_counters` | perf-measure | 20,320 | 23: 4,851 | 0 | 572 | 390 | 740 | 13,767 | 428 | 45 | 2 | E P |
| `Gate___read__bf723764bdf36b81` | scaling-harness +2 | 20,303 | 5: 3,752 | 108 | 0 | 30 | 280 | 16,133 | 406 | 131 | 0 | E |
| `Gate___read__f218e5ff06943484` | scaling-harness +2 | 20,280 | 5: 3,875 | 108 | 0 | 0 | 280 | 16,017 | 406 | 135 | 0 | E |
| `LaneCommit_drain` | lane-commit | 20,212 | 20: 1,935 | 0 | 88 | 90 | 260 | 17,839 | 556 | 188 | 7 | - |
| `core_net_tls___client_connect` | run-cold-context +2 | 20,156 | 32: 2,835 | 0 | 132 | 180 | 760 | 16,249 | 511 | 68 | 8 | E |
| `__core_crypto_cipher_layer_cipher_auth_encrypt_ext` | run-cold-context +2 | 20,067 | 39: 3,404 | 0 | 0 | 0 | 80 | 16,583 | 489 | 76 | 0 | E |
| `run_print_comparisons` | run-comparison | 20,064 | 27: 3,141 | 0 | 440 | 300 | 360 | 15,823 | 464 | 136 | 6 | E P |
| `__core_crypto_rsa_validate_crt_core` | run-cold-context +2 | 20,060 | 9: 5,629 | 0 | 0 | 0 | 60 | 14,371 | 365 | 100 | 0 | E |
| `run_rounds` | build-system | 20,029 | 41: 4,151 | 0 | 132 | 90 | 60 | 15,596 | 490 | 146 | 5 | E |
| `MemorySafetyFuzz_generate_corpus` | memory-safety-fuzz | 20,012 | 32: 2,969 | 0 | 176 | 120 | 2,660 | 14,087 | 519 | 90 | 0 | E I |
| `Mixed_mixed_sources` | scaling-corpora +1 | 20,012 | 23: 4,275 | 0 | 0 | 0 | 1,700 | 14,037 | 544 | 117 | 17 | E |
| `run_run` | stream-bench | 19,988 | 20: 1,029 | 0 | 0 | 120 | 200 | 18,639 | 548 | 123 | 4 | - |
| `__core_net_ssl_msg_ssl_decrypt_buf` | run-cold-context +2 | 19,981 | 19: 1,186 | 0 | 88 | 150 | 1,840 | 16,717 | 507 | 133 | 0 | - |
| `__core_crypto_bignum_div_mpi` | run-cold-context +2 | 19,907 | 22: 4,282 | 0 | 1,144 | 780 | 3,020 | 10,681 | 844 | 206 | 0 | E P I |
| `run___read__e849c8ee3b4e9290` | selfhost-run | 19,903 | 5: 3,027 | 0 | 0 | 0 | 320 | 16,556 | 424 | 136 | 0 | E |
| `PerformanceSurface_prepare_history` | run-performance-surface | 19,878 | 40: 4,271 | 0 | 44 | 0 | 0 | 15,563 | 551 | 118 | 7 | E |
| `Memory_encode` | read-into-memory +1 | 19,705 | 0: 0 | 12 | 0 | 0 | 0 | 19,693 | 46 | 10 | 0 | - |
| `core_encoding_read_option__4ef5d426ded4c039` | comptime-items | 19,625 | 17: 6,799 | 0 | 0 | 30 | 340 | 12,456 | 289 | 66 | 0 | E |
| `run_timed` | hello-compile | 19,597 | 27: 2,731 | 816 | 176 | 120 | 960 | 14,794 | 443 | 126 | 3 | E |
| `__core_net_ssl_msg_ssl_get_next_record__bbfed1bf3fc38c06` | run-cold-context +2 | 19,572 | 22: 670 | 0 | 0 | 30 | 2,400 | 16,472 | 429 | 134 | 0 | I |
| `Timer_run_child` | timer | 19,374 | 29: 3,286 | 18 | 44 | 180 | 240 | 15,606 | 530 | 109 | 9 | E |
| `run_bench` | stream-bench | 19,341 | 32: 2,546 | 0 | 264 | 180 | 80 | 16,271 | 505 | 117 | 9 | E |
| `Gate_growth` | scaling-harness +2 | 19,327 | 17: 3,855 | 6,030 | 88 | 240 | 360 | 8,754 | 251 | 56 | 3 | E S |
| `__core_net_ssl_tls_ssl_parse_certificate__fe4dd10a465fbca9` | run-cold-context +2 | 19,273 | 39: 4,520 | 0 | 220 | 390 | 4,480 | 9,663 | 621 | 114 | 0 | E I |
| `core_encoding_json___decode_typed__c8388b6f295029a9` | read-into-memory | 19,251 | 26: 6,832 | 0 | 0 | 30 | 900 | 11,489 | 361 | 81 | 1 | E |
| `Gate_measure_counters` | scaling-harness +2 | 19,014 | 15: 3,040 | 0 | 528 | 390 | 140 | 14,916 | 423 | 111 | 2 | E P |
| `Integrate10_test_entry` | integrate10 | 19,008 | 12: 2,208 | 0 | 0 | 0 | 0 | 16,800 | 368 | 130 | 7 | E |
| `Project_project` | build-system | 18,989 | 13: 1,526 | 0 | 0 | 0 | 0 | 17,463 | 560 | 188 | 20 | - |
| `run_run` | coverage | 18,976 | 15: 2,998 | 24 | 0 | 0 | 240 | 15,714 | 564 | 35 | 17 | E |
| `core_http_client___build_fill` | run-cold-context +2 | 18,878 | 26: 853 | 0 | 176 | 120 | 840 | 16,889 | 526 | 62 | 5 | - |
| `Policy_strict_performance_policy` | run-policy +1 | 18,775 | 0: 0 | 0 | 0 | 0 | 0 | 18,775 | 429 | 45 | 1 | - |
| `core_encoding_read_list__187760a2df556e1f` | asm-parity | 18,761 | 23: 5,918 | 0 | 44 | 60 | 960 | 11,779 | 378 | 90 | 3 | E |
| `__core_net_ssl_tls_ssl_verify_certificate__071500d124ee111b` | run-stage-entry +2 | 18,694 | 23: 3,270 | 0 | 0 | 360 | 320 | 14,744 | 340 | 67 | 0 | E |
| `run___jet_cli_run` | build-system-adapter | 18,623 | 23: 3,120 | 0 | 88 | 150 | 240 | 15,025 | 427 | 62 | 5 | E |
| `run_run_process` | optimization | 18,600 | 4: 301 | 0 | 0 | 0 | 60 | 18,239 | 495 | 80 | 4 | - |
| `__core_crypto_cipher_aes_expand` | run-cold-context +2 | 18,576 | 40: 4,289 | 0 | 1,716 | 1,170 | 1,580 | 9,821 | 447 | 51 | 0 | E P |
| `core_encoding_json___decode_typed_from__2928bca3b9d75489` | jetc-profile | 18,574 | 22: 6,210 | 0 | 0 | 30 | 60 | 12,274 | 345 | 86 | 1 | E |
| `Gate___read__f0c8e4876ad1cc83` | scaling-harness +2 | 18,506 | 11: 3,583 | 0 | 44 | 60 | 480 | 14,339 | 400 | 120 | 2 | E |
| `__core_crypto_pkcs5_pbes2` | run-cold-context +2 | 18,504 | 23: 1,760 | 0 | 0 | 0 | 2,080 | 14,664 | 537 | 140 | 0 | I |
| `Corpora_impls_traits` | scaling-enqueue +4 | 18,502 | 13: 3,682 | 0 | 0 | 0 | 460 | 14,360 | 402 | 89 | 8 | E |
| `__core_net_ssl_srv12_ssl_srv12_prepare_server_key_exchange__ddba89b39271c197` | run-cold-context +2 | 18,438 | 29: 1,426 | 0 | 0 | 120 | 1,940 | 14,952 | 563 | 141 | 0 | I |
| `Corpora_impls_traits` | scaling-corpora | 18,416 | 12: 3,613 | 0 | 0 | 0 | 440 | 14,363 | 395 | 90 | 7 | E |
| `__core_crypto_md_pbkdf2` | run-cold-context +2 | 18,396 | 29: 6,930 | 0 | 1,320 | 900 | 1,900 | 7,346 | 617 | 47 | 0 | E P I |
| `RunColdContext_environment` | run-cold-context | 18,377 | 18: 4,450 | 0 | 440 | 300 | 1,040 | 12,147 | 331 | 92 | 0 | E P |
| `unit_unicode_is_case_ignorable` | batch10 +35 | 18,367 | 0: 0 | 0 | 0 | 0 | 0 | 18,367 | 0 | 0 | 0 | U |
| `run_read_receipts` | scaling-report | 18,321 | 16: 2,834 | 0 | 88 | 60 | 260 | 15,079 | 351 | 119 | 1 | E |
| `__core_crypto_dhm_calc_core__df1c0507476b1c83` | run-cold-context +2 | 18,300 | 15: 4,929 | 0 | 0 | 0 | 40 | 13,331 | 322 | 94 | 0 | E |
| `__core_crypto_x509_x509_get_rsassa_pss_params` | run-cold-context +2 | 18,292 | 22: 2,824 | 0 | 0 | 0 | 2,880 | 12,588 | 535 | 162 | 0 | E I |
| `Summarize___read__237139d73c3b35a4` | jetc-trace | 18,254 | 5: 2,088 | 0 | 0 | 0 | 260 | 15,906 | 378 | 111 | 2 | E |
| `__core_crypto_pkparse_pk_parse_subpubkey` | run-cold-context +2 | 18,217 | 26: 6,140 | 0 | 0 | 0 | 2,260 | 9,817 | 775 | 128 | 0 | E I |
| `__core_net_ssl_keys13_ssl_tls13_compute_application_transform` | run-cold-context +2 | 18,157 | 27: 3,782 | 0 | 0 | 150 | 560 | 13,665 | 371 | 69 | 0 | E |
| `Gate___read__94fab17d0c8fbb36` | scaling-harness +2 | 18,153 | 5: 3,496 | 0 | 0 | 30 | 0 | 14,627 | 363 | 116 | 0 | E |
| `RunColdContext_run_http` | run-cold-context | 18,124 | 32: 3,695 | 0 | 88 | 30 | 940 | 13,371 | 505 | 139 | 2 | E |
| `Gate___read__fb57d05e641f6cd5` | scaling-harness +2 | 18,050 | 5: 3,661 | 0 | 0 | 0 | 0 | 14,389 | 362 | 121 | 0 | E |
| `Scoreboard_rejudged_cell` | run-scoreboard | 17,978 | 12: 842 | 684 | 0 | 0 | 40 | 16,412 | 503 | 120 | 4 | - |
| `unit_unicode_incb_class` | comptime-items +10 | 17,975 | 0: 0 | 0 | 0 | 0 | 0 | 17,975 | 0 | 0 | 0 | U |
| `run_run` | jetc-profile | 17,923 | 24: 3,606 | 0 | 44 | 390 | 160 | 13,723 | 427 | 97 | 2 | E |
| `core_regex_compile_with` | batch10 +22 | 17,848 | 27: 5,183 | 0 | 176 | 210 | 980 | 11,299 | 410 | 70 | 1 | E |
| `run_source_index` | jetc-profile | 17,835 | 18: 3,063 | 0 | 220 | 120 | 40 | 14,392 | 447 | 117 | 7 | E |
| `Stage_stop_process` | run-stage-entry | 17,823 | 17: 6,019 | 0 | 0 | 0 | 860 | 10,944 | 313 | 98 | 3 | E |
| `core_encoding_read_option__eb73263fcebe349c` | comptime-items | 17,823 | 14: 5,763 | 0 | 0 | 0 | 300 | 11,760 | 268 | 69 | 0 | E |
| `run_jet_name__lambda_7c0980149d58fce8` | jetc-profile | 17,769 | 21: 3,410 | 0 | 0 | 0 | 280 | 14,079 | 352 | 87 | 2 | E |
| `core_net_url_host_to_ascii` | run-cold-context +2 | 17,725 | 15: 2,727 | 0 | 132 | 90 | 2,020 | 12,756 | 371 | 117 | 3 | E I |
| `run_jet_mode` | selfhost-run | 17,699 | 10: 3,797 | 0 | 0 | 0 | 120 | 13,782 | 354 | 111 | 0 | E |
| `NativeCCompiler_run` | native-c-compiler | 17,689 | 13: 4,829 | 0 | 0 | 0 | 300 | 12,560 | 384 | 51 | 6 | E |
| `run_elf` | asm-parity | 17,673 | 4: 834 | 0 | 0 | 0 | 520 | 16,319 | 293 | 100 | 2 | - |
| `Profile_phase_intervals` | scaling-harness +3 | 17,615 | 10: 3,695 | 3,720 | 0 | 60 | 80 | 10,060 | 234 | 90 | 2 | E S |
| `Roots_roots` | batch10 +8 | 17,591 | 22: 2,306 | 0 | 132 | 90 | 440 | 14,623 | 466 | 151 | 4 | E |
| `Memory___read__2c7e982a77526f8e` | read-into-memory | 17,588 | 6: 2,747 | 0 | 44 | 60 | 0 | 14,737 | 381 | 115 | 0 | E |
| `Gate___read__23f685caf2be5926` | scaling-harness +2 | 17,583 | 10: 3,001 | 0 | 0 | 0 | 420 | 14,162 | 383 | 120 | 2 | E |
| `run_native_provider` | selfhost-run | 17,491 | 11: 4,204 | 0 | 0 | 0 | 100 | 13,187 | 367 | 112 | 1 | E |
| `PerformanceSurface_paired_bootstrap` | run-performance-surface | 17,393 | 9: 3,087 | 468 | 132 | 210 | 580 | 12,916 | 375 | 72 | 0 | E |
| `run_machine` | build-system | 17,382 | 26: 3,690 | 0 | 528 | 360 | 1,080 | 11,724 | 457 | 137 | 0 | E P |
| `core_http_client___client_parse_head` | run-cold-context +2 | 17,374 | 27: 3,453 | 0 | 572 | 570 | 560 | 12,219 | 431 | 147 | 0 | E P |
| `Corpora_data_tables` | scaling-corpora +5 | 17,362 | 13: 2,264 | 0 | 0 | 0 | 580 | 14,518 | 379 | 85 | 11 | E |
| `__core_crypto_x509crt_x509_crt_verify_items` | run-cold-context +2 | 17,296 | 32: 3,209 | 0 | 528 | 510 | 1,620 | 11,429 | 418 | 86 | 0 | E P |
| `__core_crypto_x509crt_x509_crt_check_san_ip` | run-cold-context +2 | 17,289 | 27: 4,852 | 0 | 748 | 510 | 1,680 | 9,499 | 333 | 91 | 0 | E P |
| `run_print_results` | run-result | 17,280 | 27: 3,469 | 0 | 440 | 300 | 360 | 12,711 | 390 | 110 | 6 | E P |
| `run_workload_description` | enum-codec-bench | 17,258 | 21: 1,313 | 0 | 528 | 0 | 0 | 15,417 | 422 | 40 | 7 | P |
| `MemorySafetyFuzz_record_finding` | memory-safety-fuzz | 17,219 | 6: 80 | 0 | 0 | 150 | 80 | 16,909 | 443 | 64 | 3 | - |
| `Domains_domain_files` | scaling-corpora | 17,194 | 13: 4,256 | 0 | 0 | 0 | 960 | 11,978 | 416 | 74 | 11 | E |
| `RuntimeCCensus_archive_members` | runtime-c-census | 17,184 | 14: 2,850 | 0 | 44 | 0 | 900 | 13,390 | 334 | 100 | 2 | E |
| `Corpora_corpus` | scaling-corpora | 17,057 | 14: 3,213 | 0 | 0 | 0 | 40 | 13,804 | 241 | 49 | 4 | E |
| `core_files_list` | hello-compile | 17,046 | 24: 3,833 | 0 | 440 | 270 | 2,080 | 10,423 | 417 | 87 | 1 | E P I |
| `Memory___read__234d1277d61f8aad` | read-into-memory | 17,039 | 5: 2,755 | 0 | 0 | 0 | 0 | 14,284 | 361 | 115 | 0 | E |
| `run___read__004b832f9607fbe5` | comptime-items | 17,002 | 5: 2,583 | 0 | 0 | 0 | 160 | 14,259 | 355 | 109 | 0 | E |
| `__core_net_ssl_msg_ssl_encrypt_buf__4deb9efbcf4fad69` | run-cold-context +2 | 16,959 | 18: 536 | 0 | 0 | 0 | 2,380 | 14,043 | 460 | 157 | 0 | I |
| `core_files_lock` | census-snap2 | 16,891 | 17: 4,266 | 0 | 0 | 0 | 1,360 | 11,265 | 390 | 85 | 0 | E |
| `run_scriptbench_issues` | asm-parity | 16,866 | 20: 4,184 | 0 | 0 | 30 | 60 | 12,592 | 338 | 86 | 4 | E |
| `__core_crypto_rsa_rsa_deduce_private_exponent` | run-cold-context +2 | 16,824 | 29: 4,011 | 0 | 0 | 0 | 720 | 12,093 | 572 | 120 | 0 | E |
| `core_http_client___client_dial_exchange` | run-cold-context +2 | 16,817 | 24: 2,226 | 0 | 0 | 0 | 1,420 | 13,171 | 470 | 97 | 1 | E |
| `run___jet_cli_parse` | enumset-bench | 16,778 | 13: 2,945 | 0 | 308 | 210 | 360 | 12,955 | 450 | 168 | 3 | E C |
| `Corpora_shapes__lambda_7a1e0959a481a356` | scaling-corpora | 16,770 | 26: 5,935 | 0 | 0 | 0 | 560 | 10,275 | 404 | 78 | 12 | E |
| `Corpora_shapes__lambda_c7a5e1f381e7dd0d` | scaling-enqueue +4 | 16,770 | 26: 5,935 | 0 | 0 | 0 | 560 | 10,275 | 404 | 78 | 12 | E |
| `run_build` | function-boundary | 16,755 | 10: 690 | 0 | 0 | 0 | 0 | 16,065 | 443 | 42 | 16 | - |
| `__core_net_ssl_srv13_ssl_srv13_write_server_hello_body__ead327370149d3a9` | run-cold-context +2 | 16,736 | 12: 818 | 0 | 0 | 150 | 2,660 | 13,108 | 470 | 133 | 0 | I |
| `__core_crypto_md_hash_finish` | core-activation +18 | 16,724 | 22: 1,968 | 0 | 792 | 660 | 1,620 | 11,684 | 363 | 51 | 0 | E P |
| `core_regex_compile` | batch10 +40 | 16,721 | 0: 0 | 0 | 0 | 0 | 0 | 16,721 | 31 | 2 | 0 | - |
| `RunColdContext_compile_case` | run-cold-context | 16,613 | 4: 222 | 0 | 0 | 0 | 0 | 16,391 | 449 | 60 | 17 | - |
| `Gate_encode` | scaling-enqueue | 16,596 | 2: 161 | 12 | 0 | 0 | 0 | 16,423 | 283 | 64 | 0 | - |
| `core_encoding_json___decode_typed_from__7cf6a97789f2a0ae` | comptime-items | 16,596 | 13: 2,871 | 0 | 44 | 60 | 220 | 13,401 | 365 | 82 | 10 | E |
| `run_measure` | run-performance-surface | 16,596 | 13: 2,585 | 0 | 0 | 0 | 0 | 14,011 | 447 | 142 | 7 | E |
| `core_net_tcp_connect_timeout` | run-stage-entry | 16,585 | 41: 6,098 | 0 | 220 | 570 | 540 | 9,157 | 400 | 75 | 6 | E |
| `run___read__d792861b185b4bfe` | selfhost-run | 16,552 | 6: 2,978 | 0 | 0 | 0 | 0 | 13,574 | 347 | 111 | 0 | E |
| `run_command_argv` | enum-codec-bench | 16,503 | 24: 3,627 | 0 | 44 | 0 | 160 | 12,672 | 357 | 111 | 5 | E |
| `run___read__0072e4c8f362a346` | comptime-items | 16,501 | 5: 2,823 | 0 | 0 | 30 | 160 | 13,488 | 334 | 104 | 0 | E |
| `core_regex_state` | batch10 +22 | 16,486 | 7: 326 | 0 | 440 | 300 | 660 | 14,760 | 233 | 56 | 0 | P |
| `Coverage_encode` | coverage +1 | 16,426 | 4: 433 | 0 | 0 | 0 | 0 | 15,993 | 263 | 51 | 0 | - |
| `core_files_try_lock` | batch10 +1 | 16,413 | 18: 4,353 | 0 | 0 | 0 | 1,260 | 10,800 | 376 | 78 | 0 | E |
| `IntegApply6_regenerate` | integ-apply6 | 16,362 | 12: 1,426 | 0 | 44 | 0 | 40 | 14,852 | 413 | 115 | 13 | - |
| `IntegApply6_run` | integ-apply6 | 16,337 | 8: 2,075 | 0 | 88 | 60 | 20 | 14,094 | 400 | 125 | 9 | E |
| `core_http_client___build_send` | run-cold-context +2 | 16,276 | 17: 1,105 | 0 | 0 | 0 | 1,020 | 14,151 | 405 | 87 | 4 | - |
| `__core_net_ssl_tls_ssl_handshake_init` | run-cold-context +2 | 16,259 | 2: 110 | 0 | 176 | 240 | 100 | 15,633 | 199 | 24 | 0 | - |
| `Gate_measure_timing` | scaling-harness +2 | 16,251 | 10: 1,417 | 2,196 | 44 | 210 | 40 | 12,344 | 293 | 34 | 6 | S |
| `Stage_stage_web` | run-stage-entry | 16,205 | 7: 387 | 0 | 0 | 0 | 0 | 15,818 | 424 | 108 | 0 | - |
| `run_set_up` | build-system | 16,122 | 17: 1,837 | 0 | 0 | 0 | 0 | 14,285 | 436 | 80 | 6 | E |
| `Gate___read__b89de7ba314d3a5c` | scaling-harness +2 | 16,039 | 5: 2,654 | 72 | 0 | 30 | 0 | 13,283 | 341 | 108 | 0 | E |
| `MemorySafetyFuzz_classify` | memory-safety-fuzz | 16,012 | 16: 1,471 | 0 | 0 | 0 | 120 | 14,421 | 333 | 67 | 9 | - |
| `Gate___read__f95ff789f0680c03` | scaling-harness +2 | 16,007 | 5: 2,729 | 72 | 0 | 0 | 0 | 13,206 | 341 | 112 | 0 | E |
| `core_time_civil_tzif` | jetc-profile-live +1 | 15,995 | 12: 3,748 | 0 | 132 | 600 | 1,720 | 9,795 | 364 | 110 | 0 | E I |
| `ToolCases_run_side` | run-tool-entry | 15,842 | 17: 1,020 | 0 | 0 | 0 | 40 | 14,782 | 520 | 42 | 14 | - |
| `Memory___read__357bf8d7bd64a1f9` | read-into-memory | 15,837 | 20: 3,879 | 0 | 88 | 90 | 160 | 11,620 | 339 | 83 | 3 | E |
| `core_regex_backtrack` | comptime-items +10 | 15,836 | 24: 2,783 | 0 | 748 | 510 | 3,320 | 8,475 | 386 | 97 | 0 | E P I |
| `core_net_tls___open` | run-cold-context +2 | 15,817 | 6: 428 | 0 | 0 | 0 | 80 | 15,309 | 284 | 27 | 0 | - |
| `__core_crypto_ecdsa_ecdsa_write_signature__c30964d3e2e80bd2` | run-cold-context +2 | 15,808 | 25: 3,061 | 0 | 0 | 0 | 820 | 11,927 | 370 | 104 | 0 | E |
| `run_serve` | run-probe | 15,690 | 20: 2,194 | 0 | 88 | 60 | 180 | 13,168 | 366 | 122 | 5 | E |
| `run_run` | scaling-run-gate | 15,631 | 10: 1,519 | 0 | 0 | 0 | 0 | 14,112 | 408 | 82 | 10 | - |
| `TowerSpecApply_run` | tower-spec-apply | 15,602 | 12: 2,823 | 0 | 0 | 30 | 140 | 12,609 | 434 | 150 | 8 | E |
| `__core_net_ssl_cli13_ssl_tls13_write_client_hello_exts__f38346f4a62df944` | run-cold-context +2 | 15,569 | 30: 2,198 | 0 | 0 | 120 | 3,060 | 10,191 | 493 | 117 | 0 | E I |
| `run_gate_args` | scaling-enqueue | 15,504 | 7: 1,024 | 480 | 0 | 0 | 40 | 13,960 | 275 | 67 | 10 | - |
| `Stage_service_tier` | run-stage-entry | 15,495 | 5: 330 | 0 | 0 | 0 | 0 | 15,165 | 411 | 83 | 0 | - |
| `run_run` | corelib-compare | 15,433 | 9: 1,463 | 0 | 132 | 90 | 60 | 13,688 | 287 | 63 | 4 | - |
| `__core_crypto_ecp_comb_precompute` | run-cold-context +2 | 15,396 | 24: 5,614 | 0 | 0 | 0 | 1,680 | 8,102 | 436 | 70 | 0 | E I |
| `core_files___walk_batch` | hello-compile +6 | 15,272 | 11: 2,506 | 0 | 88 | 60 | 660 | 11,958 | 262 | 76 | 1 | E |
| `MemorySafetyFuzz_receipt_tree` | memory-safety-fuzz | 15,270 | 2: 39 | 0 | 0 | 0 | 0 | 15,231 | 447 | 70 | 0 | - |
| `Stage_broken_entry` | run-stage-entry | 15,270 | 4: 146 | 0 | 352 | 0 | 0 | 14,772 | 416 | 66 | 0 | P |
| `__core_net_ssl_cli12_ssl_tls12_write_client_hello_exts` | run-cold-context +2 | 15,225 | 19: 895 | 0 | 0 | 240 | 1,440 | 12,650 | 424 | 101 | 0 | - |
| `RunColdContext_baseline_from_report` | run-cold-context | 15,224 | 7: 234 | 0 | 44 | 0 | 20 | 14,926 | 399 | 69 | 1 | - |
| `core_encoding_json___decode_typed__0c2825071765259c` | asm-parity | 15,148 | 16: 3,242 | 0 | 0 | 30 | 220 | 11,656 | 326 | 78 | 7 | E |
| `run_run` | build-system-generate +1 | 15,127 | 35: 3,016 | 0 | 0 | 0 | 20 | 12,091 | 448 | 40 | 5 | E |
| `run_run__entry` | run-tool-entry | 15,076 | 34: 5,422 | 0 | 44 | 60 | 80 | 9,470 | 419 | 111 | 8 | E |
| `run_run` | enum-codec-serde-adapter | 15,065 | 27: 1,682 | 0 | 0 | 0 | 180 | 13,203 | 484 | 111 | 7 | E |
| `__core_net_ssl_gen13_ssl_tls13_write_certificate_verify__2032ddeccf9791ee` | run-cold-context +2 | 15,058 | 27: 5,358 | 0 | 176 | 120 | 1,540 | 7,864 | 412 | 61 | 0 | E I |
| `Gate___read__802d4372cdda3d37` | scaling-harness +2 | 15,047 | 11: 2,984 | 0 | 0 | 0 | 0 | 12,063 | 332 | 98 | 2 | E |
| `Gate___read__9b9a9ce6c0ebda48` | scaling-harness +2 | 15,037 | 10: 2,692 | 0 | 44 | 60 | 0 | 12,241 | 325 | 92 | 2 | E |
| `run_encode` | core-activation +2 | 15,036 | 3: 246 | 0 | 0 | 0 | 0 | 14,790 | 276 | 55 | 0 | - |
| `Coverage_load_corpus` | coverage | 15,026 | 18: 2,906 | 0 | 0 | 0 | 20 | 12,100 | 355 | 99 | 6 | E |
| `run_run__entry` | canvas-driver-lifecycle | 14,979 | 29: 1,477 | 0 | 0 | 0 | 60 | 13,442 | 472 | 97 | 4 | - |
| `run_encode` | enum-codec-bench | 14,971 | 1: 64 | 0 | 0 | 0 | 160 | 14,747 | 298 | 67 | 0 | - |
| `Summarize___read__90996c063de3dc43` | jetc-trace | 14,966 | 5: 2,870 | 0 | 0 | 0 | 1,000 | 11,096 | 285 | 86 | 0 | E |
| `unit_unicode_is_zero_width` | comptime-items +10 | 14,907 | 0: 0 | 0 | 0 | 0 | 0 | 14,907 | 0 | 0 | 0 | U |
| `run_adapt` | build-system-adapter +1 | 14,856 | 8: 558 | 0 | 0 | 0 | 0 | 14,298 | 342 | 34 | 5 | - |
| `__core_net_ssl_cli12_ssl_cli12_write_client_key_exchange__0de0366b3debcf5d` | run-cold-context +2 | 14,855 | 19: 1,036 | 0 | 0 | 30 | 2,560 | 11,229 | 501 | 155 | 0 | I |
| `__core_crypto_cipher_crypt4` | run-cold-context +2 | 14,837 | 7: 350 | 0 | 1,672 | 1,140 | 380 | 11,295 | 264 | 60 | 0 | P |
| `run_command` | scaling-generate | 14,716 | 14: 811 | 0 | 0 | 30 | 420 | 13,455 | 371 | 73 | 7 | - |
| `core_encoding_json_parse_value` | asm-parity +36 | 14,709 | 17: 1,000 | 354 | 88 | 240 | 580 | 12,447 | 387 | 48 | 12 | - |
| `__core_crypto_ecdsa_ecdsa_verify` | run-cold-context +2 | 14,706 | 19: 3,395 | 0 | 0 | 0 | 240 | 11,071 | 572 | 100 | 0 | E |
| `Stage_summarize_samples` | run-stage-entry | 14,682 | 5: 213 | 0 | 132 | 0 | 0 | 14,337 | 368 | 59 | 0 | - |
| `run_run` | midsize | 14,678 | 23: 1,735 | 0 | 352 | 240 | 120 | 12,231 | 407 | 90 | 6 | E P |
| `core_time_zone` | jetc-profile-live +1 | 14,673 | 14: 771 | 0 | 0 | 120 | 360 | 13,422 | 342 | 108 | 7 | - |
| `core_net_socket_to_string_impl` | run-cold-context +1 | 14,660 | 37: 4,959 | 0 | 220 | 570 | 400 | 8,511 | 357 | 22 | 6 | E |
| `Corpora_shapes__lambda_974b52a0a5fb1ba9` | scaling-corpora | 14,638 | 20: 3,205 | 0 | 0 | 0 | 180 | 11,253 | 305 | 78 | 11 | E |
| `Corpora_shapes__lambda_b3e98b6d6cfe7632` | scaling-enqueue +4 | 14,638 | 20: 3,205 | 0 | 0 | 0 | 180 | 11,253 | 305 | 78 | 11 | E |
| `Memory_decode` | read-into-memory | 14,599 | 20: 3,640 | 0 | 176 | 120 | 380 | 10,283 | 319 | 82 | 3 | E |
| `__core_crypto_aead_ccm_crypt__ce2b356b6b941bbc` | run-cold-context +2 | 14,559 | 20: 1,849 | 0 | 572 | 510 | 580 | 11,048 | 325 | 64 | 0 | E P |
| `Gate_check_floors` | scaling-harness +2 | 14,546 | 5: 1,566 | 3,660 | 0 | 0 | 40 | 9,280 | 190 | 48 | 8 | E S |
| `Stage_web_artifact_sizes` | run-stage-entry | 14,515 | 11: 1,745 | 0 | 132 | 0 | 240 | 12,398 | 354 | 87 | 1 | E |
| `run_run` | run-policy | 14,501 | 27: 1,459 | 0 | 44 | 0 | 0 | 12,998 | 479 | 69 | 10 | E |
| `__core_crypto_aead_ccm_crypt__5eee6711d0bb6702` | run-cold-context +2 | 14,485 | 20: 1,849 | 0 | 572 | 510 | 580 | 10,974 | 323 | 64 | 0 | E P |
| `__core_crypto_aead_ccm_crypt__e78ddc8a48488193` | run-cold-context +2 | 14,485 | 20: 1,849 | 0 | 572 | 510 | 580 | 10,974 | 323 | 64 | 0 | E P |
| `core_http_client___client_once` | run-cold-context +2 | 14,473 | 27: 2,757 | 0 | 0 | 0 | 1,340 | 10,376 | 413 | 115 | 0 | E |
| `Report_hot` | scaling-report +1 | 14,439 | 6: 680 | 30 | 0 | 0 | 0 | 13,729 | 341 | 112 | 2 | - |
| `core_net_tls___context_seeded` | run-cold-context +2 | 14,439 | 17: 1,111 | 0 | 0 | 0 | 500 | 12,828 | 362 | 85 | 1 | - |
| `run_measure` | asm-parity | 14,426 | 19: 3,547 | 0 | 132 | 90 | 880 | 9,777 | 299 | 80 | 0 | E |
| `__core_net_ssl_srv13_ssl_srv13_write_new_session_ticket__8fdcde3add89c302` | run-cold-context +2 | 14,367 | 27: 1,959 | 0 | 0 | 300 | 2,320 | 9,788 | 483 | 100 | 0 | E I |
| `Gate___read__34110db3c6324751` | scaling-harness +2 | 14,347 | 5: 2,707 | 0 | 0 | 30 | 0 | 11,610 | 298 | 94 | 0 | E |
| `run_harness_entry` | optimization | 14,315 | 7: 1,060 | 0 | 88 | 0 | 0 | 13,167 | 417 | 70 | 5 | - |
| `core_net_tls___client_context` | run-cold-context +2 | 14,299 | 24: 1,756 | 0 | 132 | 180 | 380 | 11,851 | 344 | 85 | 3 | E |
| `Gate___read__263397fdc47b3754` | scaling-harness +2 | 14,297 | 5: 2,719 | 0 | 0 | 0 | 0 | 11,578 | 298 | 97 | 0 | E |
| `run_run_rust` | selfhost-run | 14,289 | 6: 1,388 | 0 | 0 | 0 | 60 | 12,841 | 277 | 72 | 0 | - |
| `VerifyRows_run__lambda_0033f6e2887518d7` | verify-rows | 14,285 | 10: 7,311 | 0 | 44 | 90 | 220 | 6,620 | 174 | 28 | 3 | E |
| `run___jet_cli_run` | memory-safety-fuzz | 14,259 | 34: 3,704 | 0 | 176 | 180 | 260 | 9,939 | 386 | 134 | 1 | E |
| `run_complexity_for` | selfhost-run | 14,252 | 7: 1,958 | 0 | 0 | 0 | 600 | 11,694 | 304 | 67 | 0 | E |
| `run_encode` | build-system +1 | 14,238 | 2: 161 | 12 | 0 | 0 | 0 | 14,065 | 264 | 53 | 0 | - |
| `__core_net_ssl_msg_ssl_check_client_reconnect` | run-cold-context +2 | 14,227 | 30: 2,436 | 0 | 176 | 360 | 2,700 | 8,555 | 439 | 113 | 0 | E I |
| `PerformanceSurface_pair_identity_issues` | run-performance-surface | 14,203 | 10: 743 | 0 | 0 | 0 | 80 | 13,380 | 388 | 115 | 9 | - |
| `Corpora_shapes__lambda_4122196385dce1fe` | scaling-corpora | 14,170 | 14: 4,547 | 0 | 0 | 0 | 180 | 9,443 | 229 | 56 | 5 | E |
| `Corpora_shapes__lambda_6921fc3bf2080049` | scaling-enqueue +4 | 14,170 | 14: 4,547 | 0 | 0 | 0 | 180 | 9,443 | 229 | 56 | 5 | E |
| `__core_net_ssl_gen13_ssl_tls13_generate_and_write_xxdh_key_exchange__bf09dd874c8c5de5` | run-cold-context +2 | 14,157 | 11: 1,345 | 0 | 0 | 0 | 480 | 12,332 | 398 | 80 | 0 | - |
| `run_options` | asm-parity | 14,113 | 9: 1,164 | 0 | 0 | 30 | 280 | 12,639 | 362 | 84 | 12 | - |
| `run___read__13e4fa610a7f1211` | selfhost-run | 14,103 | 5: 1,670 | 0 | 0 | 0 | 140 | 12,293 | 292 | 88 | 0 | E |
| `__core_crypto_pk_pk_encrypt__11d782aee1880875` | run-cold-context +2 | 14,033 | 33: 3,196 | 0 | 176 | 180 | 1,920 | 8,561 | 450 | 74 | 0 | E I |
| `Timer_sequence_of` | timer | 14,016 | 13: 2,752 | 0 | 0 | 0 | 0 | 11,264 | 246 | 25 | 7 | E |
| `run___read__695ea362f9d30df7` | jetc-profile | 13,978 | 10: 1,652 | 0 | 220 | 150 | 280 | 11,676 | 291 | 82 | 0 | E |
| `Tree_dyn_at` | build-system-adapter +3 | 13,961 | 18: 3,082 | 0 | 132 | 120 | 0 | 10,627 | 291 | 87 | 2 | E |
| `RuntimeCCensus_elf_symbols` | runtime-c-census | 13,950 | 14: 4,639 | 0 | 792 | 570 | 800 | 7,149 | 278 | 59 | 1 | E P |
| `Result_row_issues` | run-result | 13,912 | 7: 734 | 0 | 0 | 0 | 0 | 13,178 | 329 | 98 | 13 | - |
| `run_run` | memory-safety-fuzz | 13,884 | 23: 1,868 | 0 | 0 | 0 | 40 | 11,976 | 398 | 82 | 9 | E |
| `RunColdContext_call_api` | run-cold-context | 13,806 | 7: 121 | 0 | 0 | 0 | 80 | 13,605 | 434 | 95 | 8 | - |
| `core_encoding_json_parse_object` | run-scoreboard +36 | 13,722 | 8: 232 | 0 | 264 | 210 | 340 | 12,676 | 352 | 74 | 10 | - |
| `__core_net_ssl_msg_ssl_handle_message_type__50d72708d2172785` | run-cold-context +2 | 13,703 | 30: 1,567 | 0 | 0 | 180 | 2,780 | 9,176 | 443 | 99 | 0 | E I |
| `Gate___read__9235ef4033ba4f13` | scaling-harness +2 | 13,650 | 5: 2,110 | 216 | 0 | 0 | 160 | 11,164 | 286 | 95 | 0 | E |
| `core_encoding_json_next_key` | asm-parity +8 | 13,642 | 15: 1,279 | 0 | 176 | 420 | 1,020 | 10,747 | 344 | 71 | 7 | - |
| `RunColdContext_scratch_root` | run-cold-context | 13,639 | 16: 1,463 | 0 | 88 | 60 | 440 | 11,588 | 390 | 95 | 0 | E |
| `Gate___read__334f65e173bea22c` | scaling-harness +2 | 13,629 | 5: 2,012 | 216 | 0 | 30 | 160 | 11,211 | 285 | 91 | 0 | E |
| `run_allocations` | enumset-bench | 13,607 | 14: 2,191 | 0 | 0 | 0 | 260 | 11,156 | 354 | 100 | 3 | E |
| `Packages_write_files` | scaling-generate | 13,573 | 26: 2,587 | 0 | 264 | 180 | 0 | 10,542 | 352 | 94 | 3 | E |
| `__core_net_ssl_keys13_ssl_tls13_populate_transform` | run-cold-context +2 | 13,569 | 14: 2,501 | 0 | 0 | 360 | 280 | 10,428 | 361 | 63 | 0 | E |
| `run_summarize` | comptime-items | 13,560 | 10: 1,766 | 396 | 176 | 150 | 360 | 10,712 | 235 | 36 | 1 | E |
| `__core_crypto_pem_open_block` | run-cold-context +2 | 13,482 | 41: 3,661 | 0 | 132 | 120 | 160 | 9,409 | 451 | 64 | 1 | E |
| `__core_crypto_ecdsa_sign_core__bcc8d647729c7200` | run-cold-context +2 | 13,398 | 18: 3,729 | 0 | 0 | 0 | 780 | 8,889 | 642 | 147 | 0 | E |
| `__core_crypto_ecp_raw_reduce` | run-cold-context +2 | 13,311 | 0: 0 | 0 | 792 | 1,020 | 0 | 11,499 | 212 | 46 | 0 | P |
| `run_run_repair` | selfhost-run | 13,299 | 7: 1,171 | 0 | 0 | 0 | 60 | 12,068 | 315 | 83 | 3 | - |
| `LaneCommit_checkpoint` | lane-commit | 13,296 | 28: 2,896 | 0 | 132 | 0 | 20 | 10,248 | 322 | 87 | 1 | E |
| `__core_net_ssl_cli13_ssl_tls13_process_server_hello__d3d06318243ce7a9` | run-cold-context +2 | 13,161 | 22: 1,370 | 0 | 0 | 150 | 2,280 | 9,361 | 450 | 134 | 0 | E I |
| `RunColdContext_make_report__lambda_917ac0e977c79b00` | run-cold-context | 13,120 | 65: 2,535 | 0 | 0 | 0 | 0 | 10,585 | 459 | 56 | 0 | E |
| `PerformanceSurface_build_arm` | run-performance-surface | 13,078 | 7: 39 | 0 | 0 | 0 | 20 | 13,019 | 369 | 56 | 4 | - |
| `__core_net_ssl_cli13_ssl_tls13_process_new_session_ticket__2d7763b74aab4f1f` | run-cold-context +2 | 13,052 | 33: 1,834 | 0 | 0 | 420 | 2,700 | 8,098 | 465 | 108 | 0 | E I |
| `run___read__cebb6dc91b161d6a` | asm-parity | 13,041 | 10: 2,599 | 0 | 0 | 0 | 160 | 10,282 | 293 | 85 | 3 | E |
| `__core_net_ssl_keys13_ssl_tls13_expand_label_bytes` | run-cold-context +2 | 13,005 | 17: 3,477 | 0 | 88 | 60 | 660 | 8,720 | 346 | 41 | 0 | E |
| `core_sys_account_home` | roots-mjs | 12,970 | 14: 1,302 | 0 | 88 | 60 | 440 | 11,080 | 384 | 89 | 1 | E |
| `core_encoding_json_at__46842109d34f3525` | verify-rows | 12,962 | 15: 2,695 | 0 | 132 | 120 | 20 | 9,995 | 285 | 85 | 2 | E |
| `PerformanceSurface_measurement_context` | run-performance-surface | 12,952 | 2: 39 | 0 | 0 | 0 | 0 | 12,913 | 399 | 69 | 0 | - |
| `run___read__a2d77ef8837a49cf` | asm-parity | 12,928 | 7: 1,140 | 0 | 44 | 60 | 0 | 11,684 | 288 | 77 | 0 | - |
| `__core_net_ssl_cli_ssl_write_client_hello__62b3eaf533158afd` | run-cold-context +2 | 12,926 | 34: 1,653 | 0 | 0 | 360 | 1,940 | 8,973 | 448 | 103 | 0 | E I |
| `Gate___read__7b73d3254308b1c7` | scaling-harness +2 | 12,906 | 10: 2,518 | 108 | 0 | 0 | 160 | 10,120 | 286 | 82 | 3 | E |
| `core_sys_release` | read-into-memory +1 | 12,884 | 21: 3,325 | 0 | 528 | 360 | 1,000 | 7,671 | 330 | 97 | 0 | E P |
| `__core_crypto_pkparse_parse_key_sec1_der__a1628cb8482540bd` | run-cold-context +2 | 12,879 | 15: 464 | 0 | 0 | 30 | 1,880 | 10,505 | 322 | 108 | 0 | I |
| `Gate___read__202f89b094679398` | scaling-harness +2 | 12,870 | 9: 2,264 | 108 | 44 | 60 | 160 | 10,234 | 279 | 76 | 3 | E |
| `run___read__c612f6b61835eeeb` | canvas-driver-lifecycle | 12,865 | 4: 1,361 | 0 | 0 | 0 | 0 | 11,504 | 283 | 87 | 0 | E |
| `Scoreboard_live_loss_owner` | run-scoreboard | 12,860 | 8: 115 | 0 | 0 | 0 | 60 | 12,685 | 396 | 70 | 1 | - |
| `RunColdContext_member` | run-cold-context | 12,804 | 17: 3,106 | 0 | 132 | 120 | 0 | 9,446 | 254 | 72 | 2 | E |
| `run_record` | jetc-profile-live | 12,757 | 18: 1,186 | 0 | 0 | 0 | 220 | 11,351 | 410 | 124 | 10 | - |
| `Result_row_issues` | run-publication +1 | 12,711 | 7: 734 | 0 | 0 | 0 | 0 | 11,977 | 311 | 93 | 13 | - |
| `core_encoding_json_write_json` | corelib-compare +12 | 12,686 | 2: 102 | 72 | 88 | 30 | 280 | 12,114 | 284 | 75 | 2 | - |
| `core_text_leading_int` | optimization +1 | 12,635 | 18: 2,589 | 0 | 704 | 570 | 1,040 | 7,732 | 269 | 33 | 1 | E P |
| `run___read__c68213359d37484b` | selfhost-run | 12,619 | 5: 1,512 | 0 | 0 | 0 | 140 | 10,967 | 257 | 72 | 2 | E |
| `core_encoding_json_to_string_pretty__8d1ef022d3cfe69f` | scaling-harness +1 | 12,600 | 3: 276 | 12 | 0 | 0 | 0 | 12,312 | 224 | 47 | 0 | - |
| `run_run__entry` | run-performance-surface | 12,583 | 26: 2,779 | 0 | 0 | 30 | 60 | 9,714 | 414 | 138 | 3 | E |
| `core_encoding_json_at__681ada63f9bd6fbf` | compile-throughput +10 | 12,514 | 15: 2,695 | 0 | 132 | 120 | 0 | 9,567 | 275 | 82 | 2 | E |
| `run_print_publications` | run-publication | 12,510 | 12: 1,979 | 0 | 132 | 60 | 260 | 10,079 | 272 | 83 | 4 | E |
| `run_closed_spans` | comptime-items | 12,410 | 9: 2,909 | 0 | 440 | 300 | 340 | 8,421 | 213 | 52 | 0 | E P |
| `run_queue_plan` | scaling-enqueue | 12,369 | 8: 1,357 | 0 | 88 | 60 | 300 | 10,564 | 263 | 56 | 2 | E |
| `TowerSpecApply_node` | tower-spec-apply | 12,355 | 8: 6,974 | 0 | 0 | 0 | 180 | 5,201 | 146 | 23 | 4 | E |
| `run_contract` | build-system-adapter | 12,344 | 6: 610 | 72 | 0 | 0 | 0 | 11,662 | 240 | 69 | 7 | - |
| `__core_net_ssl_keys13_ssl_tls13_compute_handshake_transform__c9c330330583b81b` | run-cold-context +2 | 12,269 | 22: 1,184 | 0 | 0 | 0 | 320 | 10,765 | 309 | 72 | 0 | - |
| `__core_crypto_rsa_validate_params_core` | run-cold-context +2 | 12,263 | 8: 2,636 | 0 | 0 | 0 | 200 | 9,427 | 271 | 71 | 0 | E |
| `Policy_ceiling_reference_issues` | run-policy | 12,248 | 10: 2,560 | 0 | 44 | 30 | 20 | 9,594 | 255 | 78 | 6 | E |
| `__core_crypto_aead_chacha20_next` | run-cold-context +2 | 12,153 | 0: 0 | 0 | 0 | 1,020 | 20 | 11,113 | 159 | 27 | 0 | - |
| `Axis_run_probe_command` | run-tools | 12,150 | 7: 4,635 | 0 | 0 | 0 | 220 | 7,295 | 191 | 30 | 2 | E |
| `RunColdContext_extract_source` | run-cold-context | 12,148 | 7: 2,232 | 0 | 0 | 30 | 0 | 9,886 | 128 | 26 | 1 | E |
| `__core_net_ssl_keys13_ssl_tls13_xxdh_agree__5e06418ff764329e` | run-cold-context +2 | 12,084 | 25: 2,570 | 0 | 0 | 0 | 360 | 9,154 | 386 | 59 | 0 | E |
| `__core_crypto_ecp_check_pubkey` | run-cold-context +2 | 12,039 | 18: 2,950 | 0 | 0 | 0 | 520 | 8,569 | 327 | 82 | 0 | E |
| `Policy_matrix_peer_languages` | run-policy | 12,025 | 11: 1,205 | 0 | 0 | 0 | 0 | 10,820 | 289 | 84 | 0 | E |
| `core_http_client___build_chunked_body` | run-cold-context +2 | 12,006 | 21: 1,973 | 0 | 484 | 330 | 1,860 | 7,359 | 295 | 87 | 0 | E P I |
| `Number_number_from_text` | perf-measure | 11,996 | 14: 711 | 564 | 88 | 390 | 620 | 9,623 | 259 | 32 | 3 | - |
| `__core_crypto_md_ripemd160_blocks` | core-activation +18 | 11,985 | 5: 1,467 | 0 | 968 | 660 | 160 | 8,730 | 121 | 22 | 0 | E P |
| `core_math___exp_cr_accurate` | asm-parity | 11,979 | 13: 1,731 | 3,738 | 0 | 390 | 640 | 5,480 | 154 | 18 | 0 | E S |
| `Memory_counted_run` | read-into-memory | 11,962 | 8: 76 | 42 | 0 | 0 | 80 | 11,764 | 300 | 24 | 8 | - |
| `NativeCCompiler_child_hardening` | native-c-compiler | 11,942 | 3: 537 | 0 | 0 | 0 | 80 | 11,325 | 128 | 21 | 1 | - |
| `core_process_exec` | streamb10 | 11,934 | 10: 4,729 | 0 | 0 | 0 | 240 | 6,965 | 202 | 27 | 2 | E |
| `run_render_folded` | jetc-profile | 11,906 | 12: 1,908 | 0 | 308 | 240 | 680 | 8,770 | 278 | 76 | 4 | E P |
| `MemorySafetyFuzz_minimized_case` | memory-safety-fuzz | 11,896 | 2: 146 | 0 | 0 | 60 | 20 | 11,670 | 276 | 47 | 0 | - |
| `Probe_line_probe` | run-stage-entry | 11,851 | 18: 1,801 | 0 | 0 | 0 | 480 | 9,570 | 405 | 136 | 7 | E |
| `core_time___civil_tz_edge` | jetc-profile-live +1 | 11,842 | 11: 1,474 | 0 | 0 | 150 | 860 | 9,358 | 339 | 109 | 6 | E |
| `run_run` | asm-parity | 11,802 | 6: 2,252 | 0 | 0 | 0 | 140 | 9,410 | 263 | 90 | 2 | E |
| `Gate___read__902f0e4d1801ea7a` | scaling-harness +2 | 11,798 | 5: 2,198 | 0 | 0 | 0 | 0 | 9,600 | 249 | 79 | 0 | E |
| `Comparison_run_tier_cell` | run-comparison +2 | 11,781 | 9: 1,216 | 120 | 44 | 60 | 0 | 10,341 | 316 | 88 | 2 | E |
| `Gate___read__9aa1857605a0bc9f` | scaling-harness +2 | 11,780 | 5: 2,069 | 0 | 0 | 30 | 0 | 9,681 | 249 | 75 | 0 | E |
| `Corpora_shapes__lambda_b523d6b9401b7a57` | scaling-enqueue +4 | 11,768 | 15: 3,893 | 0 | 0 | 0 | 140 | 7,735 | 204 | 64 | 3 | E |
| `RunColdContext_validate_capsule` | run-cold-context | 11,758 | 12: 485 | 0 | 176 | 90 | 300 | 10,707 | 308 | 59 | 3 | - |
| `NativeCCompiler_read_rules` | native-c-compiler | 11,749 | 13: 1,869 | 0 | 352 | 240 | 0 | 9,288 | 305 | 55 | 5 | E P |
| `MemorySafetyFuzz_record_summary` | memory-safety-fuzz | 11,748 | 7: 1,971 | 0 | 308 | 210 | 980 | 8,279 | 192 | 50 | 0 | E P |
| `RunColdContext_preflight` | run-cold-context | 11,746 | 12: 1,798 | 0 | 0 | 0 | 0 | 9,948 | 289 | 95 | 6 | E |
| `RunColdContext_load_tasks` | run-cold-context | 11,717 | 7: 2,042 | 0 | 0 | 0 | 0 | 9,675 | 212 | 71 | 1 | E |
| `core_encoding_json_stringify__0e90a001db3ab80c` | asm-parity | 11,713 | 3: 321 | 108 | 0 | 0 | 0 | 11,284 | 214 | 37 | 0 | - |
| `Stage_service_tier_metrics` | run-stage-entry | 11,700 | 1: 39 | 0 | 0 | 0 | 0 | 11,661 | 354 | 69 | 0 | - |
| `run_run` | build-system | 11,675 | 12: 2,000 | 96 | 0 | 0 | 260 | 9,319 | 306 | 78 | 11 | E |
| `Gate___read__7c3c193932d31155` | scaling-harness +2 | 11,674 | 5: 2,097 | 120 | 0 | 0 | 0 | 9,457 | 241 | 77 | 0 | E |
| `core_regex_literal_facts` | batch10 +22 | 11,652 | 5: 1,218 | 0 | 220 | 210 | 440 | 9,564 | 177 | 51 | 0 | E |
| `LaneCommit_jobs_in` | lane-commit | 11,646 | 13: 1,814 | 0 | 88 | 60 | 260 | 9,424 | 210 | 68 | 0 | E |
| `run_parse_options` | optimization | 11,637 | 7: 2,142 | 0 | 0 | 0 | 20 | 9,475 | 274 | 73 | 4 | E |
| `Gate___read__bc1562807e9e44b6` | scaling-harness +2 | 11,624 | 5: 1,968 | 120 | 0 | 30 | 0 | 9,506 | 241 | 73 | 0 | E |
| `core_encoding_json___decode_typed__27bcbc4f20234bbd` | jetc-trace | 11,576 | 7: 707 | 0 | 264 | 180 | 260 | 10,165 | 281 | 47 | 9 | P |
| `Corpora_shapes__lambda_8e3f6fe7648299fa` | scaling-corpora | 11,539 | 12: 3,150 | 0 | 0 | 0 | 520 | 7,869 | 283 | 69 | 7 | E |
| `Corpora_shapes__lambda_d5a9f3ebfcd4fe7d` | scaling-enqueue +4 | 11,539 | 12: 3,150 | 0 | 0 | 0 | 520 | 7,869 | 283 | 69 | 7 | E |
| `__core_crypto_pkparse_parse_key_pkcs8_unencrypted_der__ce71bf0dc2e4b9ac` | run-cold-context +2 | 11,535 | 14: 1,902 | 0 | 0 | 0 | 1,940 | 7,693 | 417 | 126 | 0 | E I |
| `run_copy_tree` | asm-parity | 11,524 | 14: 1,810 | 0 | 88 | 60 | 260 | 9,306 | 249 | 90 | 0 | E |
| `__core_net_ssl_srv13_ssl_srv13_offered_psks_check_identity_match` | run-cold-context +2 | 11,491 | 14: 513 | 0 | 0 | 0 | 380 | 10,598 | 189 | 55 | 0 | - |
| `Gate_absolute` | scaling-enqueue | 11,485 | 22: 2,274 | 0 | 176 | 210 | 0 | 8,825 | 273 | 22 | 4 | E |
| `Stage_measure_runs` | run-stage-entry | 11,480 | 9: 781 | 0 | 132 | 0 | 0 | 10,567 | 293 | 59 | 0 | - |
| `core_encoding_json_patch_edit` | run-performance-surface | 11,480 | 7: 636 | 0 | 132 | 120 | 440 | 10,152 | 236 | 67 | 4 | - |
| `__core_crypto_ecp_comb_recode_scalar` | run-cold-context +2 | 11,424 | 23: 2,432 | 0 | 748 | 510 | 220 | 7,514 | 239 | 50 | 0 | E P |
| `run___read__a95ed8d3ac2f3ad3` | selfhost-run | 11,421 | 5: 2,198 | 0 | 0 | 0 | 0 | 9,223 | 239 | 76 | 0 | E |
| `Corpora_shapes__lambda_04acc94fe5f8b1f0` | scaling-corpora | 11,371 | 14: 3,613 | 0 | 0 | 0 | 140 | 7,618 | 197 | 63 | 3 | E |
| `run_read_receipts__lambda_e373ef135e6e60d0` | scaling-report | 11,340 | 27: 2,250 | 0 | 176 | 330 | 0 | 8,584 | 259 | 19 | 5 | E |
| `run___read__84be95422a4fafb9` | asm-parity | 11,331 | 6: 1,915 | 0 | 0 | 30 | 360 | 9,026 | 244 | 70 | 0 | E |
| `__core_crypto_pkcs12_pkcs12_pbe` | run-cold-context +2 | 11,314 | 16: 1,505 | 0 | 88 | 60 | 760 | 8,901 | 370 | 110 | 0 | E |
| `core_process_status` | batch10 | 11,314 | 7: 6,847 | 0 | 0 | 0 | 180 | 4,287 | 109 | 18 | 0 | E |
| `run___read__c28e9a1bcbe7e92a` | asm-parity | 11,259 | 5: 1,304 | 0 | 0 | 0 | 0 | 9,955 | 246 | 71 | 0 | E |
| `MemorySafetyFuzz_mark_runner_failure` | memory-safety-fuzz | 11,210 | 5: 141 | 0 | 0 | 0 | 0 | 11,069 | 315 | 50 | 1 | - |
| `run_jet_binary` | hello-compile | 11,210 | 17: 1,611 | 0 | 44 | 0 | 500 | 9,055 | 300 | 68 | 1 | E |
| `IntegApply6_stages` | integ-apply6 | 11,190 | 9: 924 | 0 | 0 | 0 | 80 | 10,186 | 335 | 96 | 13 | - |
| `Mixed_words_worker__lambda_1a587c3e97eb0740` | scaling-generate | 11,172 | 24: 3,945 | 0 | 0 | 0 | 820 | 6,407 | 265 | 76 | 4 | E |
| `Scoreboard_run_reference_verdict` | run-scoreboard | 11,108 | 10: 956 | 84 | 44 | 30 | 0 | 9,994 | 310 | 57 | 4 | - |
| `core_encoding_json___cursor_skip_object` | asm-parity +8 | 11,107 | 4: 165 | 0 | 44 | 60 | 60 | 10,778 | 283 | 40 | 10 | - |
| `core_regex_parse_escape` | batch10 +22 | 11,075 | 18: 1,181 | 0 | 132 | 150 | 920 | 8,692 | 308 | 60 | 8 | E |
| `Json_ordered` | perf-measure | 11,052 | 8: 1,110 | 0 | 308 | 210 | 320 | 9,104 | 180 | 52 | 0 | E P |
| `Stage_wait_for_ready` | run-stage-entry | 11,039 | 16: 862 | 72 | 88 | 0 | 240 | 9,777 | 329 | 61 | 3 | - |
| `run_input` | run-policy | 11,010 | 19: 1,496 | 0 | 0 | 0 | 20 | 9,494 | 305 | 71 | 7 | E |
| `LaneCommit_command` | lane-commit | 10,992 | 7: 6,847 | 0 | 0 | 0 | 180 | 3,965 | 99 | 13 | 0 | E |
| `run_run__entry` | run-probe | 10,964 | 29: 1,503 | 0 | 0 | 60 | 160 | 9,241 | 367 | 96 | 4 | E |
| `run_generate` | scaling-generate | 10,925 | 6: 129 | 0 | 0 | 90 | 240 | 10,466 | 290 | 53 | 3 | - |
| `CensusSnap2_build_rows` | census-snap2 | 10,839 | 9: 1,481 | 0 | 0 | 0 | 40 | 9,318 | 165 | 51 | 1 | E |
| `Packages_consumer_tree` | scaling-corpora +5 | 10,811 | 9: 2,386 | 0 | 88 | 60 | 440 | 7,837 | 248 | 62 | 6 | E |
| `run_measure` | function-boundary | 10,771 | 9: 654 | 60 | 0 | 90 | 260 | 9,707 | 241 | 40 | 0 | - |
| `__core_crypto_pk_pk_ecc_set_key` | run-cold-context +2 | 10,731 | 2: 78 | 0 | 0 | 0 | 0 | 10,653 | 154 | 22 | 0 | - |
| `__core_crypto_rsa_rsassa_pkcs1_v15_encode` | run-cold-context +2 | 10,713 | 2: 159 | 0 | 0 | 0 | 420 | 10,134 | 248 | 81 | 0 | - |
| `run___read__50a1bcabc302cafb` | selfhost-run | 10,682 | 5: 1,397 | 0 | 0 | 0 | 480 | 8,805 | 226 | 68 | 0 | E |
| `run___read__ca82c7a110a32b1c` | asm-parity | 10,677 | 5: 1,423 | 0 | 0 | 0 | 320 | 8,934 | 231 | 69 | 0 | E |
| `run___read__3b8ef6b10209f843` | selfhost-run | 10,674 | 5: 1,424 | 0 | 0 | 0 | 320 | 8,930 | 231 | 69 | 0 | E |
| `core_time_format_rfc3339` | build-system +6 | 10,652 | 31: 1,521 | 0 | 0 | 0 | 1,880 | 7,251 | 342 | 79 | 5 | E I |
| `core_encoding_json_pointer` | build-system-generate +1 | 10,635 | 12: 2,488 | 0 | 132 | 120 | 0 | 7,895 | 222 | 68 | 1 | E |
| `Number_js_number` | perf-measure | 10,627 | 12: 1,894 | 144 | 176 | 120 | 480 | 7,813 | 260 | 63 | 7 | E |
| `run_input` | run-result | 10,624 | 18: 1,413 | 0 | 0 | 0 | 20 | 9,191 | 292 | 65 | 7 | E |
| `Batch10_read_all` | batch10 | 10,576 | 11: 1,537 | 0 | 88 | 60 | 260 | 8,631 | 204 | 67 | 1 | E |
| `__core_net_ssl_msg_ssl_read_record__d279893b712a1266` | run-cold-context +2 | 10,565 | 3: 229 | 0 | 220 | 150 | 420 | 9,546 | 133 | 35 | 0 | P |
| `Packages_write_files` | scaling-harness +2 | 10,472 | 17: 1,919 | 0 | 264 | 180 | 0 | 8,109 | 249 | 74 | 2 | E P |
| `run_flush` | jetc-profile | 10,463 | 3: 210 | 0 | 132 | 180 | 200 | 9,741 | 192 | 40 | 2 | - |
| `Domains_generated__lambda_a92c2fa06273df55` | scaling-corpora | 10,429 | 3: 1,241 | 0 | 0 | 0 | 20 | 9,168 | 286 | 62 | 14 | E |
| `Domains_generated__lambda_c26d9cb8e9537610` | scaling-enqueue +4 | 10,429 | 3: 1,241 | 0 | 0 | 0 | 20 | 9,168 | 286 | 62 | 14 | E |
| `Mixed_words_worker__lambda_db592db8f6eb37d3` | scaling-corpora | 10,341 | 20: 3,264 | 0 | 0 | 0 | 740 | 6,337 | 234 | 67 | 3 | E |
| `PerformanceSurface_validate_manifest_pair` | run-performance-surface | 10,305 | 2: 214 | 0 | 0 | 0 | 80 | 10,011 | 218 | 72 | 0 | - |
| `Scoreboard_scoreboard_metric_comparison` | run-scoreboard | 10,263 | 9: 1,027 | 0 | 88 | 0 | 0 | 9,148 | 281 | 66 | 4 | E |
| `run_function_symbols` | hot-text | 10,241 | 15: 2,722 | 0 | 572 | 420 | 340 | 6,187 | 216 | 58 | 0 | E P |
| `Policy_matrix_peer_languages` | run-publication +1 | 10,229 | 10: 1,038 | 0 | 0 | 0 | 0 | 9,191 | 265 | 83 | 0 | E |
| `run_store_perf` | jetc-profile-live | 10,228 | 14: 2,198 | 0 | 220 | 150 | 400 | 7,260 | 206 | 67 | 0 | E P |
| `core_sys_config_dir` | function-boundary +1 | 10,207 | 13: 1,161 | 0 | 88 | 60 | 440 | 8,458 | 281 | 64 | 1 | E |
| `Memory_build` | read-into-memory | 10,199 | 9: 295 | 0 | 0 | 30 | 20 | 9,854 | 312 | 20 | 10 | - |
| `MemorySafetyFuzz_finding_tree` | memory-safety-fuzz | 10,188 | 1: 39 | 0 | 0 | 0 | 0 | 10,149 | 302 | 49 | 0 | - |
| `__core_crypto_aead_gcm_update__91be9d7a84f8a009` | run-cold-context +2 | 10,141 | 9: 480 | 0 | 792 | 540 | 800 | 7,529 | 201 | 19 | 0 | P |
| `run___read__158b81928e8848c2` | selfhost-run | 10,118 | 6: 1,688 | 0 | 0 | 0 | 0 | 8,430 | 219 | 61 | 2 | E |
| `Gate_absolute` | scaling-harness +2 | 10,086 | 18: 1,993 | 0 | 176 | 210 | 0 | 7,707 | 227 | 20 | 4 | E |
| `Gate___read__ee57ff1bbbfc934a` | scaling-harness +2 | 10,081 | 5: 1,449 | 0 | 0 | 30 | 160 | 8,442 | 216 | 61 | 0 | E |
| `CensusSnap2_snapshot` | census-snap2 | 10,053 | 9: 843 | 0 | 0 | 0 | 20 | 9,190 | 276 | 71 | 7 | - |
| `Gate___read__d1d3be3cfb0044e1` | scaling-harness +2 | 10,049 | 5: 1,498 | 0 | 0 | 0 | 160 | 8,391 | 217 | 65 | 0 | E |
| `__core_crypto_aead_gcm_update__847331b081c31173` | run-cold-context +2 | 9,982 | 9: 480 | 0 | 792 | 540 | 800 | 7,370 | 198 | 19 | 0 | P |
| `__core_crypto_aead_gcm_update__8b9b166bf3ef8bbe` | run-cold-context +2 | 9,982 | 9: 480 | 0 | 792 | 540 | 800 | 7,370 | 198 | 19 | 0 | P |
| `run_run` | run-publication | 9,982 | 19: 2,142 | 0 | 220 | 270 | 0 | 7,350 | 260 | 75 | 4 | E P |
| `run___read__0ef958a10cb8e857` | selfhost-run | 9,957 | 5: 1,615 | 0 | 0 | 0 | 0 | 8,342 | 215 | 64 | 0 | E |
| `Gate_save` | scaling-run-gate | 9,899 | 6: 1,018 | 0 | 308 | 210 | 140 | 8,223 | 242 | 31 | 3 | E P |
| `__core_crypto_block_camellia_setkey_enc` | run-cold-context +2 | 9,886 | 16: 1,553 | 0 | 1,144 | 780 | 360 | 6,049 | 228 | 13 | 0 | E P |
| `Tree_read_json` | build-system-adapter +5 | 9,829 | 19: 1,232 | 0 | 0 | 0 | 20 | 8,577 | 282 | 60 | 6 | E |
| `VerifyRows_row_line` | verify-rows | 9,818 | 10: 239 | 0 | 0 | 90 | 80 | 9,409 | 298 | 51 | 10 | - |
| `__core_net_ssl_tls_ssl_psk_derive_premaster__c520c5e4a32a45d6` | run-cold-context +2 | 9,818 | 9: 43 | 0 | 88 | 120 | 660 | 8,907 | 246 | 51 | 0 | - |
| `Gate___read__1019e8da7772133f` | scaling-harness +2 | 9,775 | 5: 1,569 | 96 | 0 | 0 | 0 | 8,110 | 207 | 64 | 0 | E |
| `run_stage_files` | memory-safety-fuzz | 9,765 | 11: 1,653 | 0 | 88 | 0 | 0 | 8,024 | 245 | 88 | 1 | E |
| `Stage_aot_tier` | run-stage-entry | 9,758 | 2: 39 | 0 | 0 | 0 | 0 | 9,719 | 281 | 56 | 0 | - |
| `Gate___read__9395c99b1975f3d0` | scaling-harness +2 | 9,752 | 5: 1,464 | 96 | 0 | 30 | 0 | 8,162 | 207 | 60 | 0 | E |
| `Gate_save` | scaling-harness +1 | 9,733 | 6: 1,018 | 0 | 308 | 210 | 140 | 8,057 | 234 | 28 | 3 | E P |
| `RunColdContext_call_command` | run-cold-context | 9,727 | 3: 231 | 0 | 0 | 0 | 0 | 9,496 | 284 | 58 | 7 | - |
| `PerformanceSurface_artifact_identity` | run-performance-surface | 9,717 | 22: 1,132 | 60 | 0 | 510 | 400 | 7,615 | 310 | 35 | 1 | E |
| `core_regex_parse_atom` | batch10 +22 | 9,715 | 15: 1,320 | 0 | 176 | 210 | 2,300 | 5,709 | 314 | 70 | 2 | E I |
| `__core_crypto_cipher_kw_wrap` | run-cold-context +2 | 9,683 | 20: 2,639 | 0 | 616 | 420 | 360 | 5,648 | 221 | 50 | 0 | E P |
| `__core_crypto_block_crypt_block` | run-cold-context +5 | 9,658 | 9: 459 | 0 | 440 | 300 | 240 | 8,219 | 109 | 23 | 0 | P |
| `core_regex_parse_seq` | batch10 +22 | 9,656 | 17: 1,512 | 0 | 308 | 240 | 1,800 | 5,796 | 274 | 69 | 2 | E P I |
| `__core_net_ssl_cli12_ssl_cli12_parse_server_dh_params` | run-cold-context +2 | 9,568 | 12: 3,252 | 0 | 0 | 0 | 200 | 6,116 | 183 | 43 | 0 | E |
| `Midsize_go_unit__value_ce7834d90a99df0d` | midsize | 9,562 | 42: 1,638 | 0 | 0 | 0 | 0 | 7,924 | 468 | 45 | 30 | E |
| `core_sys_env_braced` | batch10 +15 | 9,559 | 7: 100 | 0 | 88 | 180 | 420 | 8,771 | 248 | 63 | 5 | - |
| `IntegApply6_staged_mode` | integ-apply6 | 9,523 | 6: 936 | 0 | 0 | 0 | 160 | 8,427 | 275 | 78 | 16 | - |
| `run_assemble` | asm-parity | 9,494 | 17: 1,107 | 0 | 0 | 0 | 20 | 8,367 | 297 | 86 | 3 | E |
| `core_text_runtime_kernel_text_normal_decompose_cp` | jetc-profile +2 | 9,473 | 14: 1,228 | 0 | 88 | 60 | 1,500 | 6,597 | 203 | 40 | 2 | E I |
| `core_files_entries_glob_tree` | jetc-profile-live | 9,464 | 8: 1,653 | 0 | 0 | 0 | 640 | 7,171 | 182 | 57 | 0 | E |
| `CensusSnap2_runtime` | census-snap2 | 9,424 | 7: 773 | 0 | 0 | 0 | 60 | 8,591 | 294 | 89 | 10 | - |
| `Stage_settle_run_tier` | run-stage-entry | 9,416 | 7: 125 | 252 | 0 | 30 | 0 | 9,009 | 292 | 64 | 3 | - |
| `core_sys_cache_dir` | optimization | 9,360 | 13: 1,152 | 0 | 88 | 60 | 440 | 7,620 | 265 | 61 | 0 | E |
| `__core_crypto_md_hash` | core-activation +14 | 9,333 | 14: 1,708 | 0 | 352 | 240 | 1,220 | 5,813 | 205 | 39 | 0 | E P I |
| `run_run_command` | memory-safety-fuzz | 9,272 | 8: 4,485 | 0 | 0 | 0 | 180 | 4,607 | 126 | 21 | 1 | E |
| `__core_crypto_cipher_kw_unwrap` | run-cold-context +2 | 9,206 | 19: 2,602 | 0 | 616 | 420 | 320 | 5,248 | 209 | 14 | 0 | E P |
| `Stage_stage_steps` | run-stage-entry | 9,200 | 8: 1,049 | 0 | 0 | 0 | 0 | 8,151 | 201 | 62 | 1 | E |
| `run_repo_root` | coverage | 9,192 | 42: 2,701 | 0 | 176 | 180 | 0 | 6,135 | 316 | 56 | 3 | E |
| `run_run__entry` | scaling-report | 9,176 | 23: 1,626 | 0 | 0 | 0 | 20 | 7,530 | 327 | 52 | 4 | E |
| `__core_net_ssl_gen13_ssl_tls13_write_certificate__9ef34d3c9a9e3d8e` | run-cold-context +2 | 9,114 | 12: 2,171 | 0 | 0 | 30 | 1,320 | 5,593 | 246 | 42 | 0 | E I |
| `__core_net_ssl_srv12_ssl_srv12_parse_client_key_exchange__2c2cc107e5d165cf` | run-cold-context +2 | 9,095 | 6: 197 | 0 | 0 | 30 | 1,380 | 7,488 | 326 | 114 | 0 | I |
| `core_encoding_json_write_stringify` | asm-parity +25 | 9,065 | 3: 133 | 252 | 88 | 60 | 200 | 8,332 | 208 | 62 | 1 | - |
| `__core_crypto_rsa_apply_crt` | run-cold-context +2 | 9,062 | 16: 4,082 | 0 | 88 | 60 | 1,280 | 3,552 | 310 | 49 | 0 | E I |
| `Gate_profile_top` | scaling-harness +2 | 9,051 | 4: 925 | 0 | 0 | 120 | 80 | 7,926 | 190 | 54 | 2 | E |
| `Corpora_shapes__lambda_00f4af2fd014c225` | scaling-enqueue +4 | 8,973 | 19: 3,314 | 0 | 0 | 0 | 440 | 5,219 | 230 | 62 | 2 | E |
| `Corpora_shapes__lambda_a78dda96dc2d5e16` | scaling-corpora | 8,973 | 19: 3,314 | 0 | 0 | 0 | 440 | 5,219 | 230 | 62 | 2 | E |
| `Packages_manifest` | scaling-corpora | 8,952 | 0: 0 | 0 | 0 | 0 | 0 | 8,952 | 88 | 8 | 3 | - |
| `core_regex_matches` | selfhost-run | 8,909 | 11: 2,435 | 0 | 88 | 60 | 820 | 5,506 | 174 | 45 | 0 | E |
| `Corpora_shapes__lambda_686711324aed0968` | scaling-corpora | 8,868 | 16: 3,045 | 0 | 0 | 0 | 560 | 5,263 | 224 | 15 | 4 | E |
| `Corpora_shapes__lambda_c20c3204e884ecaf` | scaling-enqueue +4 | 8,868 | 16: 3,045 | 0 | 0 | 0 | 560 | 5,263 | 224 | 15 | 4 | E |
| `core_encoding_json_pointer_select` | build-system +20 | 8,827 | 9: 623 | 0 | 44 | 120 | 380 | 7,660 | 220 | 68 | 5 | - |
| `core_encoding_json_json_equal` | run-performance-surface | 8,764 | 1: 39 | 312 | 176 | 120 | 0 | 8,117 | 87 | 4 | 0 | P |
| `Stage_timer_sample` | run-stage-entry | 8,745 | 7: 277 | 0 | 0 | 0 | 60 | 8,408 | 265 | 48 | 1 | - |
| `core_encoding_json_parse_array` | asm-parity +36 | 8,717 | 4: 439 | 0 | 0 | 30 | 300 | 7,948 | 212 | 54 | 8 | - |
| `core_http_client___client_close` | run-cold-context +2 | 8,701 | 48: 2,069 | 0 | 0 | 0 | 540 | 6,092 | 353 | 38 | 4 | E |
| `run_compare` | enumset-bench | 8,683 | 2: 389 | 234 | 88 | 150 | 20 | 7,802 | 129 | 31 | 0 | - |
| `Timer_launcher` | timer | 8,682 | 18: 1,590 | 0 | 176 | 180 | 0 | 6,736 | 255 | 26 | 4 | E P |
| `RunColdContext_judge` | run-cold-context | 8,681 | 0: 0 | 0 | 0 | 0 | 0 | 8,681 | 277 | 44 | 5 | - |
| `Gate___read__6557219bde0a2eb8` | scaling-harness +2 | 8,677 | 4: 1,166 | 0 | 0 | 30 | 0 | 7,481 | 188 | 58 | 0 | E |
| `core_text_fmt_decimal` | enum-codec-bench +9 | 8,673 | 10: 1,136 | 2,616 | 0 | 0 | 820 | 4,101 | 152 | 25 | 4 | E S |
| `LaneCommit_apply_job` | lane-commit | 8,640 | 2: 164 | 0 | 44 | 0 | 0 | 8,432 | 253 | 71 | 10 | - |
| `core_math_ln` | asm-parity +3 | 8,636 | 12: 2,304 | 3,438 | 0 | 90 | 280 | 2,524 | 95 | 6 | 0 | E S |
| `core_http___with_header` | run-cold-context | 8,629 | 1: 433 | 0 | 0 | 0 | 0 | 8,196 | 99 | 9 | 0 | - |
| `Policy_matrix_peer_languages` | run-scoreboard | 8,625 | 8: 841 | 0 | 0 | 0 | 0 | 7,784 | 204 | 62 | 0 | - |
| `Domains_float_literals` | scaling-corpora +2 | 8,602 | 11: 2,001 | 0 | 88 | 60 | 80 | 6,373 | 165 | 47 | 1 | E |
| `Stage_source_metrics` | run-stage-entry | 8,595 | 9: 490 | 0 | 44 | 30 | 100 | 7,931 | 245 | 35 | 1 | - |
| `run_verdict` | scaling-module-lookups-proof | 8,580 | 2: 767 | 1,908 | 0 | 90 | 240 | 5,575 | 156 | 47 | 4 | S |
| `LaneCommit_drain_trapped` | lane-commit | 8,473 | 15: 2,342 | 0 | 0 | 0 | 860 | 5,271 | 240 | 45 | 0 | E I |
| `Result_tier_issues` | run-publication +2 | 8,472 | 5: 357 | 0 | 0 | 0 | 0 | 8,115 | 198 | 55 | 10 | - |
| `__core_net_ssl_gen13_ssl_tls13_process_certificate_verify__5dffe1f06575d890` | run-cold-context +2 | 8,471 | 23: 924 | 0 | 0 | 120 | 1,340 | 6,087 | 304 | 75 | 0 | E I |
| `Domains_generated__lambda_233698d4b990c8fe` | scaling-enqueue +4 | 8,403 | 3: 1,447 | 0 | 0 | 0 | 0 | 6,956 | 204 | 47 | 13 | E |
| `Domains_generated__lambda_f87395a4b31c411f` | scaling-corpora | 8,403 | 3: 1,447 | 0 | 0 | 0 | 0 | 6,956 | 204 | 47 | 13 | E |
| `Measure_read_trace` | perf-measure | 8,390 | 8: 1,927 | 0 | 44 | 30 | 20 | 6,369 | 168 | 58 | 0 | E |
| `Integrate10_named` | integrate10 | 8,369 | 11: 1,480 | 0 | 88 | 60 | 260 | 6,481 | 165 | 54 | 0 | E |
| `__core_crypto_aead_ccm_update__57cbe6c4763607ea` | run-cold-context +2 | 8,363 | 9: 1,224 | 0 | 528 | 360 | 740 | 5,511 | 186 | 18 | 0 | E P |
| `__core_crypto_aead_ccm_update__79ade47b0d71e5cf` | run-cold-context +2 | 8,363 | 9: 1,224 | 0 | 528 | 360 | 740 | 5,511 | 186 | 18 | 0 | E P |
| `core_sys_env_word` | batch10 +15 | 8,303 | 7: 425 | 0 | 352 | 240 | 580 | 6,706 | 189 | 53 | 1 | P |
| `__core_crypto_cipher_xts` | run-cold-context +2 | 8,297 | 12: 2,133 | 0 | 572 | 390 | 600 | 4,602 | 199 | 39 | 0 | E P |
| `Gate_project` | scaling-harness +2 | 8,292 | 3: 454 | 1,464 | 0 | 30 | 40 | 6,304 | 102 | 18 | 0 | S |
| `core_encoding_json_to_string_pretty__37cbca07777ffd58` | scaling-module-lookups-proof | 8,288 | 1: 127 | 24 | 0 | 0 | 0 | 8,137 | 141 | 30 | 0 | - |
| `run_print_probe` | run-probe | 8,274 | 20: 705 | 36 | 0 | 0 | 0 | 7,533 | 253 | 48 | 2 | - |
| `__core_net_ssl_msg_ssl_flight_transmit__8dd8a17ad4cbad09` | run-cold-context +2 | 8,267 | 11: 707 | 0 | 308 | 210 | 2,100 | 4,942 | 234 | 66 | 0 | P I |
| `__core_crypto_aead_ccm_update__4da481fea5a8f565` | run-cold-context +2 | 8,251 | 8: 1,071 | 0 | 528 | 360 | 700 | 5,592 | 180 | 18 | 0 | E P |
| `__core_crypto_x509_x509_get_time` | run-cold-context +2 | 8,246 | 13: 1,282 | 0 | 0 | 60 | 1,580 | 5,324 | 224 | 50 | 0 | E I |
| `core_files_entries_copy_tree` | streamb10 | 8,208 | 7: 1,718 | 0 | 0 | 0 | 760 | 5,730 | 186 | 56 | 0 | E |
| `LaneCommit_commit_job` | lane-commit | 8,184 | 3: 218 | 0 | 0 | 0 | 0 | 7,966 | 237 | 82 | 5 | - |
| `Stage_noise_summary` | run-stage-entry | 8,176 | 2: 183 | 1,788 | 44 | 60 | 0 | 6,101 | 179 | 34 | 0 | S |
| `run_phase_line` | enumset-bench | 8,164 | 6: 1,424 | 0 | 88 | 0 | 0 | 6,652 | 151 | 42 | 3 | E |
| `__core_crypto_x509_x509_get_name` | run-cold-context +2 | 8,158 | 14: 1,324 | 0 | 44 | 30 | 1,380 | 5,380 | 238 | 59 | 0 | E I |
| `__core_crypto_ecp_sw_rhs` | run-cold-context +2 | 8,146 | 5: 1,117 | 0 | 0 | 0 | 0 | 7,029 | 171 | 52 | 0 | E |
| `core_text_parse___version_runs_order` | verify-rows | 8,145 | 9: 514 | 0 | 792 | 540 | 720 | 5,579 | 151 | 23 | 0 | P |
| `IntegLand4_commit` | integ-land4 | 8,113 | 2: 193 | 0 | 0 | 0 | 0 | 7,920 | 199 | 63 | 3 | - |
| `Domains_float_literals` | scaling-harness +2 | 8,080 | 11: 2,001 | 0 | 88 | 60 | 80 | 5,851 | 152 | 43 | 1 | E |
| `RunColdContext_command_spec` | run-cold-context | 8,052 | 7: 4,404 | 0 | 0 | 0 | 200 | 3,448 | 97 | 14 | 0 | E |
| `core_process_spec` | census-snap2 +4 | 8,010 | 7: 4,404 | 0 | 0 | 0 | 180 | 3,426 | 88 | 13 | 0 | E |
| `core_regex_add` | batch10 +31 | 8,004 | 4: 51 | 0 | 704 | 480 | 660 | 6,109 | 161 | 32 | 0 | P |
| `run_allocations` | core-activation | 7,968 | 9: 1,959 | 66 | 44 | 30 | 200 | 5,669 | 203 | 69 | 2 | E |
| `Stage_jet_tiers` | run-stage-entry | 7,962 | 10: 888 | 0 | 44 | 0 | 0 | 7,030 | 217 | 28 | 3 | E |
| `core_args_cli_unknown` | build-system-adapter +2 | 7,948 | 8: 2,493 | 0 | 264 | 180 | 180 | 4,831 | 147 | 41 | 3 | E P |
| `Corpora_shapes__lambda_8c25304f5fa38212` | scaling-corpora | 7,934 | 16: 3,528 | 0 | 0 | 0 | 640 | 3,766 | 219 | 51 | 3 | E |
| `Corpora_shapes__lambda_dddd0af746e4f1b1` | scaling-enqueue +4 | 7,934 | 16: 3,528 | 0 | 0 | 0 | 640 | 3,766 | 219 | 51 | 3 | E |
| `IntegApply6_based` | integ-apply6 | 7,929 | 1: 41 | 0 | 0 | 0 | 0 | 7,888 | 232 | 66 | 8 | - |
| `__core_crypto_pkcs12_pkcs12_derivation` | run-cold-context +2 | 7,921 | 15: 2,478 | 0 | 132 | 90 | 760 | 4,461 | 260 | 40 | 0 | E |
| `core_archive_huff_codes` | run-cold-context +2 | 7,907 | 6: 1,141 | 0 | 528 | 360 | 260 | 5,618 | 123 | 32 | 0 | E P |
| `__core_net_ssl_tls_ssl_derive_keys` | run-cold-context +2 | 7,851 | 11: 1,157 | 0 | 0 | 120 | 60 | 6,514 | 169 | 31 | 0 | E |
| `__core_crypto_ecp_sw_group` | run-cold-context +2 | 7,812 | 11: 1,850 | 0 | 0 | 0 | 400 | 5,562 | 181 | 38 | 0 | E |
| `Gate___read__4df1170376447e48` | scaling-harness +2 | 7,803 | 5: 1,192 | 84 | 0 | 30 | 0 | 6,497 | 167 | 49 | 0 | E |
| `core_time_format` | optimization +1 | 7,791 | 28: 1,291 | 0 | 0 | 0 | 1,580 | 4,920 | 242 | 50 | 0 | E I |
| `core_files_touch` | lane-commit | 7,763 | 24: 2,135 | 0 | 176 | 180 | 0 | 5,272 | 242 | 62 | 2 | E P |
| `Gate___read__e1c2c23af378e937` | scaling-harness +2 | 7,760 | 5: 1,246 | 84 | 0 | 0 | 0 | 6,430 | 168 | 53 | 0 | E |
| `core_time_civil_zoned_parts` | jetc-profile-live +1 | 7,732 | 12: 667 | 0 | 88 | 150 | 1,060 | 5,767 | 198 | 42 | 0 | I |
| `run_size` | hello-compile | 7,676 | 7: 529 | 0 | 0 | 0 | 0 | 7,147 | 182 | 31 | 1 | - |
| `ToolCases_hash_tree` | run-tool-entry | 7,668 | 8: 673 | 0 | 88 | 60 | 40 | 6,807 | 177 | 51 | 1 | - |
| `__core_crypto_bignum_add_sub_mpi` | run-cold-context +2 | 7,628 | 15: 2,072 | 0 | 308 | 210 | 820 | 4,218 | 215 | 49 | 0 | E P I |
| `run_variant_record` | optimization | 7,573 | 1: 39 | 0 | 0 | 0 | 0 | 7,534 | 235 | 36 | 1 | - |
| `core_encoding_binary_parse_fmt` | asm-parity +1 | 7,546 | 6: 1,349 | 0 | 176 | 150 | 920 | 4,951 | 171 | 60 | 0 | E P I |
| `__core_net_ssl_tls_ssl_free` | run-cold-context +2 | 7,540 | 0: 0 | 0 | 0 | 0 | 0 | 7,540 | 77 | 19 | 0 | - |
| `core_sys_env_file` | enum-codec-jet-adapter +2 | 7,526 | 13: 1,418 | 0 | 44 | 30 | 0 | 6,034 | 188 | 49 | 0 | E |
| `__core_net_ssl_srv12_ssl_srv12_parse_certificate_verify__dc5d18cd5f7cdafb` | run-cold-context +2 | 7,515 | 10: 320 | 0 | 0 | 180 | 1,020 | 5,995 | 238 | 66 | 0 | I |
| `RunColdContext_summarize` | run-cold-context | 7,480 | 3: 313 | 24 | 264 | 0 | 40 | 6,839 | 200 | 35 | 0 | P |
| `__core_crypto_ecp_point_write_binary` | run-cold-context +2 | 7,466 | 4: 332 | 0 | 0 | 0 | 420 | 6,714 | 202 | 65 | 0 | - |
| `Stage_stager` | run-stage-entry | 7,445 | 17: 1,593 | 0 | 0 | 60 | 940 | 4,852 | 268 | 57 | 3 | E I |
| `core_encoding_json___cursor_skip_array` | asm-parity +8 | 7,424 | 3: 161 | 0 | 0 | 30 | 280 | 6,953 | 190 | 23 | 8 | - |
| `__core_crypto_rsa_pss_sign_core__47db03392a1a0139` | run-cold-context +2 | 7,403 | 10: 210 | 0 | 0 | 120 | 1,140 | 5,933 | 209 | 42 | 0 | I |
| `run_identity_for` | selfhost-run | 7,398 | 5: 1,160 | 0 | 0 | 0 | 0 | 6,238 | 161 | 53 | 2 | E |
| `Packages_source_digest` | scaling-corpora +1 | 7,395 | 5: 605 | 0 | 44 | 0 | 0 | 6,746 | 138 | 36 | 1 | - |
| `core_files_collapse_impl` | run-cold-context +4 | 7,363 | 16: 1,232 | 0 | 132 | 300 | 120 | 5,579 | 181 | 47 | 3 | E |
| `Gate___read__dca6e360a3e1f8f7` | scaling-harness +2 | 7,354 | 3: 547 | 0 | 0 | 30 | 240 | 6,537 | 152 | 44 | 0 | - |
| `__core_crypto_pkparse_pk_parse_key_pkcs8_encrypted_der__b481c715a0361c91` | run-cold-context +2 | 7,338 | 8: 1,344 | 0 | 0 | 0 | 860 | 5,134 | 244 | 60 | 0 | E I |
| `__core_crypto_rsa_rsa_rsassa_pss_verify_ext` | run-cold-context +2 | 7,328 | 12: 733 | 0 | 88 | 210 | 1,460 | 4,837 | 228 | 59 | 0 | E I |
| `LaneCommit_files_of` | lane-commit | 7,318 | 5: 612 | 0 | 0 | 0 | 0 | 6,706 | 146 | 44 | 1 | - |
| `Stage_runs_state` | run-stage-entry | 7,317 | 5: 178 | 0 | 0 | 30 | 0 | 7,109 | 200 | 36 | 0 | - |
| `run___read__95c1e6d730549226` | asm-parity | 7,295 | 3: 545 | 0 | 0 | 0 | 240 | 6,510 | 154 | 48 | 0 | - |
| `run___read__a410363e88f54d5f` | selfhost-run | 7,295 | 3: 545 | 0 | 0 | 0 | 240 | 6,510 | 154 | 48 | 0 | - |
| `Shape_applicability_fact_issues` | run-shape | 7,291 | 5: 590 | 0 | 0 | 0 | 0 | 6,701 | 168 | 46 | 7 | - |
| `core_files_canonicalize` | jetc-profile-live +2 | 7,277 | 21: 1,911 | 0 | 264 | 240 | 0 | 4,862 | 197 | 18 | 1 | E P |
| `core_regex_replace_all_with` | corelib-compare +5 | 7,269 | 8: 2,153 | 0 | 176 | 120 | 600 | 4,220 | 169 | 52 | 0 | E P |
| `run_encode` | jetc-profile | 7,269 | 1: 85 | 0 | 0 | 0 | 0 | 7,184 | 137 | 28 | 0 | - |
| `Shape_http_service_probe_issues` | run-shape | 7,260 | 8: 610 | 0 | 0 | 0 | 0 | 6,650 | 205 | 66 | 3 | - |
| `PerformanceSurface_parse_surface_args` | run-performance-surface | 7,258 | 4: 1,364 | 0 | 0 | 0 | 100 | 5,794 | 151 | 49 | 1 | E |
| `core_math___dint_log` | asm-parity +3 | 7,240 | 15: 1,617 | 0 | 0 | 0 | 2,080 | 3,543 | 228 | 57 | 0 | E I |
| `core_files_getsize` | enumset-bench +1 | 7,201 | 18: 1,000 | 0 | 0 | 510 | 340 | 5,351 | 222 | 18 | 1 | E |
| `run_run` | optimization | 7,201 | 19: 1,952 | 0 | 176 | 180 | 20 | 4,873 | 203 | 60 | 3 | E P |
| `run_probe_tools` | memory-safety-fuzz | 7,180 | 2: 154 | 0 | 264 | 0 | 0 | 6,762 | 191 | 31 | 3 | P |
| `Batch10_error_lines` | batch10 | 7,172 | 7: 1,324 | 0 | 132 | 90 | 20 | 5,606 | 139 | 29 | 1 | E |
| `CensusSnap2_jetpack` | census-snap2 | 7,161 | 4: 119 | 0 | 0 | 0 | 60 | 6,982 | 221 | 37 | 7 | - |
| `MemorySafetyFuzz_normalize_finding_text` | memory-safety-fuzz | 7,159 | 4: 156 | 0 | 0 | 0 | 0 | 7,003 | 135 | 22 | 0 | - |
| `Gate_floors` | scaling-harness +2 | 7,153 | 8: 1,068 | 162 | 0 | 0 | 80 | 5,843 | 165 | 40 | 2 | E |
| `__core_net_ssl_msg_ssl_prepare_record_content` | run-cold-context +2 | 7,094 | 3: 117 | 0 | 132 | 150 | 380 | 6,315 | 121 | 23 | 0 | - |
| `Policy_metric_applicability` | run-comparison +5 | 7,092 | 6: 394 | 0 | 0 | 0 | 0 | 6,698 | 218 | 49 | 4 | - |
| `core_process_which` | hello-compile +1 | 7,091 | 9: 884 | 0 | 176 | 120 | 60 | 5,851 | 171 | 25 | 2 | E P |
| `Memory_pair_row` | read-into-memory | 7,086 | 8: 743 | 0 | 0 | 0 | 60 | 6,283 | 200 | 24 | 6 | E |
| `__core_crypto_block_aria_setkey_enc` | run-cold-context +2 | 7,076 | 11: 1,095 | 0 | 220 | 150 | 520 | 5,091 | 154 | 21 | 0 | E P |
| `run_encode` | runtime-allocator +2 | 7,074 | 0: 0 | 0 | 0 | 0 | 0 | 7,074 | 21 | 4 | 0 | - |
| `core_files_read` | comptime-items +1 | 7,050 | 6: 465 | 0 | 0 | 0 | 0 | 6,585 | 226 | 31 | 1 | - |
| `jet_ir_last_e7e771085da16ccf` | run-cold-context +2 | 7,049 | 0: 0 | 0 | 0 | 0 | 1,800 | 5,249 | 197 | 169 | 0 | I |
| `__core_crypto_des_des3_key` | run-cold-context +2 | 7,042 | 18: 760 | 0 | 1,320 | 900 | 0 | 4,062 | 148 | 18 | 0 | E P |
| `core_encoding_json_pointer_segments` | build-system +1 | 7,028 | 7: 1,451 | 0 | 132 | 120 | 0 | 5,325 | 159 | 49 | 0 | E |
| `__core_crypto_rsa_rsa_deduce_crt` | run-cold-context +2 | 7,004 | 8: 1,480 | 0 | 0 | 0 | 20 | 5,504 | 165 | 41 | 0 | E |
| `RunColdContext_make_report__lambda_d17e924654254c81` | run-cold-context | 6,966 | 23: 674 | 0 | 0 | 0 | 0 | 6,292 | 218 | 30 | 0 | - |
| `core_http___with_body` | run-cold-context +2 | 6,962 | 0: 0 | 0 | 0 | 0 | 0 | 6,962 | 55 | 3 | 0 | - |
| `__core_crypto_bignum_core_gcd_modinv_odd` | run-cold-context +2 | 6,960 | 10: 928 | 0 | 220 | 180 | 100 | 5,532 | 171 | 26 | 0 | E P |
| `Project_c_unit__value_0b31986e408b3eb1` | build-system-generate | 6,935 | 42: 1,638 | 0 | 0 | 0 | 0 | 5,297 | 358 | 46 | 1 | E |
| `Project_c_unit__value_85dcb385f82978eb` | build-system | 6,935 | 42: 1,638 | 0 | 0 | 0 | 0 | 5,297 | 358 | 46 | 1 | E |
| `jet_ir_drop_e04b8e1de3d592d2` | run-cold-context +2 | 6,927 | 0: 0 | 0 | 0 | 0 | 1,800 | 5,127 | 194 | 167 | 0 | I |
| `Stage_start_peer` | run-stage-entry | 6,915 | 12: 790 | 0 | 0 | 0 | 460 | 5,665 | 250 | 53 | 5 | E |
| `MemorySafetyFuzz_evidence_tree` | memory-safety-fuzz | 6,910 | 1: 39 | 0 | 0 | 0 | 0 | 6,871 | 211 | 33 | 0 | - |
| `core_encoding_json___decode_typed__b796387e3a1de260` | scaling-harness +2 | 6,897 | 6: 577 | 0 | 0 | 0 | 60 | 6,260 | 153 | 19 | 1 | - |
| `Gate_summary` | scaling-harness +2 | 6,874 | 5: 659 | 756 | 0 | 0 | 0 | 5,459 | 104 | 28 | 3 | S |
| `PerformanceSurface_sample_head` | run-performance-surface | 6,869 | 1: 39 | 0 | 0 | 0 | 0 | 6,830 | 220 | 46 | 0 | - |
| `Publication_state` | run-publication | 6,867 | 3: 320 | 0 | 0 | 0 | 0 | 6,547 | 170 | 31 | 0 | - |
| `run_total_cells` | asm-parity | 6,841 | 14: 231 | 360 | 0 | 0 | 320 | 5,930 | 201 | 31 | 9 | S |
| `__core_crypto_drbg_fill` | run-cold-context +2 | 6,837 | 15: 1,350 | 0 | 264 | 180 | 420 | 4,623 | 173 | 37 | 0 | E P |
| `run_run_binary` | corelib-compare | 6,824 | 7: 355 | 0 | 88 | 60 | 320 | 6,001 | 205 | 44 | 2 | - |
| `Shape_provenance_issues` | run-shape | 6,809 | 4: 445 | 72 | 0 | 0 | 0 | 6,292 | 159 | 47 | 5 | - |
| `core_encoding_base64_encode` | asm-parity +37 | 6,780 | 15: 2,465 | 0 | 264 | 180 | 40 | 3,831 | 164 | 47 | 0 | E P |
| `run_add` | comptime-items | 6,759 | 1: 123 | 1,800 | 0 | 210 | 60 | 4,566 | 79 | 11 | 0 | S |
| `__core_net_ssl_msg_ssl_write_record__dc379d9c37e28093` | run-cold-context +2 | 6,753 | 8: 246 | 0 | 132 | 150 | 980 | 5,245 | 188 | 60 | 0 | I |
| `__core_crypto_ecp_raw_koblitz` | run-cold-context +2 | 6,738 | 9: 447 | 0 | 528 | 360 | 320 | 5,083 | 125 | 22 | 0 | P |
| `__core_crypto_md_sha512_blocks` | core-activation +18 | 6,732 | 3: 171 | 0 | 1,452 | 990 | 140 | 3,979 | 103 | 5 | 0 | P |
| `run_load_reasoning` | selfhost-run | 6,724 | 4: 240 | 0 | 0 | 0 | 0 | 6,484 | 217 | 8 | 9 | - |
| `__core_crypto_md_keccak_absorb` | core-activation +18 | 6,721 | 7: 344 | 0 | 748 | 510 | 240 | 4,879 | 102 | 9 | 0 | P |
| `Mixed_options_worker` | scaling-corpora +1 | 6,716 | 12: 76 | 0 | 0 | 0 | 440 | 6,200 | 204 | 13 | 10 | - |
| `PerformanceSurface_make_trial` | run-performance-surface | 6,703 | 2: 39 | 504 | 0 | 0 | 0 | 6,160 | 174 | 27 | 0 | S |
| `__core_crypto_aead_chacha20_update` | run-cold-context +2 | 6,652 | 9: 403 | 0 | 528 | 360 | 480 | 4,881 | 148 | 31 | 0 | P |
| `Batch10_census_rows` | batch10 | 6,615 | 7: 1,010 | 0 | 44 | 30 | 0 | 5,531 | 144 | 38 | 3 | E |
| `core_encoding_json_to_string_pretty__613683a478296c6e` | corelib-compare | 6,599 | 2: 184 | 24 | 0 | 0 | 0 | 6,391 | 132 | 21 | 0 | - |
| `run_gains__lambda_2ba15792722ccec7` | asm-parity | 6,588 | 2: 293 | 0 | 0 | 0 | 0 | 6,295 | 105 | 15 | 0 | - |
| `Memory_paired_bootstrap` | read-into-memory | 6,558 | 7: 1,858 | 336 | 88 | 120 | 200 | 3,956 | 112 | 29 | 0 | E S |
| `jet_ir_drop_d55ebf41ae63ef89` | run-cold-context +2 | 6,466 | 0: 0 | 0 | 0 | 0 | 1,740 | 4,726 | 182 | 156 | 0 | I |
| `__core_crypto_bignum_mont_r2` | run-cold-context +2 | 6,443 | 12: 1,705 | 0 | 132 | 120 | 420 | 4,066 | 160 | 34 | 0 | E P |
| `core_text_runtime_kernel_text_normal_compose` | jetc-profile +2 | 6,442 | 14: 843 | 0 | 176 | 120 | 1,660 | 3,643 | 181 | 52 | 0 | E P I |
| `Shape_language_issues` | run-shape | 6,433 | 6: 711 | 0 | 0 | 0 | 0 | 5,722 | 117 | 29 | 4 | E |
| `core_files_lock_errno` | batch10 +2 | 6,432 | 1: 60 | 0 | 0 | 0 | 20 | 6,352 | 105 | 18 | 0 | - |
| `Stage_stage_batch` | run-stage-entry | 6,425 | 8: 372 | 0 | 0 | 0 | 0 | 6,053 | 193 | 56 | 1 | - |
| `Domains_generated__lambda_61eab52a251a2720` | scaling-enqueue +4 | 6,420 | 2: 480 | 0 | 0 | 0 | 0 | 5,940 | 179 | 40 | 9 | - |
| `Domains_generated__lambda_88781d20b764cac5` | scaling-corpora | 6,420 | 2: 480 | 0 | 0 | 0 | 0 | 5,940 | 179 | 40 | 9 | - |
| `__core_net_ssl_cli13_ssl_tls13_write_identities_of_pre_shared_key_ext` | run-cold-context +2 | 6,401 | 8: 175 | 0 | 0 | 120 | 1,060 | 5,046 | 188 | 60 | 0 | I |
| `run_run__lambda_3c51ca60404987b2` | corelib-compare | 6,356 | 10: 1,633 | 0 | 0 | 0 | 180 | 4,543 | 138 | 53 | 0 | E |
| `unit_unicode_is_cased` | batch10 +35 | 6,350 | 0: 0 | 0 | 0 | 0 | 0 | 6,350 | 0 | 0 | 0 | U |
| `PerformanceSurface_validation_tree` | run-performance-surface | 6,342 | 4: 320 | 0 | 44 | 0 | 0 | 5,978 | 187 | 32 | 0 | - |
| `RunColdContext_base_row` | run-cold-context | 6,342 | 2: 79 | 0 | 44 | 0 | 0 | 6,219 | 197 | 32 | 0 | - |
| `Packages_finish` | scaling-enqueue +4 | 6,340 | 3: 275 | 0 | 0 | 0 | 0 | 6,065 | 147 | 22 | 8 | - |
| `core_time_date` | run-cold-context +2 | 6,333 | 25: 1,249 | 0 | 0 | 0 | 1,480 | 3,604 | 205 | 44 | 0 | E I |
| `__core_net_ssl_srv12_ssl_srv12_parse_encrypted_pms__3284a5192db44e55` | run-cold-context +2 | 6,278 | 20: 917 | 0 | 0 | 180 | 780 | 4,401 | 190 | 31 | 0 | E I |
| `unit_unicode_is_extended_pictographic` | comptime-items +10 | 6,278 | 0: 0 | 0 | 0 | 0 | 0 | 6,278 | 0 | 0 | 0 | U |
| `Stage_build_and_measure` | run-stage-entry | 6,268 | 9: 214 | 0 | 0 | 0 | 40 | 6,014 | 205 | 18 | 5 | - |
| `run_parse_diagnostic` | selfhost-run | 6,261 | 8: 1,822 | 0 | 0 | 0 | 0 | 4,439 | 112 | 41 | 0 | E |
| `core_math_pow_n` | enum-codec-bench | 6,259 | 7: 1,217 | 3,108 | 0 | 0 | 580 | 1,354 | 95 | 26 | 0 | E S |
| `__core_crypto_x509crt_x509_crt_signer_flags` | run-cold-context +2 | 6,238 | 17: 1,631 | 0 | 0 | 60 | 180 | 4,367 | 150 | 43 | 0 | E |
| `core_encoding_json_stringify__08319004774c5375` | comptime-items | 6,212 | 2: 206 | 0 | 0 | 0 | 0 | 6,006 | 96 | 16 | 0 | - |
| `LaneCommit_revert_job` | lane-commit | 6,210 | 5: 202 | 0 | 44 | 0 | 0 | 5,964 | 173 | 49 | 6 | - |
| `run_shown` | run-performance-surface | 6,179 | 13: 1,045 | 0 | 132 | 60 | 0 | 4,942 | 178 | 38 | 1 | E P |
| `Publication_axis_publication.own2` | run-publication | 6,162 | 6: 395 | 0 | 0 | 0 | 0 | 5,767 | 179 | 58 | 4 | - |
| `IntegApply6_generator_of` | integ-apply6 | 6,153 | 11: 910 | 0 | 88 | 120 | 60 | 4,975 | 167 | 25 | 0 | E |
| `core_archive_gzip_decompress` | run-cold-context +2 | 6,145 | 8: 628 | 0 | 220 | 390 | 120 | 4,787 | 156 | 27 | 0 | E P |
| `core_text_runtime_kernel_unicode_string_append_cp` | asm-parity +25 | 6,145 | 2: 78 | 0 | 0 | 0 | 140 | 5,927 | 139 | 46 | 0 | - |
| `Memory_run` | read-into-memory | 6,106 | 5: 80 | 0 | 0 | 0 | 0 | 6,026 | 195 | 25 | 7 | - |
| `Mixed_words_worker` | scaling-corpora +1 | 6,060 | 7: 1,141 | 0 | 0 | 120 | 360 | 4,439 | 190 | 29 | 11 | E |
| `run_mem_gate` | function-boundary | 6,053 | 6: 612 | 0 | 88 | 60 | 60 | 5,233 | 138 | 39 | 1 | E |
| `unit_unicode_is_emoji` | comptime-items +10 | 6,050 | 0: 0 | 0 | 0 | 0 | 0 | 6,050 | 0 | 0 | 0 | U |
| `PerformanceSurface_binding_issues.own10` | run-performance-surface | 6,048 | 5: 195 | 0 | 0 | 0 | 0 | 5,853 | 137 | 47 | 3 | - |
| `core_text_fmt_hex` | corelib-compare +2 | 6,037 | 12: 1,159 | 0 | 0 | 0 | 220 | 4,658 | 149 | 38 | 2 | E |
| `__core_net_ssl_tls_ssl_write_certificate__0bb8e83f16e7c8d1` | run-cold-context +2 | 6,036 | 7: 1,402 | 0 | 0 | 30 | 920 | 3,684 | 171 | 36 | 0 | E I |
| `core_regex_parse_group` | batch10 +22 | 6,006 | 4: 177 | 0 | 0 | 90 | 620 | 5,119 | 151 | 57 | 2 | I |
| `core_encoding_json_stringify__940fe5ffce86521f` | runtime-allocator | 6,002 | 4: 427 | 24 | 0 | 0 | 0 | 5,551 | 121 | 19 | 0 | - |
| `Probe_probe_matches` | run-probe +1 | 5,994 | 14: 535 | 0 | 44 | 0 | 0 | 5,415 | 171 | 35 | 0 | - |
| `__core_crypto_bignum_gcd_modinv_odd` | run-cold-context +2 | 5,983 | 9: 690 | 0 | 88 | 90 | 180 | 4,935 | 128 | 21 | 0 | E |
| `core_files_walk_validate` | hello-compile +6 | 5,958 | 6: 798 | 0 | 0 | 0 | 600 | 4,560 | 168 | 42 | 2 | E I |
| `core_regex_walk` | batch10 +22 | 5,955 | 1: 47 | 0 | 440 | 300 | 100 | 5,068 | 95 | 22 | 0 | P |
| `__core_crypto_pk_pk_sign__ddfd5dc87b37f11d` | run-cold-context +2 | 5,912 | 7: 497 | 0 | 0 | 0 | 100 | 5,315 | 160 | 27 | 0 | - |
| `Midsize_rust_unit__value_c408ec3623aed7cd` | midsize | 5,895 | 36: 1,404 | 0 | 0 | 0 | 0 | 4,491 | 305 | 39 | 1 | E |
| `__core_crypto_block_ctr32_blocks` | run-cold-context +5 | 5,890 | 11: 1,792 | 0 | 264 | 180 | 120 | 3,534 | 129 | 36 | 0 | E P |
| `__core_net_ssl_tls_ssl_tls_prf_generic` | run-cold-context +2 | 5,889 | 11: 2,235 | 0 | 88 | 60 | 480 | 3,026 | 202 | 27 | 0 | E |
| `run_tight` | corelib-compare +1 | 5,868 | 2: 76 | 0 | 0 | 0 | 0 | 5,792 | 106 | 9 | 0 | - |
| `core_text_runtime_kernel_text_width_decode_scalars` | asm-parity +25 | 5,862 | 3: 725 | 0 | 440 | 300 | 460 | 3,937 | 105 | 30 | 0 | E P |
| `unit_compiler_text_numeric` | batch10 +34 | 5,852 | 0: 0 | 0 | 0 | 0 | 0 | 5,852 | 0 | 0 | 0 | - |
| `run___read__05ecae1ff890cd18` | selfhost-run | 5,850 | 5: 1,120 | 0 | 0 | 0 | 0 | 4,730 | 129 | 36 | 0 | E |
| `run_target_identity` | optimization | 5,842 | 8: 725 | 0 | 0 | 0 | 0 | 5,117 | 184 | 25 | 0 | E |
| `Gate_parse_compiler` | scaling-enqueue | 5,834 | 10: 720 | 0 | 0 | 30 | 0 | 5,084 | 152 | 28 | 2 | E |
| `core_encoding___decode_list__7f045b9dccaef953` | read-into-memory | 5,834 | 4: 915 | 0 | 0 | 0 | 0 | 4,919 | 121 | 24 | 4 | E |
| `__core_net_ssl_msg_ssl_fetch_input__4b013d5eabfaa661` | run-cold-context +2 | 5,830 | 4: 125 | 0 | 0 | 0 | 800 | 4,905 | 167 | 40 | 0 | I |
| `Roots_run` | roots-mjs | 5,813 | 0: 0 | 0 | 0 | 0 | 20 | 5,793 | 145 | 38 | 3 | - |
| `core_regex_make_match` | batch10 +15 | 5,803 | 6: 1,408 | 0 | 88 | 60 | 680 | 3,567 | 123 | 34 | 0 | E I |
| `__core_net_ssl_tls_ssl_parse_server_name_ext` | run-cold-context +2 | 5,790 | 11: 1,349 | 0 | 44 | 30 | 1,260 | 3,107 | 191 | 37 | 0 | E I |
| `__core_net_ssl_cli12_ssl_cli12_write_encrypted_pms__e0bb1edaf9a7f85f` | run-cold-context +2 | 5,779 | 10: 397 | 0 | 0 | 90 | 440 | 4,852 | 164 | 38 | 0 | - |
| `__core_net_ssl_tls_ssl_handshake_free` | run-cold-context +2 | 5,778 | 0: 0 | 0 | 0 | 0 | 0 | 5,778 | 71 | 20 | 0 | - |
| `core_files_stat` | asm-parity +3 | 5,757 | 17: 907 | 0 | 0 | 510 | 340 | 4,000 | 196 | 14 | 1 | E |
| `jet_std_process_pipeline` | census-snap2 +1 | 5,739 | 0: 0 | 0 | 0 | 0 | 0 | 5,739 | 144 | 17 | 1 | - |
| `core_encoding_read_option__ae5ff5d0aae47fe1` | scaling-harness +2 | 5,687 | 5: 1,365 | 0 | 0 | 30 | 160 | 4,132 | 134 | 36 | 3 | E |
| `Corpora_shapes__lambda_1c4090c207913247` | scaling-corpora | 5,684 | 8: 1,775 | 0 | 0 | 0 | 120 | 3,789 | 117 | 28 | 4 | E |
| `Corpora_shapes__lambda_93d4ab5fa4875344` | scaling-enqueue +4 | 5,684 | 8: 1,775 | 0 | 0 | 0 | 120 | 3,789 | 117 | 28 | 4 | E |
| `core_encoding___decode_list__a393c11fcd88ef3d` | scaling-harness +2 | 5,683 | 3: 788 | 78 | 0 | 0 | 0 | 4,817 | 105 | 24 | 3 | E |
| `core_net_tls___load_system_roots` | run-cold-context +2 | 5,681 | 8: 991 | 0 | 44 | 30 | 80 | 4,536 | 132 | 42 | 0 | E |
| `run_print_table` | comptime-items | 5,662 | 3: 447 | 36 | 0 | 0 | 80 | 5,099 | 147 | 37 | 9 | - |
| `run_build_jet` | corelib-compare | 5,649 | 4: 560 | 0 | 44 | 30 | 20 | 4,995 | 146 | 15 | 4 | - |
| `RunColdContext_rel_path` | run-cold-context | 5,646 | 10: 1,810 | 0 | 132 | 90 | 0 | 3,614 | 116 | 48 | 0 | E P |
| `__core_crypto_asn1_write_len_and_tag` | run-cold-context +2 | 5,645 | 13: 729 | 0 | 88 | 150 | 820 | 3,858 | 165 | 33 | 0 | E I |
| `core_encoding___decode_list__86bcdb7d0f873ad5` | scaling-harness +2 | 5,639 | 4: 912 | 0 | 0 | 0 | 0 | 4,727 | 110 | 23 | 3 | E |
| `__core_crypto_cipher_cfb128` | run-cold-context +2 | 5,638 | 5: 190 | 0 | 528 | 360 | 500 | 4,060 | 119 | 14 | 0 | P |
| `run_outputs` | asm-parity | 5,623 | 9: 763 | 0 | 0 | 0 | 0 | 4,860 | 119 | 35 | 1 | E |
| `core_encoding_read_option__921dc47ba6eb3e18` | asm-parity | 5,619 | 5: 1,365 | 0 | 0 | 30 | 160 | 4,064 | 132 | 36 | 3 | E |
| `core_files_list_dir` | integ-land4 +4 | 5,615 | 5: 557 | 0 | 88 | 60 | 260 | 4,650 | 111 | 37 | 0 | - |
| `core_encoding_read_option__bd0cded012188c24` | scaling-harness +2 | 5,606 | 5: 1,365 | 0 | 0 | 0 | 160 | 4,081 | 133 | 38 | 3 | E |
| `PerformanceSurface_compute_statistics` | run-performance-surface | 5,575 | 2: 544 | 1,152 | 0 | 120 | 0 | 3,759 | 79 | 25 | 0 | S |
| `unit_lk_msend` | asm-parity +3 | 5,574 | 0: 0 | 0 | 0 | 0 | 0 | 5,574 | 69 | 11 | 0 | - |
| `__core_encoding_stream_jcs_distance` | asm-parity +32 | 5,563 | 3: 118 | 0 | 0 | 0 | 680 | 4,765 | 122 | 25 | 0 | I |
| `Mixed_records_worker` | scaling-corpora +1 | 5,550 | 7: 76 | 0 | 0 | 0 | 260 | 5,214 | 181 | 9 | 14 | - |
| `core_encoding_read_option__96ec3a35b8c36947` | asm-parity | 5,538 | 5: 1,365 | 0 | 0 | 0 | 160 | 4,013 | 131 | 38 | 3 | E |
| `core_time_civil_from_day` | jetc-profile-live +1 | 5,538 | 19: 724 | 0 | 0 | 0 | 1,140 | 3,674 | 170 | 26 | 0 | E I |
| `core_encoding___decode_list__1df70674223bbc1f` | jetc-profile | 5,532 | 4: 915 | 0 | 0 | 0 | 0 | 4,617 | 116 | 24 | 4 | E |
| `__core_crypto_rsa_rsa_public` | run-cold-context +2 | 5,531 | 5: 1,330 | 0 | 0 | 0 | 180 | 4,021 | 101 | 24 | 0 | E |
| `__core_crypto_ecp_jac_normalize_many` | run-cold-context +2 | 5,516 | 7: 753 | 0 | 88 | 90 | 40 | 4,545 | 118 | 22 | 0 | E |
| `core_encoding___decode_list__16f952ea023f0e9a` | selfhost-run | 5,503 | 4: 912 | 0 | 0 | 0 | 0 | 4,591 | 107 | 23 | 3 | E |
| `__core_crypto_aead_chachapoly_finish` | run-cold-context +2 | 5,493 | 16: 597 | 0 | 88 | 360 | 260 | 4,188 | 128 | 8 | 0 | E |
| `__core_crypto_md_hash_update` | run-tool-entry | 5,485 | 11: 1,084 | 0 | 352 | 240 | 1,240 | 2,569 | 148 | 24 | 0 | E P I |
| `Result_metric_cell_issue` | run-publication +1 | 5,478 | 1: 39 | 264 | 0 | 0 | 0 | 5,175 | 158 | 49 | 0 | - |
| `__core_crypto_rsa_blind_exponent__2bcc8da8ca3a6a44` | run-cold-context +2 | 5,463 | 6: 770 | 0 | 176 | 120 | 460 | 3,937 | 140 | 29 | 0 | E P |
| `__core_crypto_rsa_rsaes_pkcs1_v15_unpadding` | run-cold-context +2 | 5,462 | 17: 787 | 0 | 440 | 360 | 380 | 3,495 | 139 | 9 | 0 | E P |
| `Midsize_jet_unit__value_666f872adfea31cb` | build-system | 5,442 | 33: 1,287 | 0 | 0 | 0 | 0 | 4,155 | 281 | 36 | 1 | E |
| `Midsize_jet_unit__value_6c109d4ed10c78d0` | midsize | 5,442 | 33: 1,287 | 0 | 0 | 0 | 0 | 4,155 | 281 | 36 | 1 | E |
| `Midsize_jet_unit__value_77835c86744a80cc` | build-system-generate | 5,442 | 33: 1,287 | 0 | 0 | 0 | 0 | 4,155 | 281 | 36 | 1 | E |
| `__core_crypto_bignum_read_string` | run-cold-context +2 | 5,415 | 8: 908 | 0 | 176 | 150 | 360 | 3,821 | 186 | 54 | 0 | E P |
| `run_print_probes__lambda_c485008743f3e2fc` | run-probe | 5,412 | 1: 39 | 0 | 0 | 0 | 0 | 5,373 | 131 | 17 | 3 | - |
| `run_reasoning_template` | selfhost-run | 5,408 | 2: 444 | 0 | 0 | 0 | 0 | 4,964 | 107 | 28 | 0 | - |
| `ToolCases_hash_tree` | run-stage-entry | 5,398 | 3: 417 | 0 | 0 | 0 | 0 | 4,981 | 115 | 38 | 1 | - |
| `core_encoding___decode_list__c6ac45cc57cd4524` | scaling-harness +2 | 5,395 | 4: 912 | 0 | 0 | 0 | 0 | 4,483 | 105 | 23 | 3 | E |
| `core_http_client_request` | run-cold-context | 5,380 | 2: 135 | 0 | 0 | 0 | 0 | 5,245 | 72 | 12 | 1 | - |
| `__core_net_ssl_msg_ssl_extract_add_data_from_record` | run-cold-context +2 | 5,370 | 3: 310 | 0 | 88 | 60 | 100 | 4,812 | 108 | 33 | 0 | - |
| `__core_net_ssl_tls_ssl_parse_alpn_ext` | run-cold-context +2 | 5,349 | 7: 926 | 0 | 88 | 60 | 920 | 3,355 | 152 | 34 | 0 | E I |
| `run_copy_tree` | selfhost-run | 5,328 | 8: 679 | 0 | 0 | 0 | 0 | 4,649 | 162 | 51 | 0 | E |
| `run_summarize` | enum-codec-bench | 5,327 | 4: 444 | 0 | 44 | 0 | 80 | 4,759 | 123 | 11 | 0 | - |
| `run_hot_text_modules` | hot-text | 5,323 | 7: 643 | 0 | 44 | 30 | 0 | 4,606 | 102 | 11 | 2 | E |
| `__core_crypto_block_camellia_setkey_dec` | run-cold-context +2 | 5,312 | 3: 884 | 0 | 352 | 240 | 460 | 3,376 | 110 | 30 | 0 | E P |
| `core_encoding___decode_list__e6082091fa9bef78` | scaling-harness +2 | 5,299 | 4: 912 | 0 | 0 | 0 | 0 | 4,387 | 101 | 23 | 3 | E |
| `core_files_relative` | optimization +3 | 5,282 | 10: 1,762 | 0 | 132 | 90 | 0 | 3,298 | 111 | 47 | 0 | E P |
| `core_net_display` | run-cold-context +2 | 5,282 | 0: 0 | 0 | 0 | 0 | 0 | 5,282 | 108 | 3 | 1 | - |
| `run_stripped` | asm-parity | 5,279 | 3: 286 | 0 | 132 | 300 | 200 | 4,361 | 158 | 42 | 5 | P |
| `core_encoding_read_int__3e29d4fe0286bcdf` | asm-parity +7 | 5,254 | 7: 555 | 0 | 88 | 150 | 920 | 3,541 | 139 | 39 | 0 | E I |
| `core_encoding_read_list__5e0f0f3fd7305fe1` | scaling-harness +2 | 5,253 | 3: 845 | 0 | 0 | 30 | 120 | 4,258 | 122 | 28 | 3 | E |
| `RunColdContext_read_json` | run-cold-context | 5,251 | 3: 240 | 0 | 0 | 0 | 0 | 5,011 | 173 | 25 | 8 | - |
| `__core_crypto_ecp_chunk_reduce` | run-cold-context +2 | 5,228 | 6: 274 | 0 | 352 | 270 | 260 | 4,072 | 97 | 8 | 0 | P |
| `__core_crypto_md_hash_update_range` | run-cold-context +2 | 5,214 | 7: 280 | 0 | 352 | 240 | 880 | 3,462 | 121 | 19 | 0 | P I |
| `jet_ir_last_e28b6c359c429fb9` | run-cold-context +2 | 5,213 | 0: 0 | 0 | 0 | 0 | 720 | 4,493 | 129 | 129 | 0 | I |
| `core_encoding_json_stringify__c96a087dd55fd692` | runtime-allocator | 5,203 | 1: 79 | 0 | 0 | 0 | 0 | 5,124 | 105 | 18 | 0 | - |
| `Mixed_options_worker__lambda_b9cf7a507bf0d8c9` | scaling-corpora | 5,202 | 13: 1,725 | 0 | 44 | 120 | 460 | 2,853 | 133 | 26 | 0 | E |
| `Mixed_options_worker__lambda_c7ef2aa24262cc32` | scaling-generate | 5,202 | 13: 1,725 | 0 | 44 | 120 | 460 | 2,853 | 133 | 26 | 0 | E |
| `run_collect_files` | selfhost-run | 5,197 | 2: 200 | 0 | 0 | 0 | 0 | 4,997 | 84 | 27 | 0 | - |
| `core_encoding___decode_list__f1f34aad7cb8628f` | scaling-harness +2 | 5,183 | 4: 912 | 0 | 0 | 0 | 0 | 4,271 | 99 | 23 | 3 | E |
| `core_math___dint_add` | asm-parity +4 | 5,171 | 6: 143 | 0 | 0 | 0 | 700 | 4,328 | 119 | 17 | 0 | I |
| `__core_crypto_bignum_core_shift_r` | run-cold-context +2 | 5,168 | 7: 353 | 0 | 616 | 420 | 220 | 3,559 | 99 | 12 | 0 | P |
| `Comparison_runtime_evidence` | run-comparison +2 | 5,147 | 6: 361 | 0 | 0 | 0 | 0 | 4,786 | 166 | 54 | 2 | - |
| `core_net_tls___flush` | run-cold-context +2 | 5,143 | 13: 1,079 | 0 | 0 | 0 | 1,120 | 2,944 | 178 | 43 | 1 | E I |
| `unit_rt_aes_group` | run-cold-context +2 | 5,136 | 0: 0 | 0 | 0 | 0 | 0 | 5,136 | 3 | 0 | 0 | - |
| `core_encoding___decode_list__50eedef7bf151776` | scaling-harness +2 | 5,131 | 4: 912 | 0 | 0 | 0 | 0 | 4,219 | 98 | 23 | 3 | E |
| `core_encoding___decode_list__e933620f2bb55e41` | selfhost-run | 5,131 | 4: 912 | 0 | 0 | 0 | 0 | 4,219 | 98 | 23 | 3 | E |
| `__core_crypto_ecp_mul_shortcuts` | run-cold-context +2 | 5,115 | 4: 357 | 0 | 0 | 0 | 80 | 4,678 | 140 | 32 | 0 | - |
| `PerformanceSurface_binding_issues` | run-performance-surface | 5,114 | 5: 195 | 0 | 0 | 0 | 0 | 4,919 | 123 | 33 | 3 | - |
| `core_encoding___decode_list__2ba46830c29d81fd` | scaling-harness +2 | 5,095 | 4: 912 | 0 | 0 | 0 | 0 | 4,183 | 98 | 23 | 3 | E |
| `Mixed_numbers_worker__lambda_65ef804f277d9ed3` | scaling-corpora | 5,094 | 10: 720 | 0 | 0 | 0 | 920 | 3,454 | 127 | 23 | 0 | E I |
| `Mixed_numbers_worker__lambda_c94c1536e7d8b8c8` | scaling-generate | 5,094 | 10: 720 | 0 | 0 | 0 | 920 | 3,454 | 127 | 23 | 0 | E I |
| `core_text_decode_indexed` | build-system +5 | 5,090 | 3: 794 | 0 | 132 | 90 | 580 | 3,494 | 105 | 38 | 0 | E P I |
| `__core_crypto_aead_poly1305_blocks` | run-cold-context +2 | 5,084 | 6: 296 | 0 | 0 | 570 | 320 | 3,898 | 99 | 20 | 0 | - |
| `__core_net_ssl_msg_ssl_write_handshake_msg_ext__e16bb201d731b3b1` | run-cold-context +2 | 5,079 | 7: 362 | 0 | 88 | 90 | 860 | 3,679 | 135 | 37 | 0 | I |
| `__core_net_ssl_cli12_ssl_cli12_parse_alpn_ext__c6046534cc3f76cf` | run-cold-context +2 | 5,077 | 7: 865 | 0 | 0 | 30 | 380 | 3,802 | 150 | 49 | 0 | E |
| `RunColdContext_run_case` | run-cold-context | 5,070 | 4: 147 | 0 | 0 | 0 | 0 | 4,923 | 129 | 42 | 1 | - |
| `Json_json_write` | perf-measure | 5,069 | 2: 227 | 36 | 0 | 0 | 0 | 4,806 | 122 | 11 | 8 | - |
| `__core_net_ssl_srv12_ssl_srv12_ciphersuite_match` | run-cold-context +2 | 5,063 | 10: 414 | 0 | 176 | 120 | 260 | 4,093 | 122 | 19 | 0 | P |
| `__core_crypto_aead_ghash` | run-cold-context +2 | 5,062 | 9: 451 | 0 | 0 | 180 | 300 | 4,131 | 92 | 12 | 0 | - |
| `run_run` | scaling-harness | 5,043 | 2: 43 | 0 | 0 | 0 | 0 | 5,000 | 142 | 21 | 5 | - |
| `__core_crypto_bignum_mod_mpi` | run-cold-context +2 | 5,031 | 6: 1,244 | 0 | 0 | 0 | 340 | 3,447 | 171 | 53 | 0 | E |
| `__core_net_ssl_msg_ssl_ct_hmac` | run-cold-context +2 | 5,031 | 7: 2,352 | 0 | 0 | 0 | 380 | 2,299 | 152 | 34 | 0 | E |
| `Tree_parse_json` | run-stage-entry | 5,029 | 9: 387 | 0 | 0 | 0 | 20 | 4,622 | 146 | 30 | 5 | - |
| `RunColdContext_write_report` | run-cold-context | 5,023 | 0: 0 | 0 | 0 | 0 | 0 | 5,023 | 158 | 20 | 5 | - |
| `core_encoding_read_list__17f5e076e59bfe5d` | selfhost-run | 5,019 | 3: 842 | 0 | 0 | 0 | 120 | 4,057 | 118 | 30 | 3 | E |
| `core_net_url_normalize_path` | run-cold-context +2 | 5,016 | 4: 781 | 0 | 132 | 150 | 0 | 3,953 | 114 | 37 | 1 | E P |
| `core_encoding_read_list__ad62a3e3bd155e34` | scaling-harness +2 | 5,009 | 3: 845 | 0 | 0 | 30 | 120 | 4,014 | 117 | 28 | 3 | E |
| `PerformanceSurface_definition_issues` | run-performance-surface | 4,996 | 4: 465 | 0 | 0 | 0 | 0 | 4,531 | 114 | 37 | 3 | - |
| `Domains_web__lambda_1d2869d454a0d357` | scaling-corpora | 4,995 | 1: 49 | 0 | 0 | 30 | 80 | 4,836 | 187 | 38 | 3 | - |
| `Domains_web__lambda_563ac6de0588efbc` | scaling-enqueue +4 | 4,995 | 1: 49 | 0 | 0 | 30 | 80 | 4,836 | 187 | 38 | 3 | - |
| `__core_net_ssl_keys13_ssl_tls13_calculate_verify_data` | run-cold-context +2 | 4,993 | 7: 239 | 0 | 0 | 0 | 40 | 4,714 | 107 | 14 | 0 | - |
| `core_process_run_all` | integrate10 | 4,989 | 7: 805 | 0 | 0 | 0 | 0 | 4,184 | 132 | 21 | 1 | E |
| `__core_crypto_des_setkey` | run-cold-context +2 | 4,967 | 1: 49 | 0 | 132 | 90 | 140 | 4,556 | 56 | 6 | 0 | P |
| `RunColdContext_scratch_dir` | run-cold-context | 4,965 | 4: 141 | 0 | 44 | 30 | 160 | 4,590 | 154 | 12 | 4 | - |
| `Stage_record_asset` | run-stage-entry | 4,949 | 6: 308 | 0 | 0 | 0 | 0 | 4,641 | 103 | 7 | 0 | - |
| `core_files_join` | build-system-adapter +7 | 4,937 | 14: 1,103 | 0 | 132 | 240 | 100 | 3,362 | 142 | 33 | 2 | E P |
| `jet_ir_last_357168c8674dc07b` | asm-parity +3 | 4,936 | 0: 0 | 0 | 0 | 0 | 280 | 4,656 | 101 | 72 | 0 | - |
| `__core_net_ssl_srv12___ssl_srv12_take_premaster__3f507f3d3c5505eb` | run-cold-context +2 | 4,935 | 3: 87 | 0 | 132 | 240 | 140 | 4,336 | 100 | 15 | 0 | P |
| `run_compare__lambda_e829ef5d57ecac09` | selfhost-run | 4,935 | 4: 852 | 0 | 0 | 0 | 80 | 4,003 | 96 | 23 | 1 | E |
| `core_encoding___decode_list__1930a8a87bfb2a48` | scaling-harness +2 | 4,932 | 4: 912 | 0 | 0 | 0 | 0 | 4,020 | 95 | 23 | 3 | E |
| `core_encoding_read_list__9add0b253d04ef1b` | scaling-harness +2 | 4,927 | 3: 842 | 0 | 0 | 0 | 120 | 3,965 | 116 | 30 | 3 | E |
| `__core_crypto_cipher_ofb` | run-cold-context +2 | 4,924 | 6: 116 | 0 | 352 | 240 | 420 | 3,796 | 106 | 13 | 0 | P |
| `core_text_fmt_sci` | run-performance-surface | 4,921 | 6: 122 | 168 | 0 | 0 | 840 | 3,791 | 124 | 13 | 3 | I |
| `core_encoding___decode_list__6020f3f96c99478a` | selfhost-run | 4,916 | 4: 912 | 0 | 0 | 0 | 0 | 4,004 | 95 | 23 | 3 | E |
| `Corpora_shapes__lambda_63103e15111f7f99` | scaling-corpora | 4,913 | 10: 1,801 | 0 | 0 | 0 | 140 | 2,972 | 128 | 33 | 3 | E |
| `Corpora_shapes__lambda_a2e98bbebe1cf7b6` | scaling-enqueue +4 | 4,913 | 10: 1,801 | 0 | 0 | 0 | 140 | 2,972 | 128 | 33 | 3 | E |
| `core_encoding_read_list__affbbbece6fcd110` | scaling-harness +2 | 4,913 | 3: 845 | 0 | 0 | 30 | 120 | 3,918 | 113 | 28 | 3 | E |
| `PerformanceSurface_empty_sample` | run-performance-surface | 4,902 | 3: 97 | 0 | 44 | 0 | 0 | 4,761 | 146 | 21 | 0 | - |
| `unit_unicode_is_east_asian_wide` | comptime-items +10 | 4,896 | 0: 0 | 0 | 0 | 0 | 0 | 4,896 | 0 | 0 | 0 | U |
| `jet_ir_close_909522f776ef86c3` | run-cold-context +2 | 4,894 | 3: 201 | 0 | 0 | 0 | 0 | 4,693 | 108 | 8 | 0 | - |
| `run_summary` | build-system | 4,894 | 2: 43 | 492 | 0 | 150 | 80 | 4,129 | 72 | 7 | 1 | S |
| `jet_ir_box_unique_a026d39d547d7ab2` | run-cold-context +2 | 4,890 | 0: 0 | 0 | 0 | 0 | 0 | 4,890 | 92 | 1 | 0 | - |
| `core_encoding_binary_iter_unpack` | asm-parity | 4,889 | 4: 730 | 0 | 88 | 60 | 260 | 3,751 | 105 | 37 | 0 | E |
| `core_regex_member_matches` | batch10 +31 | 4,889 | 5: 222 | 0 | 0 | 0 | 160 | 4,507 | 120 | 26 | 0 | - |
| `core_encoding_json_next_item` | asm-parity +6 | 4,887 | 1: 47 | 0 | 0 | 150 | 200 | 4,490 | 133 | 23 | 6 | - |
| `run_load_variant` | optimization | 4,881 | 4: 76 | 0 | 0 | 0 | 0 | 4,805 | 159 | 19 | 6 | - |
| `core_encoding___decode_list__4f3fd9bd86285569` | asm-parity +6 | 4,880 | 3: 782 | 0 | 0 | 0 | 0 | 4,098 | 97 | 24 | 3 | E |
| `core_encoding___decode_list__954c3b8d45ae3dd2` | scaling-harness +2 | 4,864 | 4: 912 | 0 | 0 | 0 | 0 | 3,952 | 94 | 23 | 3 | E |
| `core_encoding___decode_list__b0846ca9c5c8a923` | asm-parity | 4,864 | 4: 912 | 0 | 0 | 0 | 0 | 3,952 | 94 | 23 | 3 | E |
| `core_encoding_json_to_string__68000ffe2231210b` | timer | 4,856 | 0: 0 | 0 | 0 | 0 | 0 | 4,856 | 87 | 23 | 0 | - |
| `core_encoding___decode_list__6f47539d42e43e27` | asm-parity | 4,854 | 4: 912 | 0 | 0 | 0 | 0 | 3,942 | 94 | 23 | 3 | E |
| `run_tree` | perf-measure | 4,848 | 4: 181 | 90 | 0 | 0 | 140 | 4,437 | 167 | 21 | 4 | - |
| `core_time___civil_tz_name` | jetc-profile-live +1 | 4,846 | 4: 172 | 0 | 528 | 390 | 260 | 3,496 | 136 | 25 | 3 | P |
| `Midsize_zig_unit__value_dfedbe89bf144919` | midsize | 4,838 | 29: 1,131 | 0 | 0 | 0 | 0 | 3,707 | 249 | 32 | 1 | E |
| `__core_crypto_pk_pk_decrypt_fixed__32737e1282976530` | run-cold-context +2 | 4,835 | 13: 432 | 0 | 0 | 0 | 180 | 4,223 | 149 | 17 | 0 | - |
| `PerformanceSurface_identity_issues` | run-performance-surface | 4,834 | 5: 195 | 0 | 0 | 0 | 20 | 4,619 | 122 | 31 | 0 | - |
| `core_encoding_read_list__7f60b10c8243baff` | scaling-harness +2 | 4,831 | 3: 842 | 0 | 0 | 0 | 120 | 3,869 | 112 | 30 | 3 | E |
| `core_net_tls___write_chunk` | run-cold-context +2 | 4,831 | 6: 300 | 0 | 0 | 0 | 560 | 3,971 | 145 | 33 | 1 | I |
| `__core_crypto_cipher_sbox` | run-cold-context +2 | 4,829 | 0: 0 | 0 | 0 | 480 | 0 | 4,349 | 64 | 16 | 0 | - |
| `core_http_timeout` | run-cold-context | 4,824 | 0: 0 | 0 | 0 | 0 | 20 | 4,804 | 51 | 2 | 0 | - |
| `core_encoding___decode_list__f31acabf23ec6371` | scaling-harness +2 | 4,820 | 3: 780 | 0 | 0 | 0 | 0 | 4,040 | 95 | 24 | 3 | E |
| `__core_crypto_cipher_ctr` | run-cold-context +2 | 4,818 | 6: 308 | 0 | 264 | 180 | 580 | 3,486 | 111 | 16 | 0 | P I |
| `core_regex_parse_unicode_class` | batch10 +22 | 4,817 | 5: 474 | 0 | 44 | 90 | 240 | 3,969 | 131 | 33 | 1 | - |
| `__core_crypto_x509crt_x509_crt_get_certificate_policies` | run-cold-context +2 | 4,790 | 4: 78 | 0 | 0 | 0 | 460 | 4,252 | 129 | 21 | 0 | - |
| `core_net_tls___await` | run-cold-context +2 | 4,789 | 15: 792 | 0 | 0 | 0 | 780 | 3,217 | 181 | 35 | 2 | E I |
| `core_regex_step` | batch10 +20 | 4,778 | 5: 170 | 0 | 484 | 330 | 600 | 3,194 | 104 | 24 | 0 | P I |
| `core_files_read_bytes` | roots-sh +1 | 4,769 | 2: 125 | 0 | 0 | 0 | 0 | 4,644 | 156 | 19 | 1 | - |
| `core_encoding_read_list__1dfea0bcbb204309` | scaling-harness +2 | 4,744 | 3: 845 | 36 | 0 | 30 | 120 | 3,713 | 113 | 30 | 3 | E |
| `core_encoding_json_write_json_string` | asm-parity +37 | 4,735 | 2: 218 | 0 | 0 | 0 | 0 | 4,517 | 105 | 34 | 0 | - |
| `ToolCases_run_tool_command` | run-stage-entry +1 | 4,715 | 3: 79 | 0 | 0 | 0 | 40 | 4,596 | 136 | 16 | 5 | - |
| `__core_crypto_rsa_mgf_mask` | run-cold-context +2 | 4,715 | 7: 2,051 | 0 | 220 | 150 | 700 | 1,594 | 134 | 24 | 0 | E P I |
| `core_encoding_read_list__d7dac9871b5c76a9` | scaling-harness +2 | 4,709 | 3: 845 | 0 | 0 | 30 | 120 | 3,714 | 110 | 28 | 3 | E |
| `Result_metric_cell_issue` | run-scoreboard | 4,706 | 1: 39 | 168 | 0 | 0 | 0 | 4,499 | 142 | 46 | 0 | - |
| `core_net_url_percent_decode` | run-cold-context +2 | 4,700 | 4: 708 | 0 | 176 | 120 | 220 | 3,476 | 109 | 40 | 0 | E P |
| `Coverage_parse_cells` | coverage | 4,697 | 3: 742 | 0 | 44 | 60 | 60 | 3,791 | 79 | 18 | 0 | E |
| `run_hot_text_declare` | hot-text | 4,696 | 8: 1,187 | 0 | 44 | 30 | 0 | 3,435 | 90 | 25 | 1 | E |
| `__core_net_ssl_tls_ssl_write_finished__c5b0394650866da0` | run-cold-context +2 | 4,693 | 3: 181 | 0 | 132 | 120 | 220 | 4,040 | 117 | 36 | 0 | P |
| `run___read__070ee84bc175ad65` | selfhost-run | 4,666 | 4: 596 | 0 | 0 | 0 | 0 | 4,070 | 100 | 25 | 0 | E |
| `core_encoding_read_list__c3069902a11b7330` | selfhost-run | 4,663 | 3: 842 | 0 | 0 | 0 | 120 | 3,701 | 109 | 30 | 3 | E |
| `core_encoding_read_list__70e3ae81a0d180ac` | scaling-harness +2 | 4,652 | 3: 845 | 36 | 0 | 0 | 120 | 3,651 | 112 | 32 | 3 | E |
| `__core_net_ssl_cli_ssl_cli_write_supported_groups_ext` | run-cold-context +2 | 4,638 | 7: 416 | 0 | 0 | 0 | 660 | 3,562 | 118 | 27 | 0 | I |
| `Stage_found_executable` | run-stage-entry | 4,621 | 1: 76 | 0 | 0 | 0 | 440 | 4,105 | 113 | 51 | 0 | - |
| `core_math_stats_median` | asm-parity +1 | 4,615 | 8: 962 | 468 | 44 | 210 | 40 | 2,891 | 80 | 19 | 0 | E S |
| `core_regex_parse_group_name` | batch10 +22 | 4,605 | 4: 502 | 0 | 44 | 90 | 320 | 3,649 | 123 | 35 | 1 | E |
| `run_accepts` | asm-parity | 4,595 | 1: 51 | 0 | 44 | 0 | 140 | 4,360 | 139 | 35 | 1 | - |
| `run_evidence_tree` | selfhost-run | 4,580 | 2: 160 | 0 | 44 | 0 | 0 | 4,376 | 109 | 11 | 0 | - |
| `__core_crypto_ecp_jac_add_mixed` | run-cold-context +2 | 4,577 | 0: 0 | 0 | 176 | 120 | 0 | 4,281 | 86 | 14 | 0 | P |
| `__core_crypto_ecp_mul_mod` | run-cold-context +2 | 4,566 | 12: 616 | 0 | 0 | 0 | 420 | 3,530 | 140 | 32 | 0 | E |
| `Policy_required_policy_cells` | run-policy +2 | 4,555 | 2: 158 | 0 | 0 | 0 | 0 | 4,397 | 73 | 26 | 0 | - |
| `run___read__2f801c6772743f8a` | canvas-driver-lifecycle | 4,555 | 4: 596 | 0 | 0 | 0 | 0 | 3,959 | 100 | 25 | 0 | E |
| `core_encoding_read_map__8bf9083d1c6654c9` | scaling-harness +2 | 4,552 | 4: 1,095 | 0 | 0 | 30 | 0 | 3,427 | 107 | 27 | 2 | E |
| `core_encoding___decode_map__1b51c0028013bfbd` | scaling-harness +2 | 4,546 | 4: 846 | 0 | 0 | 0 | 0 | 3,700 | 90 | 19 | 2 | E |
| `__core_crypto_pk_pk_verify` | run-cold-context +2 | 4,537 | 12: 876 | 0 | 0 | 0 | 620 | 3,041 | 180 | 34 | 0 | E I |
| `__core_crypto_aead_poly1305_key` | run-cold-context +2 | 4,527 | 16: 1,090 | 0 | 132 | 240 | 0 | 3,065 | 119 | 22 | 0 | E P |
| `jet_ir_lazy_line_next_6a14118c20fc7a32` | scaling-harness +3 | 4,520 | 3: 117 | 0 | 0 | 60 | 0 | 4,343 | 127 | 25 | 1 | - |
| `run_validate_nix` | build-system | 4,518 | 6: 416 | 0 | 0 | 0 | 0 | 4,102 | 122 | 16 | 3 | - |
| `__core_crypto_block_block_crypt_cbc__6faf0dac12be509a` | run-cold-context +2 | 4,504 | 5: 491 | 0 | 616 | 420 | 0 | 2,977 | 89 | 6 | 0 | E P |
| `__core_crypto_block_block_crypt_cbc__728b822baea9ee9f` | run-cold-context +2 | 4,504 | 5: 491 | 0 | 616 | 420 | 0 | 2,977 | 89 | 6 | 0 | E P |
| `Packages_pack` | scaling-corpora +5 | 4,501 | 2: 251 | 0 | 0 | 0 | 180 | 4,070 | 88 | 22 | 4 | - |
| `core_math_exp` | asm-parity | 4,483 | 2: 246 | 1,416 | 0 | 120 | 240 | 2,461 | 48 | 10 | 0 | S |
| `run_check` | hot-text | 4,483 | 4: 749 | 0 | 0 | 0 | 0 | 3,734 | 116 | 32 | 3 | E |
| `core_encoding_read_map__d60f7cd378916eec` | scaling-harness +2 | 4,476 | 4: 1,095 | 0 | 0 | 0 | 0 | 3,381 | 106 | 29 | 2 | E |
| `Coverage_corpus_issues` | coverage | 4,473 | 3: 385 | 0 | 44 | 30 | 20 | 3,994 | 98 | 22 | 5 | - |
| `core_encoding_read_list__ae283a4c9aaaa256` | scaling-harness +2 | 4,471 | 3: 845 | 0 | 0 | 30 | 120 | 3,476 | 106 | 28 | 3 | E |
| `core_files_resolve` | selfhost-run | 4,462 | 5: 634 | 0 | 44 | 30 | 0 | 3,754 | 120 | 29 | 3 | E |
| `core_encoding_read_list__78a553fc475075ed` | selfhost-run | 4,457 | 3: 842 | 0 | 0 | 0 | 120 | 3,495 | 106 | 30 | 3 | E |
| `__core_net_ssl_keys13_ssl_tls13_compute_resumption_master_secret` | run-cold-context +2 | 4,452 | 5: 145 | 0 | 0 | 30 | 140 | 4,137 | 101 | 22 | 0 | - |
| `run_python_repr` | enum-codec-bench +1 | 4,447 | 0: 0 | 0 | 0 | 0 | 0 | 4,447 | 90 | 10 | 2 | - |
| `__core_crypto_bignum_core_to_mont_rep_wide` | run-cold-context +2 | 4,426 | 6: 695 | 0 | 176 | 120 | 200 | 3,235 | 90 | 9 | 0 | E P |
| `__core_crypto_ecp_comb_core__0caae1f35df7b882` | run-cold-context +2 | 4,417 | 7: 795 | 0 | 44 | 60 | 180 | 3,338 | 104 | 12 | 0 | E |
| `__core_crypto_ecp_comb_core__1102650631f68127` | run-cold-context +2 | 4,417 | 7: 795 | 0 | 44 | 60 | 180 | 3,338 | 104 | 12 | 0 | E |
| `core_encoding___decode_map__2066f239206f6996` | scaling-harness +2 | 4,405 | 4: 846 | 0 | 0 | 0 | 0 | 3,559 | 86 | 19 | 2 | E |
| `core_encoding_read_list__ace42a4ff424a2b5` | scaling-harness +2 | 4,405 | 3: 842 | 0 | 0 | 0 | 120 | 3,443 | 105 | 30 | 3 | E |
| `__core_crypto_x509crt_x509_crt_verify_with_profile_at` | run-cold-context +2 | 4,398 | 4: 569 | 0 | 176 | 120 | 240 | 3,293 | 88 | 22 | 0 | E P |
| `core_encoding_read_list__f83db08be7b0eafd` | scaling-harness +2 | 4,396 | 3: 845 | 0 | 0 | 30 | 120 | 3,401 | 105 | 28 | 3 | E |
| `__core_crypto_md_hmac_key` | run-cold-context +2 | 4,393 | 9: 1,245 | 0 | 176 | 120 | 160 | 2,692 | 121 | 21 | 0 | E P |
| `Corpora_shapes` | scaling-corpora +5 | 4,386 | 1: 79 | 0 | 0 | 0 | 0 | 4,307 | 116 | 21 | 0 | - |
| `PerformanceSurface_sample_issues` | run-performance-surface | 4,381 | 0: 0 | 36 | 0 | 0 | 0 | 4,345 | 154 | 47 | 8 | - |
| `run___read__0118a935a972c47d` | canvas-driver-lifecycle | 4,378 | 4: 596 | 0 | 0 | 0 | 0 | 3,782 | 96 | 25 | 0 | E |
| `core_encoding_read_list__7a0f85673c406215` | asm-parity +3 | 4,371 | 3: 845 | 0 | 0 | 30 | 120 | 3,376 | 105 | 28 | 3 | E |
| `Batch10_quarantine` | batch10 | 4,361 | 5: 1,158 | 0 | 44 | 30 | 0 | 3,129 | 87 | 30 | 1 | E |
| `run_shown` | run-tools | 4,359 | 3: 195 | 0 | 44 | 0 | 40 | 4,080 | 90 | 13 | 2 | - |
| `__core_crypto_aead_gcm_starts__b2333ea4a1d91759` | run-cold-context +2 | 4,349 | 6: 392 | 0 | 264 | 210 | 0 | 3,483 | 89 | 19 | 0 | P |
| `Probe_normalized_header_value` | run-probe | 4,348 | 2: 219 | 0 | 0 | 30 | 0 | 4,099 | 73 | 18 | 1 | - |
| `core_time___civil_tz_time` | jetc-profile-live +1 | 4,348 | 5: 412 | 0 | 44 | 120 | 800 | 2,972 | 133 | 36 | 2 | I |
| `Domains_compiler__lambda_91f533b25e8a193d` | scaling-enqueue +4 | 4,337 | 0: 0 | 0 | 0 | 0 | 40 | 4,297 | 149 | 32 | 2 | - |
| `Domains_compiler__lambda_a59abcf8c907cb82` | scaling-corpora | 4,337 | 0: 0 | 0 | 0 | 0 | 40 | 4,297 | 149 | 32 | 2 | - |
| `__core_crypto_aead_gcm_starts__7d8b82b55534ea03` | run-cold-context +2 | 4,330 | 6: 392 | 0 | 264 | 210 | 0 | 3,464 | 88 | 19 | 0 | P |
| `__core_crypto_aead_gcm_starts__f83cadfbfca807ee` | run-cold-context +2 | 4,330 | 6: 392 | 0 | 264 | 210 | 0 | 3,464 | 88 | 19 | 0 | P |
| `core_encoding_read_list__9ddcab8d0b038b78` | scaling-harness +2 | 4,330 | 3: 842 | 0 | 0 | 0 | 120 | 3,368 | 104 | 30 | 3 | E |
| `__core_net_ssl_srv12_ssl_srv12_write_renegotiation_ext` | run-cold-context +2 | 4,329 | 3: 141 | 0 | 0 | 180 | 260 | 3,748 | 99 | 26 | 0 | - |
| `run_mean_cells` | asm-parity | 4,314 | 3: 40 | 0 | 0 | 0 | 0 | 4,274 | 147 | 48 | 0 | - |
| `core_encoding_read_list__cc40ffc4e3f5d310` | asm-parity +4 | 4,305 | 3: 842 | 0 | 0 | 0 | 120 | 3,343 | 104 | 30 | 3 | E |
| `jet_crypto_aes_blocks` | run-cold-context +2 | 4,305 | 0: 0 | 0 | 0 | 0 | 0 | 4,305 | 20 | 1 | 0 | - |
| `__core_crypto_asn1_get_alg` | run-cold-context +2 | 4,304 | 5: 578 | 0 | 0 | 60 | 940 | 2,726 | 129 | 44 | 0 | E I |
| `__core_crypto_cipher_mix_columns` | run-cold-context +2 | 4,302 | 0: 0 | 0 | 0 | 480 | 0 | 3,822 | 71 | 16 | 0 | - |
| `__core_encoding_stream_utf16_units` | enum-codec-bench +7 | 4,281 | 3: 470 | 0 | 308 | 210 | 180 | 3,113 | 79 | 20 | 0 | E P |
| `core_encoding_json___plain_string` | asm-parity +8 | 4,281 | 7: 370 | 0 | 88 | 90 | 220 | 3,513 | 92 | 16 | 0 | P |
| `run_compare` | selfhost-run | 4,276 | 2: 252 | 0 | 0 | 0 | 20 | 4,004 | 64 | 11 | 0 | - |
| `run_spawn` | runtime-allocator | 4,273 | 1: 41 | 0 | 0 | 0 | 0 | 4,232 | 117 | 14 | 5 | - |
| `__core_crypto_dhm_make_common__fcef88c8d2412300` | run-cold-context +2 | 4,264 | 5: 430 | 0 | 0 | 0 | 40 | 3,794 | 110 | 29 | 0 | E |
| `Mixed_shapes_worker` | scaling-corpora +1 | 4,263 | 7: 76 | 0 | 0 | 0 | 240 | 3,947 | 142 | 8 | 12 | - |
| `core_encoding___decode_map__f5704f66ca6a3ecc` | scaling-harness +2 | 4,253 | 4: 846 | 0 | 0 | 0 | 0 | 3,407 | 84 | 19 | 2 | E |
| `__core_crypto_ecp_jac_add_affine` | run-cold-context +2 | 4,245 | 0: 0 | 0 | 0 | 0 | 0 | 4,245 | 81 | 14 | 0 | - |
| `core_encoding_json_stringify__f7120b528834a6ec` | runtime-allocator | 4,245 | 3: 333 | 0 | 0 | 0 | 0 | 3,912 | 73 | 11 | 0 | - |
| `core_encoding___decode_map__4c2bec549a045d8e` | scaling-harness +2 | 4,242 | 3: 731 | 42 | 0 | 0 | 0 | 3,469 | 85 | 20 | 2 | E |
| `CensusSnap2_census_errors` | census-snap2 | 4,238 | 2: 402 | 0 | 88 | 60 | 0 | 3,688 | 105 | 27 | 4 | P |
| `__core_crypto_bignum_core_montmul` | run-cold-context +2 | 4,234 | 0: 0 | 0 | 528 | 390 | 120 | 3,196 | 69 | 12 | 0 | P |
| `core_encoding_read_map__084a44ccf7e0803a` | scaling-harness +2 | 4,230 | 4: 1,095 | 36 | 0 | 30 | 0 | 3,069 | 100 | 27 | 2 | E |
| `Probe_node_headers` | run-probe +1 | 4,225 | 3: 40 | 0 | 44 | 0 | 0 | 4,141 | 97 | 17 | 2 | - |
| `PerformanceSurface_balanced_order` | run-performance-surface | 4,218 | 4: 616 | 0 | 264 | 180 | 340 | 2,818 | 91 | 23 | 0 | E P |
| `core_encoding___decode_list__7e8a73d8cbf37a03` | asm-parity | 4,216 | 3: 780 | 0 | 0 | 0 | 0 | 3,436 | 83 | 21 | 3 | E |
| `__core_net_ssl_defs_ssl_session_wipe` | run-cold-context +2 | 4,214 | 0: 0 | 0 | 0 | 0 | 0 | 4,214 | 41 | 13 | 0 | - |
| `unit_int_division` | asm-parity +53 | 4,206 | 0: 0 | 0 | 0 | 0 | 0 | 4,206 | 48 | 2 | 0 | - |
| `__core_crypto_bignum_core_shift_l` | run-cold-context +2 | 4,194 | 7: 357 | 0 | 528 | 360 | 260 | 2,689 | 90 | 11 | 0 | P |
| `Scoreboard_missing_cell_failures` | run-scoreboard | 4,191 | 3: 363 | 0 | 44 | 0 | 0 | 3,784 | 94 | 17 | 0 | - |
| `__core_crypto_aead_ccm_first_block__1c0c86079e850994` | run-cold-context +2 | 4,191 | 4: 169 | 0 | 88 | 120 | 440 | 3,374 | 99 | 15 | 0 | P I |
| `Measure_walk_counters` | perf-measure | 4,173 | 1: 78 | 384 | 0 | 90 | 0 | 3,621 | 78 | 17 | 3 | S |
| `__core_crypto_aead_ccm_first_block__24f925be0c9bca7a` | run-cold-context +2 | 4,172 | 4: 169 | 0 | 88 | 120 | 440 | 3,355 | 98 | 15 | 0 | P I |
| `__core_crypto_aead_ccm_first_block__a07f50d3bdc5469b` | run-cold-context +2 | 4,172 | 4: 169 | 0 | 88 | 120 | 440 | 3,355 | 98 | 15 | 0 | P I |
| `__core_crypto_ecp_field_new` | run-cold-context +2 | 4,169 | 10: 783 | 0 | 0 | 60 | 280 | 3,046 | 146 | 24 | 0 | E |
| `__core_crypto_x509crt_x509_crt_parse_der` | run-cold-context +2 | 4,168 | 0: 0 | 0 | 0 | 0 | 0 | 4,168 | 88 | 5 | 0 | - |
| `core_time_unix_ns` | enum-codec-bench +1 | 4,167 | 10: 544 | 0 | 0 | 0 | 600 | 3,023 | 118 | 29 | 1 | E I |
| `core_net_parse_ipv4_bytes` | run-stage-entry | 4,162 | 3: 571 | 0 | 176 | 120 | 200 | 3,095 | 87 | 42 | 0 | E P |
| `Stage_unavailable_tier` | run-stage-entry | 4,157 | 1: 39 | 0 | 0 | 0 | 0 | 4,118 | 128 | 21 | 0 | - |
| `run_identity_with_execution` | optimization | 4,150 | 1: 39 | 0 | 0 | 0 | 0 | 4,111 | 128 | 28 | 0 | - |
| `__core_net_ssl_gen13_ssl_tls13_is_supported_versions_ext_present_in_exts` | run-cold-context +2 | 4,148 | 8: 395 | 0 | 0 | 0 | 1,180 | 2,573 | 145 | 38 | 0 | I |
| `__core_net_ssl_srv13_ssl_srv13_parse_key_shares_ext` | run-cold-context +2 | 4,142 | 6: 380 | 0 | 0 | 0 | 1,180 | 2,582 | 128 | 44 | 0 | I |
| `core_math_stats_median_high` | read-into-memory +1 | 4,126 | 8: 962 | 444 | 44 | 150 | 40 | 2,486 | 73 | 18 | 0 | E S |
| `core_encoding_read_map__aaff01daaf0ca791` | scaling-harness +2 | 4,122 | 4: 1,095 | 36 | 0 | 0 | 0 | 2,991 | 99 | 29 | 2 | E |
| `__core_net_ssl_srv12_ssl_srv12_parse_session_ticket_ext` | run-cold-context +2 | 4,119 | 3: 270 | 0 | 0 | 90 | 100 | 3,659 | 104 | 15 | 0 | - |
| `__core_net_ssl_cli12_ssl_cli12_parse_new_session_ticket__2e245e45239f7bfa` | run-cold-context +2 | 4,118 | 6: 240 | 0 | 0 | 180 | 580 | 3,118 | 133 | 33 | 0 | I |
| `Batch10_adds_word` | batch10 | 4,111 | 0: 0 | 0 | 0 | 0 | 0 | 4,111 | 31 | 1 | 0 | - |
| `__core_crypto_x509crt_x509_crt_parse` | run-cold-context +2 | 4,101 | 7: 1,294 | 0 | 0 | 30 | 220 | 2,557 | 109 | 35 | 0 | E |
| `__core_crypto_block_cbc_blocks` | run-cold-context +5 | 4,099 | 9: 1,130 | 0 | 308 | 210 | 180 | 2,271 | 91 | 21 | 0 | E P |
| `unit_libm_fma` | asm-parity +4 | 4,099 | 0: 0 | 1,110 | 0 | 0 | 0 | 2,989 | 32 | 0 | 0 | S |
