# 08 — Syntax Gallery (one syntax choice, every language)

**Status:** owner reading material. A browsing tool, not a decision record.

Each section isolates **one syntax choice** and shows how a broad set of
languages spell it, in code blocks. Languages that spell it *identically* are
grouped into one block to cut repetition. Jet's current choice (per
`src/syntax.rs` / `02-syntax-decisions.md`) is called out as **▶ Jet today** so
you can compare directly and pick a favorite. Where Jet hasn't decided, it says
**▶ Jet: undecided**.

Languages sampled across the doc (not all appear in every section): Rust, Go,
Zig, Swift, Kotlin, Java, C#, C, C++, Python, Ruby, JavaScript, TypeScript,
Scala, Haskell, OCaml, F#, Elm, Gleam, Elixir, Erlang, Clojure, Julia, Nim,
Crystal, Dart, V, Odin, Hare, Roc, Koka, Grain, Unison, Lua, PHP, Mojo, Carbon.

Legend: code blocks show the *minimal* idiomatic form. Comments in blocks are
clarifying notes, not part of the comparison.

---

## 1. Immutable vs. mutable variable bindings

```rust
// Rust
let x = 1;          // immutable
let mut y = 1;      // mutable
```
```swift
// Swift, Kotlin (val/var), Scala (val/var)
let x = 1           // Swift immutable
var y = 1           // Swift mutable
```
```kotlin
// Kotlin, Scala
val x = 1
var y = 1
```
```go
// Go
x := 1              // both mutable; immutability only via `const`
const c = 1
```
```javascript
// JavaScript / TypeScript
const x = 1;
let y = 1;
```
```python
# Python, Ruby, Lua, Julia, Elixir(rebind) — no keyword, all mutable
x = 1
```
```haskell
-- Haskell, Elm, F#, OCaml — bindings immutable by default, no keyword
let x = 1            -- (F#/OCaml)   |   x = 1  (Haskell top level)
```
```nim
# Nim
let x = 1            # immutable
var y = 1            # mutable
const c = 1          # compile-time
```
```zig
// Zig
const x = 1;
var y: i32 = 1;
```

**▶ Jet today (S2):** `val` (immutable) / `var` (mutable). Rejected `let`,
`let mut`, `set`. Matches Swift/Kotlin/Scala — the most widely-loved spelling.

---

## 2. Function definition

```rust
// Rust, (Mojo `fn`)
fn add(a: i32, b: i32) -> i32 { a + b }
```
```go
// Go
func add(a int, b int) int { return a + b }
```
```swift
// Swift
func add(_ a: Int, _ b: Int) -> Int { a + b }
```
```kotlin
// Kotlin
fun add(a: Int, b: Int): Int { return a + b }
fun add(a: Int, b: Int) = a + b      // expression body
```
```python
# Python
def add(a, b): return a + b
```
```javascript
// JavaScript / TypeScript
function add(a, b) { return a + b }
const add = (a, b) => a + b;
```
```haskell
-- Haskell
add :: Int -> Int -> Int
add a b = a + b
```
```ocaml
(* OCaml / F# *)
let add a b = a + b
```
```scala
// Scala
def add(a: Int, b: Int): Int = a + b
```
```zig
// Zig
fn add(a: i32, b: i32) i32 { return a + b; }
```
```elixir
# Elixir
def add(a, b), do: a + b
```
```nim
# Nim:  proc add(a, b: int): int = a + b      (func for pure)
```

**▶ Jet today (S1):** `fn add(a: Int, b: Int) -> Int { ... }`. Rust/Zig family.
`func`/`def` produce teaching errors (S14). Return type after `->`.

---

## 3. Calling a function / method (UFCS question)

