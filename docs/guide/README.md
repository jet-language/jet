# The Jet guide

Jet is a small compiled language. You write ordinary-looking code; a
compiler turns it into a fast native binary and, along the way, catches the
memory mistakes that usually only show up at 3 a.m. in production. There is
no garbage collector and no `&`, `*`, or lifetime soup to learn first.

Here is a whole program:

```jet
fn main() {
    print("hello, world");
}
```

Run it:

```
jet run hello.jet
```

```
hello, world
```

That's the deal for the whole guide: every page opens with code you can run,
shows what it prints, and only then explains why it works that way. The
precise rules and the names of things are all here too — they're just not the
first thing you read.

## Reading order

Each page builds on the one before it, but they're short. If you've used any
other language you can skim the first three and slow down at **Ownership** —
that's the part of Jet that isn't like the others.

1. [Getting started](01-getting-started.md) — values, branches, loops, functions
2. [Ownership](02-ownership.md) — who owns a value, and how Jet keeps it safe
3. [Structs, enums, and optionals](03-data.md) — your own types
4. [Errors as values](04-errors.md) — no exceptions, no surprises
5. [Lists, maps, and closures](05-collections.md) — collections and the lambdas that work on them
6. [Traits and generics](06-traits-and-generics.md) — shared behavior, written once

## Running anything in this guide

Save a snippet to a file ending in `.jet` and use one of:

```
jet run  file.jet      # compile and run it now
jet build file.jet     # leave a native binary in ./build/
jet test file.jet      # run the test "..." { } blocks in the file
jet fmt  file.jet      # reformat it to the one true style
```

`jet run` on a single file needs nothing else — no project, no manifest, no
config. A folder full of `.jet` files that import each other works the same
way; you only reach for a manifest once you're pulling in outside packages.

## A note on the tone of the errors

When Jet rejects your program it tries to talk like a person who wants you to
succeed, not like a compiler. It names the thing you wrote, explains the rule
behind the complaint, and suggests a concrete fix:

```
Error [E0102]: nothing named `pirnt` exists here
  --> hello.jet:2:5
    |
  2 |     pirnt("hi")
    |     ^^^^^
 Why: only functions that have been defined (or built in, like `print`) can be called
 Fix: did you mean `print`?
```

You'll see real errors like this throughout the guide, because reading them is
part of learning the language.
