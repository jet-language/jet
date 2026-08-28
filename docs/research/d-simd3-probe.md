# D-SIMD3 probe report

Date: 2026-08-27. Decision: D-SIMD3=B, card #2261.

The ratified D-SIMD1/2 operation set is constructor, `splat`, lane index,
element-wise `+ - * /`, named reductions, `reduce(.Add/.Mul/.Min/.Max/.Avg)`,
and `from_array`/`to_array`. The D-SIMD3 family adds `F32x8`, `F64x4`, and
signed/unsigned integer lanes at 128-bit and 256-bit widths.

| Operation | AOT | Cranelift JIT | TIR/interpreter |
|---|---|---|---|
| Constructor and `splat` | Prelude `jet_math_<T>_*` | `Math::jet_jit_math_call` | `MathLayout::construct` / `apply_static` |
| Element-wise arithmetic | `MathTaskMem::jet_lane_ops` | `Math::zip_binop` / `zip_int_binop` | `MathLayout::zip_op` / `zip_int_op` |
| Lane index | Prelude lane function | `Math::jet_jit_math_call` lane arm | `MathLayout::lane_at` |
| Reductions | Prelude reduction functions | `Math::reduce_op` / `reduce_int_op` | `MathLayout::reduce_op` / `reduce_int_op` |
| Fixed-list bridges | Prelude `from_array` / `to_array` | typed list marshalling | `MathLayout` list conversion |

The source witness is `examples/features/lowlevel/simd_wide.jet`, with its
golden output in `examples/features/expected/lowlevel/simd_wide.out`. The
foundation lane registry check passed with:

```text
scripts/agent/lane-check.sh -p jet-foundation
CHECK OK
```

The required workspace lane check remains blocked by unrelated dirty-tree
errors in `crates/jet-comptime/src/Comptime/Methods/core_calls.rs`, the
included `Prelude/Core/FSOps.rs`, `Prelude/CoreLib/JetStd/Collections.rs`, and
`Comptime/CorePureParity.rs`. Full AOT/JIT/interpreter runtime probes remain
unverified until that shared compile lane is green.