```c
// C, Java, JS, Python, Go, Rust, Swift, Kotlin — classic call
result = add(a, b);
obj.method(arg);
```
```nim
# Nim, D, (Koka dot-calls) — Uniform Function Call Syntax
result = add(a, b)
result = a.add(b)         # same call, method-style; any fn callable as method
echo "hi".len             # len("hi")
```
```elixir
# Elixir / F# — pipe replaces method chains
[1,2,3] |> Enum.map(&double/1) |> Enum.sum()
```
```haskell
-- Haskell — application by juxtaposition, no parens/commas
add a b
```
```ruby
# Ruby — parens optional
add a, b
obj.method arg
```

**▶ Jet today:** classic `add(a, b)` and `obj.method(arg)` (S27 receivers).
UFCS (Nim-style) is **undecided** — see `07` §Nim; big ergonomics, smallness
cost.

Owner: What are the tradeoffs of the nim version? It seems nice & convenient, but there are no free lunches.

---

## 4. Comments

```rust
// Rust, Go, Zig, C, C++, Java, JS, TS, Swift, Kotlin, Dart, Scala, V
// line comment
/* block comment */
```
```python
# Python, Ruby, Elixir, Nim, Julia, Crystal, R, Perl, shell
# line comment
```
```haskell
-- Haskell, Elm, Lua(--), SQL, Ada
-- line comment
{- block (Haskell/Elm) -}
```
```lisp
; Clojure / Lisp
; line comment
```

**▶ Jet today (S5):** `//` line comments. No block-comment decision yet
(undecided). Doc-comment spelling also undecided.

Owner: I want to add block comments with the same syntax as Rust/Go/C++

---

## 5. String interpolation

```swift
// Swift
"hi \(name), you are \(age)"
```
```kotlin
// Kotlin, Scala (s"..."), Dart, Groovy
"hi $name, you are ${age + 1}"
```
```javascript
// JavaScript / TypeScript — template literals (backticks)
`hi ${name}, you are ${age}`
```
```python
# Python f-strings, Ruby (#{}), CoffeeScript
f"hi {name}, you are {age}"      # Python
"hi #{name}"                      # Ruby / Elixir / Crystal
```
```rust
// Rust
format!("hi {name}, you are {age}")
println!("hi {}", name)
```
```nim
# Nim
fmt"hi {name}"
```

**▶ Jet today (S8):** `"hi {name}"` — braces inside quoted text, `{{`/`}}` for
literals (S20). Matches Python f-string/C#/Kotlin family. No `+` concat for
strings (one obvious way).

Owner: I like the highlighting for the javascript/python curly braces that make it clear they are part of the string interpolation syntax, rather than string itself. We need to implement this.

---

## 6. Multi-line / raw strings

```python
# Python, Julia, Elixir (triple-quoted)
"""
multi
line
"""
```
```rust
// Rust raw string
r#"no \escapes "quotes" ok"#
```
```swift
// Swift multiline
"""
multi
line
"""
```
```go
// Go raw string (backticks, no escapes)
`multi
line`
```
```scala
// Scala stripMargin
"""|multi
   |line""".stripMargin
```
```zig
// Zig — line-prefixed multiline
\\multi
\\line
```

**▶ Jet today:** **undecided.** Candidates: triple-quote (Python/Swift), Go
backticks, Zig `\\` prefix. Interpolation-in-multiline behavior also TBD.

Owner: Let's implement triple quote style.

---

## 7. If / else (statement vs. expression)

```rust
// Rust, Kotlin, Scala, Swift(if expr), Nim — if is an expression
let m = if a > b { a } else { b };
```
```c
// C, Java, Go, JS, TS, Dart, Zig — if is a statement
if (a > b) { m = a; } else { m = b; }
```
```python
# Python — conditional expression
m = a if a > b else b
```
```haskell
-- Haskell, Elm, F#, OCaml — if/then/else expression
m = if a > b then a else b
```
```ruby
# Ruby — if expression + trailing modifier
m = if a > b then a else b end
puts "big" if a > b
```

**▶ Jet today (KW_IF/KW_ELSE):** `if cond { } else { }`, C-family braces but no
parens around the condition. Whether `if` *yields a value* (expression form) is
worth confirming against the Rust/Kotlin model.

