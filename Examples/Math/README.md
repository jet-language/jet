# Math

Numbers, exact arithmetic, operators, randomness and numeric compute.

Run a lesson from the repository root, for example `jet run Examples/Math/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ApproxIntoFloat](ApproxIntoFloat.jet) — A whole number that meets a Float converts with an exact check, so a value no Float holds stops.
- [BitNot](BitNot.jet) — Integer bit_not flips bits; !
- [Combinatorics](Combinatorics.jet) — core.math.combinatorics generates over any element type; list methods cover the sequence work.
- [Constants](Constants/) — core.math constants: PI, E, TAU, INFINITY and NAN are comptime values, read as `math.PI`, never called.
- [Decimal](Decimal.jet) — Decimal literals use exact base-10 arithmetic.
- [ExpMinusOne](ExpMinusOne.jet) — 2^x - 1 and 10^x - 1, correctly rounded: the same bits on every tier, and full precision for small x, where exp2(x) - 1 cancels.
- [FloatExactOps](FloatExactOps.jet) — IEEE 754 exact operations: ties-to-even rounding, rounding into a fixed width, the IEEE remainder, signed-zero min and max, total order and NaN payloads.
- [FloorDiv](FloorDiv.jet) — Floor division: infix `/%` divides and rounds the answer down, and `/%=` divides a mutable binding in place.
- [Fraction](Fraction.jet)
- [HalfTurnTrig](HalfTurnTrig.jet) — sin, cos and tan of pi * x, correctly rounded. They give the same bits on every tier, and exact answers stay exact.
- [IntDiv](IntDiv.jet) — What `/` answers: the true quotient, always.
- [IntExact](IntExact.jet) — Default Int is exact, with a machine-word fast path and spill.
- [IntPowNegativeStop](IntPowNegativeStop.jet) — A negative runtime exponent stops; it never returns a sentinel.
- [InverseHalfTurnTrig](InverseHalfTurnTrig.jet) — asin, acos, atan and atan2 divided by pi, correctly rounded: angles in half-turns, with the same bits on every tier and exact angles exact.
- [MathAudit](MathAudit.jet) — core.math owns libm breadth, integer safety, and the remaining high-witness float/decompose gaps.
- [MixedTypes](MixedTypes.jet)
- [Modulo](Modulo.jet) — Two remainders.
- [MoreMath](MoreMath.jet)
- [NegatedDecimalLiteral](NegatedDecimalLiteral.jet) — Parentheses preserve exact Decimal literal negation.
- [NumericFloor](NumericFloor.jet) — Numeric floor flagship for Examples/Math/.
- [NumericMethods](NumericMethods.jet) — One numeric meaning on AOT, run, and compile time.
- [PowerOperator](PowerOperator.jet) — Power is a numeric method; ^ only hands over ownership.
- [PreludeMinMaxClamp](PreludeMinMaxClamp.jet) — `min`, `max` and `clamp` are prelude names over one ordered type, with no import.
- [RandomAudit](RandomAudit.jet)
- [RandomSeededSequence](RandomSeededSequence.jet)
- [ReciprocalSquareRoot](ReciprocalSquareRoot.jet) — 1/sqrt(x) correctly rounded, and powers of ten. Both give the same bits on every tier.
- [Spaceship](Spaceship.jet) — Primitive `<=>`, Ordering combinators, and a user comparator.
- [TypedAbs](TypedAbs.jet)
- [XorOperator](XorOperator.jet) — Bitwise exclusive-or: infix `~|` keeps a bit where exactly one side has it — the exclusive variant of `|`.

## Compute

- [ComputeAutoDiff](Compute/ComputeAutoDiff.jet)
- [ComputeCapabilities](Compute/ComputeCapabilities.jet)
- [ComputeDevice](Compute/ComputeDevice.jet)
- [ComputeKernel](Compute/ComputeKernel.jet)
- [ComputeLinalg](Compute/ComputeLinalg.jet)
- [ComputeNDArray](Compute/ComputeNDArray.jet)
- [ComputeSimd](Compute/ComputeSimd.jet)
- [ComputeTensor](Compute/ComputeTensor.jet)
- [ComputeVulkanWebGPU](Compute/ComputeVulkanWebGPU.jet)
