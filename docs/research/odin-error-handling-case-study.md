# Odin-style error handling case study for Jet

## Short version

Jet has the important foundation: errors are values, not exceptions. A
fallible Jet function returns `T ? E`, callers must handle that result with
`?`, `or`, or a `switch`, and `panic` is reserved for bugs.

The Odin lesson is not "copy Odin exactly." Odin has multiple return values,
so a procedure commonly returns `(value, error)` or `(value, ok)`. Jet keeps a
single fallible return surface, but gives the useful value and error code both
places in the function type:

```jet
pub fn TestFunc(value: Int) -> Int ? String {
    if value < 0 {
        return err("value must be non-negative");
    }
    return ok(value * 2);
}
```

Read aloud: `TestFunc` returns `Int`, unless it fails, in which case the error
is a `String`.

When the exact error type does not matter, Jet uses a default `Error` type:

```jet
pub fn LoadConfig(path: String) -> Config ? {
    val text = read_file(path)?;
    return ok(parse_config(text)?);
}
```

This means `Config ? Error`. The current implementation backs `Error` with
`String`; the intended long-term shape is a richer error carrier plus an
explicit conversion trait.

## What Jet does today

Current Jet error handling is described in `docs/guide/04-errors.md`,
`docs/plans/epoch-1/m04-errors.md`, and the living spec in
`docs/admin/01-spec.md`.

The current model is:

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

fn load() -> Int ? ParseError {
    val n = parse_age("7")?;
    return ok(n * 2);
}
```

The fundamentals are good:

- A fallible function says so in its type.
- The success value and error value are ordinary values.
- The caller cannot silently use `Int ? E` as if it were `Int`.
- `?` propagates a failure to the caller.
- `or` gives a fallback or an explicit bailout.
- `switch` handles success and error explicitly.
- `panic` and `require` are for programmer mistakes, not normal user errors.

The important notation choice is that Jet does not expose `Result<T, E>` as
user syntax. `Result` remains an implementation/lowering concept; the language
surface is `T ? E`.

## Odin's error handling in plain language

Odin does not use exceptions for normal errors. It treats error information as
ordinary return data.

In practice, Odin code often looks like this:

```odin
f, err := os.open("my_file.txt")
if err != os.ERROR_NONE {
    // handle error
}
defer os.close(f)
```

The real-world idea is simple:

1. Try to open the file.
2. You get both a file handle and an error value.
3. If the error value says something went wrong, handle it immediately.
4. If it worked, keep using the file.
5. `defer` makes cleanup visible near the acquisition.

That is different from exception-based languages:

```text
open file
read file
parse file
save settings
```

In an exception language, any of those lines might secretly jump away unless
you know the called API. In Odin, the possible failure is part of the values
being returned.

### Multiple returns

Odin has multiple return values, so a procedure can return a useful value and
an error side by side:

```odin
read_user :: proc(id: int) -> (User, Error) {
    ...
}

user, err := read_user(42)
if err != nil {
    return err
}
```

For a grocery-store example, imagine asking an employee for a product:

```text
find_item("milk") -> (aisle_number, problem)
```

The answer is not a thrown exception. It is a pair:

- `(7, none)` means "milk is in aisle 7."
- `(0, out_of_stock)` means "there is no aisle number because milk is out."
- `(0, unknown_item)` means "we do not carry that item."

The caller can decide what to do: show aisle 7, suggest a substitute, or ask
for help.

Jet should preserve that power by making the "both" case explicit in the
success value. A function that can produce partial data plus a status code can
return a struct as the success payload and reserve `err(...)` for the case
where there is no usable value:

```jet
enum ImportCode {
    Clean;
    SkippedRows(Int);
}

struct ImportReport {
    saved: Int;
    code: ImportCode;
}

