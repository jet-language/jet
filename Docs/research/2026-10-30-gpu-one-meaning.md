# GPU work in Jet: one way in, one meaning on every device

Date: 2026-10-30. Card #6344, criteria 1 to 3. Author: MemDesign-2 lane
(design only). Probes live in `/mnt/jetscratch/lanes/MemDesign-2/probes/`
(`g/` for GPU and facts, `perf/` for the timing cell) and ran with
`/mnt/jetscratch/discovery/base/latest/jet` (commit 4e514f96e1) against a
`git archive` export of dev 4e514f96e1. Every probe ran with `jet run
--optimize=none|basic|full` (o0, o1, o2) through `probes/tiers.sh`; outputs are
in `probes/out/<name>.<tier>.{out,err,rc}`. Unless a row says otherwise, all
three levels gave the same result.

The owner asked four things: how Jet can split one program into CPU and GPU
parts as Mojo does, whether Jet can have a portable GPU kernel library, how a
user pushes code to the GPU without a large barrier, and how math such as sine
stays portable and accurate. This note answers each from ratified law, from
what the compiler does today, and from peer languages. It raises one owner
choice, filed as the ballot `D-GPU-DEVICE-OPS1`; everything else is
implementation under law that already exists.

## Ratified and open decisions

Tower decisions (read with `node Tools/tower/tower.mjs decision show <ID>`):

| Decision | Status | What it settles for this card |
|---|---|---|
| D-GPU-COMPUTE-LAUNCH1 | ratified A, 2026-09-11 | `compute.launch(kernel, inputs: …)` launches a checked `#Kernel`; a real GPU, never an implicit CPU substitute. |
| D-GPU-SHADER-SURFACE1 | ratified B, 2026-09-11 | Graphics functions use `#Shader`; `#Kernel` stays for compute. |
| D-GPU-F32-STORAGE1 | ratified A, 2026-09-12 | A typed `[F32]` list becomes a Tensor without widening. |
| D-GPU-INDEXED-RESOURCE1 | ratified A, 2026-10-05 | One GPU calculation returns one answer, or places several through indexed writes. |
| D-ACCEL-LIB1 | ratified A, 2026-10-28 | GPUs and accelerators are libraries over a public, versioned kernel IR, each checked against the CPU result. |
| D-CORE-BOUNDARY1 | ratified A, 2026-10-05 | Chip knowledge lives in the library as typed instruction entries with a plain Jet body. |
| D-CORE-FEATURE-FLOW1 | ratified A, 2026-10-05 | Chip features are automatic by default with explicit expert control. |
| D-HOST-CAPS1 | ratified A, 2026-10-07 | Typed hardware facts through `$build.hardware`, `requires:` and `uses:`. |
| D-AUTOPIN1 | ratified A, 2026-09-28 | Every automatic choice (including placement) can be seen and locked. |
| D-EXEC3 | ratified A, 2026-10-07 | Build levels share speed-ups and are checked against each other. |
| D-EFFTYPE-KERNEL1 | ratified A, 2026-10-30 | The compiler builds in machine powers; Core declares the rest. |
| D-EFFTYPE-TARGET1 | ratified A, 2026-10-30 | Each build's effect limit comes from its target profile. |
| D-COMPUTE-AUTODIFF-SCOPE1 | ratified B, 2026-10-17 | Scope of automatic differentiation in `core.compute`. |
| D-ARRAY-LANESCMP1 | ratified C, 2026-10-30 | Lane-wise comparisons are methods returning `Lanes<Bool, N>`. |
| D-LANES1, D-CPU-DISPATCH1 | ratified A | CPU vector lanes and automatic CPU chip copies. |
| D-MATH-BITS1 | open, recommendation A | `core.math` returns the same correctly rounded bits on every platform, level and at compile time. |

Decisions kept in `Docs/spec/syntax-decisions.md` (not in Tower):

- D-COMPUTE1=D, one Core compute family (`syntax-decisions.md:6892-6896`).
- D-COMPUTE-TYPE1=D, `Tensor<T>` owner and unified `View` (`:6898-6903`).
- D-COMPUTE-PLACE1=D, `.Auto` placement with receipts; a fallback "cannot
  change precision, effects, failure shape, or observable results"
  (`:6905-6910`).
- D-COMPUTE-KERNEL1=D, proved safe kernels plus an audited raw tier
  (`:6912-6917`).
