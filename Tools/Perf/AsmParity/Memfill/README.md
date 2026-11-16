# memfill: the memory kernel's byte fill

`run.jet` runs `mem.fill<U8>` 4096 times per pass. Each fill stores its own
byte at an offset of 0 to 63 bytes into one page-aligned block. The sizes run
from 1 byte to 16 KiB and are weighted toward small fills: a draw's top three
bits pick one of eight size classes (up to 8, 16, 32, 64, 128, 512, 2048 or
16384 bytes), and each class gets 484 to 549 of the 4096 fills. The sum reads
the first and the last byte of each fill. `ref.S` is the machine ceiling. It
does the same work from the same generator, with the same layout. The fill
goes through one memset routine. A CPU check chooses that routine once at
load, and the program calls it through a stack slot. The routine's shape and
its 2048-byte `rep stosb` threshold are in `ref.S`'s header.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in two measures:

- **cycles**: the `perf stat` median from `asm-parity.mjs`.
- **code bytes**: counted as `../Memcopy/README.md` describes: the `nm -S`
  size of the side's `routines`, the bytes the DWARF line table assigns to the
  side's `use` lines, and the side's `data`.

| | Jet | ceiling |
| --- | --- | --- |
| routines | `jet_rt_mem_fill_bytes` | `dispatch`, `fill_avx2`, `fill_sse2` |
| uses | `mem.fill<U8>(` lines of `run.jet` | `call [rsp]` lines of `ref.S` |
| data | none | 8: the routine's stack slot |

The `mem.fill` line names only the call. The pointer and the byte are
computed on the lines before it, just as the reference loads its argument
registers on lines outside the use. A `mem.fill<U8>` lowers to one call of
`jet_rt_mem_fill_bytes` with the byte in a register. If the Jet fill moves to
another runtime routine, name that routine in the `jet` side's `routines`.

The package names `run.jet` as its memory kernel: the cell measures a raw
memory primitive, so its one file holds the block's allocation and the reads
of the filled bytes.