fn import_rows(path: String) -> ImportReport ? {
    val rows = read_rows(path)?;
    if rows.len() == 0 {
        return err("no rows to import");
    }
    return ok(ImportReport {
        saved: 41,
        code: ImportCode.SkippedRows(2),
    });
}
```

That keeps the ternary shape available:

- `ok(report with Clean)` means full success.
- `ok(report with SkippedRows(n))` means usable result plus status.
- `err(message)` means no usable result.

The key design rule is that `?` only propagates the third case. It should not
silently discard a status code attached to a usable value.

### `or_return`

Odin also has `or_return`, which shortens the common "if error, return it"
pattern.

Conceptually, this:

```odin
user, err := read_user(id)
if err != nil {
    return err
}
```

can become this:

```odin
user := read_user(id) or_return
```

Plain language: "Get the user. If that failed, stop this function and pass the
failure upward. Otherwise, give me the user."

This is the same job Jet's `?` already does:

```jet
val user = read_user(id)?;
```

### `or_else` and `Maybe(T)`

Odin has `Maybe(T)`, which is similar to an optional. It means "there might be
a `T`, or there might be nothing." Odin also supports fallback-style operators
such as `or_else`.

Real-world example:

```text
find_coupon(customer) or_else no_discount
```

That means "use the customer's coupon if one exists; otherwise continue with
no discount." Jet's `or` already covers the same ergonomic space:

```jet
val discount = find_coupon(customer) or 0;
```

### `or_continue`

Odin also has `or_continue`, useful in loops. It lets a loop skip failed items
without turning the whole loop body into nested error checks.

Real-world example: processing imported rows from a spreadsheet.

```text
for each row:
    parse row, or skip this row
    validate row, or skip this row
    save row
