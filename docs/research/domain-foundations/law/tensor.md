# tensor

## Ratified

- **D-COMPUTE1=D** — `core.compute` owns “one Core compute family” for eager work, fusion, differentiation, placement, and expert device/buffer/stream/kernel work; external accelerator providers are explicit bridges and must differentially match Core semantics. — `docs/spec/syntax-decisions.md:5883-5887`
- **D-COMPUTE-TYPE1=D** — `Tensor<T>` owns ranked multidimensional storage; static shapes are checked, runtime shapes use typed fallible operations, and `View<T>` is the sole borrowed strided projection. `Vec<N>` and `Matrix<M,N>` share the substrate and cross zero-copy; differentiability is a transform property, not a second type. — `docs/spec/syntax-decisions.md:5889-5894`
- **D-COMPUTE-PLACE1=D** — `.Auto` is the beginner default; experts pin device, memory, precision, and transfer policy. Transfers, excess allocations, and fallbacks emit stable receipts; fallback must be named and cannot change precision, effects, failure shape, or observable results. — `docs/spec/syntax-decisions.md:5896-5901`
- **D-COMPUTE-KERNEL1=D / D-COMPUTE-KERNEL-SURFACE1=B** — sema proves bounds, alias/race freedom, captures, barrier uniformity, and control flow before a function becomes a kernel. `#Kernel(.parallel)` is the explicit safe marker; the shipped subset is conservative read-only/effect-free code, while indexed writes, loops, captures, and provider calls remain rejected. — `docs/spec/syntax-decisions.md:5903-5916`
- **D-COMPUTE-AUTODIFF1=D** — `compute.grad`/`value_and_grad` are described as scalar-loss defaults; `jvp`/`vjp` compose for mixed/higher-order work; unsupported mutation/control fails at its source span; custom derivatives live once at the operation definition and are checked. — `docs/spec/syntax-decisions.md:5918-5923`
- **D-COMPUTE-GRAD1=E** — the current public name is `compute.gradient` with direct and bound-transform arities; `compute.value_and_gradient` follows the same shape; both lower to one VJP core and `wrt:` names are checked. JVP remains a bound transform and Tensor effects carry through unchanged. — `docs/spec/syntax-decisions.md:5925-5936`
- **D-COMPUTE-VJP1=A** — `compute.vjp` returns `VjpRun` with `.value`, `.pull(seed)`, and `.grads`; scalar output uses a unit seed, while non-scalar output requires an explicit seed. — `docs/spec/syntax-decisions.md:5938-5942`
- **D-COMPUTE-BACKEND1=D** — default policy is `F32Strict + Reproducible`; fast math, reassociation, and nondeterministic reductions require named recorded policies; every backend differentially conforms to the CPU oracle; registered providers are CPU, Metal, CUDA, Vulkan, and browser WebGPU. — `docs/spec/syntax-decisions.md:5944-5950`
- **D-COMPUTE-RAWBOUNDARY1=A** — a raw-device contract is provider-issued and opaque to safe Jet, accepted only inside `#Unsafe`, and carried through TIR into the launch receipt; with no raw-device provider in the Epoch 3 CPU policy, the built-in constructor fails closed. — `docs/spec/syntax-decisions.md:5952-5958`
- **D-TYPE2-IMAG1=A** — suffix `i` is an imaginary number on the unit-literal path; `Complex` is the ratified type, `i` alone remains an ordinary identifier, and no new grammar is added. — `docs/spec/syntax-decisions.md:6970-6985`

## Shipped

