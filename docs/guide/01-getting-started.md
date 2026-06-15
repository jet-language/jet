# Getting started

## Values

```jet
fn main() {
    val name = "Jet";
    val year = 2026;
    val version = 0.1;
    val ready = true;

    print("{name} {version} (ready: {ready})");
    print("next year is {(year + 1)}");
}
```

```
Jet 0.1 (ready: true)
next year is 2027
```

`val` names a value that never changes. Most of your bindings will be `val` —
in Jet, not-changing is the default and changing is the thing you opt into:

```jet
var fuel = 3;
fuel -= 1;   // fine, fuel is a var
```

If you try to reassign a `val`, the compiler stops you. That's not a scolding;
it's the language taking "this never changes" at its word so it can reason
about your program (and so can the next person reading it).

You didn't write any types above and didn't need to — Jet infers them from the
value. You *can* spell them out when it helps a reader:

```jet
val name: String = "Jet";
val ready: Bool = true;
```

The four built-in scalar types are `Int` (64-bit whole numbers), `Float`
(decimals), `Bool` (`true`/`false`), and `String` (text). There is exactly one
string type, on purpose.

### Text and interpolation

Anything in `{ }` inside a string is an expression that gets printed in place:

```jet
print("sum {(7 + 3)}, name {name}");
```

If you want a literal brace, double it — `{{` prints `{`. The usual escapes
work inside quotes: `\n`, `\t`, `\"`, `\\`. A string is always on one line.

> **The fine print.** Int and Float never mix silently — `2 + 2.0` is an error,
> not a quiet conversion, because the rounding that hides in that conversion is
> exactly the kind of bug Jet would rather you decide on purpose. A `Float`
> always prints with a decimal part, so `5.0` shows as `5.0`, never `5`.

## Making decisions

```jet
fn describe(celsius: Float) {
    if celsius <= 0.0 {
        print("{celsius} C: bundle up");
    } else if (celsius >= 18.0) && (celsius <= 26.0) {
        print("{celsius} C: just right");
    } else {
        print("{celsius} C: fine");
    }
}
```

Conditions are plain `Bool` expressions. The comparison operators are
`==  !=  <  >  <=  >=`; the logic operators are `&&`, `||`, and `!`. A
condition has to actually be a `Bool` — Jet won't treat `0` or `""` as
"falsy", because "is this number zero or is it a mistake?" is a question worth
answering out loud.

## Loops

```jet
for n in 1..5 {
    print(n);
}
```

```
1
2
3
4
5
```

`1..5` is **inclusive** — it counts 1 through 5. That trips up people coming
from languages where the end is excluded, so it's worth saying twice:
`1..5` includes 5.

`while` repeats as long as its condition holds, and `break`/`continue` work in
both kinds of loop:

```jet
var fuel = 3;
while fuel > 0 {
    print("t-minus {fuel}");
    fuel -= 1;
}
print("liftoff");
```

## when

When you're choosing between several conditions, `when` reads better than a
ladder of `else if`. Each arm is a condition; the first true one wins, and
`else` is required so there's no forgotten case:

```jet
fn label(n: Int) -> String {
    when n {
        n % 15 == 0 -> { return "FizzBuzz"; };
        n % 3 == 0  -> { return "Fizz"; };
        n % 5 == 0  -> { return "Buzz"; };
        else        -> { return "{n}"; };
    }
}
```

(Once you have your own enum types, `when` gets sharper — it can check that
you've covered every variant. That's in [the data chapter](03-data.md).)

## Functions

```jet
fn greet() {
    print("hi there");
}

fn double(n: Int) -> Int {
    return n * 2;
}

fn main() {
    greet();
    print(double(21));
}
```

```
hi there
42
```

A function that promises a return type with `-> Type` has to actually return a
value on every path out — Jet checks that you didn't forget the last branch.
`main` is where the program starts; it takes no arguments and returns nothing.

That's the whole of the "boring" part of the language. Next is the part that
makes Jet Jet: [ownership](02-ownership.md).

---

### Conventions, briefly

- Statements end in `;`. Block headers (`if`, `while`, `for`, `fn`) don't.
- Comments are `//` to end of line, or `/* … */` for a block (these nest).
- `jet fmt` settles every formatting question for you — 4-space indent, one
  statement per line, no knobs. Don't argue with it; let it win.