```

This is not the same as fatal failure. One bad row should not stop the import.
That distinction matters: good error handling lets code say whether failure
means "stop everything," "use a fallback," "skip this item," or "handle here."

### Implicit context

Odin has an implicit `context` value in each scope. It is passed to Odin
procedure calls and can carry cross-cutting behavior such as allocation,
logging, and error-related policy.

The useful idea for Jet is not necessarily implicit context itself. The useful
idea is central policy without noisy parameter passing. A program can have a
default way to allocate, log, or shape errors, while still allowing local code
to override it.

For Jet, this points toward a standard `Error` type and a conversion trait
rather than every function manually choosing `String` or a custom enum.

## The general method

The general method behind Odin, Go, Rust, and current Jet is:

1. Expected failures are data.
2. Function signatures show whether failure can happen.
3. Callers must make a choice.
4. The language gives a short path for the boring choice.
5. Crashes are reserved for bugs.

Here are the common choices in real-world terms.

### Recover here

The user did not provide a setting, so use a default:

```jet
val retries = read_int("retries") or 3;
```

This is not exceptional. It is a normal business rule.

### Propagate upward

Loading a project requires reading a file, parsing JSON, and validating fields.
The middle function does not know how to present the error to the user, so it
passes it upward:

```jet
fn load_project(path: String) -> Project ? {
    val text = read_file(path)?;
    val json = parse_json(text)?;
    return ok(validate_project(json)?);
}
```

The function reads as a straight-line recipe, but every `?` is a visible exit
point.

### Handle explicitly

The top-level UI knows how to explain each case:

```jet
switch load_project(path) {
    it == ok(project) -> {
        open(project);
    };
    it == err(e) -> {
        show_error(e);
    };
}
```

This is where specific user-facing decisions belong.

### Panic

An internal invariant broke:

```jet
require(index >= 0);
```

That is not a recoverable user error. It means the program's own logic is
wrong.

## Proposed Jet direction

### 1. Keep `T ? E` as the public model

Jet should expose only one fallible type spelling:

```jet
fn f() -> Int ? String
```

Internally, the compiler may still lower that to Rust `Result<Int, String>`.
That is an implementation detail, not a second Jet spelling.

### 2. Add inferred default error syntax

Jet allows:

```jet
fn f() -> Int ?
```

as shorthand for `Int ? Error`, where `Error` is the prelude default error
type.

This matches the philosophy: safe by default, explicit when needed. Beginners
can write fallible code without designing an error enum on day one. Library
authors can still expose precise errors:

```jet
pub fn parse_age(raw: String) -> Int ? ParseError
```

### 3. Add an error conversion trait

Jet could define a trait like:

```jet
trait IntoError {
    fn into_error(self) -> Error;
}
```

Then `?` in a `-> T ?` function can convert compatible lower-level errors into
the default `Error`.

Example:

```jet
fn load_profile(path: String) -> Profile ? {
    val text = read_file(path)?;       // FileError -> Error
    val data = parse_json(text)?;      // JsonError -> Error
    return ok(Profile.from_json(data)?); // ProfileError -> Error
}
```

Without conversion, the same function would need to manually wrap each error.
With conversion, the happy path stays readable while the type system still
knows every fallible call is being handled.

For custom APIs, callers can choose precision:

```jet
fn load_profile(path: String) -> Profile ? LoadProfileError {
    ...
}
```

In that form, `?` should only propagate errors that are already
`LoadProfileError` or can be converted into it by an explicit trait
implementation.

### 4. Prefer explicit conversion over magical conversion

The conversion trait should be opt-in. `String` can convert to `Error` by
default, and standard library error types can convert to `Error` by default.
But arbitrary unrelated error enums should not silently collapse into each
other.

Good:

```jet
impl FileError: IntoError {
    fn into_error(self) -> Error {
        return Error.message("file error: {self}");
    }
}
```

Risky:

```text
Any type can become any error automatically.
```

That would make `?` convenient but too vague. Jet should make the common path
easy without erasing meaning accidentally.

### 5. Keep `main` explicit for now

Current Jet says `main` may not return a fallible type. That rule is defensible
because the top of a program should decide what failure means.

If Jet later allows:

```jet
fn main() -> Unit ? {
    run_app()?;
}
```

then it should have a pinned default behavior: print the default `Error` and
exit with a known non-zero code. Until that behavior is designed, explicit
top-level handling is clearer.

## Syntax notes

The proposed syntax:

```jet
pub fn TestFunc(value: Int) -> Int ? String {}
```

shares a sigil with optionals:

```jet
fn maybe_id() -> (Int?)
```

So the parser needs a clear rule:

- `T ? E` in a function return is fallible `T ? E`.
- `T ?` in a function return is fallible `T ? Error`.
- Users may write `T?` in a function return; the formatter canonicalizes it to
  `T ?`.
- A function that returns an optional writes `-> (T?)`.
- Outside a return position, `T?` remains optional and explicit fallible
  annotations write `T ? E`.

## Recommended design

Adopt the Odin-inspired principle, but keep Jet's existing result model.

Recommended design:

1. Keep `T ? E`, `ok`, `err`, `?`, `or`, `switch`, `panic`, and `require` as
   the semantic foundation.
2. Use `fn f() -> T ? E` for explicit errors.
3. Use `fn f() -> T ?` for the default `Error`.
4. Grow the prelude `Error` type to hold at least a message, optional code,
   and optional source/context.
5. Add an explicit conversion trait used by `?` when the enclosing function's
   error type differs from the callee's error type.
6. Keep precise custom error enums for libraries and domains where callers
   need to branch on cases.
7. Keep `panic` for broken invariants only.

This gives Jet the same practical win people like in Odin: error paths are
visible, cheap, ordinary, and hard to forget. It also keeps Jet's current
strength: a single, teachable fallible-value model with compiler-enforced
handling.

## Sources

- Jet current guide: `docs/guide/04-errors.md`
- Jet M4 plan: `docs/plans/epoch-1/m04-errors.md`
- Jet living spec: `docs/admin/01-spec.md`
- Odin overview: https://odin-lang.org/docs/overview/