- D-COMPUTE-KERNEL-SURFACE1=B, the `#Kernel(.parallel)` marker. "This epoch
  ships the conservative read-only, effect-free expression subset; indexed
  writes, loops, captures, and opaque/provider calls remain rejected until
  their proof facts exist" (`:6919-6925`).
- D-COMPUTE-BACKEND1=D, default F32Strict plus Reproducible; "Fast math,
  reassociation, and nondeterministic reductions require named recorded
  policies"; every backend conforms to the CPU oracle (`:6953-6959`).
- D-COMPUTE-RAWBOUNDARY1=A, raw device code only inside `#Unsafe` with a
  provider-issued contract (`:6961-6967`).
- D-SIMD3=B (`:3762`), D-EXEC2=A (`:3773`), D-DET1 (`:3303`), D-FLOATW1
  (`:1082`).
- Table rows: D-GPU-HELPER-SCOPE1=A (`:9124`), D-GPU-PACKAGING1=A (`:9126`),
  D-ACCEL1=A (`:8458`), D-TIER-FORM1=A (`:9735`).

The drafts D-GPU-GROUP1, D-GPU-SUBGROUP1, D-GPU-ATOMIC1, D-GPU-TILE1,
D-GPU-U321 and D-GPU-RANKED-SCOPE1 were written but never ratified; they are
gated on performance evidence (`Docs/research/mine-for-jet-2026-09-12.md:3082-3088`).

## What runs today

| Probe | Program | Result on o0, o1, o2 |
|---|---|---|
| g1, g15 | `#Kernel(.parallel) fn axpy(a: Float, x: Float, y: Float) -> Float { a * x + y }`, called on the CPU | Prints `cpu:5.0` and `axpy:7.0`. The marker parses and sema accepts the kernel. |
| g2b | `compute.launch(add, inputs: (left: xs, right: ys))` | E1004 "`core.compute` has no item `launch`". |
| g2c | the ratified example's positional `inputs: (xs, ys)` | E0048 "Jet tuples name every member". |
| g8 | `compute.from_list` of an `[F32]` list | E0112 "wants [Float] … but this is [F32]" (D-GPU-F32-STORAGE1 unbuilt). |
| g3, g4 | `compute.from_list`, `compute.add`, device values | ICE 101, "ir.build unlowered … runtime route jet_compute_from_list" and "argument type ComputeDevice … unlowered". |
| tm4k | gauntlet `Tools/gauntlet/entries/tensor-map-4k/jet/run.jet` | ICE 101, "runtime route jet_compute_full". |
| g5 | `#Shader` | E0927, unknown marker. |
| g6, g7 | `print` or `math.sin` inside a kernel | E1102 "cannot prove no reachable effects or opaque calls". |
| g11 | a kernel calling a pure helper `fn sq(x: F32) -> F32 { x * x }` | E1102, same obligation (D-GPU-HELPER-SCOPE1 unbuilt). |
| g12, g13 | `math.sqrt`, `math.abs` in a kernel | E1102 "audited Core compute calls only". |
| g10 | a mutable local in a kernel | E1102 "immutable scalar locals". |
| g9 | an `Invocation` parameter | E1102, not sendable. |

The marker is declared at `Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet:43`
and its mode and proof record at `Compiler/JetFoundation/Source/AST/Items.jet:704-722`.
The kernel obligations quoted above are raised in
`Compiler/JetSema/Source/Sema/Kernel.jet:93`, `:132` and `:146`. `Core/Compute/Compute.jet`
is 18 lines of opaque carriers, and the `jet_compute_*` routes exist only as
call rows (`Compiler/JetFoundation/Source/Registry/CoreCallRows.jet:662-723`), which is
why tensor operations reach the IR builder unlowered. These ICEs are the known
family tracked by #5454 (`Tools/testkit/known-failing.tsv:33-37`, `:290-291`). In the browser
prelude, `.Auto` placement maps to the CPU
(`Compiler/Prelude/Web/Core/ComputeWebGpu.js:525`), against
D-GPU-COMPUTE-LAUNCH1's "never an implicit CPU substitute".

Core math is C libm: `Compiler/JetRuntime/Source/CompilerNeededMath.jet:3-4`
says "Import that same C operation, never approximate transcendental
functions", and `:28` imports `sin`. Probe h5 prints
`math.sin(36.844070105782698)` as `-0.754598304262754` and
`math.cos(1.0e22)` as `0.523214785395139`, the same digits on every level.