Owner: I want to support both expression & statement. I also want to support putting the conditional expression inside of parenthesis for readability, but it is supported in both forms.

---

## 8. Pattern matching / multi-way branch

```rust
// Rust
match x {
    0 => "zero",
    1..=9 => "digit",
    _ => "many",
}
```
```kotlin
// Kotlin
when (x) {
    0 -> "zero"
    in 1..9 -> "digit"
    else -> "many"
}
```
```swift
// Swift
switch x {
case 0: "zero"
case 1...9: "digit"
default: "many"
}
```
```scala
// Scala, (Haskell `case of`, Elm `case of`, F#/OCaml `match with`)
x match {
  case 0 => "zero"
  case n if n < 10 => "digit"
  case _ => "many"
}
```
```elixir
# Elixir
case x do
  0 -> "zero"
  n when n < 10 -> "digit"
  _ -> "many"
end
```
```go
// Go — switch, no fallthrough by default
switch {
case x == 0: return "zero"
case x < 10: return "digit"
default:     return "many"
}
```

**▶ Jet today (S24):** keyword **`switch`**, arms use `->`. `match`/`case`/
`default` are teaching errors. Exhaustiveness is the design intent (M3 enums).
Spelling is Go-ish keyword + Rust-ish arrow arms.

Owner: I want to change the keyword to when from switch. 

---

## 9. Loops (iteration)

```rust
// Rust, (Swift `for x in`, Kotlin `for (x in)`, Nim `for x in`)
for x in items { }
for i in 0..10 { }
while cond { }
loop { }            // Rust infinite
```
```go
// Go — one keyword
for i := 0; i < 10; i++ { }
for x := range items { }
for cond { }
```
```python
# Python, (Ruby `.each`, JS `for..of`)
for x in items: ...
while cond: ...
```
```c
// C, Java, JS, C++ — classic three-part
for (int i = 0; i < 10; i++) { }
```
```elixir
# Elixir — comprehension / recursion, no mutable for-loop
for x <- items, do: f(x)
```

**▶ Jet today (S19):** `while cond { }` and `for i in <range> { }`. No C-style
three-part loop. `loop` keyword exists (S-loop) for the infinite case.

---

## 10. Ranges

```rust
// Rust
0..10       // half-open (excludes 10)
0..=10      // inclusive
```
```swift
// Swift
0..<10      // half-open
0...10      // inclusive
```
```kotlin
// Kotlin
0 until 10  // half-open
0..10       // inclusive
0..10 step 2
```
```python
# Python, Go(no literal) — half-open via range()
range(0, 10)        # 0..9
```
```ruby
# Ruby
0...10      // exclusive
0..10       // inclusive
```
```julia
# Julia, MATLAB
0:10        // inclusive
0:2:10      // with step
```

**▶ Jet today (S22):** `1..10` is **inclusive** (counts 1 through 10) — chosen
to kill the beginner off-by-one. Note this differs from Rust/Swift half-open
`..`; matches Ruby `..`, Kotlin `..`, Julia `:`.

---

## 11. Collection literals (list / map / set)

```python
# Python
[1, 2, 3]
{"a": 1, "b": 2}
{1, 2, 3}            # set
```
```rust
// Rust
vec![1, 2, 3]
HashMap::from([("a", 1)])
```
```go
// Go
[]int{1, 2, 3}
map[string]int{"a": 1}
```
```swift
// Swift
[1, 2, 3]
["a": 1, "b": 2]
Set([1, 2, 3])
```
```elixir
# Elixir
[1, 2, 3]
%{"a" => 1}          # map
MapSet.new([1,2,3])
```
```clojure
; Clojure
[1 2 3]              ; vector
{:a 1 :b 2}          ; map
#{1 2 3}             ; set
```

**▶ Jet today:** list type is `[T]` (canonical, S65) / `List<T>` accepted;
`Map` (S38). Literal *syntax* for constructing maps/sets is worth confirming —
Swift's `["a": 1]` and Python `{}` are the legible front-runners.

