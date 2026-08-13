# Matrix and linear algebra surface

Status: design record for card #1437. The surface decisions are ratified. This card
adds no implementation.

Foundation: **D-TYPE2-MEASURE1=A** (ratified 2026-08-06) and
[`type-system-v2-carriers-and-knowledge.md`](type-system-v2-carriers-and-knowledge.md).
That decision gives Jet one substrate for every compile-time number in a type. It
covers list lengths, lane counts, dimension exponents, and matrix sides. Every
archetype in this document uses `Matrix<M, N>` and `Vec<N>` on that substrate. No
second shape-encoding mechanism appears here. The measure proposal states that this
substrate gives card #1437 its foundation without deciding its syntax.

All Jet snippets in this proposal are illustrative. They are not tested until the
implementation cards land.

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Executive summary

Jet has no general matrix value or matrix literal today. `core.compute` exposes
ranked `Tensor<T>` storage through functions, and `core.linalg` has a small fixed
family such as `Vec2/3/4` and `Mat3/Mat4`. This proposal supplies the missing surface:
shape-checked `Matrix<M, N>` and `Vec<N>`, grid literals, matrix and cell arithmetic,
broadcasting, indexing, windows, and named system solves.

The owner has ratified six surface decisions:

| Area | Decision | Result |
|---|---|---|
| Matrix multiplication | `D-MATRIX-MUL1=F` | `Matrix * Matrix` composes maps; `Matrix .* Matrix` selects cell arithmetic. |
| Literals | `D-MATRIX-LIT1=E` | A line break or `;` ends a row. |
| Broadcasting | `D-MATRIX-BCAST1=E` | Extent `1` repeats along that axis. |
| Indexing | `D-MATRIX-INDEX1=E` | Brackets handle cells, bands, and windows; `.row` and `.col` name axes. |
| Solving | `D-MATRIX-SOLVE1=B` | `.solve` answers square systems; `.least_squares` answers tall systems. |
| Availability | `D-MATRIX-HOME1=D` | Basic matrix use is in the Prelude; advanced linear algebra is in `core.linalg`. |

No owner choice remains open in this proposal. The decision table at the end is the
ballot slate already ratified for card #1437. No Tower write is part of this worktree.

## Worked example

The example is a least-squares fit over the first eight rows of R's `cars` data:

| speed | 4 | 4 | 7 | 7 | 8 | 9 | 10 | 10 |
|---|---|---|---|---|---|---|---|---|
| distance | 2 | 10 | 4 | 22 | 16 | 10 | 18 | 26 |

The task is `distance = intercept + slope * speed`. Every archetype builds `x` as
`Matrix<8, 2>`, `y` as `Matrix<8, 1>`, solves the fit, reports mean squared error,
and predicts distance at 12 mph. The expected values are intercept `-3.4232`, slope
`2.2947`, mean squared error `36.5047`, and prediction `24.1129` feet.

## Ratified surface

### Shape types and storage

`Matrix<M, N>` is a shape-typed matrix with `M` rows and `N` columns. `Vec<N>` is the
public shape type for a vector. Their `M`, `N`, and `N` values are measures under
D-TYPE2-MEASURE1.
The checker rejects a matrix product when its inner sides differ and computes the
outer sides when they match:

~~~
a :: Matrix<3, 4>
b :: Matrix<4, 2>
c :: a * b                         // Matrix<3, 2>
bad :: a * Matrix<3, 2>             // rejected: 4 does not match 3
~~~

`Tensor<T>` owns ranked storage. `Vec<N>` and `Matrix<M, N>` share that substrate and
cross it without a copy (D-COMPUTE-TYPE1=D). Stored elements use the ratified
column-major layout. The fixed graphics family remains an alias family over the
general substrate.

### Literals

A row ends at a source line break or at `;`. The type carries the expected shape, so
the reader does not count a flat list by eye.

~~~
tall :: Matrix<3, 2>.{
    1, 4
    1, 7
    1, 10
}

small :: Matrix<1, 2>.{ 1, 12 }
same :: Matrix<2, 2>.{ 1, 0; 0, 1 }
~~~