The test machine has an AMD Radeon RX 7900 XTX (RADV NAVI31, Vulkan 1.4, seen
with `vulkaninfo`), so a real device exists for the cells below. What blocks
them is the compiler, not hardware.

## How peers do it

**Mojo.** A kernel is an ordinary function launched from the host with
`ctx.enqueue_function[vector_addition](…, grid_dim=1, block_dim=…)` and indexes
its data with `thread_idx.x`
(`Mojo/docs/site/code/tools/notebooks/vector_addition.mojo:28-81`, corpus
commit 85356f6562ed). The compiler builds the same source once for the host and
once per GPU target. Inside a kernel, `comptime if` on compile-target
predicates such as `is_nvidia_gpu()` is evaluated for the kernel's target,
while host-side `std.sys.info` checks describe the host
(docs.modular.com/mojo/manual/gpu/fundamentals; `stdlib/std/sys/info.mojo:1081`,
`:1111`, `:1839-1845`). Mojo's `sin` uses that split: on NVIDIA F32 it calls
the PTX instruction `sin.approx.ftz.f32`, on Apple GPUs `llvm.air.sin`, and
elsewhere `llvm.sin` (`stdlib/std/math/math.mojo:1749-1788`). Its `exp` uses
`exp2(x * log2e)` on GPUs for F16 and F32 and a Mojo polynomial on CPUs
(`math.mojo:609-658`). So Mojo's sine is portable in the sense that it compiles
everywhere, not in the sense that it gives the same digits everywhere.

**CUDA.** `sinf` is accurate to 2 ULP; the intrinsic `__sinf` has absolute
error 2^-21.41 on [-π, π] and is larger outside; `--use_fast_math` silently
replaces `sinf` with `__sinf`. NVIDIA states that functions outside IEEE 754
"may differ across different platforms, including between host, device, and
different device architectures", and that contraction is on by default (CUDA
Programming Guide v13.4.2, section 5.5, "Mathematical Functions").

**Metal.** The Metal Shading Language 4.1 specification sets
`-fmetal-math-fp32-functions` and `-fmetal-math-mode` both to `fast` by
default; `fast` allows reassociation and assumes no NaNs or infinities
(section 1.6.3, pages 15-16).

**WebGPU and WGSL.** WGSL states accuracy per built-in: F32 `sin` has
"absolute error at most 2^-11" on [-π, π], `exp` is 3 + 2·|x| ULP, division
2.5 ULP, and implementations may reassociate and fuse (W3C WGSL, sections
15.7.4 and 15.7.5).

**OpenCL C 3.0.** `sin` is within 4 ULP; `native_sin` is implementation
defined (Khronos OpenCL C 3.0 specification, section 7.4, "Relative Error as
ULPs").

**Triton.** `tl.dot` uses tensor cores in TF32 by default on NVIDIA, and the
documentation warns that F32 inputs "may be truncated to TF32 format (19-bit
floating point) without rounding" (triton-lang.org, `triton.language.dot`).

**SYCL 2020.** One C++ source holds host and device code ("single-source");
implementations compile it either with separate host and device passes (SMCP)
or one pass (SSCP) (Khronos SYCL 2020, sections 3.12.1-3.12.2).

**Halide.** The algorithm and its schedule are separate; the schedule picks
tiling, vectorizing and placement without changing the result
(halide-lang.org; Ragan-Kelley et al., PLDI 2013).

**Futhark.** Parallel work is written with second-order array combinators such
as `map` and `reduce`, which the compiler flattens for the GPU (Futhark
language reference, section 3.14.1).

**Correctly rounded math.** CORE-MATH provides correctly rounded functions and
checks univariate binary32 functions exhaustively
(core-math.gitlabpages.inria.fr). Float matrix units do not promise a plain
loop's rounding (Fasi, Higham, Mikaitis and Pranesh, "Numerical behavior of
NVIDIA tensor cores", PeerJ Computer Science 7:e330, 2021).

The pattern across peers: every vendor path lets the chip change the digits by
default (CUDA contraction, Metal fast math, Mojo per-target sine, Triton TF32).
Jet's ratified law already refuses that: D-COMPUTE-BACKEND1 makes strict and
reproducible the default and checks every device against the CPU oracle. That
is the one place Jet can clearly beat Mojo.

## One way to push code to the GPU

