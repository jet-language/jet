# mapping: the memory kernel's page mapping

Each pass of `run.jet` maps a fresh 64 KiB region of private pages with
`mem.Region.reserve`, then unmaps it with the region's close. The sum adds up
every region's size.

`ref.S` is the machine ceiling. It does the same work with one `mmap` and one
`munmap` per pass, and keeps the one check a mapping needs: the refusal test
on mmap's result, at its smallest. A refused mapping exits 70, as Jet's panic
does. Like Jet's region, each mapping is registered in a page map: a flat
table of one word per 64 KiB unit of the 47-bit address space, mapped once
NORESERVE at start. Map stores the base at the mapping's unit, and unmap
clears it, one store each.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in both cycles and code
bytes. Code bytes are counted as `../Memcopy/README.md` describes.

| | Jet | ceiling |
| --- | --- | --- |
| routines | `jet_rt_mem_region_reserve`, `jet_rt_mem_region_release`, `unit_alloc_reservation_map`, `unit_alloc_page_set` | none |
| uses | `// the mapping` lines of `run.jet` | `# the mapping` lines of `ref.S` |
| data | none | none |

Jet's routines are the mapping's own runtime code. The page map's helpers
that the compiler inlined into them count inside their sizes. A region in
the window keeps its small record in the window's slot for its first unit,
with no heap block; a region outside the window takes its record from the
heap allocator, which the `allocfree` cell measures, so it does not count
here.

The reference's uses are every line of its two calls: the arguments, the
syscalls, the check, the refusal exit, and the page map's store and clear.
The table's own mapping runs once at start, as a runtime maps its page map
before the first region, so it is not a use. On Jet's side, the reserve line
and the close line count everything they lower to, the panic included.

This cell is a measurement first: on the runtime it was written against, Jet
loses. Each reserve maps one extra page-map unit, unmaps the alignment slack
with up to two more munmaps, allocates a record, and registers the pages in
the page map under its global lock. Release looks the record up, clears the
page map, and frees the record.
