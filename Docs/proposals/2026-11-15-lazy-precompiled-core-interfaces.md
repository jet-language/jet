# Lazy precompiled Core interfaces

Status: design note for #6328 (precompiled Core and Prelude). Nothing here
is ratified by this note. The design it narrows is
`Docs/proposals/precompiled-libraries-2026-11-05.md`. The record size audit
is `Docs/audits/2026-11-13-core-record-size-ceiling.md`.

Two decisions frame it:

- On 2026-11-14 the front-end Core record was retired. Since then every
  compile loads Core from the sources built into jet.
- On 2026-11-15 Main answered how c22 reads. A compile may load the
  declarations and types of a checked interface that was built when jet was
  built. That is loading, not a re-check, as long as it costs a small
  fraction of hello's user time. Producing those declarations at compile
  time by parsing Core text is scanning source, and c22 forbids it. That
  holds for a header-only parse and for a one-declaration-at-a-time parse.
  Core text is parsed only when jet is built, to produce the interface.

So **step A** ships the prebuilt interface. **Step B**, lazy checked
bodies or machine code for the bodies a compile still needs, lands only if
it measures faster.

Facts marked **measured** come from the gate-p149 jet (dev of 2026-11-15)
on the team machine. They were measured outside any checkout, with no
`JET_TOOLCHAIN_ROOT`, so Core is the built-in Core.

## 1. What a compile does with Core today

1. It reads Core text from the sources built into jet.
2. It lexes and scans every Core unit its imports reach, for spellings and
   import demands (`jet_driver_unit_spellings`,
   `jet_driver_collect_core_identifier_demand_threads`).
3. It parses the declarations of those units. CoreReach holds back
   constants and the top-level functions of routed modules
   (`jet_driver_reach_hold`). It parses those only when they are reached.
4. It registers what reach keeps.
5. It checks the reached Core bodies on demand (CoreDemand.jet).

`jet run` takes seal stand-ins (CoreSeal.jet) for sealed bodies and binds
their calls to `jet_core.o`.

**Measured**, median of 5 runs of wall time (traced counters from one more
run):

| Program | `jet check` | `jet run` | Core parsed | Units | Items reached / loaded | Bodies checked |
|---|---|---|---|---|---|---|
| hello (1 line) | 19 ms | 25 ms | 11 KB | 1 | 0 / 13 | 0 |
| jetc-trace Summarize (148 lines) | 489 ms | 675 ms | 2,323 KB | 41 | 818 / 2,642 | 292 |
| RunColdContext (1,264 lines) | 1,325 ms | 1,860 ms | 5,145 KB | 113 | 3,903 / 7,880 | 2,123 |

**Hello, measured** from the per-span thread CPU (`cpu_ns`) of one traced
`jet run`. hello is single-threaded, so this is its whole front end. The
process user time is 29 ms.

| Work | CPU |
|---|---|
| Parse `Core/Prelude/Prelude.jet` (11 KB) and `core.jet` | 1.2 ms |
| Parse `hello.jet` | 0.3 ms |
| Register (hello, Core Prelude, compiler-owned Prelude rows) | 6.2 ms |
| Sema bodies, all modules | 1.8 ms |
| Finalize | 3.9 ms |
| Link | 2.4 ms |

So hello spends about 1.2 ms (4% of user time) reading Core text. The rest
is fixed compiler work on hello and the compiler-owned rows. That fixed
cost is outside this note.

**Summarize, measured**: one traced `jet check`. Core parsing runs on
worker threads, so these are wall spans with a 10 ms trace clock:

| Span | Wall |
|---|---|
| load (lex, scan, hold, parse) | 260 ms |
| register | 80 ms |
| core demand | 80 ms |
| the rest of finalize | 140 ms |

Its user code is 7 KB of the 2.3 MB parsed. Outside hello, the cold time is
Core front-end work.

Against c22:

- **Reads.** This already holds outside a checkout. `strace` of
  `jet run hello.jet` shows no `openat`, `statx` or `getdents64` on any
  toolchain path; its 31 `getdents64` calls list the program's own
  directory.
- **Scans.** This does not hold. Every compile lexes and parses Core text:
  11 KB for hello and 2.3 MB for Summarize.
