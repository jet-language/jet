# Odin array programming audit (2026-11-16)

This audit compares Jet with Odin's built-in array programming, one feature per
row: fixed arrays, swizzles, enumerated arrays, matrices, complex numbers and
quaternions, SIMD vectors and SOA data. It answers #6177 criterion 1. Each row
gives the Odin feature, a probe written in today's Jet, what the probe
printed, a status, and the card criterion or owner ballot that owns the gap.

## 1. How to read it

**Source.** The Odin feature list is the Odin overview,
<https://odin-lang.org/docs/overview/>, read on 2026-11-16. Each section names
the overview headings it covers, and every feature those headings show has a
row.

**What "Jet today" means.** Each probe was run with `jet run` on dev
5bea729463, using the batch compiler built from that commit. The result column
quotes what the probe printed, or the first diagnostic code it reported.

**Status words.**

- **PASS:** the probe runs and prints what the Odin example prints.
- **PARTIAL:** Jet has the feature in a narrower form. The row says what is
  missing.
- **MISSING:** the probe is refused.
- **NOT ADOPTED:** a ratified decision gives Jet a different answer to the
  same need. The row names the decision.

**Links.** A row that is not PASS names an open card criterion, a ratified
ballot that is not built yet, or an open owner ballot. The open owner ballots
are listed in section 9. Under I7, a feature with no ratified spelling is not
built until the owner rules on it.

## 2. Fixed arrays

Odin headings: Fixed arrays, Array programming, Array casts, Slices.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| A1 | `[5]int{1, 2, 3, 4, 5}`, a stack array of fixed length | `a: [Int#5]{1, 2, 3, 4, 5}` | runs | PASS | — |
| A2 | `[?]int{…}`, length inferred from the literal | `a: [Int#_]{1, 2, 3}; print(a.len())` | `3` | PASS | — |
| A3 | `len(x)` | `a.len()` | `5` | PASS | — |
| A4 | constant index checked while compiling | `a: [Int#3]{1, 2, 3}; a[5]` | E0965 | PASS | — |
| A5 | runtime index checked by default | `a[i]` with `i = 7` | stop E3010 | PASS | — |
| A6 | `#no_bounds_check` block | `#no_bounds_check { print(a[1]) }` | E0927 | NOT ADOPTED | D-OOBPROOF1: Jet removes a check when it can prove the index in range, with no unchecked block |
| A7 | designated initializers `0 = …`, `3..=5 = …` | `[Int#8]{0: 1, 3..5: 9, else: 0}` | E0003 | MISSING | D-ARRAY-KEYED1=A (ratified, not built) |
| A8 | `a + b`, `a - b`, `a * b`, `a / b`, `a % b` between two `[N]T` | `a + b` on two `[Int#3]` | E0109 | MISSING | #6177 c2 |
| A9 | array op scalar, `a * 2` | `a * 2` on `[Int#3]` | E0109 | MISSING | #6177 c2 |
| A10 | unary minus, `-a` | `-a` on `[Int#3]` | E0109 | MISSING | #6177 c2 |
| A11 | `a == b`, `a != b` | on two `[Int#3]` | `true`, `false` | PASS | — |
| A12 | bit operators `&`, `\|`, `~`, `&~` between arrays | `a.bit_and(b)` on `[Int#3]` | E0311 | MISSING | #6177 c2 |
| A13 | compound assignment, `a += b` | `a := …; a += b` on `[Int#3]` | E0109 | MISSING | #6177 c2 (follows from A8) |
| A14 | nested arrays, `[2][2]i32` | `[[Int#2]#2]{…}; m[1][0]` | `3` | PASS | — |
| A15 | array cast, `cast([4]f32)a`, applied to each element | `[Float#4]{a}` | E0963; `a.map(…)` gives a `[Float]` list, not `[Float#4]` | PARTIAL | D-ARRAY-CONVERT1 (section 9, B1) |
| A16 | slicing a fixed array, `a[1:4]` | `a[1..<4]` | `[1, 1, 2]` | PASS | — |
| A17 | iterating, `for x in a` | `loop x in a -> print(x)` | `1 2 3` | PASS | — |
| A18 | a fixed array passed where a slice is taken | `total(a)` with `total: fn(xs: [Int])` | `6`; the array is copied into a list (Examples/Collections/FixedArrays.jet) | PARTIAL | D-ARRAY-WINDOW1=A (ratified, not built: the array should be passed as a read window) |
| A19 | index type may be any integer, character or enum type | `[Int#NodeIX]` with `NodeIX: distinct 0..<4` | E0003 | MISSING | D-ARRAY-INDEX1=A (ratified, not built) |
| A20 | `[dynamic; N]T`, a dynamic array with fixed capacity | `[Int#0..8]{}` | E0003 | MISSING | D-ARRAY-CAPACITY1=A (ratified, not built) |

