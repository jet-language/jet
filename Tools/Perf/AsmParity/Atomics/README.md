# atomics: the memory kernel's atomic word

`run.jet` runs 4096 rounds per pass on one `Atomic<I64>` counter. Each round:

- adds a step and reads the old value back (`add`);
- loads the word (`load`);
- exchanges the loaded value for one more (`compare_exchange`). When the
  exchange is won, the sum gains one.

Each pass ends by storing zero (`store`). The sum adds up every old value
and every exchange won.

`ref.S` is the machine ceiling. It does the same work, with each atomic as
its one sequentially consistent x86-64 instruction on the word: `lock xadd`,
`mov`, `lock cmpxchg` (branching on its ZF) and `xchg`. The word's address
is held in a register. Neither side checks anything: an atomic word has
nothing to check.

The cell is a memory kernel cell (its `kernel` entry in `../cells.json`). Its
release row (O2) must satisfy Jet / ceiling <= 1.00 in two measures:

- **cycles**: the `perf stat` median from `asm-parity.mjs`.
- **code bytes**: counted as `../Memcopy/README.md` describes. The primitive
  has no routine, so this is only the bytes the DWARF line table assigns to
  the side's `use` lines.

| | Jet | ceiling |
| --- | --- | --- |
| routines | none | none |
| uses | `counter.` lines of `run.jet` | `# the atomic` lines of `ref.S` |
| data | none | none |

A use line holds only the atomic. The step, the next value and the sums are
computed on the lines around it, just as the reference computes them on
lines outside its four atomics.

The package names `run.jet` as its memory kernel: the cell measures the
atomic word primitive, and its one file holds the word and every atomic on
it.
