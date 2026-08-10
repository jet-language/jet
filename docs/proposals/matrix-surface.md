# Matrix and linear algebra surface

Status: proposal for owner decision. Card #1437. No code lands from this document.

Foundation: **D-TYPE2-MEASURE1=A** (ratified 2026-08-06) and
`docs/proposals/type-system-v2-carriers-and-knowledge.md`. That decision gives Jet one
substrate for every compile-time number that rides inside a type: fixed-list lengths,
SIMD lane counts, dimension exponents, and matrix sides. Every archetype below writes
its shapes as `Matrix<M, N>` and `Vec<N>` on that one substrate. None of them proposes a
second shape-encoding mechanism. The measure decision states in its own text that it
"hands the matrix card its foundation... without deciding its syntax". Deciding that
syntax is this document's only job.

## Executive summary

Jet has no matrix surface today. `core.compute` exposes ranked `Tensor<T>` storage through
functions — `compute.matrix(2, 3, 2.0)`, `compute.matmul(a, b)`, `compute.set(&t, [0, 1], 4.0)`
— and `core.linalg` ships a closed family of fixed sizes (`Vec2/3/4`, `Mat3/Mat4`). There
is no shape-typed matrix value, no literal, and no operator arithmetic beyond the small
closed family. A least-squares fit today is a chain of fallible function calls.

Four archetypes below write the same fit end to end. They are four different user
experiences, not one design in four coats:

| | How you multiply matrices | How you multiply cell by cell | How you solve |
|---|---|---|---|
| **A — Textbook** | `a * b` | `a .* b` | `x \ y` |
| **B — One glyph, one meaning** | `a.matmul(b)` | `a * b` | `x.solve(y)` |
| **C — One new sigil** | `a @ b` | `a * b` | `x.solve(y)` |
| **D — The type says what it is** | `a * b` on `Matrix` | `a * b` on `Grid` | `x.solve(y)` |

Six owner choices come out of the design. They are minted as ballots on card #1437:
the multiply spelling, the literal spelling, the broadcasting rule, the indexing
spelling, the solve spelling, and where the types live.

## What is already settled

Do not re-decide these. Every archetype below obeys all of them.

- **Shapes ride the measure substrate** — `Matrix<M, N>`, `Vec<N>`, dimension arithmetic
  (D-TYPE2-MEASURE1=A). A side mismatch is a compile error, not a runtime check.
- **`Tensor<T>` owns ranked storage; `Vec<N>` and `Matrix<M, N>` share that substrate and
  cross it with no copy** (D-COMPUTE-TYPE1=D).
- **Views are a place rule, not a method** — a bare place projection is a read window,
  `&place` is the exclusive write window, `~place` is an independent owned copy
  (D-SHAPE-PLACE1=A). A method call is not part of a place, so an in-place edit must be
  written on a bracket place.
- **Stored element order is column-major, and the small fixed family carries `F64`
  components** (D-LINALG1).
- **Operators dispatch through ordinary trait hooks** (D-OPDEF1=A). The built-in math
  family is closed today (E2511), but the dispatch mechanism is the ordinary one.
- **The operator slate**: `^` raises to a power and groups to the right (D-EXPOP1=A,
  D-EXPSEM1=A); `/%` divides and rounds down (D-FLOORDIV1=A); `%` is the floored modulo
  and `%%` the truncated remainder (D-MODSEM1=A); `Int / Int` gives a Float (D-INTDIV1=A);
  `Int` is arbitrary precision (D-INTBIG1=A); exclusive-or is `~|` (D-XORSPELL1=A).
- **`==` on a math value answers one `Bool`**, not a grid of them (D-LINALG1). Cell-by-cell
  comparison is a named method in every archetype, so `if a == b` never means "a grid
  of yes and no".

## The worked example

Eight stopping-distance measurements, taken from Ezekiel's 1930 study and shipped in R
as the first eight rows of the `cars` data set. Speed is in miles per hour, stopping
distance in feet.

