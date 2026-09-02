# Compute: Mandelbrot escape-time checksum

Run a direct binary64 Mandelbrot kernel over the declared grid. For each
stride-sampled coordinate, start with `z = 0` and `c = (3.5*x/width - 2.5,
2.0*y/height - 1.0)`, then apply `z = z*z + c` for `i = 0..<iterations`.
Return `i + 1` on the first post-update `|z|^2 > 4` escape, or the iteration
bound when the point remains bounded. Emit the sample count and the sum of
those counts modulo 1,000,000,007 as `samples=N` and `checksum=N`.

The normal fixture uses a fixed 16,000 x 16,000 grid, stride 64, and 64
iterations. Reject malformed, nonpositive, oversized, or sample-count-overflow
inputs before allocating or entering the kernel. Peer implementations must
use strict IEEE-754 binary64 arithmetic without fast-math or fused multiply
add contraction.

The hostile fixture is a named batch containing malformed, nonpositive,
oversized, and sample-count-overflow cases. Each rejected case emits the same
zero-result framing with its case name.
