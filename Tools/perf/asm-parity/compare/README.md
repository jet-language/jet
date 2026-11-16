# compare: the memory kernel's byte compare

`run.jet` builds 1024 pairs of `[U8]` lists and compares each pair with the
safe `==` once per pass. The sizes run from 1 byte to 16 KiB and are weighted
toward small lists: a draw's top three bits pick one of eight size classes
(up to 8, 16, 32, 64, 128, 512, 2048 or 16384 bytes). A second draw's top bit
decides whether the pair differs, in one byte at a drawn position. Each pass
first flips the first byte of one right list, so no pass repeats the one
before it and no compare can be hoisted out of the pass loop. The sum counts
the equal pairs.

`ref.S` is the machine ceiling. It does the same work from the same
generator, with every list at a 16-byte boundary in one mapping, as the heap
places a list's bytes. The compare goes through one equality routine, which
a CPU check chooses once at load; the program calls it through a stack slot.
The pairs of this cell always have equal lengths, so the routine takes one
size. Its shape is in `ref.S`'s header.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in cycles and in code
bytes, counted as `../memcopy/README.md` describes.

| | Jet | ceiling |
| --- | --- | --- |
| routines | none: the compare is built inline | `dispatch`, `equal_avx2`, `equal_sse2` |
| uses | `// the compare` lines of `run.jet` | `# the compare` lines of `ref.S` |
| data | none | 8: the routine's stack slot |

This cell is a measurement first. On the compiler it was written against,
the `[U8]` equality lowers inline to a length compare and then a byte-at-a-time
loop. If the equality moves to a runtime routine, name that routine in the
`jet` side's `routines`.