## 3. Swizzles

Odin heading: Swizzle operations. Odin gives every array of length 4 or less
the fields `xyzw` and `rgba`, and has a `swizzle(a, i, …)` builtin for any
length.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| W1 | `.x`, `.zyx` read on a fixed array | `a.x`, `a.zyx` on `[Float#3]` | E0302 | MISSING | #6177 c3 |
| W2 | `xyzw` read on a vector type | `v.yz`, `col.wzyx` on `Vec3`, `Vec4` | `[2.0, 3.0]`, `[1.0, 0.2, 0.5, 1.0]` | PASS | — |
| W3 | swizzle write | `pos.xy = Vec2(0.0, 1.0); pos.z = 5.0` on `Vec3` | `[0.0, 1.0, 5.0]` | PASS | — |
| W4 | repeated lanes in a read | `v.xx` on `Vec3` | `[1.0, 1.0]` | PASS | — |
| W5 | `rgba` names | `col.r`, `col.rgb` on `Vec4` | E0302 | MISSING | #6177 c3 |
| W6 | swizzle write on a fixed array | `a.xy = …` on `[Float#3]` | E0302 (as W1) | MISSING | #6177 c3 |
| W7 | swizzles on every lane type | `i.x` on `I32x4` | E0302; only `F32x4` and `F64x2` swizzle | PARTIAL | #6177 c3 |
| W8 | `swizzle(a, 2, 1, 0)`, by index, any length | `a.swizzle(2, 1, 0)` on `[Float#3]` | E0311 | MISSING | D-ARRAY-SHUFFLE1 (section 9, B2) |
| W9 | overlapping write refused | `v.xx = …` (Tests/UI/swizzle_overlap_write.jet) | E3111 | PASS | — |

## 4. Enumerated arrays

Odin heading: Enumerated array.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| E1 | `[Direction][2]int{.North = …}`, one slot per enum case | `[Int#Direction]{10, 20, 30, 40}; t[.East]` | E0963 | MISSING | D-ARRAY-ENUMTABLE1=A (ratified, not built) |
| E2 | `#partial`, leaving some cases zero | keyed literal with `else:` | E0003 (as A7) | MISSING | D-ARRAY-KEYED1=A; Jet has no silent zero, so the gaps are filled with `else:` |
| E3 | `#sparse`, an enum with gaps in its values | `[Int#Code]{1, 2}` with cases `A = 1`, `C = 5` | E0963 | MISSING | D-ARRAY-ENUMTABLE1=A: one slot per case in case order, whatever the case values |

## 5. Matrices

