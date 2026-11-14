# Precompiled Core record: size ceiling and gaps (2026-11-13)

Card #6328 c22. The c22 size gate is a record of 64 MB or less. After the
gate, the aim is a record a few times the size of the Core source. This audit
sets out three things:

- what the record is measured against;
- the smallest record that would still hold every fact the loader needs;
- where the current record goes over that, and which slice removes each gap.

Every figure below was measured on the lane host or computed from measured
counts. A figure that is an estimate says so.

Sources measured:

- the `jet.core/v8` record from slice r8 (96,190,054 bytes), written by
  `jet core-record --write`;
- the `jet.core/v7` record from build b20 (113,545,029 bytes), for the
  per-field logical counts;
- the decoding scripts, kept with the lane under `$JET_SCRATCH_ROOT/lanes/G12Docs-5/ceil/`.

## 1. Reference points

### Jet Core and Prelude source

| what | files | raw bytes | gzip -9 bytes |
|---|---:|---:|---:|
| `Core/**/*.jet` | 271 | 18,732,375 | 3,337,845 |
| `Core/**/*.jet` + `Compiler/Prelude` | 281 | 19,649,709 | 3,530,953 |

### Machine code of all Core at O2

**Not measured.** Jet has no build that lowers every Core function. A program
lowers only the Core it reaches. The runtime pack (`jet_runtime.pack.o`,
.text 1,837,347 bytes) is the runtime, not Core.

The nearest measured figure comes from the seed jet binary (.text 87,721,231
bytes):

- 3,360 of its function symbols map to 2,363 distinct Core top-level
  functions;
- Core has 11,287 top-level function keys in all;
- those 3,360 symbols hold 5,154,745 bytes of .text, an average of 2.18 KB per
  Core function.

To measure the full figure, a run must lower every Core function at O2. That is
a heavy job, and only the lander runs heavy jobs.

### Peers' shipped standard-library metadata

| peer | metadata shipped for std | bytes | its std source |
|---|---|---:|---:|
| Rust 1.97.1: `.rmeta` of the 19-crate std link closure | all of it | 104,579,047 | 51,549,997 for all `library/` crates |
| | libcore + liballoc + libstd only | 80,124,471 | core+alloc+std `src/`: 963 files, 13,248,680 raw, 2,610,597 gz |
| | (rlib object code, same closure) | 8,961,890 | |
| Go 1.26.7: `__.PKGDEF` export data, `go list -export std`, `CGO_ENABLED=0`, 353 packages | all of it | 9,761,432 | 3,002 files, 31,453,763 raw, 7,274,485 gz (no cmd, testdata or tests) |
| | (`_go_.o` object code, same archives) | 155,444,317 | |
| Zig 0.16.0 | none: std is compiled from source in every build | 0 | `lib/std`: 550 files, 16,416,866 raw, 3,041,838 gz |

Notes on the Rust figures:

- libcore's `.rmeta` alone is 64,066,679 bytes.
- The libcore source pulls in `core_arch` (stdarch), which is 19,663,229 bytes
  of source. The x86 and x86_64 parts are 8,194,131 of that.

What each peer stores, so that the comparison is fair:

- **Rust `.rmeta`**
  ([rustc dev guide, "Libraries and metadata"](https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html)):
  - every item's signature, type, trait impl and span;
  - the source map;
  - MIR only for generic, `#[inline]` and const functions.
  The code for the other functions ships as object code inside the `.rlib`.
