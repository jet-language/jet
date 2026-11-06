# Math

Numbers, exact arithmetic, operators, randomness and numeric compute.

Run a lesson from the repository root, for example `jet run Examples/Math/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ApproxIntoFloat](ApproxIntoFloat.jet) — A whole number that meets a Float converts with an exact check, so a value no Float holds stops.
- [BitNot](BitNot.jet) — Integer bit_not flips bits; !
- [Combinatorics](Combinatorics.jet) — core.math.combinatorics generates over any element type; list methods cover the sequence work.
- [Constants](Constants/) — core.math constants: PI, E, TAU, INFINITY and NAN are comptime values, read as `math.PI`, never called.
- [Decimal](Decimal.jet) — Decimal literals use exact base-10 arithmetic.
- [FloorDiv](FloorDiv.jet) — Floor division: infix `/%` divides and rounds the answer down, and `/%=` divides a mutable binding in place.
- [Fraction](Fraction.jet)
- [IntDiv](IntDiv.jet) — What `/` answers: the true quotient, always.
- [IntExact](IntExact.jet) — Default Int is exact, with a machine-word fast path and spill.
- [IntPowNegativeStop](IntPowNegativeStop.jet) — A negative runtime exponent stops; it never returns a sentinel.
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