| speed | 4 | 4 | 7 | 7 | 8 | 9 | 10 | 10 |
|---|---|---|---|---|---|---|---|---|
| **distance** | 2 | 10 | 4 | 22 | 16 | 10 | 18 | 26 |

The task: fit `distance = intercept + slope * speed` by least squares, report the mean
squared error, and predict the stopping distance at 12 mph.

The answer, to four places: intercept `-3.4232`, slope `2.2947`, mean squared error
`36.5047`, prediction at 12 mph `24.1129` feet.

Every program below prints those four numbers. Each one builds the design matrix `x`
(a column of ones beside the speeds, shape 8 by 2) and the observed column `y`
(shape 8 by 1), then solves the normal equations.

---

## Archetype A — Textbook

**The idea.** The formula on the page is the line you type. `*` between two matrices is
matrix multiplication, because that is what `*` means in every mathematics textbook and
in every mathematics tool. Cell-by-cell work asks for itself with a leading dot: `.*`,
`./`, `.^`. Solving is `\`, exactly as MATLAB and Julia spell it.

**Shapes.** `Matrix<M, N>` and `Vec<N>` on the D-TYPE2-MEASURE1 measure substrate. The
inner sides of a product must match, and the checker composes the outer sides.

**Literals.** A line break ends a row, so the source looks like the grid.

**Broadcasting.** A value whose axis has extent 1 stretches along that axis.

**Indexing.** `m[i, j]`. A range in a slot keeps that axis; a bare `..` means the whole
axis.

```jet
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    stop  :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    // The design matrix: a column of ones beside the speeds.
    x :: Matrix.columns(la.ones<8>(), speed)     // Matrix<8, 2>
    y :: stop.as_column()                        // Matrix<8, 1>

    // `\` solves. Square system: the exact answer. Tall system: least squares.
    beta :: x \ y ?? panic("no fit")             // Matrix<2, 1>
    print("intercept {beta[0, 0]}  slope {beta[1, 0]}")
    // intercept -3.4232…  slope 2.2947…

    // The normal-equation spelling gives the same answer:
    //   beta :: (x.t * x) \ (x.t * y) ?? panic("singular")

    residual :: y - x * beta                     // Matrix<8, 1>
    print("mean squared error {(residual .* residual).mean()}")
    // mean squared error 36.5047…

    ask :: Matrix<1, 2>.{
        1, 12
    }
    print("12 mph stops in {(ask * beta)[0, 0]} feet")
    // 12 mph stops in 24.1129… feet
}
```

**The beginner sees** the textbook formula and nothing else. Shapes are inferred from the
data, so no shape is ever written by hand in this program.

**The expert reaches for** windows, in-place edits, and layout, all through rules the
language already has:

```jet
    speeds :: x[.., 1]        // read window over the second column — no copy
    &x[.., 1] *= 2.0          // write window: doubles that column in place
    kept :: ~x[0..<4, ..]     // an owned copy that can outlive `x`

    #Layout(row_major)
    struct Frame { pixels: Matrix<1080, 1920> }   // column-major is the default