- **Re-checks.** This holds for hello, which checks 0 Core bodies. It does
  not hold for larger programs: Summarize's `jet run` checks 212 Core bodies
  and takes 129 seal stand-ins.

## 2. What the retired record taught

The record precompiled the whole registration graph and decoded it at
load. It lost to parsing for three reasons, logged on #6328 on 2026-11-14:

- **The decoder was slow.** It cost about 10 us per row byte, against about
  0.58 s for the whole in-tree Core front end.
- **The unit was a whole module.** A record unit kept every declaration,
  and its spellings widened reach. fft-4096 loaded 32 units against 6.
- **It was big and too rich.** It was 61 to 96 MB, against a 14.2 MB
  ceiling. Of that hand-minimal 14.2 MB, 8.01 MB was checked bodies. Its
  places pointed into a syntax tree that had to be rebuilt first.

What the interface must avoid follows from each:

- no per-field decoding;
- no module-sized units;
- no bodies;
- no syntax tree.

## 3. Step A: the prebuilt interface

### 3.1 What it holds

When jet is built, the step that checks Core for `jet_core.o` and
`core.seal` also writes the Core interface. It writes from the registered
and checked tables, never from text. There is one row per declaration:

- **Identity:** kind, name, owner (the type of a method; the type and trait
  of an impl), module, and its source path and line for diagnostics.
- **Signature:**
  - generic parameters and bounds;
  - parameter types and access marks;
  - the result type;
  - effect row and failure set;
  - view provenance of the result.
- **Types:** fields, variants, layouts, derived traits and trait impls,
  with method tables.
- **Constants:** the value, when user code can read it at comptime.
- **Seal facts:**
  - a stand-in replaces a body check: stop edge, module-state writes,
    failure facts;
  - the `jet_core.o` symbol the call binds to.
- **Reach:** the declaration's spellings, as ranges into one word table,
  and its import demands. Reach then runs on the interface alone.

Types are referred to by index into one interned type table, and names by
offset into one string table.

### 3.2 Layout: used in place, loaded per declaration

The interface is fixed-width rows in `Int` arrays plus the string table. It
is embedded in jet as constant data, the way `JET_CLI_CORE_SOURCES` is
today. A compile reads it in place:

- A row is a slice of an array. Loading a declaration copies its row's
  fields into sema's registration rows, with nothing to decode.
- A type loads when a loaded row first names it.
- Nothing loads because it shares a module with something reached. The
  unit of laziness is the declaration, so an unreached declaration costs
  nothing.

The estimated size is under 6 MB, which is the 14.2 MB ceiling minus its
8.01 MB of bodies. Step A1 measures it.

### 3.3 How a compile uses it

On built-in Core, the interface is the only way a compile gets a Core
declaration:

- No Core text is lexed, scanned or parsed.
- The hold stubs and the strip of unreached items go away, because nothing
  is parsed before reach.
- CoreReach decides reach from the interface's spellings and demands.
- Sema takes a reached declaration's registration rows from its interface
  row.
- Sema takes a reached sealed function's facts from its seal fields. That
  replaces today's seal read: 14,573 text lines, each found in the
  declaration table.
- On `jet run`, a sealed call binds to `jet_core.o`, as today.

The Prelude goes through the same interface. hello then reads no Core or
Prelude text at all.

A toolchain tree, where a contributor may have edited Core, produces its
interface in memory before the compile. It uses the same writer, over the
tree's text, and then loads through the same reader. So there is one load
path and one meaning, and only the tree pays to produce it.

### 3.4 Budget

This is Main's condition for counting interface loading as loading:

- **hello:** Core interface loading must cost at most 1 ms of CPU. That is
  under 4% of hello's 29 ms user time, and no more than the 1.2 ms of Core
  text parsing it replaces. The trace gets a span and counters for it (rows
  loaded, types loaded, bytes touched) so the budget is measured on every
  run.
- **Every other program:** never slower than the built-in-source compile it
  replaces, in wall time and peak RSS, on check and run.

### 3.5 What step A leaves

Some compiles still need a Core body:

- a generic Core function instantiated by user code;
- a body that has no seal line (a result with view provenance, or an
  unclean failure union);