---

## 12. Optional / null safety

```swift
// Swift — the gold-standard UX
var x: Int? = nil
if let v = x { use(v) }
let y = x ?? 0           // default
let z = obj?.field       // optional chaining
guard let v = x else { return }
```
```kotlin
// Kotlin
var x: Int? = null
val y = x ?: 0           // Elvis / default
val z = obj?.field       // safe call
x?.let { use(it) }
```
```rust
// Rust
let x: Option<i32> = None;
let y = x.unwrap_or(0);
if let Some(v) = x { use(v) }
```
```haskell
-- Haskell / Elm (Maybe), F#/OCaml (option)
x :: Maybe Int          -- Just v | Nothing
fromMaybe 0 x
```
```typescript
// TypeScript
let x: number | null = null;
const y = x ?? 0;
const z = obj?.field;
```

**▶ Jet today (S32/S35):** `Int?` is "maybe an Int"; `value`/`null` spellings
(lowercase). `or` provides the fallback (S35, like Kotlin `?:` / Swift `??`).
**Undecided:** optional chaining (`?.`) and a `guard`/`if let`-style binding —
both strong Swift/Kotlin ergonomics worth a decision (see `07` §Swift).

---

## 13. Error handling

```rust
// Rust — Result + `?` propagation
fn read() -> Result<String, Error> {
    let s = fs::read_to_string(p)?;
    Ok(s)
}
```
```go
// Go — explicit error value, no propagation operator
v, err := doThing()
if err != nil { return err }
```
```zig
// Zig — error union `!T`, `try`, `errdefer`
fn read() !File {
    const f = try openFile(p);   // `try` = propagate
    errdefer f.close();
    return f;
}
```
```swift
// Swift — typed throws + try/catch
do { let s = try read() } catch { handle(error) }
```
```java
// Java, C#, Python, C++, JS — exceptions
try { risky(); } catch (IOException e) { handle(e); }
```
```ocaml
(* OCaml/F# Result, Haskell Either, Elm Result *)
match read p with Ok s -> ... | Error e -> ...
```
```gleam
// Gleam — Result + `use` to flatten
use s <- result.try(read(p))
Ok(s)
```

**▶ Jet today (S7/S34/S35):** fallible type `T ? E`; `ok`/`err` constructors;
`?` suffix propagates; `or` gives a fallback. No exceptions (`throw`/`catch`/
`unwrap` are teaching errors). This is the Rust/Zig family with Jet spelling.
A cleanup primitive (Zig `errdefer` / Go `defer`) is **undecided** and recurs
across `07` — pairs naturally with the `transact` proposal.

---

## 14. Anonymous functions / closures

```rust
// Rust
|x| x + 1
|x: i32| -> i32 { x + 1 }
```
```javascript
// JavaScript / TypeScript, (Java/C# `->`/`=>`, Dart, Scala `=>`)
x => x + 1
(x, y) => x + y
```
```kotlin
// Kotlin — braces, implicit `it`
{ x -> x + 1 }
list.map { it + 1 }
```
```swift
// Swift — trailing closure
{ x in x + 1 }
list.map { $0 + 1 }
```
```python
# Python
lambda x: x + 1
```
```haskell
-- Haskell, Elm, F#/OCaml(fun)
\x -> x + 1
```
```elixir
# Elixir
fn x -> x + 1 end
&(&1 + 1)            # capture shorthand
```

**▶ Jet today (S46):** lambda arrow is **`=>`** (distinct from `->` used for
return types and switch arms). JS/Scala/C# family. `lambda`/`|...|` are
teaching errors (E0032/E0033).

---

## 15. Pipelines / chaining

```elixir
# Elixir, F#, Gleam (|>), OCaml, Julia (|>), Elm
data
|> transform
|> filter(pred)
|> sum
```
```clojure
; Clojure threading macros
(->> data (map f) (filter p) (reduce +))
```
```rust
// Rust, Swift, Kotlin, JS, Scala — method chaining (no pipe operator)
data.iter().map(f).filter(p).sum()
```

