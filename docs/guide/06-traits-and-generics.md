# Traits and generics

The last two ideas let you write a piece of behavior once and use it across many
types. They're the tools you reach for when you stop writing scripts and start
building something with shape to it.

## Generics: same code, any type

You already used generics — `[Int]` is one. Writing your own means putting a
type variable in angle brackets and letting the caller fill it in:

```jet
struct Pair<T> {
    first: T;
    second: T;
}

fn make_pair<T>(a: T, b: T) -> Pair<T> {
    return Pair<T> {first: a, second: b};
}

fn main() {
    val p: Pair<Int> = make_pair(1, 2);
    print(p.first);
}
```

```
1
```

`Pair<T>` is a pair of *anything*, as long as both halves are the same type. The
`T` is a stand-in chosen per use: `Pair<Int>` here, `Pair<String>` somewhere
else. You write the logic once; the compiler stamps out a specialized version
for each type you actually use, so there's no runtime cost for the convenience.

## Traits: behavior several types agree to provide

A trait is a promise — "any type that is a `Shape` can tell me its area and its
name":

```jet
trait Shape {
    fn area(self) -> Float;
    fn name(self) -> String;
}
```

Types then say how they keep that promise. Two ways, same meaning — put an
`impl Shape` block inside the type, or write `impl Type: Shape` outside it:

```jet
struct Circle {
    radius: Float;

    impl Shape {
        fn area(self) -> Float { return 3.14159 * self.radius * self.radius; }
        fn name(self) -> String { return "circle"; }
    }
}

struct Square {
    side: Float;
}

impl Square: Shape {
    fn area(self) -> Float { return self.side * self.side; }
    fn name(self) -> String { return "square"; }
}
```

Now the useful part. A trait name can stand in for a type, so a function can
take "any `Shape`" and a list can hold a mix of them:

```jet
fn print_area(s: Shape) {
    print("{s.name()}: {s.area()}");
}

fn main() {
    val shapes: [Shape] = [Circle {radius: 1.0}, Square {side: 2.0}];
    shapes.each((s) => {
        print_area(s);
    });
}
```

```
circle: 3.14159
square: 4.0
```

`print_area` doesn't know or care whether it got a circle or a square — only
that it's a `Shape`, so `.area()` and `.name()` are available. Picking the right
one at runtime is handled for you; there's no boxing or dispatch ceremony in
your code.

## Bounds: generics that require a trait

Combine the two and you can write code that's generic *and* gets to assume some
behavior. "Find the largest element" only makes sense if elements can be
compared, so you say so with a bound — `T: Comparable`:

```jet
fn largest<T: Comparable>(xs: [T]) -> (T?) {
    if xs.len() == 0 {
        return null;
    }
    var best = xs[0];
    var i = 1;
    while i < xs.len() {
        if xs[i] > best {
            best = xs[i];
        }
        i += 1;
    }
    return value(best);
}
```

The bound is the whole game: inside `largest`, `xs[i] > best` is allowed
*because* you required `Comparable`. Call it with a type that isn't comparable
and the compiler turns you down at the call, not deep inside the function.

## The built-in traits, and `derive`

Some traits are so common Jet handles them for you. Every type can already be
printed and compared for equality without you writing anything. For ordering —
`<`, `>`, sorting — you opt in with one line:

```jet
struct Score {
    points: Int;
    derive Comparable;
}

fn main() {
    var scores = [Score {points: 10}, Score {points: 20}];
    scores.sort_by((s: Score) => s.points);
    print(scores[0].points);
}
```

```
10
```

`derive Comparable;` tells Jet to generate the comparison for you from the
fields, so `Score` works anywhere `Comparable` is required. You only write a
trait implementation by hand when the automatic one isn't what you mean.

## What you actually have to remember

- `Type<T>` makes a type generic; the caller picks `T`, with no runtime cost.
- A `trait` lists method signatures; types implement it inside (`impl Trait`) or
  outside (`impl Type: Trait`).
- A trait name *is* a usable type — `fn f(s: Shape)`, `[Shape]` — and
  dispatch is invisible.
- `<T: Trait>` lets generic code assume behavior; mismatches are caught at the call.
- Printing and equality are automatic; `derive Comparable;` adds ordering.

That's the tour. From here the reference material in `docs/spec/spec.md` is the
exact, complete word on everything you've seen, and `docs/spec/diagnostics.md`
lists every error the compiler can produce and why.