Odin headings: matrix type, Submatrix casting, Casting non-square matrices,
Technical information of matrix types. Jet has two matrix families today:
`Mat2`, `Mat3` and `Mat4` in core.math (Float, column-major, fixed size), and
the heap `Matrix<M, N>` of core.compute, whose shape is checked while
compiling. Examples/Math/LinearAlgebra.jet shows the fixed family.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| M1 | `matrix[R, C]T` of any size up to 64 elements, with integer, float or complex elements | `Mat2`, `Mat3`, `Mat4`; `compute.matrix(3, 4, 1.0)` | runs; Float only, and fixed sizes only 2x2, 3x3 and 4x4 | PARTIAL | D-ARRAY-MAT21=A (the three become `Matrix<N, N>`); element types: D-ARRAY-MATOPS1 (section 9, B9) |
| M2 | 2x2 matrices | `Mat2.new(0.0, 1.0, -1.0, 0.0)` | runs; products, transpose, determinant and inverse | PASS | — |
| M3 | matrix literal | `Mat3(1.0, …, 9.0)`, column-major | runs | PASS | — |
| M4 | `m[1, 2]`, row and column index | `m[1, 2]` on `Mat3` | E0003 | MISSING | D-MATRIX-INDEX1=E (ratified, not built) |
| M5 | a scalar acts as a scaled identity; identity matrix | `Mat3.identity()`, `m * 2.0` | identity runs on `Mat2`, `Mat3`, `Mat4`; `m * 2.0` is E2511 | PARTIAL | D-ARRAY-MATOPS1 (section 9, B9: the scalar scale) |
| M6 | matrix times matrix | `m * m` on `Mat3` | `[30.0, 36.0, …, 150.0]` | PASS | — |
| M7 | matrix times column vector | `m * Vec3(1.0, 0.0, 0.0)` | `[1.0, 2.0, 3.0]` | PASS | — |
| M8 | row vector times matrix, `v * m` | `Vec3(…) * m` | E2511 | MISSING | D-MATRIX-BCAST1=E (ratified, not built: `v.as_row() * m`) |
| M9 | `a + b`, `a - b` | `m + m` on `Mat3` | `[2.0, 4.0, …, 18.0]` | PASS | — |
| M10 | `&`, `\|`, `~`, `&~` on integer matrices | `Mat3(1, 2, …, 9)` | builds a Float matrix; there are no integer matrices | MISSING | D-ARRAY-MATOPS1 (section 9, B9) |
| M11 | `hadamard_product(a, b)` | `m.hadamard(m)` | E0102 | MISSING | D-MATRIX-MUL1=F (ratified, not built: `a .* b`) |
| M12 | `transpose(m)` | `m.transpose()` | `[1.0, 4.0, 7.0, …]` | PASS | — |
| M13 | `outer_product(a, b)` | `a.outer(a)` on `Vec3` | E0102 | MISSING | D-ARRAY-MATOPS1 (section 9, B9) |
| M14 | `matrix_flatten(m)`, column-major | `m.to_array()` | the 9 floats in column order | PASS | — |
| M15 | `determinant`, `adjugate`, `inverse`, `inverse_transpose`, `hermitian_adjoint`, `matrix_trace`, `matrix_minor` | `m.determinant()`, `m.inverse()` on `Mat2`, `Mat3`, `Mat4`; core.compute `det`, `inv` | `determinant` and `inverse` run, `inverse` fails with `MatrixError.Singular` (D-ARRAY-INVERSE1); the other five are E0102 | PARTIAL | D-ARRAY-MATOPS1 (section 9, B9) |
| M16 | `conj(m)` for complex elements | `m.conj()` on `Mat3` | E0102; there are no complex matrices | MISSING | D-ARRAY-CONJ1 and D-ARRAY-COMPLEXT1 (section 9, B5 and B6) |
| M17 | cast to a smaller or larger square matrix | `Mat4{m}` with `m: Mat3` | E0303 | MISSING | D-ARRAY-MATRESIZE1 (section 9, B3) |
| M18 | cast between non-square shapes with the same element count | `reshape` in Core/Compute/Compute.jet | heap tensors only; nothing for `Mat3` or `Mat4` | PARTIAL | D-ARRAY-MATRESIZE1 (section 9, B3) |
| M19 | `#row_major` storage | `Mat3.from_rows(…)` | E0311; column-major only | MISSING | D-ARRAY-ROWMAJOR1 (section 9, B4) |

## 6. Complex numbers and quaternions