**▶ Jet: undecided.** A pipe operator (`|>`) is the single most-recurring
"nice to have" in the survey (`07` §8). Decision: pipe operator vs.
method-chaining-only. Front-runner spelling is F#/Elixir `|>`.

---

## 16. Type annotation placement

```rust
// Rust, Swift, Kotlin, Scala, TS, Python, Go(after), Zig — name : Type
let x: Int = 1
fn f(a: Int) -> Int
```
```go
// Go — type after name, no colon
var x int = 1
func f(a int) int
```
```c
// C, Java, C++, C# — type before name
int x = 1;
int f(int a);
```
```haskell
-- Haskell, Elm — separate signature line, `::`
x :: Int
f :: Int -> Int
```

**▶ Jet today:** `name: Type` (post-name colon) — Rust/Swift/Kotlin/TS family.
Used in bindings, params, and `-> Type` returns.

---

## 17. Struct / record definition & construction

```rust
// Rust
struct Point { x: i32, y: i32 }
let p = Point { x: 1, y: 2 };
```
```go
// Go
type Point struct { X, Y int }
p := Point{X: 1, Y: 2}
```
```swift
// Swift, (Kotlin data class, Scala case class)
struct Point { var x: Int; var y: Int }
let p = Point(x: 1, y: 2)
```
```typescript
// TypeScript — structural
type Point = { x: number; y: number }
const p = { x: 1, y: 2 };
```
```ocaml
(* OCaml / F# / Elm / Haskell records *)
type point = { x : int; y : int }
let p = { x = 1; y = 2 }
```

**▶ Jet today (S29, KW_STRUCT):** `struct Point { x: Int, y: Int }`; fields are
private-by-default (S18). Construction spelling is S29. Rust/Swift family.
`class` is a teaching error (E0021).

---

## 18. Sum types / enums / tagged unions

```rust
// Rust, Swift(enum with assoc), (Scala enum)
enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
}
```
```haskell
-- Haskell, Elm, (PureScript)
data Shape = Circle Float | Rect Float Float
```
```ocaml
(* OCaml / F# *)
type shape = Circle of float | Rect of float * float
```
```typescript
// TypeScript — discriminated union
type Shape =
  | { kind: "circle"; r: number }
  | { kind: "rect"; w: number; h: number };
```
```zig
// Zig — tagged union
const Shape = union(enum) { circle: f64, rect: Rect };
```

**▶ Jet today (S30, KW_ENUM):** `enum` for sum types. `Option` is `T?` (S32),
`Result` is `T ? E` (S34) — so the two most common sums get sugar rather than
explicit enum spelling.

---

## 19. Generics

```rust
// Rust, (C#, Java, Kotlin, Swift, TS, Scala — all `<T>`)
fn first<T>(xs: Vec<T>) -> T { ... }
```
```go
// Go (1.18+)
func First[T any](xs []T) T { ... }
```
```haskell
-- Haskell, Elm, F#/OCaml — lowercase type vars, no brackets
first :: [a] -> a
```
```typescript
// TypeScript
function first<T>(xs: T[]): T { ... }
```

**▶ Jet today:** generics are M9 / Tier 2 (traits-or-comptime path, S28/S57).
`[T]` is the list type (S65). Bracket choice for user generics (`<T>` vs `[T]`)
interacts with the `[T]` list literal — worth a deliberate decision.

---

## 20. Traits / interfaces / typeclasses

```rust
// Rust
trait Area { fn area(&self) -> f64; }
impl Area for Circle { fn area(&self) -> f64 { ... } }
```
```go
// Go — structural, implicit
type Area interface { Area() float64 }
```
```haskell
-- Haskell typeclass, (Scala given/trait, Swift protocol)
class Area a where area :: a -> Float
instance Area Circle where area c = ...
```
```swift
// Swift protocol
protocol Area { func area() -> Double }
extension Circle: Area { func area() -> Double { ... } }
```
```kotlin
// Kotlin / Java / C# / TS / Dart
interface Area { fun area(): Double }
```

