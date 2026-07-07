# metal

Freestanding implementation slice. Current deterministic mode keeps the Life
core host-runnable while the file includes a gated UART `mem.volatile_write`
path for freestanding/codegen proof. `tests/slices.rs` checks
`jet build --freestanding`; QEMU/no-std symbol proof remains board-specific.