Odin heading: Complex numbers and quaternions.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| Q1 | complex type and `+ - * /` | `c: Complex(3.0, 4.0); c * c` | `-7 + 24i` | PASS | — |
| Q2 | `abs(c)` | `c.abs()` | `5.0` | PASS | — |
| Q3 | `real(c)`, `imag(c)` | `c.real`, `c.imaginary` | `3.0`, `4.0` | PASS | — |
| Q4 | `conj(c)` | `c.conj()` | E0102 | MISSING | D-ARRAY-CONJ1 (section 9, B5: the method name) |
| Q5 | imaginary literal with a float, `3.0 + 4.0i` | `c := 3.0 + 4.0i` | E0133: `+` between Decimal and Complex | MISSING | #4730 c5 |
| Q6 | complex32, complex64 and complex128 widths | `Complex<F32>(3.0, 4.0)` | E0119: `Complex` is Float only | PARTIAL | D-ARRAY-COMPLEXT1 (section 9, B6) |
| Q7 | quaternion type, `1 + 2i + 3j + 4k` | `Quaternion(1.0, 0.0, 0.0, 0.0)` | E0102 | MISSING | D-ARRAY-QUAT1=A; #4730 c1 |
| Q8 | quaternion arithmetic, and rotating a vector | `q * q` | E0102 (no type) | MISSING | #4730 c1, c2 |
| Q9 | `q.x`, `q.w`, `q.xyz` read and write | `q.xyz` | E0102 (no type) | MISSING | D-ARRAY-QUAT1=A; #4730 c1 |
| Q10 | `quaternion_angle_axis`, `quaternion_slerp`, `quaternion_mul_vector3` | `Quaternion.from_axis_angle(…).rotate(v)` | E0107 (no type) | MISSING | #4730 c2 |

## 7. SIMD vectors

Odin heading: SIMD vector types. Jet's lane types are a fixed family in
core.math (`F32x4`, `F32x8`, `F64x2`, `F64x4`, and integer lanes from `I8x16`
to `U64x4`).

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| S1 | `#simd[4]f32`, lane-wise `+ - * /` | `a * b + a` on `F32x4` | `[11.0, 22.0, 33.0, 44.0]` | PASS | — |
| S2 | integer lanes, lane-wise arithmetic | `a + a`, `a * a` on `I32x4` | `[2, 4, 6, 8]`, `[1, 4, 9, 16]` | PASS | — |
| S3 | a scalar broadcast to every lane, `a * 2` | `a * 2.0` on `F32x4`; `F32x4.splat(10.0)` | E2511; `splat` runs | PARTIAL | D-ARRAY-LANEBCAST1 (section 9, B7) |
| S4 | `simd.extract(c, 2)` | `c[2]` | `33.0` | PASS | — |
| S5 | `simd.replace` | `a.x = F32{9.0}` on `F32x4` | `[9.0, 2.0, 3.0, 4.0]` | PASS | — |
| S6 | `simd.reduce_add_ordered` and the other reductions | `c.sum()`, `c.reduce(.Max)` | `110.0`, `4.0` | PASS | — |
| S7 | `simd.to_array` | `c.to_array()` | `[11.0, 22.0, 33.0, 44.0]` | PASS | — |
| S8 | `simd.lanes_lt` and the other lane-wise comparisons | `a.lanes_lt(b)` | E0102 | MISSING | D-ARRAY-LANESCMP1=C (ratified, not built) |
| S9 | `simd.select(mask, a, b)` | `m.select(a, b)` | E0102 (no mask) | MISSING | D-ARRAY-LANESCMP1=C (the `Lanes<Bool, N>` mask) |
| S10 | lane-wise conversion, `cast(#simd[4]i32)a` | `a.to_i32x4()` | E0102 | MISSING | D-ARRAY-CONVERT1 (section 9, B1, with A15) |
| S11 | `swizzle` and `simd.shuffle` on lanes | `a.wzyx` on `F32x4` | `[4.0, 3.0, 2.0, 1.0]`; not on integer lanes | PARTIAL | #6177 c3 |
| S12 | shifts and bit operators on integer lanes | `a << 1` on `I32x4` | E0109 | MISSING | #6177 c6 |
| S13 | unary minus on lanes and vectors | `-a` on `F32x4`, `-v` on `Vec3` | E0109 | MISSING | #6177 c2 (no new spelling: see section 9) |
| S14 | lane-wise `/` on vectors | `a / b` on `Vec3` | E2511 | MISSING | #6177 c2 (no new spelling: see section 9) |
| S15 | any power-of-two width up to 64; integer, float, Bool or rawptr lanes | `F32x16.splat(1.0)` | E0107; a fixed family, no Bool lanes, nothing wider than 32 lanes | PARTIAL | #4167 (D-LANES1) |
| S16 | lanes mapped to the target's SIMD registers | Examples/LowLevel/LinalgSIMD.jet | native instructions on the common float widths, a scalar loop elsewhere (D-SIMD3=B) | PARTIAL | #4167 c2 |

