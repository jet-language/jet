# Lists, maps, and closures

## Lists

```jet
fn main() {
    var nums: [Int] = [3, 1, 2];
    nums.push(4);
    nums.sort();
    print(nums[0]);
    print(nums[1..3].join(","));
}
```

```
1
2,3,4
```

A `[T]` holds many values of one type. You write a literal with `[ ]`,
index with `[i]`, and slice a range with `[start..end]` — and like the `for`
ranges from chapter one, that slice is **inclusive**, so `[1..3]` is indices 1,
2, and 3. The methods you'd expect are there — `push`, `sort`, `len`, `join`,
and more.

Indexing out of bounds doesn't get to corrupt anything; it stops with a clear
report. And when you only *might* have an element, ask for it the safe way:

```jet
print(nums.get(99) ?? -1);
```

`get` hands back an optional, so `nums.get(99) ?? -1` reads "the element if it's
there, otherwise -1" — no crash, no forgotten check.

## Maps

```jet
fn main() {
    var counts: [String, Int] = [:];
    for word in "the quick the brown".split(" ") {
        counts[word] = (counts.get(word) ?? 0) + 1;
    }
    for key, count in counts {
        print("{key}: {count}");
    }
}
```

```
brown: 1
quick: 1
the: 2
```

A `[K, V]` associates keys with values. The empty map literal is `[:]` (the
colon is what tells it apart from an empty list). `counts.get(word) ?? 0` is the
classic "current count, or zero if I haven't seen this key" — the same `??` from
the errors chapter, doing the same job. Looping over a map gives you the key and
value together: `for key, count in counts`.

## Closures

A closure is a function you write inline, usually to hand to something else. The
arrow is `=>`:

```jet
fn main() {
    val nums = [1, 2, 3, 4, 5];

    val squares = nums.map((n: Int) => n * n);
    print(squares);

    val big = squares.filter((n) => n > 5);
    print(big);

    val total = nums.reduce(0, (acc: Int, n: Int) => acc + n);
    print(total);
}
```

```
[1, 4, 9, 16, 25]
[9, 16, 25]
15
```

`map` makes a new list by transforming each element, `filter` keeps the ones
that pass a test, and `reduce` folds the whole list down to one value (starting
from `0` here, adding each element). The full set on `[T]` is `map`,
`filter`, `each`, `find`, `any`, `all`, `sort_by`, and `reduce`. You can leave
the parameter type off when Jet can already tell what it must be — that's why
`filter((n) => n > 5)` doesn't repeat `: Int`.

`sort_by` takes a closure that says what to sort *by*:

```jet
var words = ["pear", "apple", "fig"];
words.sort_by((w: String) => w.len());
print(words);     // [fig, pear, apple]  — by length
```

## Closures and ownership

Closures follow the same ownership rules as everything else (see
[Ownership](02-ownership.md)). A closure that just reads a captured value
borrows it; one that's passed somewhere it might outlive the current scope has
to *own* what it captured. For cheap clonable values Jet copies them for you and
notes it with a lint; for everything else you mark the capture `take(name)` so
the hand-off is, as always, visible in the code rather than a surprise.

For the common case — `map`, `filter`, `each` over a list right here, right now
— you don't think about any of this. It just works.

## What you actually have to remember

- `[T]` with `[ ]`, index `[i]`, slice `[a..b]`; `get` returns an optional.
- `[K, V]` with `[:]` for empty; `for key, value in m` to walk it.
- Closures are `(params) => expr` (or `=> { ... }`); types are optional when inferable.
- `map` / `filter` / `reduce` / `sort_by` / `each` / `find` / `any` / `all` cover most list work.

Next: [traits and generics](06-traits-and-generics.md) — writing behavior once
and sharing it across types.
