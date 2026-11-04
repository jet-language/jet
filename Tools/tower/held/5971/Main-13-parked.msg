Main-13: a fixed-width narrowing that does not fit stops with E3010, as from Int (#5971)

A `Type{x}` conversion from one fixed-width integer to another that cannot
hold every source value took the plain machine cast, so I32{I64}, U8{U16},
I8{I16}, U8{I32{-1}}, U32{I64{-1}} and I64{U64{max}} wrapped silently. The
same narrowing from an Int stops with E3010. Now the fixed-width source
widens to its exact Int and takes the same checked fixed conversion, so the
stop and its line are identical on o0, o1 and o2. Conversions the
destination always holds keep the plain cast.

The sign-crossing route also fixes U16{I8{5}}, which printed garbage high
bits (20997) on the old cast.

Proof: tests/cases/numeric_widening/fixed_narrowing_stops_like_int.case
covers every sign class plus the Int reference at --optimize=none, basic and
full. Examples/features/math/fixed_narrowing_stop.jet is the golden example
on o0, o1 and o2.