## 8. SOA data types

Odin headings: SOA data types, SOA struct arrays, SOA struct slices and
dynamic arrays, SOA pointers, soa_zip and soa_unzip. Jet today has
`#Layout(columnar)` on a struct, which stores every list of that struct one
column per field.

| # | Odin feature | Jet probe | Result | Status | Owner |
|---|---|---|---|---|---|
| O1 | `#soa[N]T`, chosen per collection | `#Layout(columnar)` on the struct | runs; chosen per struct, not per collection | PARTIAL | #6314 c12, c16 (D-SOA-SITE1=A, `Columns<T>`) |
| O2 | `v[0].x` and `v.x[0]` give the same place | `ps[1].x` runs; `ps.x` is E0302 | E0302 | MISSING | D-ARRAY-COLUMNS1=A; #6314 c16 |
| O3 | SOA slices and dynamic arrays (`append`, `pop`, `len`, `cap`) | `push`, `len`, index and `loop` on a columnar list (Examples/LowLevel/LayoutColumnar.jet) | runs | PASS | — |
| O4 | SOA pointers, `p := &v[2]; p.x = 123`, and field writes | `ps[1].x = 7` and `bump(&ps[1])` on a columnar list | E1108 (field writes are not supported yet), E0003 | MISSING | #6314 c7 (element references into columns), c16 |
| O5 | `soa_zip` and `soa_unzip` | `zip_columns(xs, ys)` | E0102 | MISSING | D-ARRAY-ZIPCOLUMNS1 (section 9, B8) |
| O6 | an SOA of `[3]f32` keeps the `xyzw` fields | `ps[0].pos.x` with `pos: [Float#3]` | E0302 | MISSING | #6177 c3, then #6314 c16 |

## 9. Owner ballots

These rows need a spelling the owner has not ratified. Each has an open owner
ballot and is not built until the owner rules on it (I7).

| Ballot | Decision | Rows | Question |
|---|---|---|---|
| B1 | D-ARRAY-CONVERT1 | A15, S10 | How is an array or a lane converted element by element: a method (`a.to<F32>()`), a typed literal (`[Float#4]{a}`), or a conversion function? |
| B2 | D-ARRAY-SHUFFLE1 | W8 | Is there an index swizzle for any length (`a.swizzle(2, 1, 0)`), or do named lanes for length 4 or less stay the only form? |
| B3 | D-ARRAY-MATRESIZE1 | M17, M18 | How is a square matrix cast to a smaller or larger one, and how is a matrix reshaped to another shape with the same element count? |
| B4 | D-ARRAY-ROWMAJOR1 | M19 | Is row-major storage offered, and how is it chosen? |
| B5 | D-ARRAY-CONJ1 | Q4 | Is the complex conjugate `c.conj()` or `c.conjugate()`? |
| B6 | D-ARRAY-COMPLEXT1 | Q6 | Does `Complex` take an element type (`Complex<F32>`), or does it stay Float only? |
| B7 | D-ARRAY-LANEBCAST1 | S3 | Does a scalar operand broadcast across the lanes of a lane type (`a * 2.0`), as it already does for `Vec3 * Float`? |
| B8 | D-ARRAY-ZIPCOLUMNS1 | O5 | Is there a zip of separate lists into one columnar view, and an unzip back? |
| B9 | D-ARRAY-MATOPS1 | M1, M5, M10, M13, M15 | Which of Odin's other matrix built-ins does the fixed family name, and how: a scalar scale (`m * 2.0`), integer elements with bit operators, `outer`, `trace`, `adjugate`, `minor` and `inverse_transpose`? |

