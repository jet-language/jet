# Errors as values

Jet has no exceptions. A function that can fail says so in its return type, and
the failure travels back to you as an ordinary value you can't accidentally
ignore. If that sounds like the optionals chapter — a result you have to
acknowledge — it's the same idea with a reason attached.

```jet
enum ParseError {
    Empty;
    BadDigit(String);
}

fn parse_age(raw: String) -> Int ? ParseError {
    if raw == "" {
        return err(ParseError.Empty);
    }
    if raw == "x" {
        return err(ParseError.BadDigit(raw));
    }
    return ok(42);
}
```

A fallible function returns `T ? E` — `T` on success, `E` on failure. You
build the two outcomes with `ok(value)` and `err(reason)`. The error type is
just a type you choose: an enum like `ParseError` when you want named cases, or
a plain `String` when a message is enough.

When a message is enough and you do not care about a custom error type, leave
the error type off:

```jet
fn read_count(path: String) -> Int ? {
    return err("could not read count");
}
```

`Int ?` uses Jet's default `Error` type.

Now the interesting part is how you *consume* one, because Jet gives you three
ways and each is the right one somewhere.

## `or` — give me a fallback and move on

```jet
val age = parse_age("42") or 0;
print(age);
```

```
42
```

`or` says "the success value, or this if it failed." It's the quickest path
when you have a sensible default. The right-hand side can also bail out instead
of supplying a value — `parse_age(s) or return`, or `or panic("bad input")` —
so `or` covers "recover" and "give up" both.

## `?` — pass the failure up to my caller

```jet
fn load() -> Int ? ParseError {
    val n = parse_age("7")?;
    return ok(n * 2);
}
```

The `?` is the workhorse. On success it hands you the inner value and keeps
going; on failure it stops the function then and there and returns the error to
*your* caller. Your code reads like the happy path — `val n =
parse_age("7")?;` — with the error handling factored out into one character.
The catch: `?` can only live in a function that itself returns a compatible
fallible type, because that's where the early error goes.

## `switch` — handle each outcome explicitly

When you actually want to do different things for success and failure, switch
over the result:

```jet
switch parse_age("x") {
    it == ok(n)  -> { print(n); };
    it == err(e) -> { print(e); };
}
```

The subject is an expression rather than a plain name, so Jet lets you call it
`it` inside the arms. `ok(n)` binds the success value to `n`; `err(e)` binds the
error to `e` — the same test-and-bind move as optionals.

## When it's a bug, not an error

Some failures aren't recoverable conditions — they're "this should never
happen," and continuing would be worse than stopping:

```jet
require(age >= 0);
require(age >= 0, "age went negative");
panic("unreachable: the parser already validated this");
```

`require` checks a condition and `panic` stops outright. Both print a friendly
report to stderr and exit — they're for programmer mistakes, not for the user
typing the wrong thing. The rule of thumb: if a caller could reasonably handle
it, return an `err`; if it means your own code is wrong, `require`/`panic`.

## main can't quietly fail

`main` may not return a fallible type. At the top of your program you have to decide
what a failure *means* — print something and exit, fall back to a default, or
panic — using the same `or` / `switch` / `panic` tools. Failures don't get to
disappear off the top of the stack.

## What you actually have to remember

- Fallible functions return `T ? E`, or `T ?` for the default `Error`; build outcomes with `ok` / `err`.
- `value or fallback` — recover or bail with a default.
- `value?` — propagate the error to your caller (only inside a fallible function).
- `switch` with `it == ok(n)` / `it == err(e)` — handle both sides yourself.
- `require` / `panic` are for bugs, not for expected bad input.

One syntax edge: in a function return type, `T?` is formatted as `T ?` and
means a fallible return with the default `Error`. If a function really returns
an optional value, parenthesize it: `fn find() -> (Int?)`.

Next: [lists, maps, and the closures that work on them](05-collections.md).
