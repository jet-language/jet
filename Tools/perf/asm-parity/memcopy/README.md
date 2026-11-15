# memcopy: the memory kernel's byte copy and move

`run.jet` runs `mem.copy<U8>` 8192 times per pass. There are 4096 operations,
each a copy between two ranges plus a move inside a third range. The sizes
run from 1 byte to 16 KiB and are weighted toward small copies. The moves
overlap in either direction. `ref.S` is the machine ceiling. It does the same
work from the same generator, with the same layout: one page-aligned block
holding the source at 0, the target at 18432 and the moved bytes at 36864.
The copy and move go through one memmove routine. A CPU check chooses that
routine once at load, and the program calls it through a stack slot. The
routine's shape, its dispatch and why the 2048-byte `rep movsb` threshold was
chosen are in `ref.S`'s header.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in two measures:

- **cycles**: the `perf stat` median from `asm-parity.mjs`.
- **code bytes**: counted as described below.

## Code bytes

Code bytes count only the primitive's own code, never the whole file. The
file bytes stay a reported column. The count is the sum of three parts, taken
the same way on both sides by `code` in `../asm-parity.mjs`:

1. **Routines.** Add the `nm -S` size of each symbol in the side's
   `routines`. These are the primitive's functions, its CPU dispatch, and any
   table or constant it alone owns.
2. **Uses.** Add every byte the DWARF line table
   (`objdump --dwarf=decodedline`) assigns to a line of the side's `source`
   whose text contains `use`. A row's bytes run up to the next row of its
   sequence. Every copy the compiler emits of a call site counts.
3. **Data.** Add `data`: bytes the primitive keeps that no symbol names.

The two sides are counted as follows:

| | Jet | ceiling |
| --- | --- | --- |
| object read | the release build's split debug file. `--debug-info=split` leaves the image byte-identical to a plain `--release` build. | `ref.S` assembled again with `as -g` |
| routines | `jet_rt_mem_move` | `dispatch`, `move_avx2`, `move_sse2` |
| uses | `mem.copy<U8>(` lines of `run.jet` | `call [rsp]` lines of `ref.S` |
| data | none | 8: the routine's stack slot |

Each `mem.copy` line names only the call. The pointers are computed on the
lines before it, just as the reference loads its argument registers on lines
outside the use. As a result, a use counts what the call itself lowers to:
the count check, any argument moves and the call.

When the Jet copy gains routines or data (for example a dispatch word set at
runtime init), add their symbols to the `jet` side's `routines`.