S13 and S14 need no new spelling: they extend `-` and `/` to vectors and
lanes, which D-VECARITH1 already allows for this family. They belong in a
#6177 criterion beside c2.

## 10. Counts

Of the 83 rows: 30 PASS, 13 PARTIAL, 39 MISSING and 1 NOT ADOPTED. The card
log of 2026-10-29 recorded swizzle writes as failing on every tier. They now
run (W3, S5).

## Finding dispositions

Each row that is not PASS is a finding. A row that waits on an open owner
ballot in section 9 is tracked by #6177, whose plan sends every new spelling
to an owner ballot before it is built.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| A6 | decision | D-OOBPROOF1 |
| A7 | decision | D-ARRAY-KEYED1 |
| A8 | card | #6177 |
| A9 | card | #6177 |
| A10 | card | #6177 |
| A12 | card | #6177 |
| A13 | card | #6177 |
| A15 | card | #6177 (ballot B1, D-ARRAY-CONVERT1) |
| A18 | decision | D-ARRAY-WINDOW1 |
| A19 | decision | D-ARRAY-INDEX1 |
| A20 | decision | D-ARRAY-CAPACITY1 |
| W1 | card | #6177 |
| W5 | card | #6177 |
| W6 | card | #6177 |
| W7 | card | #6177 |
| W8 | card | #6177 (ballot B2, D-ARRAY-SHUFFLE1) |
| E1 | decision | D-ARRAY-ENUMTABLE1 |
| E2 | decision | D-ARRAY-KEYED1 |
| E3 | decision | D-ARRAY-ENUMTABLE1 |
| M1 | card | #6177 (D-ARRAY-MAT21; ballot B9, D-ARRAY-MATOPS1) |
| M4 | decision | D-MATRIX-INDEX1 |
| M5 | card | #6177 (ballot B9, D-ARRAY-MATOPS1) |
| M8 | decision | D-MATRIX-BCAST1 |
| M10 | card | #6177 (ballot B9, D-ARRAY-MATOPS1) |
| M11 | decision | D-MATRIX-MUL1 |
| M13 | card | #6177 (ballot B9, D-ARRAY-MATOPS1) |
| M15 | card | #6177 (ballot B9, D-ARRAY-MATOPS1) |
| M16 | card | #6177 (ballots B5 and B6, D-ARRAY-CONJ1, D-ARRAY-COMPLEXT1) |
| M17 | card | #6177 (ballot B3, D-ARRAY-MATRESIZE1) |
| M18 | card | #6177 (ballot B3, D-ARRAY-MATRESIZE1) |
| M19 | card | #6177 (ballot B4, D-ARRAY-ROWMAJOR1) |
| Q4 | card | #6177 (ballot B5, D-ARRAY-CONJ1) |
| Q5 | card | #4730 |
| Q6 | card | #6177 (ballot B6, D-ARRAY-COMPLEXT1) |
| Q7 | decision | D-ARRAY-QUAT1 |
| Q8 | card | #4730 |
| Q9 | decision | D-ARRAY-QUAT1 |
| Q10 | card | #4730 |
| S3 | card | #6177 (ballot B7, D-ARRAY-LANEBCAST1) |
| S8 | decision | D-ARRAY-LANESCMP1 |
| S9 | decision | D-ARRAY-LANESCMP1 |
| S10 | card | #6177 (ballot B1, D-ARRAY-CONVERT1) |
| S11 | card | #6177 |
| S12 | card | #6177 |
| S13 | card | #6177 |
| S14 | card | #6177 |
| S15 | card | #4167 |
| S16 | card | #4167 |
| O1 | card | #6314 |
| O2 | decision | D-ARRAY-COLUMNS1 |
| O4 | card | #6314 |
| O5 | card | #6177 (ballot B8, D-ARRAY-ZIPCOLUMNS1) |
| O6 | card | #6177 |
<!-- /audit-dispositions -->
