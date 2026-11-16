# rawword: the memory kernel's raw word access

`run.jet` runs 4096 operations per pass over one page-aligned block of 8192
words. Each operation steps a typed pointer to one index (`offset`), reads
the word there through `.*`, adds its step, steps to a second index and
writes the sum there through `mem.volatile_write`. One draw of a 31-bit
generator gives the two indexes and the step. The indexes are stored as `U32`
offsets, as the reference's table stores them in 4 bytes each, so both sides
read the same data. The sum adds up every word
read. `ref.S` is the machine ceiling. It does the same work from the same
generator, with the same layout. Each access is one instruction, with the
index scaled into the address operand. Neither side checks anything on the
access: every index is below the block's length by construction, and the
`#Unsafe` block states this.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in two measures:

- **cycles**: the `perf stat` median from `asm-parity.mjs`.
- **code bytes**: counted as `../memcopy/README.md` describes. The primitive
  has no routine, so this is only the bytes the DWARF line table assigns to
  the side's `use` lines.

| | Jet | ceiling |
| --- | --- | --- |
| routines | none | none |
| uses | `words.offset(` lines of `run.jet` | `# the raw` lines of `ref.S` |
| data | none | none |

A use line holds only the step and the access. The indexes, the step and
the sum written are computed on the lines before it, just as the reference
loads and adds them on lines outside its two accesses.

The package names `run.jet` as its memory kernel: the cell measures a raw
memory primitive, so its one file holds the block's allocation and every
access to it.