**▶ Jet today (S28):** keyword **`trait`** + `impl Type { }` (S27).
`interface` is a teaching error (E0022). Rust/Swift-protocol family, nominal.

---

## 21. Module / import

```python
# Python
import os
from os import path as p
```
```rust
// Rust
use std::collections::HashMap;
use std::fmt::{self, Display};
```
```go
// Go
import "fmt"
import ( "fmt"; "os" )
```
```javascript
// JavaScript / TypeScript
import { foo, bar as baz } from "./mod";
import * as M from "./mod";
```
```java
// Java, C#(using), Kotlin, Scala
import java.util.List;
```
```haskell
-- Haskell, Elm
import Data.Map as M exposing (lookup)
```

**▶ Jet today (S16):** `import` with optional `as`; file-path or module-name
based (M6). `use`/`def`-style imports are teaching errors. First-party roots
reserved (S51): `std`, `jet`, `http`, etc.

---

## 22. Visibility / export

```go
// Go — capitalization decides export (no keyword)
func Exported() {}     // exported
func internal() {}     // package-private
```
```rust
// Rust
pub fn exported() {}
pub(crate) fn within() {}
```
```kotlin
// Kotlin, Swift, C#, Java, Scala — keyword, public default varies
public fun f() {}
private fun g() {}
internal fun h() {}    // Kotlin
```
```typescript
// TypeScript — `export` prefix
export function f() {}
```

**▶ Jet today (S18):** **private by default**, prefix `pub` to export.
Rust-family. Rejected: public-by-default (Go), explicit `private`. Grouped
`pub { }` blocks explicitly declined (hollows out the default).

---

## 23. Named & default arguments

```python
# Python — both
def f(a, b=10, *, c): ...
f(1, c=3)              # b defaults to 10
```
```swift
// Swift — argument labels (mandatory unless `_`)
func move(to p: Point, animated: Bool = true)
move(to: dest, animated: false)
```
```kotlin
// Kotlin
fun f(a: Int, b: Int = 10) { }
f(a = 1, b = 2)
```
```gleam
// Gleam — labelled args
pub fn replace(in s: String, each x: String) { }
replace(in: "a", each: "b")
```
```c
// C, Java, Go, Rust — NO named/default args (positional only)
f(1, 10);              // Rust/Go: no defaults, no labels
```

**▶ Jet: undecided.** High-value readability feature present in Swift, Kotlin,
Python, C#, Gleam, Ruby. Rust/Go/Zig deliberately omit it. A top candidate per
`07` §8. Decision: support named args? default values? labels like Swift/Gleam?

---

## 24. Statement terminator

```c
// C, Java, Rust, Zig, Swift(optional), JS(optional), C#, C++ — semicolons
let x = 1;
```
```python
# Python, Ruby, Go, Kotlin, Scala 3, Lua, Elixir, Swift — newline-terminated
x = 1
```
```go
// Go — semicolons inserted automatically by the lexer at newlines
x := 1
```

**▶ Jet today (S6):** **semicolons required after every statement**, including
the last before `}`. One rule, no exceptions. Rejected newline-termination and
optional-before-`}`. (Stricter than most peers — a deliberate consistency call.)

---

## 25. Equality & comparison

```c
// Almost universal: C, Java, Rust, Go, Swift, Kotlin, Python, JS(===), Zig
a == b
a != b
a < b   a <= b   a > b   a >= b
```
```lua
-- Lua, (SQL `<>`)
a ~= b                 -- not-equal
```
```haskell
-- Haskell, (OCaml `<>`/`=`)
a /= b                 -- not-equal
```
```javascript
// JavaScript — strict vs loose
a === b   a !== b      // strict (recommended)
```

