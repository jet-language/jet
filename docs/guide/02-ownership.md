# Ownership

This is the chapter that makes Jet different, so it gets a little more room.
The good news: there's no new syntax to memorize up front. You write functions
the way you'd expect, and Jet adds a few small words only where they earn their
keep.

Start with a program that does nothing surprising:

```jet
fn show(msg: String) {
    print(msg);
}

fn main() {
    val greeting = "hello";
    show(greeting);
    print(greeting);
}
```

```
hello
hello
```

`show` reads `greeting` and prints it. Afterward `main` still has `greeting`
and prints it again. Nothing was consumed, nothing was copied that didn't need
to be. This is the default: **a function parameter is a shared, read-only look
at the caller's value.** Most functions only need that, so most functions are
written exactly like `show` — no extra words.

Three things change that default, and you reach for them in order of rarity.

## `mut` — I need to change it

```jet
fn bump(mut n: Int) {
    n += 1;
}

fn main() {
    var score = 41;
    bump(mut score);
    print(score);
}
```

```
42
```

A function that changes its argument marks the parameter `mut`, and — this is
the part people like once they get used to it — **the caller also writes
`mut`**, right at the call: `bump(mut score)`. You never have to wonder whether
a function call quietly reshaped one of your variables. If it can, it says so,
in your code, at the call site. Pass a `val` where `mut` is required and the
compiler points at the binding and tells you to make it a `var`.

## `take` — I'm keeping it

```jet
fn archive(take name: String) -> String {
    return name;
}

fn main() {
    val saved = archive(take "vault");
    print(saved);
}
```

```
vault
```

Sometimes a function needs to *keep* a value — store it, return it, hand it off
— not just glance at it. That's `take`: the value moves in and the caller gives
it up. Same as `mut`, the caller writes `take` too, so handing over ownership is
always visible.

What happens if you use a value after you've given it away?

```jet
val name = "vault";
val saved = archive(take name);
print(name);          // error: name was given away on the line above
```

```
Error [E0121]: `name` was given away earlier, so it can't be used here
 Why: after a value moves somewhere else, the old name no longer holds it
 Fix: give away a copy instead (`name.clone()`) where it moved
```

This is the whole point. In a language with hidden moves, that line would be a
runtime crash or a subtle aliasing bug. In Jet it's a compile error with the
exact line where the value left. You fix it by deciding what you actually
meant: clone it, or restructure so you don't need it afterward.

### Clone when you genuinely want two

```jet
val saved = archive(take name.clone());
print(name);          // fine — archive got a copy, you kept the original
```

`.clone()` is explicit on purpose. Copies cost memory and time, and Jet would
rather you ask for them than discover them in a profiler. (For cheap things
like `Int` and `Bool`, copying is free and automatic — you'll never write
`.clone()` on a number.)

## `view` — borrow it back out

Occasionally a function wants to hand back a borrowed look at something it was
given, rather than a fresh value:

```jet
fn headline(text: String) -> view String {
    return text;
}
```

`view` is the return-side counterpart to the default read-only parameter. You
won't need it often, and there's a guardrail: a `view` can only point back at
something that outlives the call — a parameter, a simple local, a constant —
never freshly made text that would vanish when the function returns. Try that
and you get a clear error instead of a dangling pointer.

## The one rule underneath all of this

Everything above is one idea wearing different hats:

> While something is being changed, nobody else may be looking at it.

That's the rule that lets Jet promise no data races and no use-after-free
without a garbage collector. You don't prove it by hand — the compiler checks
it — but knowing it explains *why* `mut` at the call site, why `take` consumes,
and why two simultaneous `mut`s on the same value in one call are rejected.

> **Under the hood.** Jet compiles to Rust and lets Rust's verifier confirm the
> result, so a default parameter becomes `&T`, `mut` becomes `&mut T`, `take`
> becomes a plain `T`, and `view` becomes a borrowed return with the lifetime
> filled in for you. You never write `&`, `*`, or a lifetime — and in v1 you
> can't, by design. Tier-2 stored references are a later, opt-in chapter the
> language doesn't make you read first.

## What you actually have to remember

- Default parameter: a read-only borrow. Write nothing.
- Changing it? `mut` on the parameter **and** at the call.
- Keeping it? `take` on the parameter **and** at the call.
- Want a copy? `.clone()`, out loud.
- Used-after-give-away is a compile error that names the line, not a crash.

Next: build your own types with [structs, enums, and optionals](03-data.md).
