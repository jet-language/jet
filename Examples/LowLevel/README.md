# LowLevel

Sized numbers, layout, SIMD, inline assembly, freestanding and embedded targets.

Run a lesson from the repository root, for example `jet run Examples/LowLevel/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [Cross](Cross.jet)
- [FixedOverflowModes](FixedOverflowModes.jet) — Every fixed-width overflow mode the IR carries, run in range so the program prints instead of stopping: default trapping operators (expression and compound…
- [Freestanding](Freestanding.jet) — Typed no-OS target example.
- [InlineAsm](InlineAsm.jet)
- [LayoutC](LayoutC.jet) — `#Layout(c, align(64))` — C-compatible struct layout with an explicit portable alignment.
- [LayoutColumnar](LayoutColumnar.jet) — `#Layout(columnar)` — cache-friendly struct-of-arrays.
- [LayoutOptimized](LayoutOptimized.jet) — A struct without `#Layout` is laid out by the compiler: its fields take offsets by descending alignment (declaration order among equals), packed at their…
- [LinalgSimd](LinalgSimd.jet) — Built-in linear-algebra and SIMD lane types.
- [LowLevel](LowLevel.jet)
- [MMIOBoardWrite](MMIOBoardWrite.jet) — MMIO-style board write.
- [OSTargetGating](OSTargetGating.jet)
- [PointerCastDeref](PointerCastDeref.jet) — Jai's compact cast+deref chain slot.value_pointer.(*Bool).* is rejected outright as a single expression.
- [SimdWide](SimdWide.jet) — Native AOT acceleration is the default for the complete portable lane family.
- [SizedFloats](SizedFloats.jet) — Sized-float math is precision-correct.
- [SizedIntegers](SizedIntegers.jet) — Fixed-width integer types for expert and FFI/binary work.
- [Swizzle](Swizzle.jet) — Named lane swizzles on vector/SIMD types.
- [UnsafeObligations](UnsafeObligations.jet) — The audited region states its reason and tracks the obligations that the safe checker cannot infer from an opaque pointer alone.

## Embedded

- [AllocatorFirmware](Embedded/AllocatorFirmware/) — Typed firmware witness for `board.sensor_v1`.
- [Battery](Embedded/Battery/) — Host replay of the embedded battery.
- [BrowserWasm](Embedded/BrowserWasm/) — Typed browser witness for `wasm.browser`.
- [HeapFreeCore](Embedded/HeapFreeCore/) — Typed target witness: heap-free Core on `wasm.no-os`.
- [HostedDefault](Embedded/HostedDefault/) — Typed hosted-default witness.
- [Wasi](Embedded/Wasi/) — Typed WASI witness for `wasm.wasi`.