- **Go export data**
  ([cmd/compile README](https://go.dev/src/cmd/compile/README),
  [noder/linker.go](https://github.com/golang/go/blob/master/src/cmd/compile/internal/noder/linker.go)):
  - exported types, signatures and constants;
  - bodies only for inlinable and generic functions.
  Everything else ships as object code (`_go_.o`).
- **Jet today**, for every Core function:
  - the raw parsed AST;
  - the checked body tree;
  - the facts derived from it.
  It ships no object code. Every program lowers the Core it reaches from the
  checked trees.

So Rust and Go keep bodies for only a subset of functions and ship the rest as
machine code. Jet's record has to keep a checked body for every function.

## 2. The ceiling: a minimal hand-designed record

The ceiling holds exactly the facts the loader and lowering need. It holds no
derived index and no text that can be rebuilt from ids. Each part is computed
as count × ideal bytes, from counts measured in the record.

| part | count (measured) | ideal encoding | ceiling bytes |
|---|---|---|---:|
| Checked bodies | 695,467 expression nodes and 160,087 statements in 15,814 function packets | see below | 8,010,000 |
| Constant values | 3,529 constants; 2,469,217 bytes of `Bytes` data, 825,435 at gzip -9 | raw bytes kept, plus lists (~0.45 MB) and scalars at 6 B | 2,940,000 (1,300,000 with the byte data compressed) |
| Strings | 35,250 identifiers (427,242 B) and 42,985 literal/doc texts (1,203,000 B) | identifiers front-coded in blocks of 16 (288,387); literals raw | 1,491,387 |
| Declaration surface | 19,643 functions and methods; 37,164 parameters; 1,664 nominals; 5,670 fields; 1,436 variants; 2,618 impls with 3,654 methods; 1,690 type defs; 6,296 distinct types; 16,233 exports; 1,213 imports with 1,405 bindings | 10 B per function, 4 B per parameter, 8 B per nominal or impl, 4-5 B per field, variant or type, 3 B per export | 550,000 |
| Symbol table | 76,553 symbols | kind 1 B + owner varint 2 B + name id 2 B | 383,000 |
| Function headers | 15,814 packets; 28,951 parameters | 16 B per packet + 4 B per parameter | 370,000 |
| Line tables | 224,003 line starts | delta varints (measured) | 228,897 |
| Derived per-function facts | 65K call edges; 15,814 effect and failure summaries | 2 B per edge + 4 B per function | 190,000 |
| Lookup indexes (hash slots), Core lint diagnostics, per-node offsets | | rebuilt at load, not reported, or implicit in a sequential layout | 0 |
| Raw AST (declarations and bodies) | | not read by lowering (see §4) | 0 |
| **Total** | | | **≈14.2 MB** |

### How the checked-body figure was built

All counts were measured on v7. In the tight measure each value costs only its
own varint or tag byte, with no shape ids and no node table.

1. The tight, schema-directed size of every `s5.function.tree` is 21,552,954
   bytes.
2. Removing what a body repeats or can derive brings it to 14,090,968:
   - a Local node names its local by index instead of repeating the whole
     local (−5,278,634);
   - `expr_types` drop their ids, since the order is the node order
     (−695,467);
   - node ids are dropped (−553,509);
   - local symbol keys, which are derived text, are dropped (−423,123);
   - the Binary `line` field is dropped, since the span gives it (−241,805);
   - panic `location` is dropped, since the span gives it (−220,906).
3. Child references relative to the node (1 B) instead of absolute packed ids
   bring it to 10,676,195. That breaks down as:

   | part | bytes |
   |---|---:|
   | node tags and payload | 4,467,964 |
   | statements | 2,846,902 |
   | spans | 2,091,455 |
   | type per node | 695,467 |
   | per-function type tables | 553,996 |

4. Hand layout of the two heaviest remaining parts:
   - statements at 5 B each instead of the measured 17.8 B (−2,046,467);
   - Field nodes at 4 B each instead of the measured 12.5 B (−620,007).

   This gives **8.01 MB**, about 9.4 B per node plus statement.

### The ceiling against the reference points

| measure | ceiling 14.2 MB is | current v8 (96.19 MB) is |
|---|---:|---:|
| Core + Prelude source (19.65 MB) | 0.72× | 4.9× |
| the same at gzip -9 (3.53 MB) | 4.0× | 27× |
| Go std export data (9.76 MB; no bodies except inlinable and generic) | 1.45× | 9.9× |
| Rust core+alloc+std `.rmeta` (80.12 MB; MIR only for generic and inline functions) | 0.18× | 1.2× |

The v8 record itself compresses to 34,747,745 bytes at gzip -6. That much
redundancy is left in the encoding.

## 3. Gap table

The current bytes are measured on the v8 record:

- Each stored node is charged to the first path that reaches it, walking
  s3, s5, s6, s7 and s8 in that order.
- Each stored string's front-coded bytes are charged to the first path that
  names it.
- The rows sum to the file size.

| part | current bytes | ceiling bytes | ratio | root cause | fix | slice |
|---|---:|---:|---:|---|---|---|
| Raw AST (declarations and bodies) | 29,045,108 | 0 | — | Core's whole parsed AST is stored. Lowering never reads it. Parts: s6 `program.items` 17,391,905; s3 `constants.declaration` 4,996,871; s3 `nominals.source` 3,202,754; s3 `exports.declaration` 2,359,762; other 1.09 MB | Registration rows become headers without bodies. The sema walkers either skip seeded modules or read the record's facts | **r9** |
| Checked bodies (s5 tree) | 26,752,220 | 8,010,000 | 3.3× | Self-describing items (shape id per object, tag per item). Node references are absolute ids reached through the node table. Local nodes repeat the whole local. Derived local key text (1,488,690 string bytes) | A schema-directed body codec: node order is the id, child references are relative, plus a locals table and one type per node | r11 |
| Derived per-function facts | 11,298,695 | 190,000 | 59× | `name_references` (8,138,694) and `call_facts` (~2.2 MB) are stored, though both can be derived again from the trees | Derive them when read; keep only the call edges | r10 |
| Declaration surface (s3/s6 symbols, identities, types, nominals, impls) | 8,443,608 | 550,000 | 15× | Identity keys are stored as text (for example `s6.functions.identity_key` at 917,724). Declarations are held per module in both s3 and s6 | Use structured ids and keep one copy | r12 |
| Header symbol table | 7,056,189 | 383,000 | 18× | Each of the 76,553 keys is spelled as the full text path of its owners | Rows of (kind, owner, name) | r10 |
| Function headers (s5.function minus tree) | 4,120,803 | 370,000 | 11× | Each parameter carries a whole local symbol (`params` 1,557,750). The stack and heap facts, stored as text, take 583,690 | Parameters become locals-table indexes; drop the derived text | r11 |
| Node table offsets | 3,675,064 | 0 | — | A u32 end is kept for each of the 918,766 stored nodes | Goes away with the r9 and r11 codecs | r11 |
| Constant values | 3,547,364 | 2,940,000 | 1.2× | Near the ceiling. The bulk is embedded byte data | Optional: compress the byte blobs (to 0.83 MB) | — |
| Lookup indexes and line tables (s6) | 1,269,805 | 228,897 | 5.5× | Hash slots are stored. Line starts are absolute (853,089) | Rebuild the indexes at load; delta-code the lines | r12 |
| Container overhead (shapes, string block ends, length tables, s1/s2/s4/s9/s10/s11) | 787,385 | ~0 | — | Self-describing container | Goes with the codecs | r11 |
| Core lint diagnostics | 193,813 | 0 | — | Core lint findings are stored but never reported in a user compile | Drop them | r10 |
| **Total** | **96,190,054** | **≈14,200,000** | **6.8×** | | | |

### Effect on the 64 MB gate

These are computed from the table and assume each slice removes exactly its
row:

| slices landed | record size |
|---|---:|
| r9 alone | ≈67.1 MB, still over the gate |
| r9 + symbol table rows (r10, −6.67 MB) | ≈60.5 MB, under the gate |
| r9 + `name_references` derived on read (r10, −8.1 MB) | ≈59.0 MB, under the gate |
| r9 + r10 + r11 + r12 | ≈14-16 MB, about 0.75× the source |

If r9 has to keep the constants' declaration AST (5.0 MB), then r9 plus the
symbol-table part of r10 only reaches ≈65.5 MB. In that case r10 needs both of
its parts.

## 4. Does the whole AST need storing?

**No. The raw AST is not needed.** The checked trees are needed.

- **Lowering needs only the checked bodies.** `Compiler/JetIR/Source/Build.jet`
  (lines 381-385) builds the IR from the checked `FnBody` list
  (`IRProgram.functions: [FnBody]`), and generics are monomorphized from those
  bodies. No IR step reads the raw AST. Core also has 0 functions marked
  inline or inline_always and 0 comptime functions, so no inline or comptime
  path needs a Core body in its source form.
- **Where the raw AST is read today.** It is read only by sema walkers that
  walk Core modules while checking a program. Some of those (class ii) read a
  Core body to judge user code:
  - taint crossing;
  - HTTP route shape;
  - C callback context and foreign thread safety;
  - variadic bound specialization;
  - trait default methods copied into impls;
  - Prelude error conversions;
  - codec pending and liveness empty-body checks;
  - core demand;
  - effect source.

  r9 serves each of these from the record's facts. Four kinds of declaration
  stay whole, because their bodies are themselves the facts:
  - trait default methods;
  - ErrorConv;
  - GenericModule and TemplateLoop;
  - marker declarations.
- **Shipping machine code instead of checked trees would be larger.** The
  ceiling stores a checked body at about 0.5 KB per function (8.01 MB / 15,814
  packets). The measured Core code in the jet binary is 2.18 KB per function.
  Object code would also have to ship once per optimization level and per
  target.

  [INFERENCE] The 2,363 functions in the binary may not be typical of all Core.
  The full O2 measure (§1) would settle this.
