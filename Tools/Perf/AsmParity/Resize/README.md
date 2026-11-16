# resize: the memory kernel's grow in place or move

`run.jet` runs 4096 appends per pass over 256 byte lists. Each one grows a
list by 1 to 64 bytes with `mem.list_append`, weighted toward small counts:
a draw's top bits pick one of four classes (up to 8, 16, 32 or 64 bytes),
and a second draw picks the list. A full buffer grows to twice its capacity,
or to the new length when that is more. A list that would pass 1 KiB starts
over empty, which releases its buffer. The sum adds each list's length after
each append.

`ref.S` is the machine ceiling. It does the same work from the same
generator on `../Allocfree/ref.S`'s minimal slab, with classes up to 2 KiB.
A buffer's capacity is its class's size, so the resize is that slab's: an
append that fits the block's class grows in place, and one that does not
moves to a block of the larger class and releases the old one.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in cycles and in code
bytes, counted as `../Memcopy/README.md` describes.

| | Jet | ceiling |
| --- | --- | --- |
| routines | the runtime's move, take and release paths (the `jet` side's `routines`) | `grow`, `alloc`, `free`, `copy` |
| uses | `// the resize` lines of `run.jet` | `# the resize` lines of `ref.S` |
| data | none | none |

The Jet routines are the ones a profile of the cell samples. On the compiler
the cell was written against, a full List never grows in place: the append
takes a new buffer, moves the bytes and releases the old buffer. If the
growth moves to other routines, name the routines a profile of the cell
samples.

This cell is a measurement first: the Jet row may be over the ceiling.