Ratified law already gives one mechanism with three depths. Nothing below adds
syntax beyond the one ballot.

**Beginner: tensor operations, no kernel.** The smallest GPU program uses
`core.compute` operations on tensors, with `.Auto` placement:

```jet
xs :: compute.from_list([1.0, 2.0, 3.0]) ?? panic("xs")
ys :: compute.from_list([4.0, 5.0, 6.0]) ?? panic("ys")
print(compute.to_list(compute.add(xs, ys)))
```

Placement picks a GPU when one is present and emits a receipt
(D-COMPUTE-PLACE1, D-AUTOPIN1); a CPU fallback must be named and "cannot
change … observable results". Today this program stops with ICE 101 (probe g3).

**Next step: a marked kernel and a launch.** The user marks an ordinary
function and launches it by name:

```jet
#Kernel(.parallel)
fn add(left: F32, right: F32) -> F32 { left + right }

out :: compute.launch(add, inputs: (left: xs, right: ys)) ?? panic("launch")
```

The same function stays callable on the CPU (probe g15), so a user can test it
with ordinary code first. Jet tuples name every member (S73, probe g2c), so the
inputs are named after the kernel's parameters. The ratified example in
D-GPU-COMPUTE-LAUNCH1 writes a positional `(xs, ys)`, which E0048 rejects; the
decision's example text needs the named form when `launch` lands. This is a
spelling fix under two ratified rules, not a new choice.

**Expert: indexed kernels, memory spaces, matrix units, raw tier.**

- Indexed writes and per-thread positions through `Invocation`
  (D-GPU-INDEXED-RESOURCE1); today E1102 (probe g9).
- Workgroup-shared memory, subgroups, atomics and tiles are the unratified
  D-GPU-GROUP/SUBGROUP/ATOMIC/TILE drafts and #3026 criteria 11-16. Each needs
  its own proof fact before sema admits it.
- Matrix units (tensor cores) are the one open owner choice, ballot
  `D-GPU-DEVICE-OPS1`. The recommendation: matrix units are library operations
  such as a tile multiply-add, each with a plain Jet body that defines its
  answer. Device libraries lower them to the unit when the chip has one and
  the precision profile allows it. Kernel code reads `$build.hardware` facts,
  which are the same for the CPU reference and every device build, but never
  asks which device is compiling it. Under the default strict profile, float
  matrix units are not used, because they round differently from the plain
  loop; a kernel opts in with a named profile recorded in the receipt.
  Integer units that sum 8-bit products into 32 bits are exact while the sums
  fit, which the tile shape bounds, so they can be used by default.
- Raw vendor code stays in `#Unsafe` with a provider contract
  (D-COMPUTE-RAWBOUNDARY1).

## Split compilation

Mojo compiles one source twice. Jet's ratified pieces already describe the same
split with a stronger check:

1. Sema proves a `#Kernel` and attaches one proof record before the IR is built
   (D-COMPUTE-KERNEL-SURFACE1).
2. The kernel lowers once to the public, versioned kernel IR (D-ACCEL-LIB1).
   The host part compiles through the ordinary pipeline at o0, o1 or o2; the
   build level changes only the host code, never the kernel IR, which keeps I9.