The grid form works in a tall declaration. The one-line form works inside a call.
Both forms carry one shape and one literal idea.

### Matrix multiplication and cell arithmetic

`Matrix * Matrix` is matrix multiplication. It matches inner measures and composes
outer measures. A leading dot selects cell arithmetic on a `Matrix`; it is legal only
there:

~~~
pose :: Matrix<3, 3>
spin :: Matrix<3, 3>
map :: pose * spin                   // Matrix<3, 3>, map composition
cells :: pose .* spin                // Matrix<3, 3>, cell by cell
~~~

`Grid` and lane vectors already multiply cell by cell under `*` (D-VECARITH1). A dot
on a surface that is already cell by cell is refused, so one operation has one
spelling. Matrix power uses `^`; repeated matrix multiplication is the meaning of
`m ^ 3`. A leading dot selects cell power where the Matrix surface permits it.

The other scalar operators keep their ratified meanings. `/` follows Float division;
`Int / Int` gives a Float; `Int` is arbitrary precision. `/%` rounds division down,
`%` is floored modulo, and `%%` is truncated remainder. Lifting those operations to
cells never changes the scalar rule. `==` returns one `Bool` for a math value; a
cell-by-cell comparison is a named operation.

### Broadcasting

An axis of extent `1` repeats silently. The value must state its direction before it
meets a matrix:

~~~
table :: Matrix<8, 2>
mean :: Matrix<1, 2>.{ 6.0, 1.0 }       // row broadcast
centred :: table - mean                 // Matrix<8, 2>

speed :: Vec<8>
column :: speed.as_column()             // Matrix<8, 1>
row :: speed.as_row()                   // Matrix<1, 8>
~~~

An operation that would repeat both operands into a larger result is rejected. Its
diagnostic names `.as_row()` or `.as_column()` as the explicit spelling that resolves
the direction.

### Indexing, slicing, and windows

Brackets handle precise access. `.row(i)` and `.col(j)` are the readable names for
whole axes. A scalar index drops its axis; a range keeps its axis.

~~~
cell :: table[2, 1]                    // Float
row :: table.row(2)                    // row view
column :: table.col(1)                 // column view
band :: table[0..<4, ..]               // Matrix<4, 2> window
one_row :: table[2, ..]                // one axis dropped
~~~

The existing place rule controls ownership. A bare place is a read window, `&place`
is an exclusive write window, and `~place` is an owned copy (D-SHAPE-PLACE1=A):

~~~
speeds :: table[.., 1]                 // read window; no copy
&table[.., 1] *= 2.0                   // write window; in place
kept :: ~table[0..<4, ..]              // owned Matrix<4, 2>
~~~

Layout stays column-major by default. An expert can select the existing layout
marker for a storage-boundary type. Views and in-place edits do not introduce a
second matrix mechanism.

### Solving and the home boundary

`x.solve(y)` answers a square system. `x.least_squares(y)` answers a tall system and
returns the closest fit. The method says which question was asked; a reader does not
need to infer it from the shape.

Types, literals, indexing, transpose, `+`, `-`, `*`, and reductions need no import.
Solving, determinants, decompositions, and transforms live in `core.linalg`, in the
same way that `sqrt` lives in `core.math`.

## Archetype 1 — Beginner table fit

The beginner starts with two vectors. The library builds the design matrix, and the
named solve states the fit question. The shape comments show the measure carried by
each step.

~~~
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    distance :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    x :: Matrix.columns(la.ones<8>(), speed)       // Matrix<8, 2>
    y :: distance.as_column()                      // Matrix<8, 1>
    beta :: x.least_squares(y) ?? panic("no fit")  // Matrix<2, 1>

    residual :: y - x * beta                       // Matrix<8, 1>
    mse :: (residual .* residual).mean()           // Float: 36.5047
    query :: Matrix<1, 2>.{ 1, 12 }
    prediction :: query * beta                     // Matrix<1, 1>: 24.1129

    print("intercept {beta[0, 0]} slope {beta[1, 0]}")
    print("mse {mse}")
    print("at 12 mph {prediction[0, 0]}")
}
~~~

