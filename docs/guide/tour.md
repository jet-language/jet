# Jet in 15 minutes

A quick tour of the language. Every code block below is runnable — `cargo test
tour_snippets` compiles each one.

## Hello

Every program starts in `main`. `print` takes one value and writes it with a
newline:

```jet
fn main() {
    print("hello, world");
}
```

## Functions

Functions are declared with `fn`. Add `-> Type` when the function returns a
value; use `return` to hand it back. `main` takes no parameters and returns
nothing.

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

## `val` and `var`

`val` names something that never changes. `var` names something you can update.
Most bindings should be `val` — changing is the thing you opt into.

```jet
fn main() {
    val year = 2026;
    var fuel = 3;
    fuel -= 1;
    print("year {year}, fuel {fuel}");
}
```

## `if`, `while`, and `for`

Conditions must be `Bool` — Jet won't treat `0` or `""` as false. Ranges like
`1..5` are **inclusive** (they include 5).

```jet
fn main() {
    if 21 > 18 {
        print("warm enough");
    }
    var n = 0;
    while n < 3 {
        print(n);
        n += 1;
    }
    for i in 1..3 {
        print("step {i}");
    }
}
```

## Structs

A struct groups named fields. Methods live inside the struct body; static
methods don't take `self`.

```jet
struct Point {
    x: Float;
    y: Float;

    fn dist_sq(self) -> Float {
        return (self.x * self.x) + (self.y * self.y);
    }
}

fn main() {
    val p = Point { x: 3.0, y: 4.0 };
    print(p.dist_sq());
}
```

## Enums

Enums name a fixed set of variants. `when` over an enum must cover every case.

```jet
enum Light {
    Red;
    Yellow;
    Green;
}

fn label(light: Light) -> String {
    when light {
        light == Red -> { return "stop"; };
        light == Yellow -> { return "caution"; };
        light == Green -> { return "go"; };
    }
}

fn main() {
    print(label(Light.Green));
}
```

## Errors: `?` and `??`

Functions that can fail return `T ? E`. Build outcomes with `ok(value)` and
`err(reason)`. `??` gives you a fallback; `?` passes failure up to your caller.

```jet
fn parse_n(raw: String) -> Int ? String {
    if raw == "" {
        return err("empty");
    }
    return ok(42);
}

fn load() -> Int ? String {
    val n = parse_n("7")?;
    return ok(n * 2);
}

fn main() {
    val a = parse_n("42") ?? 0;
    print(a);
    val b = load() ?? 0;
    print(b);
}
```

## Collections

`[T]` is an ordered sequence; `[K, V]` is key–value lookup. Literals
use `[ ]` and `[: ]`.

```jet
fn main() {
    var nums: [Int] = [3, 1, 2];
    nums.push(4);
    nums.sort();
    print(nums[0]);

    var counts: [String, Int] = [:];
    counts["jet"] = 1;
    print(counts["jet"]);
}
```

## Imports

Import standard modules by name. Both `std.fs` and `jet.std.fs` mean the same
thing. Quoted paths like `import "helper"` load `.jet` files from your project.

```jet
import std.io as io;

fn main() {
    val args = io.args();
    print("got {args.len()} arguments");
}
```

## `std.fs` and `std.io`

The standard library is built in. Fallible calls return `T ? E` — handle them
with `??`, `?`, or `when`.

```jet
import std.fs as fs;
import std.io as io;

fn main() {
    val path = "/tmp/jet_tour.txt";
    when fs.write(path, "hello\njet") {
        it == ok(_) -> {};
        it == err(_) -> { return; };
    }
    when fs.read(path) {
        it == ok(text) -> { print(text); };
        it == err(_) -> { return; };
    }
    io.eprint("logged to stderr");
}
```

## Closures

`(x) => …` is a small function value. List methods like `map`, `filter`, and
`reduce` take closures.

```jet
fn main() {
    val nums = [1, 2, 3, 4, 5];
    val squares = nums.map((n: Int) => n * n);
    val big = squares.filter((n) => n > 5);
    print(big);
    val total = nums.reduce(0, (acc: Int, n: Int) => acc + n);
    print(total);
}
```

## Traits (brief)

A trait is a promise that a type can provide certain methods. Write `impl Trait`
inside a struct or `impl Type: Trait` outside it. Generic functions take type
parameters in angle brackets.

```jet
trait Named {
    fn name(self) -> String;
}

struct Circle {
    radius: Float;

    impl Named {
        fn name(self) -> String {
            return "circle";
        }
    }
}

fn greet<T: Named>(x: T) {
    print(x.name());
}

fn main() {
    greet(Circle { radius: 1.0 });
}
```

---

**Next steps:** [Getting started guide](guide/01-getting-started.md) ·
[Ownership](guide/02-ownership.md) · [Error index](errors/) ·
[Standard library](../reference/stdlib.md)