```

**Against the operator slate.** `^` on a square matrix is the matrix power — `m ^ 3` is
`m * m * m` — and `.^` raises every cell. `/` divides cell by cell and never solves, so
`\` is the one solve spelling. `/%` and `%` apply cell by cell under the scalar rule.
`==` answers one `Bool`; `.eq_cells(b)` answers a grid.

**What this costs.** `*` reads two ways depending on what sits beside it. Two square
matrices multiply as maps under `*` and cell by cell under `.*`, and both are
well-shaped, so a person who meant the other one gets a wrong answer with no error.

---

## Archetype B — One glyph, one meaning

**The idea.** No operator ever changes meaning. Every arithmetic operator is cell by
cell on every math value, everywhere, including matrices. Linear algebra is named:
`.matmul(b)`, `.t`, `.solve(y)`, `.det()`, `.inverse()`. No new sigils enter the
language.

**Shapes.** As above, on the measure substrate. `.matmul` composes the outer sides and
requires the inner sides to match.

**Literals.** Each row is an ordinary list, so nothing new is learned.

**Broadcasting.** Same rule as A.

**Indexing.** `m[i, j]`, with `m.row(i)` and `m.col(j)` for whole axes.

```jet
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    stop  :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    x :: Matrix.columns(la.ones<8>(), speed)     // Matrix<8, 2>
    y :: stop.as_column()                        // Matrix<8, 1>

    xt :: x.t                                    // Matrix<2, 8>, a view
    beta :: xt.matmul(x).solve(xt.matmul(y)) ?? panic("singular")
    print("intercept {beta[0, 0]}  slope {beta[1, 0]}")
    // intercept -3.4232…  slope 2.2947…

    residual :: y - x.matmul(beta)               // Matrix<8, 1>
    print("mean squared error {(residual * residual).mean()}")
    // mean squared error 36.5047…

    ask :: Matrix<1, 2>.{ [1, 12] }
    print("12 mph stops in {ask.matmul(beta)[0, 0]} feet")
    // 12 mph stops in 24.1129… feet
}
```

**The beginner sees** one meaning per symbol. `a * b` multiplies cell by cell whether
`a` is a number, a lane vector, or a matrix. Nothing is guessed from the shapes.

**The expert reaches for** the same window rules, plus an accumulating multiply that
writes into storage that already exists:

```jet
    out := Matrix<2, 2>.zeros()
    xt.matmul_into(&out, x)   // no new storage
    &x[.., 1] *= 2.0
```

**Against the operator slate.** `^` raises every cell to a power; the matrix power is
`m.power(3)`. `/` divides cell by cell. `/%` and `%` apply cell by cell. `==` answers one
`Bool`.

**What this costs.** The textbook line disappears. `(XᵀX)⁻¹Xᵀy` becomes a chain of method
calls, and long algebra reads as a pipeline instead of a formula. The audience Jet is
courting — the people who already write `^` for powers — write matrix products constantly.

---

## Archetype C — One new sigil

**The idea.** Cell-by-cell arithmetic keeps every existing operator, and matrix
multiplication gets one new infix symbol of its own: `@`. Python added exactly this in
2014 after twenty years of overloading `*`. The two multiplications are visibly
different in the source, and neither one changes meaning by type.

**Shapes.** As above. `@` matches the inner sides and composes the outer sides.

**Literals, broadcasting, indexing.** As B.

```jet
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    stop  :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    x :: Matrix.columns(la.ones<8>(), speed)     // Matrix<8, 2>
    y :: stop.as_column()                        // Matrix<8, 1>

    beta :: (x.t @ x).solve(x.t @ y) ?? panic("singular")
    print("intercept {beta[0, 0]}  slope {beta[1, 0]}")
    // intercept -3.4232…  slope 2.2947…

    residual :: y - x @ beta                     // Matrix<8, 1>
    print("mean squared error {(residual * residual).mean()}")
    // mean squared error 36.5047…

    ask :: Matrix<1, 2>.{ [1, 12] }
    print("12 mph stops in {(ask @ beta)[0, 0]} feet")
    // 12 mph stops in 24.1129… feet
}
```

**The beginner sees** two multiply symbols and must learn which is which once. After
that, no line is ambiguous and no shape decides a meaning.

**The expert reaches for** the same windows and the same in-place multiply as B.

**Against the operator slate.** `@` sits at the same precedence as `*`, so `a @ b * c`
groups left to right. `^` raises every cell. `/`, `/%`, and `%` are cell by cell.
`==` answers one `Bool`.

**What this costs.** One more symbol in a language that has just spent three decisions
tidying its operator set. `@` is not a mathematical symbol; it has to be taught. Ported
MATLAB and Julia code needs every `*` rewritten.

---

## Archetype D — The type says what it is

**The idea.** Two shape-typed values sit on one storage substrate. `Matrix<M, N>` is a
**linear map** and multiplies by composition. `Grid<M, N>` is a **table of numbers** and
multiplies cell by cell. No operator is overloaded and no new symbol is minted: `*` asks
the type what multiplication means, which is the ordinary rule the whole language
already uses (D-OPDEF1). You say which thing you have once, when you build it, and the
rest of the file reads without ambiguity.

Crossing between them is one written word and costs nothing: `m.cells()` is a `Grid`
view of the same storage, and `g.as_map()` is a `Matrix` view of it.

**Shapes.** Both types carry `<M, N>` on the measure substrate. A `Matrix` product
matches inner sides; a `Grid` product requires equal shapes.

**Literals.** A line break ends a row, for both types.

**Broadcasting and indexing.** As A.

```jet
use core.linalg as la