Output: intercept `-3.4232`, slope `2.2947`, mean squared error `36.5047`, and
prediction `24.1129` feet.

## Archetype 2 — Analyst with the formula in view

The analyst writes the same data as two shape-typed matrices. The normal equations
make the square solve visible. This is longer than the beginner path but matches the
formula used in a notebook or a textbook.

~~~
use core.linalg

fn run() {
    x :: Matrix<8, 2>.{
        1, 4
        1, 4
        1, 7
        1, 7
        1, 8
        1, 9
        1, 10
        1, 10
    }
    y :: Matrix<8, 1>.{
        2
        10
        4
        22
        16
        10
        18
        26
    }

    xt :: x.t                                    // Matrix<2, 8>
    normal :: xt * x                              // Matrix<2, 2>
    rhs :: xt * y                                 // Matrix<2, 1>
    beta :: normal.solve(rhs) ?? panic("singular") // Matrix<2, 1>

    residual :: y - x * beta                      // Matrix<8, 1>
    mse :: (residual .* residual).mean()          // Float: 36.5047
    query :: Matrix<1, 2>.{ 1, 12 }
    prediction :: query * beta                    // Matrix<1, 1>: 24.1129

    print("intercept {beta[0, 0]} slope {beta[1, 0]}")
    print("mse {mse}")
    print("at 12 mph {prediction[0, 0]}")
}
~~~

The output is the same: intercept `-3.4232`, slope `2.2947`, mean squared error
`36.5047`, and prediction `24.1129` feet.

## Archetype 3 — Data analyst with broadcast and labels

The data analyst keeps table operations visible. The row-shaped offset broadcasts
over the eight observations. The analyst reads the speed column by name before
solving the same fit.

~~~
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    distance :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    raw :: Matrix.columns(la.ones<8>(), speed)       // Matrix<8, 2>
    offset :: Matrix<1, 2>.{ 0, 0 }                 // Matrix<1, 2>
    x :: raw + offset                                // Matrix<8, 2>, broadcast
    speed_column :: x.col(1)                         // column view of Matrix<8, 2>
    y :: distance.as_column()                        // Matrix<8, 1>

    beta :: x.least_squares(y) ?? panic("no fit")    // Matrix<2, 1>
    residual :: y - x * beta                         // Matrix<8, 1>
    mse :: (residual .* residual).mean()             // Float: 36.5047
    query :: Matrix<1, 2>.{ 1, 12 }
    prediction :: query * beta                       // Matrix<1, 1>: 24.1129

    print("intercept {beta[0, 0]} slope {beta[1, 0]}")
    print("first speed {speed_column[0]}")
    print("mse {mse}")
    print("at 12 mph {prediction[0, 0]}")
}
~~~

The added column read does not change the fit. Output remains intercept `-3.4232`,
slope `2.2947`, mean squared error `36.5047`, and prediction `24.1129` feet.

## Archetype 4 — Systems engineer with views

The systems engineer keeps the same typed calculation but controls storage. The
window edit is explicit, and the detached sample window owns its data. The layout
marker remains an expert storage choice, not a new matrix type.

~~~
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    distance :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    x :: Matrix.columns(la.ones<8>(), speed)       // Matrix<8, 2>
    y :: distance.as_column()                      // Matrix<8, 1>
    speed_view :: x[.., 1]                         // Matrix<8, 1> read view
    sample :: ~x[0..<4, ..]                        // Matrix<4, 2> owned copy
    &x[.., 1] *= 1.0                               // write view; same values

    beta :: x.least_squares(y) ?? panic("no fit")  // Matrix<2, 1>
    residual :: y - x * beta                       // Matrix<8, 1>
    mse :: (residual .* residual).mean()           // Float: 36.5047
    query :: Matrix<1, 2>.{ 1, 12 }
    prediction :: query * beta                     // Matrix<1, 1>: 24.1129

    print("intercept {beta[0, 0]} slope {beta[1, 0]}")
    print("first speed {speed_view[0]} sample rows {sample.rows}")
    print("mse {mse}")
    print("at 12 mph {prediction[0, 0]}")
}
~~~