3. The CPU reference is always compiled from the same kernel IR. Device
   libraries (Vulkan, Metal, CUDA, and WebGPU in the browser,
   D-COMPUTE-BACKEND1) lower the IR at build time when the device is named, or
   at install or first launch when placement is `.Auto`. The cache key holds
   the kernel IR hash, the library version, the hardware facts used and the
   precision profile (D-HOST-CAPS1 "Cache identity includes kind/probe package
   identities").
4. Conformance compares each device with the CPU reference: bit-equal under
   the default profile, within the stated bound under a named looser one.

The difference from Mojo is in step 3: under the recommended option of
`D-GPU-DEVICE-OPS1`, the CPU reference compiles the very code each device runs,
so a test on any machine covers every device's branch.

## A portable kernel library

#4731 asks for elementwise maps, reductions, scan, radix sort, tiled matmul,
convolution, softmax and attention, written in Jet with CPU-oracle and vendor
gauntlet cells and no new syntax. Under the law above, each library entry is an
ordinary Jet `#Kernel` (or a Core instruction entry with a plain body). Two
design points follow from Reproducible being the default:

- A reduction's combine order must be fixed by the library, not by each
  device's workgroup size. A fixed-shape pairwise tree, with block size chosen
  by the library and mapped onto workgroups by each device library, gives the
  same bits on every device. Faster, reordered reductions need a named policy
  (D-COMPUTE-BACKEND1).
- Tile sizes come from `$build.hardware` facts as compile-time parameters; they
  change speed, not results, because the fixed combine order is part of the
  plain body.

This makes the library portable in the strong sense: same answers, with speed
tuned per chip.

## Portable math

Today `math.sin` is a C libm call (`CompilerNeededMath.jet:28`), so it cannot be
proved inside a kernel (probe g7, E1102) and its bits depend on the host's libm.
D-MATH-BITS1 (open, recommendation A) makes `core.math` Jet code returning
correctly rounded results on every platform, level and at compile time, with
F32 versions "once they exist". Under it, `math.sin` becomes a pure Jet helper.
D-GPU-HELPER-SCOPE1=A then admits it into kernels, and the GPU, the CPU and
compile-time evaluation return the same bits. That answers the owner's request
for "custom declarations … more portable and accurate" better than Mojo, whose
GPU sine is the vendor approximation (above).

Two findings for the D-MATH-BITS1 author before it is ratified:

- **Wording conflict.** D-MATH-BITS1 says no option or flag switches the math
  library. D-COMPUTE-BACKEND1 allows fast math under named recorded policies,
  and #4732 says faster versions come only through a named compute precision
  profile. The two must agree on whether a profile may change what `math.sin`
  returns, or whether a fast sine is a separately named function with a stated
  bound. Either reading keeps one meaning per name; mixing them does not.
- **GPU cost.** Correctly rounded F32 functions are usually built with F64
  internals (the CORE-MATH binary32 approach). Consumer GPUs run F64 far
  slower than F32, so the F32 kernels need float-float internals or a paired
  cell that shows the cost (cell G2 below).

## Paired performance cells

AGENTS.md requires performance claims to come as two programs measured side by
side. These cells are specified now and blocked until #5454 (tensor ICEs),
#3026 (launch and indexed kernels) and #5964 (device library) land.

**G1: beginner against expert, same answer.** Workload: `y = a * x + y` over
16,777,216 F32 values, 100 passes, then a checksum.

| Side | Program |
|---|---|
| Beginner | `compute.add(compute.mul(a, xs), ys)` with `.Auto` placement. |
| Expert | an indexed `#Kernel` with `Invocation`, launched with a pinned Vulkan device. |
| Reference | the CPU oracle for the same kernel. |
| Peer | the same loop as a hand-written GLSL compute shader through Vulkan on the RX 7900 XTX. |

Report kernel time and end-to-end time separately (#3028 criteria 8 and 11),
and assert equal checksums. The gate: the beginner path within 2x of the
expert path end to end, and the expert path within 1.25x of the GLSL peer for
kernel time.

**G2: portable sine.** Workload: `math.sin` over 16,777,216 F32 inputs spread
over [-1000, 1000], on the same device.

| Side | Program |
|---|---|
| Jet | a `#Kernel` calling `math.sin` (correctly rounded, D-MATH-BITS1=A). |
| Peer | GLSL `sin` on the same device, and the CPU reference. |

Report time and the largest ULP error of each against the correctly rounded
result. The gate: Jet's answers equal the CPU reference on every input, and the
cost against the vendor sine is published, not hidden.

A CPU-only paired cell was measured for #6345 and is in
`Docs/research/2026-10-30-portable-target-capabilities.md`; it shows the scalar
F32 loop that kernels lower from is today 29x slower than C at o2.

## Criteria and cards this note feeds

- #3026 criteria 10 and 17: `compute.launch` with named inputs, and helper
  calls in kernels (probes g2b, g11).
- #4731: the fixed-order reduction rule above, and cells G1 and G2.
- #4732 and D-MATH-BITS1: the wording conflict and the F32 GPU cost.
- #5454: the tensor operation ICEs (probes g3, g4, tm4k).
- The browser `.Auto` mapping to the CPU (`ComputeWebGpu.js:525`) needs a card
  if none exists.

## Ballot

- `D-GPU-DEVICE-OPS1`: how expert kernels reach a chip's matrix units with one
  meaning. Options: A, library operations with a plain meaning (recommended);
  B, kernels branch on the device being compiled, as Mojo does; C, raw vendor
  code only. Builder and JSON in `/mnt/jetscratch/lanes/MemDesign-2/ballot/`.
