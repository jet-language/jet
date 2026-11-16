# allocfree: the memory kernel's heap

`run.jet` runs 4096 operations per pass over 256 slots. Each one releases
the block its slot holds with `mem.free_raw` and takes a fresh block with
`mem.alloc_raw`. The sizes run from 1 byte to 1 KiB and are weighted toward
small blocks: a draw's top three bits pick one of eight size classes (up to 8,
16, 32, 64, 128, 256, 512 or 1024 bytes), and a second draw picks the slot.
The sum adds every size taken.

`ref.S` is the machine ceiling. It does the same work from the same
generator, on a minimal hand slab allocator with what a real heap needs:
16-byte size classes with a free list each, a page map over one aligned
reservation so that a release needs no size, and a cross-thread free path
(a slab names its owning heap, and a block another heap frees goes onto the
slab's remote list with one `lock cmpxchg`). The details are in `ref.S`'s
header.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in cycles and in code
bytes, counted as `../Memcopy/README.md` describes.

| | Jet | ceiling |
| --- | --- | --- |
| routines | the runtime's take and release paths (the `jet` side's `routines`) | `alloc`, `free` |
| uses | `// the heap` lines of `run.jet` | `# the heap` lines of `ref.S` |
| data | none | none |

The Jet routines are the ones a profile of the cell samples: `mem.alloc_raw`
calls `jet_rt_alloc_try` and `mem.free_raw` calls `jet_rt_free`, and each
runs its helpers below. The runtime's slab refill routines are not counted,
because the profile does not sample them, while the ceiling's `alloc` counts
its own refill. If the heap's paths change, name the routines a profile of
the cell samples.

This cell is a measurement first: on the compiler it was written against,
the Jet row is far over the ceiling.
