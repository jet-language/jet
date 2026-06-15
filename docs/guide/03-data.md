# Structs, enums, and optionals

## Structs

A struct groups related fields and the methods that work on them:

```jet
struct Point {
    x: Float;
    y: Float;

    fn dist_sq(self) -> Float {
        return (self.x * self.x) + (self.y * self.y);
    }
}

fn main() {
    val p = Point {x: 3.0, y: 4.0};
    print(p.dist_sq());
}
```

```
25.0
```

You build one by naming its fields — `Point {x: 3.0, y: 4.0}` — and you call a
method with a dot: `p.dist_sq()`, never `dist_sq(p)`. The first parameter,
`self`, is the value the method was called on. It follows the same ownership
rules as any other parameter: plain `self` reads, `mut self` changes, `take
self` consumes.

A method that doesn't take `self` is a constructor-style helper you call on the
type itself:

```jet
struct Point {
    x: Float;
    y: Float;

    fn unit() -> Point {
        return Point {x: 1.0, y: 0.0};
    }
}

// Point.unit()  ->  a fresh Point
```

If you'd rather keep methods out of the type body, an `impl Point { }` block
holds them and works identically — same rules, your preference.

## Enums

An enum is a value that is exactly one of a fixed set of cases:

```jet
enum Light {
    Red;
    Yellow;
    Green;
}

fn label(light: Light) -> String {
    when light {
        light == Red    -> { return "stop"; };
        light == Yellow -> { return "caution"; };
        light == Green  -> { return "go"; };
    }
}
```

You write a case as `Light.Red` and test it with `==`. Notice this `when` has
no `else` — and doesn't need one. Because `Light` has exactly three cases, the
compiler can see you've covered all of them. Add a fourth case to the enum and
forget to handle it, and Jet tells you which one you missed:

```
Error [E0307]: `when` doesn't cover every case — missing: Green
 Why: when every arm is a pattern test, each variant must appear once
 Fix: add an arm for: Green
```

It even names the case you dropped.

That's the payoff for having real enums: "I forgot the new case" stops being a
class of bug. Cases can also carry data — `BadDigit(String)` — which you'll see
in [the errors chapter](04-errors.md).

## Optionals — a value, or nothing, and you can't forget which

Jet has no null that can hide inside any type. Instead, a type that might be
absent is spelled `T?`, and you have to acknowledge the empty case to get at the
value:

```jet
fn find_even(limit: Int) -> (Int?) {
    for i in 1..limit {
        if i % 2 == 0 {
            return value(i);
        }
    }
    return null;
}

fn main() {
    if find_even(9) == value(n) {
        print(n);
    }
    if find_even(8) == null {
        print("none");
    }
}
```

```
2
```

`value(i)` wraps a present value; `null` is the empty case (and `null` is only
ever legal for some `T?`, never for a plain `Int` or `String`). To read what's
inside, you test the optional: `find_even(9) == value(n)` checks that the result
is present *and* binds the inner number to `n` in one move. There's no way to
reach `n` without having checked, so the billion-dollar mistake simply isn't
expressible.

## What you actually have to remember

- `Type { field: v }` builds a struct; `value.method()` calls a method.
- `self` is the receiver and obeys the ownership keywords like any parameter.
- A `when` over an enum must cover every case — the compiler keeps you honest.
- "Maybe missing" is `T?`; build it with `value(x)` / `null`, read it by testing.

Next: [errors as values](04-errors.md) — the same "you can't forget the bad
case" idea, applied to things that fail.
