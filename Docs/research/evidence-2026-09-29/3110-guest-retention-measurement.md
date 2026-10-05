# #3110 — guest roots, Text ownership and retained memory (measurement)

Closer07, 2026-09-29. Library built by `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`) from the unchanged `Examples/interop/guest_library/{library.jet,package.jet}`. Machine: AMD Ryzen 9 7950X3D, Linux 7.0.11-cachyos, g++ 15.3.0 (nix `Tools/agent/jet-env`).

## Question
When a native host drives the guest Library through prepare, repeated calls, held Text results and release, do native-held values stay valid for their declared lifetime and become releasable afterwards? Is steady-state memory bounded? Allocator memory, managed values and scarce external handles are reported separately.

## Method
Scratch witness `~/.cache/jet-dev/scratch/Closer07/retention/retention.cpp` (the fixture is not modified):
1. `safe-jet.sh build --lib library.jet` → `.jet/build/libembedding.so` + `embedding.h`.
2. `jet-env g++ -O2 -std=c++17 -I .jet/build retention.cpp -o retention -pthread -ldl -rdynamic`.
3. `./retention $PWD/.jet/build/libembedding.so`, run 3 times under `systemd-run … MemoryMax=6G`.

Phases:
- dlopen;
- 1k warm-up;
- `greet("Ada")`+`jet_text_free`+`on_tick` × {1k, 10k, 100k};
- 10k `reenter` (host→guest→host);
- 256 held `greet(64 KiB)` Text results, verified intact, then freed;
- 10k × 64 KiB calls;
- dlclose.

Rows per sample:
- allocator: `VmRSS`/`VmHWM` from `/proc/self/status`;
- managed: host-side outstanding `JetText` count;
- external: open fds from `/proc/self/fd`.

## Evidence (3 runs; run 1 shown, runs 2–3 within ±12 KiB and ±10 ns)
```
process-start       rss_kb=3904  hwm_kb=3840  fds=6 outstanding_text=0
after-dlopen        rss_kb=4168  hwm_kb=4168  fds=6 outstanding_text=0
warmup-1k           rss_kb=4536  hwm_kb=4536  fds=6 outstanding_text=0   ns_per_call=119
steady-1000         rss_kb=4536  hwm_kb=4536  fds=6 outstanding_text=0   ns_per_call=94
steady-10000        rss_kb=4536  hwm_kb=4536  fds=6 outstanding_text=0   ns_per_call=91
steady-100000       rss_kb=4536  hwm_kb=4536  fds=6 outstanding_text=0   ns_per_call=86
reenter-10k ok=10000
held-256x64KiB      rss_kb=22344 hwm_kb=22344 fds=6 outstanding_text=256 ns_per_call=32438
held-texts-intact=yes
held-released       rss_kb=4976  hwm_kb=21828 fds=6 outstanding_text=0
steady-10k-64KiB    rss_kb=4976  hwm_kb=21828 fds=6 outstanding_text=0   ns_per_call=5384
after-dlclose       rss_kb=4976  hwm_kb=21828 fds=6 outstanding_text=0
```
Runs 2 and 3:
- steady-100000 RSS was 4524 and 4532 KiB, the same as steady-10000 in each run.
- held peak was 22332 and 22340 KiB.
- held-released was 4964 and 4972 KiB.

(`ns_per_call` = one `greet`+free+`on_tick` round trip, host wall clock. It is a copy-cost indication, not a benchmark.)

## Findings
- **Allocator row:**
  - The RSS delta from 10k to 100k small-Text calls is **0 KiB** in 3 of 3 runs.
  - Holding 256 × 64 KiB Text results costs +17.4 MiB RSS; the payload is 16.0 MiB, and the input copies were already freed.
  - Freeing them returns RSS to +0.43 MiB over the pre-hold level.
  - A 64 KiB round trip costs ~5.4 µs versus ~86 ns for a 3-byte one: each call copies the input and the output.
  - `dlclose` does not return that residual ~0.43 MiB, and it does not lower RSS.
- **Managed row:**
  - The 256 Text results held by the native side stay intact and readable until `jet_text_free`: byte check `hello, …!` passes, so they stay rooted for their declared lifetime.
  - Each result is releasable exactly once. The outstanding count returns to 0 before unload.
  - The Library exposes no internal outstanding-object counter, so this count is host-side accounting only.
- **External-handle row:** the unchanged fixture opens no files (fd count stays 6 throughout). A scratch variant (`~/.cache/jet-dev/scratch/Closer07/retention2`) adds `use core.files as files` and `#Export(c) pub fn read_len(salt: Int) -> Int { text :: files.read("/etc/hostname") ?? return -1 … text.len() }`, with package `authority: { holds: { allow: [FS, Mem.Alloc] } }`. Host `fdrow.cpp`, 2 runs:
  ```
  start fds=6 rss_kb=3728
  single-call result=8 fds_after_return=6
  calls=1000    mismatches=0 fds_after=6 rss_kb=4588
  calls=10000   mismatches=0 fds_after=6 rss_kb=4588
  calls=100000  mismatches=0 fds_after=6 rss_kb=4588
  after-dlclose fds=6
  ```
  The file handle is closed by the time each call returns: deterministic scope-exit cleanup, not deferred to unload or to a finalizer. RSS is flat from 1k to 100k (run 2: 4564 at every step).
  Side observation: `jet build --lib` for this variant printed `required effects: none; granted effects: FS, Mem.Alloc`, although the export reads a file. The rights summary does not list FS as required for a Library export.
- **Finalizer/re-entry:** 10k re-entrant host→guest→host calls produce no RSS growth. No finalizer is involved: Text release is the explicit `jet_text_free` ABI, not GC timing.
- **Not measured:**
  - failed-call rows: the C ABI has no typed-error export in this fixture; `panic_now` terminates the process with status 70 by contract;
  - the Component guest;
  - the callback-registration cycle (FfiCallbacks).

## Verdict
- Criteria 1 and 3: measured for the native Library Text path.
- Criterion 2: met for the native Library. The file-handle row closes at scope exit (fd count returns to baseline after every call), and the Text row uses an explicit free rather than finalizers. Not measured for a Component guest or for socket handles.
- Criterion 4's named `guest_embedding_retention_is_bounded_and_releases` does not exist in `tests/library_outputs.rs`, so it is NOT MET as written.
- No zero-allocation or real-time claim is made: each call allocates and copies.