The storage controls do not change the result: intercept `-3.4232`, slope `2.2947`,
mean squared error `36.5047`, and prediction `24.1129` feet.

## Comparisons

The data and task are the same in every language: fit `distance` from `speed`, report
the coefficients and mean squared error, then predict at 12 mph.

## MATLAB

`*` is matrix multiplication, `.*` is cell arithmetic, and `\` solves or fits a tall
system.

~~~
speed = [4 4 7 7 8 9 10 10]';
distance = [2 10 4 22 16 10 18 26]';
X = [ones(8,1) speed];
beta = X \ distance;
residual = distance - X * beta;
fprintf('intercept %.4f slope %.4f mse %.4f at12 %.4f\n', ...
    beta(1), beta(2), mean(residual .* residual), [1 12] * beta);
~~~

## Julia

Julia uses the same textbook `*` and `\` forms and adds dotted broadcasting.

~~~
speed = [4.0, 4, 7, 7, 8, 9, 10, 10]
distance = [2.0, 10, 4, 22, 16, 10, 18, 26]
X = hcat(ones(8), speed)
beta = X \ distance
residual = distance - X * beta
println(beta[1], " ", beta[2], " ", mean(residual .* residual), " ", ([1.0 12.0] * beta)[1])
~~~

## NumPy

NumPy keeps `*` cell by cell and uses `@` for matrix multiplication.

~~~
import numpy as np

speed = np.array([4, 4, 7, 7, 8, 9, 10, 10], dtype=float)
distance = np.array([2, 10, 4, 22, 16, 10, 18, 26], dtype=float)
X = np.column_stack([np.ones(8), speed])
beta, *_ = np.linalg.lstsq(X, distance, rcond=None)
residual = distance - X @ beta
print(beta[0], beta[1], np.mean(residual * residual), np.array([1.0, 12.0]) @ beta)
~~~

## R

R uses `*` for cell arithmetic and `%*%` for matrix multiplication. For this task,
`lm` is the short beginner answer.

~~~
speed <- c(4, 4, 7, 7, 8, 9, 10, 10)
distance <- c(2, 10, 4, 22, 16, 10, 18, 26)
fit <- lm(distance ~ speed)
beta <- coef(fit)
residual <- residuals(fit)
print(c(beta[1], beta[2], mean(residual * residual), predict(fit, data.frame(speed = 12))))
~~~

The field comparison gives three useful lessons. MATLAB and Julia make the textbook
formula short. NumPy separates matrix multiplication with `@`. R puts the common fit
behind `lm`. Jet keeps the beginner fit named and shape-checked, while the expert can
write the matrix formula and control views.

## Ratified decision slate

The following six IDs are the complete owner-choice slate for this proposal. Each is
ratified and recorded for card #1437. They are not open ballots.

| Decision | Ratified option | Rule carried by this proposal |
|---|---|---|
| `D-MATRIX-MUL1` | F | `*` composes `Matrix`; `.*` is the only Matrix cell override; `Grid` and lane vectors use `*` cell by cell. |
| `D-MATRIX-LIT1` | E | A row ends at a line break or `;`; tall and compact forms share one literal. |
| `D-MATRIX-BCAST1` | E | Extent `1` repeats on one stated axis; vectors declare row or column orientation. |
| `D-MATRIX-INDEX1` | E | Brackets read cells, bands, and windows; `.row(i)` and `.col(j)` name whole axes. |
| `D-MATRIX-SOLVE1` | B | `.solve(y)` is for square systems; `.least_squares(y)` is for tall systems. |
| `D-MATRIX-HOME1` | D | Basic matrix operations need no import; advanced linear algebra uses `core.linalg`. |

The shape substrate is not an owner choice here. **D-TYPE2-MEASURE1=A** already
settles it, and `type-system-v2-carriers-and-knowledge.md` is its source proposal.
Views, ownership, and layout also reuse their ratified mechanisms. Implementation cards
must carry this surface through parser, sema, TIR, AOT, JIT, interpreter, and web tiers
where the feature applies.