fn run() {
    speed :: Vec<8>.{ 4, 4, 7, 7, 8, 9, 10, 10 }
    stop  :: Vec<8>.{ 2, 10, 4, 22, 16, 10, 18, 26 }

    x :: Matrix.columns(la.ones<8>(), speed)     // Matrix<8, 2> — a linear map
    y :: stop.as_column()                        // Matrix<8, 1>

    beta :: (x.t * x).solve(x.t * y) ?? panic("singular")
    print("intercept {beta[0, 0]}  slope {beta[1, 0]}")
    // intercept -3.4232…  slope 2.2947…

    // Squaring residuals is cell-by-cell work, so ask for the cells.
    residual :: (y - x * beta).cells()           // Grid<8, 1>, a view
    print("mean squared error {(residual * residual).mean()}")
    // mean squared error 36.5047…

    ask :: Matrix<1, 2>.{
        1, 12
    }
    print("12 mph stops in {(ask * beta)[0, 0]} feet")
    // 12 mph stops in 24.1129… feet
}
```

**The beginner sees** one multiply symbol that always means "multiply these two things
the way these things multiply" — the same rule as `+` on text and `+` on numbers. The
first line of the program says which world it is in.

**The expert reaches for** the same window rules, and for the free crossing between the
two readings:

```jet
    normalized :: (x.cells() / x.cells().max_of_column()).as_map()
    &x[.., 1] *= 2.0          // write window, in place
    kept :: ~x[0..<4, ..]
```

**Against the operator slate.** `^` on a `Matrix` is the matrix power; `^` on a `Grid`
raises every cell. `/` on a `Grid` divides cell by cell; `/` between two `Matrix` values
is refused with a diagnostic that names `.solve` and `.cells()`. `/%` and `%` are `Grid`
operators only. `==` answers one `Bool` on both types.

**What this costs.** Two type names instead of one, and library authors must choose which
they accept. Code that mixes both readings in one expression writes `.cells()` or
`.as_map()` at the crossing point.

---

## The same fit in MATLAB, Julia, NumPy, and R

**MATLAB.** `*` is matrix multiply, `.*` is cell by cell, `\` solves and falls back to
least squares for a tall system, `'` transposes.

```matlab
speed = [4 4 7 7 8 9 10 10]';
dist  = [2 10 4 22 16 10 18 26]';
X = [ones(8,1) speed];
beta = X \ dist;
fprintf('intercept %.4f slope %.4f\n', beta(1), beta(2));
res = dist - X*beta;
fprintf('mse %.4f\n', mean(res.*res));
fprintf('at 12 mph: %.4f\n', [1 12]*beta);
```

**Julia.** The same operator family as MATLAB, plus a general rule: a leading dot on any
operator or function call means "do this to every element", so `sqrt.(v)` and `a .* b`
are one idea.

```julia
speed = [4.0, 4, 7, 7, 8, 9, 10, 10]
dist  = [2.0, 10, 4, 22, 16, 10, 18, 26]
X = hcat(ones(8), speed)
beta = X \ dist
println("intercept ", beta[1], " slope ", beta[2])
res = dist - X*beta
println("mse ", sum(res .* res) / 8)
println("at 12 mph: ", ([1.0 12.0] * beta)[1])
```

