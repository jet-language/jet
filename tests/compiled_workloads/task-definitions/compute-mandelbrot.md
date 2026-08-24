# Compute: Mandelbrot checksum

Build a deterministic Mandelbrot renderer/checksummer over a fixed 16,000 x
16,000 grid and iteration bound. Emit the checksum and elapsed-work count.
Run scalar and expert parallel modes with the same result. Include overflow,
NaN, cancellation, and forced-worker-failure cases. Compare release native,
cross-target, and default JIT where the peer supports them.