- `core.compute` documents Tensor creation, elementwise and shape operations, linalg/DFT, sparse operations, `gradient`/`value_and_gradient`/`jvp`/`vjp`, `matmul_f32_tile`, streams, placement, and typed `ComputeError` failures; explicit accelerators never silently fall back. — `docs/reference/core-library.md:3976-4037`
- `compute.set(&tensor, index, value)` is a shipped mutable-write surface, with `get`, shape/rank/numel, placement, and `to_list`. — `docs/reference/core-library.md:4027-4031`; executable proof: `examples/features/tooling/compute_tensor.jet`
- Autodiff, higher-order gradients, JVP/VJP, `value_and_gradient`, sparse CSR, `matmul_f32_tile`, streams, and bounds checks are exercised by `examples/features/tooling/compute_autodiff.jet`.
- Dense linalg and `compute.fft` are exercised by `examples/features/tooling/compute_linalg.jet`; the implementation explicitly says “Naive DFT on a rank-1 real tensor” and rejects accelerator tensors, non-rank-1 input, and traced tensors without a registered rule. — `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:7603-7619`
- The FFT implementation has nested `k`/`t` loops over `n`, so it is an O(n²) implementation; no ratified complexity or N=4096/487-ms performance claim was found. — `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:7635-7646`
- `matmul_f32_tile` and safe-kernel syntax are represented by `examples/features/tooling/compute_autodiff.jet` and `examples/features/tooling/compute_kernel.jet`.
- The runtime Tensor representation is `shape: Vec<i64>`, `strides: Vec<i64>`, and `data: Arc<Vec<f64>>`, with a mutable owner window type. — `crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:4013-4032`
- `JetComplex` exists as a scalar `{ real: f64, imaginary: f64 }` with arithmetic; Tensor complex dtype support is not shown by the Tensor representation or API evidence. — `crates/jet-codegen/src/Prelude/CoreLib/MathLibPure.rs:4-59`
- The built Core inventory includes `core.compute` but not `core.image`. — `docs/reference/core-library.md:4281-4296`

## Undecided

- Whether public Tensor storage is intentionally real-only (`f64` plus the explicit F32 profile), or should gain a typed dtype parameter/registry without fragmenting the one Tensor family.
- Whether Tensor operations should support `Complex`/`JetComplex` values, and what placement, autodiff, serialization, and backend policy would apply.
- Whether `compute.fft` needs a ratified complexity/performance/provider contract, including a replacement for the current O(n²) CPU DFT and the unsupported accelerator path; the probe's N=4096/487-ms measurement is not a ruling or repo evidence.
- Whether `matmul_f32_tile` needs more public guarantees than its current F32 arithmetic, ordered reduction, and receipt behavior, including target coverage and provider parity.
- Whether labeled dimensions or named axes should be part of Tensor shape checking, broadcasting, serialization, and diagnostics; no such public Core surface is ratified.
- Whether an image tensor library/API should ship separately from `core.compute`; `core.image` is absent from the built inventory.
- Which autodiff tier/provider support is required for unsupported mutation/control, non-CPU placement, complex values, and kernel code, beyond the current checked transform contracts.

## Conflicts

- D-COMPUTE-TYPE1 already chooses one Tensor family and one borrowed `View<T>` substrate. A separate image tensor type, tensor-specific borrow model, or second differentiation type would conflict unless it is an explicit package layer over that substrate.
- D-COMPUTE-GRAD1 supersedes the public `compute.grad`/`value_and_grad` spellings with `compute.gradient`/`compute.value_and_gradient`; a ballot to add the old names as a parallel API would conflict with the current surface.
- D-COMPUTE-BACKEND1 and D-COMPUTE-PLACE1 require explicit provider negotiation, receipts, CPU-oracle parity, and no silent CPU fallback. “Just fall back” cannot be the answer for unsupported FFT or accelerator operations.
- D-COMPUTE-KERNEL-SURFACE1 rejects indexed writes, loops, captures, and opaque/provider calls in the safe `#Kernel(.parallel)` subset; an unchecked safe-kernel expansion is not already granted.
- D-COMPUTE-RAWBOUNDARY1 forbids a built-in forgeable raw-device token; raw provider code is `#Unsafe` only and must carry a provider-issued contract.
- `compute.set` and `matmul_f32_tile` already ship. A proposal merely to add those names or basic mutable Tensor indexing is an implementation card, not a missing primitive.
- `D-M-*` domain ballots that were deleted during the rescope are not current law. Do not cite deleted domain IDs as permission or prohibition; only the surviving compute/memory decisions above bind this area.
- The docs are internally inconsistent on the default profile: the syntax law says default compute policy is `F32Strict + Reproducible`, while the reference surface says `Auto` is CPU for default F64 and later says general operations report F64 by default. This must be reconciled before a dtype ballot treats either spelling as settled implementation truth. — `docs/spec/syntax-decisions.md:5944-5950`; `docs/reference/core-library.md:3978-3987,4117-4125`