**NumPy.** `*` is cell by cell, `@` is matrix multiply, `.T` transposes, and the fit is a
named function.

```python
import numpy as np
speed = np.array([4, 4, 7, 7, 8, 9, 10, 10], dtype=float)
dist  = np.array([2, 10, 4, 22, 16, 10, 18, 26], dtype=float)
X = np.column_stack([np.ones(8), speed])
beta, *_ = np.linalg.lstsq(X, dist, rcond=None)
print(f"intercept {beta[0]:.4f} slope {beta[1]:.4f}")
res = dist - X @ beta
print(f"mse {np.mean(res * res):.4f}")
print(f"at 12 mph: {np.array([1.0, 12.0]) @ beta:.4f}")
```

**R.** `*` is cell by cell, `%*%` is matrix multiply, `t()` transposes, and the everyday
answer is a model-fitting function rather than matrix algebra at all.

```r
speed <- c(4, 4, 7, 7, 8, 9, 10, 10)
dist  <- c(2, 10, 4, 22, 16, 10, 18, 26)

fit <- lm(dist ~ speed)
coef(fit)                                  # (Intercept) -3.4232   speed 2.2947
mean(residuals(fit)^2)                     # 36.5047
predict(fit, data.frame(speed = 12))       # 24.1129

# the matrix spelling, for comparison
X <- cbind(1, speed)
beta <- solve(t(X) %*% X, t(X) %*% dist)
```

**What the field shows.** Three of the four give matrix multiplication its own visible
form (`*` in MATLAB and Julia, `%*%` in R, `@` in NumPy). Only NumPy took the "`*` is
always cell by cell" road, and it did so after retiring an earlier `matrix` type whose
`*` meant matrix multiply — the two readings of one glyph in one library are what forced
`@` into the language. R's answer to this exact task is not matrix algebra at all: it is
`lm()`. That is a real signal about the beginner surface, and it sits behind the solve
ballot.

## The ballot slate

| Ballot | Question |
|---|---|
| `D-MATRIX-MUL1` | What `*` means between two matrices, and how the other multiplication is spelled |
| `D-MATRIX-LIT1` | How you write a matrix down |
| `D-MATRIX-BCAST1` | When two different shapes may combine |
| `D-MATRIX-INDEX1` | How you read one cell, one row, and one window |
| `D-MATRIX-SOLVE1` | How you solve a system and fit a line |
| `D-MATRIX-HOME1` | Whether `Matrix` and `Vec` are always there or asked for |

The six are close to independent. The multiply ballot picks the archetype; the other five
apply to whichever archetype wins, except where an option names a dependency.

## What is deliberately not balloted

- **How shapes are encoded.** Settled by D-TYPE2-MEASURE1=A. Every archetype uses it.
- **Views and in-place editing.** Settled by D-SHAPE-PLACE1=A: a bare place is a read
  window, `&place` is the write window, `~place` copies. A matrix window is that rule
  applied to a bracket place, not a new mechanism. One consequence is stated in the
  indexing ballot: a method call is not a place, so an in-place column edit must be
  written on brackets.
- **Stored element order.** Settled column-major by D-LINALG1. Expert control uses the
  existing marker family (`#Layout(row_major)`), which is a new member of a ratified
  family rather than a new mechanism.
- **Element type.** `Matrix<M, N>` carries `Float` cells; the general form is
  `Matrix<T, M, N>` over the `Tensor<T>` substrate (D-COMPUTE-TYPE1=D). With arbitrary
  precision `Int` (D-INTBIG1), a whole-number matrix has an exact determinant.
- **What `==` answers.** One `Bool`, per D-LINALG1. Cell-by-cell comparison is a named
  method under every archetype, so a matrix can never appear where an `if` expects a
  yes-or-no.
- **The small fixed family.** `Vec2/3/4` and `Mat3/Mat4` stay as the graphics-facing
  aliases over the general substrate (D-LINALG1). The home ballot asks where the general
  types live, not whether the aliases survive.