**▶ Jet today (S13):** `== != < <= > >=`. The universal C-family set.

---

## 26. Boolean / logical operators

```c
// C, Java, Rust, Go, Swift, Kotlin, JS, Zig, C#, Dart
a && b    a || b    !a
```
```python
# Python, (Ruby also has &&/||)
a and b    a or b    not a
```
```haskell
-- Haskell, F#/OCaml
a && b    a || b    not a
```
```pascal
// Pascal, Ada, VB
a and b    a or b    not a
```

**▶ Jet today (S13):** `&& || !`. The word forms `and`/`or`/`not` are
recognized only to emit a teaching error (S14). C-family.

---

## 27. Type conversion / cast

```rust
// Rust
x as i64
i64::from(x)
```
```go
// Go, (C-style cast in C/Java/C#)
int64(x)
```
```swift
// Swift, Kotlin
Int64(x)               // initializer
x as? String           // safe downcast
```
```python
# Python
int(x)   str(x)   float(x)
```
```haskell
-- Haskell
fromIntegral x
```

**▶ Jet today:** `as` is reserved for import aliasing (S16) and produces a
teaching error E0030 if used for casts (S42). Conversion spelling (constructor
`Int64(x)` vs method `.to_i64()` vs `as`) is **undecided**.

---

## 28. Constants / compile-time values

```rust
// Rust
const MAX: i32 = 100;
static GREETING: &str = "hi";
```
```go
// Go
const Max = 100
```
```c
// C, C++, Java(final), C#(const)
#define MAX 100
const int MAX = 100;
```
```zig
// Zig — `const` is also normal binding; comptime for CT eval
const max = 100;
comptime { ... }
```
```python
# Python — convention only (UPPER_CASE), no enforcement
MAX = 100
```

**▶ Jet today (KW_CONST / S57):** `const` for compile-time constants;
`comptime` (S57) for compile-time evaluation, Zig-influenced. `pure fn` (S60)
relates.

---

## 29. Ternary / conditional expression

```c
// C, Java, JS, C++, C#, Swift, Kotlin(no ternary→if-expr), Dart, PHP
cond ? a : b
```
```python
# Python
a if cond else b
```
```rust
// Rust, Kotlin, Scala — no ternary; `if` is the expression
if cond { a } else { b }
```

**▶ Jet today:** no `?:` ternary (the `?` suffix means propagation, S7). If
`if` is an expression, the Rust/Kotlin form covers this — worth confirming
(see §7).

---

## 30. Variadic / spread / rest

```rust
// Rust — macros for variadic; spread via iterators
println!("{} {}", a, b);
let v = [first, ..rest];
```
```javascript
// JavaScript / TypeScript
function f(...args) {}
f(...array);
const [head, ...tail] = arr;
```
```python
# Python
def f(*args, **kwargs): ...
f(*list, **dict)
```
```go
// Go
func f(args ...int) {}
f(slice...)
```
```swift
// Swift
func f(_ xs: Int...) {}
```

**▶ Jet: undecided.** Variadics interact with the no-overloading non-goal.
Likely deferred (use a list parameter instead) — fits the smallness ratchet.

---

## 31. Destructuring / binding patterns

```javascript
// JavaScript / TypeScript
const { x, y } = point;
const [a, b] = pair;
```
```rust
// Rust, (Swift, Scala via patterns)
let Point { x, y } = point;
let (a, b) = pair;
```
```python
# Python
x, y = point
a, *rest = items
```
```elixir
# Elixir — match operator binds
%{name: n} = user
[head | tail] = list
```

**▶ Jet today:** destructuring tied to `switch`/pattern arms (S24). Standalone
let-destructuring (`val (a, b) = pair`) is **undecided**.

---

## 32. Tuples / anonymous aggregates