- a body that `jet build` inlines at O1 or O2;
- a Core function that user comptime calls, when no shipped code covers
  it.

Until step B, such a body is parsed and checked from the built-in text,
found by the span in its interface row. That is the one place where step A
still reads Core text. So c22 holds after step A for every program whose
reached Core is sealed and non-generic, hello among them. For the others it
holds only after step B. The trace counts these bodies (`core bodies from
text`), so the remaining gap is measured, not assumed.

## 4. Step B: the bodies a compile still needs

Each part is gated on measuring faster than parsing and checking the same
bodies from text.

- **Checked bodies in the interface.** A generic body, or a body that is
  inlined or unsealed, ships in checked form beside its row. It loads per
  declaration, like a row.
  - The record shows the risk: its bodies were 8 MB, and decoding them
    cost more than parsing.
  - So this part lands only if its loader beats the text path on
    Summarize and RunColdContext.
- **Machine code for more of Core.** Sealing covers more functions, so
  fewer bodies are needed at all:
  - common generic instances, such as `List<Int>` and `Map<String, _>`, as
    in section 4.3 of the 2026-11-05 note;
  - results with view provenance, once the seal records them.

## 5. Alternatives considered

- **Parsing Core text at compile time, by declaration or header-first.**
  It is cheaper than today, but c22 forbids it: it scans source.
- **Reviving the record with lazy decode.** Decoding each row byte cost
  more than parsing the same declaration. Its places also need a syntax
  tree. 3.2 keeps the interface flat and body-free for these reasons.
- **A mapped object graph (Lean `.olean`, Java class data sharing).** It is
  fast to map, but sema's tables carry ids that are specific to the
  process. Making them position-independent changes every pass. Fixed rows
  copied on first use get the same laziness.
- **A cache per program or per user.** c22 rules it out ("no runtime
  cache"), and so does the no-incremental ruling on #2517.

## 6. Landing order and proofs

Each step is its own submission with an A/B against the jet before it:

- **Programs:** hello, e2404, lint, e3401, RunColdContext and jetc-trace
  Summarize.
- **Commands:** `jet check` and `jet run`.
- **Runs:** median of 5 cold runs, outside the tree, with built-in Core.
- **Never slower:** no worse wall time or peak RSS on any program (c37).
- **Parity:** the same output and diagnostics on every golden and ui row.

| Step | Change | Extra proof |
|---|---|---|
| A1 | The interface writer runs in the Core check at jet build and embeds the interface. Compiles do not read it yet. | Round trip: for every Core declaration, the rows loaded from the interface equal the rows registration builds from source. Two builds give identical bytes. The size is recorded. |
| A2 | Reach and registration read the interface on built-in Core. Core text parsing, hold stubs and the strip go away for declarations. Seal facts come from the interface. | `parse.core_input_bytes` is 0 for hello. The hello budget in 3.4 holds. The c22 strace of hello is unchanged. |
| A3 | A toolchain tree produces its interface in memory and loads through the same reader. The source registration path for Core goes away. | `jet check Core` is clean in a tree. The tree-mode cost is recorded; it is not gated. |
| A4 | `jet check` and `jet test` take seal facts for sealed bodies on built-in Core, as `jet run` does. | Lands only if never slower. Diagnostics are identical on the case corpus. |
| B | 4's parts, one at a time. | Each lands only if faster than the text path. Each trace shows `core bodies from text` falling. |

## 7. Risks

- **The interface and Core drift apart.** The interface is written from the
  same check that writes `jet_core.o` and `core.seal`, in the same build.
  The A1 round-trip guard fails the build's tests on any drift.
- **Reach precision.** Reach runs on the interface's spellings. A
  declaration reach misses gives a missing-name error, not a wrong meaning.
  The corpus, the cases and `jet check Core` (which reaches everything)
  catch it.
- **Seal coverage.** A wrong seal fact gives a wrong effect or failure row
  with no body check. The seal comes from the same checker as the body
  check would. A4 proves diagnostic parity on the whole case corpus before
  `jet check` relies on it.
- **The load is not fast enough.** The record failed here. 3.4 makes the
  budget a gate on A2, and the trace counters show the load's cost on every
  run.
