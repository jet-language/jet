# Numeric performance primitive probe

## What I built

I built four real numeric kernels in plain Jet: radix-2 FFT, dense matrix multiply, CSR matrix-vector multiply, and a five-point stencil. I also ran the shipped Tensor tiled matmul and Tensor FFT, plus a focused `core.math.pi` defect probe. All timed inputs are deterministic and use the same shapes and values in Jet and an optimized native Rust baseline.

Files:

- `pkg/package.jet` — authority grants needed by the probes.
- `pkg/fft.jet` — radix-2 split-complex FFT at 4096 and 65536 points.
- `pkg/matmul.jet` — dense 512 x 512 Float triple loop.
- `pkg/sparse.jet` — 100,000 x 1,000 CSR MV with 1,000,000 nonzeros.
- `pkg/stencil.jet` — one 1024 x 1024 five-point sweep.
- `pkg/compute.jet`, `pkg/compute_fft.jet` — shipped Tensor paths.
- `pkg/pi_probe.jet`, `pkg/pi_literal_probe.jet`, `pkg/time_ns.jet` — defect and timing checks.
- `rust_bench.rs` — same-input Rust baseline, compiled with `rustc -O -C target-cpu=native`.

## What worked

- `scripts/agent/jet-env jet check pkg/{fft,matmul,sparse,stencil,compute,compute_fft}.jet` — all checks passed; only style, hidden-cost, or display warnings remained.
- `scripts/agent/jet-env jet run pkg/fft.jet` — default tier produced exact impulse checksums; 4096 took 151,687,664 ns and 65536 took 2,449,309,762 ns.
- `scripts/agent/jet-env jet build --release .../fft.jet; ./build/fft` — release artifact produced exact checksums; repeated 65536 timings were 10,476,357, 12,638,383, and 12,707,954 ns.
- `scripts/agent/jet-env jet run pkg/matmul.jet` — default tier produced checksum 268,435,456 and took 212,157,672,468 ns.
- `scripts/agent/jet-env jet build --release .../matmul.jet; ./build/matmul` — release timings were 334,013,201, 340,794,628, and 343,078,414 ns; native Rust was 343,795,020 ns.
- `scripts/agent/jet-env jet run pkg/sparse.jet` — default tier produced checksum 1,000,000 and took 1,740,739,778 ns.
- `scripts/agent/jet-env jet build --release .../sparse.jet; ./build/sparse` — release median was 1,931,866 ns (runs 1,950,922; 1,928,760; 3,855,315; 1,869,076; 1,931,866); native Rust was 1,464,754 ns.
- `scripts/agent/jet-env jet run pkg/stencil.jet` — default tier produced checksum 1,044,484 and took 4,584,449,492 ns.
- `scripts/agent/jet-env jet build --release .../stencil.jet; ./build/stencil` — release median was 4,430,443 ns; native Rust was 1,894,134 ns.
- `scripts/agent/jet-env jet run --release pkg/compute.jet` — shipped `compute.matmul_f32_tile` produced exact values in 82,497,552 ns and reported `algorithm=blocked-matmul;tile=8;dispatch=avx2;vector_width=8;tail=scalar`.
- `scripts/agent/jet-env jet run --release pkg/compute_fft.jet` — shipped `compute.fft` produced the correct 8192-element interleaved result in 487,254,548 ns at 4096 points.
- `scripts/agent/jet-env jet run --release pkg/pi_literal_probe.jet` — explicit `Float` pi literal worked in both tiers and printed `-1.5707963267948966`.
- `scripts/agent/jet-env jet run pkg/time_ns.jet` — `time.instant`, `Duration.in(.Nanoseconds)`, and deterministic timing worked (`x:1000000 ns:465113384 us:465113 ms:465`).

Release plain Jet loops are near the native Rust baseline for FFT and dense matmul. The shipped tiled Tensor matmul is faster than the plain loop. CSR is within roughly 1.3x of Rust; the stencil is roughly 2.3x slower. The default evaluator is useful for correctness but is 10^2–10^3x slower on these hot loops.

## Gaps

See `gaps.json`. The short list is:

- `defect`: `core.math.pi` is not lowered in a user function; default evaluation reports E0956 and release codegen aborts with an ICE at `Items.rs:3409:5`.
- `slow`: shipped `core.compute.fft` is a quadratic nested DFT; a hand-written radix-2 FFT is about 4,600x faster at 4096 on this host (487 ms versus about 0.106 ms), and scales to 65536.
- `slow`: generic indexed `[Float]` loops trigger L2510 generic-collection warnings and lose about 2.3x on the stencil versus native Rust; CSR is slower too, though close.
- `boilerplate`: CSR requires manually synchronized `data`, `indices`, and `indptr` arrays; the shipped surface exposes dense-to-sparse conversion and MV, not direct CSR storage construction.
- `boilerplate`: complex FFT requires a hand-written split `re`/`im` buffer and every complex butterfly; scalar complex arithmetic does not provide a public complex Tensor/buffer family.

## Friction

- Exact `Int` division produces `Fraction`; radix-2 index arithmetic requires `/%` and `/%=`.
- Decimal literals are not `Float`; every floating seed needed `Float{...}`.
- Borrowed array elements need explicit `~` copies before later writes, and mutable calls use `&` or ownership transfer.
- `time.start()` requires a package authority grant for `Time`; compute examples also required explicit `GPU` and `Panic` grants even for the CPU path.
- `jet check` reported L2510 on repeated generic collection indexing and L0520 for interpolated indexed values. These diagnostics are actionable, but the author still writes the representation and loop machinery.

## Defects

- `scripts/agent/jet-env jet run --release pkg/pi_probe.jet` exits 101 with: `internal compiler error: codegen reached a construct the typed IR does not cover: the expression pi at pi_probe.jet:2:35, in fn angle ... crates/jet-codegen/src/Codegen/Items.rs:3409:5`.
- `scripts/agent/jet-env jet run pkg/pi_probe.jet` reports E0956 at `pi_probe.jet:3:12`: the current evaluator does not cover the call.

## Battery notes

Not applicable: this is a primitive probe, not one of the eight critical-area probes. No `batteries.json` is produced.

## Verdict

- Plain Jet can express all four kernels and preserve exact checksums across default and release tiers.
- Release codegen reaches native-loop speed for FFT and dense matmul; shipped tiled matmul is a strong, inspectable fast path.
- The shipped FFT primitive needs an O(n log n) planner/backend before it is safe for production-size transforms.
- A contiguous-buffer proof, direct CSR storage, and complex numeric buffers would remove repeated author machinery and generic-loop cost.
- Fix `core.math.pi` lowering across evaluator and codegen; do not require numeric authors to paste constants.