```rust
// Rust, Swift, Scala, Python, Haskell, F#, OCaml, Elm
let pair = (1, "two");
pair.0          // Rust/Scala access
```
```python
# Python
pair = (1, "two")
pair[0]
```
```go
// Go — NO tuples; multiple return values instead
func f() (int, string) { return 1, "two" }
```
```swift
// Swift — named tuple members
let p = (x: 1, y: 2)
p.x
```

**▶ Jet: undecided.** Tuples vs. Go-style multi-return vs. require a struct.
Jet currently leans on structs/Result; a lightweight tuple is a possible add.

---

## 33. Type alias / newtype

```rust
// Rust
type Id = u64;              // alias (transparent)
struct Id(u64);             // newtype (distinct)
```
```typescript
// TypeScript, (Scala `type`, F#/OCaml `type`)
type Id = number;
```
```haskell
-- Haskell
type Id = Int               -- alias
newtype Id = Id Int         -- distinct
```
```go
// Go
type Id = uint64            // alias
type Id uint64              // defined type (distinct)
```

**▶ Jet: undecided.** No type-alias keyword in `syntax.rs` yet. Question:
transparent alias only, or also a distinct newtype (Haskell/Rust/Go offer
both)?

---

## 34. Numeric literals (separators, bases)

```rust
// Rust, Swift, Kotlin, Go, Java, C#, Julia, Ada — digit separators
1_000_000
0xFF   0o17   0b1010
3.14e10
```
```python
# Python (3.6+)
1_000_000
0xFF   0o17   0b1010
```
```c
// C (pre-C23), older langs — NO separators
1000000
0xFF   017       // octal = leading zero (footgun)
```

**▶ Jet: undecided** (but free win). Underscore digit separators (`1_000_000`)
appear in nearly every modern language — `07` §8 flags this as a high-confidence
adopt. Hex/octal/binary literal prefixes also need a decision.

---

## 35. Program entry point

```rust
// Rust
fn main() { }
```
```go
// Go
func main() { }       // in package main
```
```c
// C, C++, Java(class), C#
int main() { return 0; }
```
```python
# Python — convention
if __name__ == "__main__": main()
```
```haskell
-- Haskell
main :: IO ()
main = putStrLn "hi"
```
```javascript
// JavaScript, Ruby, Lua, PHP — top-level code runs; no main required
console.log("hi")
```

**▶ Jet today (S12):** `fn main()` — special-cased, no `pub` required. No
top-level statements (a file's program enters at `main`). Rust/Go family.

---

## Appendix — Jet's spelling at a glance

| Choice | Jet today | Decision |
|---|---|---|
| Binding | `val` / `var` | S2 |
| Function | `fn f(a: T) -> R { }` | S1 |
| Comment | `//` | S5 |
| Interpolation | `"hi {name}"` | S8 |
| Block | `{ }` | S3 |
| Statement end | `;` (always) | S6 |
| If/else | `if c { } else { }` | KW_IF |
| Multi-way | `switch`, arms `->` | S24 |
| Loops | `while`, `for x in r` | S19 |
| Range | `1..10` inclusive | S22 |
| Lambda | `x => x + 1` | S46 |
| Optional | `T?`, `value`/`null`, `or` | S32/S35 |
| Fallible | `T ? E`, `ok`/`err`, `?` | S7/S34 |
| Struct | `struct P { x: T }` | S29 |
| Sum type | `enum` | S30 |
| Trait | `trait` + `impl T { }` | S28/S27 |
| Import | `import ... as` | S16 |
| Visibility | private default, `pub` | S18 |
| Logical | `&& \|\| !` | S13 |
| Entry | `fn main()` | S12 |
| List type | `[T]` (or `List<T>`) | S65/S33 |

**Undecided (candidates for Open Decision rows):** pipelines (§15), named/
default args (§23), optional chaining `?.` (§12), cleanup primitive `defer`
(§13), digit separators (§34), multi-line strings (§6), tuples (§32), type
alias/newtype (§33), conversion spelling (§27), UFCS (§3), standalone
destructuring (§31), map/set literal syntax (§11), user-generics brackets (§19).
</content>
