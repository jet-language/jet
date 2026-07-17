# Language-shape research

Status: research for open Tower ballots. No Jet syntax here is ratified.

## One comparison, held constant

The first comparison uses one small task in every language:

1. Represent `Member` and `Admin` roles.
2. Define a user with a name, an active flag, and a role.
3. Return whether that user should receive the active-admin greeting.
4. Construct Ada as an active admin and call the function.

The examples compare language shape, not libraries. Capitalization and field
names follow each language's normal style. Statically typed examples define a
closed two-case role type. Dynamic examples represent the same two roles with
symbols or atoms, whose wider set remains open.

### Rust

```rust
#[derive(PartialEq)]
enum Role { Member, Admin }

struct User {
    name: String,
    active: bool,
    role: Role,
}

fn should_greet(user: &User) -> bool {
    user.active && user.role == Role::Admin
}

fn main() {
    let ada = User {
        name: "Ada".into(),
        active: true,
        role: Role::Admin,
    };
    let answer = should_greet(&ada);
}
```

Rust makes ownership visible and keeps data construction distinct from calls.
It repeats the enum qualifier and uses several construction families. Its `?`
operator is a strong model for visible, typed failure. See the
[Rust enum and pattern guide](https://doc.rust-lang.org/book/ch06-00-enums.html).

### Swift

```swift
enum Role { case member, admin }

struct User {
    let name: String
    let active: Bool
    let role: Role
}

func shouldGreet(_ user: User) -> Bool {
    user.active && user.role == .admin
}

let ada = User(name: "Ada", active: true, role: .admin)
let answer = shouldGreet(ada)
```

Swift uses expected type to shorten `.admin`. Labeled inputs make construction
readable without repeating `role` at the value. Swift's main API rule is
clarity where a call is read, while omitting words that add no information. See
the [Swift API design guidelines](https://www.swift.org/documentation/api-design-guidelines/).

### OCaml

```ocaml
type role = Member | Admin

type user = {
  name : string;
  active : bool;
  role : role;
}

let should_greet user =
  user.active && user.role = Admin

let ada = { name = "Ada"; active = true; role = Admin }
let answer = should_greet ada
```

OCaml infers the function type and uses compact algebraic data types. Modules
can expose a checked public signature, but separate signature files can repeat
facts. See the [OCaml module guide](https://ocaml.org/docs/modules).

### F#

```fsharp
type Role = Member | Admin

type User = {
    Name: string
    Active: bool
    Role: Role
}

let shouldGreet user =
    user.Active && user.Role = Admin

let ada = { Name = "Ada"; Active = true; Role = Admin }
let answer = shouldGreet ada
```

F# combines ML-style inference with a widely used pipe. The pipe reads in
execution order, but API argument order often bends around its fixed slot. See
the [F# language specification](https://fsharp.org/specs/language-spec/4.0/FSharpSpec-4.0-final.pdf).

### Haskell

```haskell
data Role = Member | Admin deriving (Eq)

data User = User
  { name   :: String
  , active :: Bool
  , role   :: Role
  }

shouldGreet :: User -> Bool
shouldGreet user = active user && role user == Admin

ada = User { name = "Ada", active = True, role = Admin }
answer = shouldGreet ada
```

Haskell separates pure values from effects and makes composition powerful.
Type-class, monad, and extension vocabulary can make the surface feel larger
than its core. See the [Haskell report](https://www.haskell.org/onlinereport/haskell2010/).

### Kotlin

```kotlin
enum class Role { Member, Admin }

data class User(
    val name: String,
    val active: Boolean,
    val role: Role,
)

fun shouldGreet(user: User): Boolean =
    user.active && user.role == Role.Admin

val ada = User(name = "Ada", active = true, role = Role.Admin)
val answer = shouldGreet(ada)
```

Kotlin's data classes remove boilerplate and its receiver builders can make
domain code concise. Its five scope functions show the cost of several similar
ways to express one job. See the
[Kotlin scope-function guide](https://kotlinlang.org/docs/scope-functions.html).

### Gleam

```gleam
pub type Role {
  Member
  Admin
}

pub type User {
  User(name: String, active: Bool, role: Role)
}

pub fn should_greet(user: User) -> Bool {
  case user {
    User(_, True, Admin) -> True
    _ -> False
  }
}

pub fn main() -> Bool {
  let ada = User(name: "Ada", active: True, role: Admin)
  should_greet(ada)
}
```

Gleam keeps the language small, uses exhaustive tagged values, and offers one
main pipe. This makes unfamiliar code easier to predict. See Gleam's
[pipeline tour](https://tour.gleam.run/functions/pipelines/).

### Elixir

```elixir
defmodule User do
  defstruct [:name, :active, :role]
end

defmodule Greeter do
  def should_greet(%User{active: true, role: :admin}), do: true
  def should_greet(%User{}), do: false
end

ada = %User{name: "Ada", active: true, role: :admin}
answer = Greeter.should_greet(ada)
```

Elixir lets patterns define function cases. Its pipe is readable, but it always
inserts the value into the first argument. See the
[Elixir patterns and guards guide](https://hexdocs.pm/elixir/patterns-and-guards.html)
and its [pipe operator](https://hexdocs.pm/elixir/Kernel.html#%7C%3E/2).

### Clojure

```clojure
(defrecord User [name active role])

(defn should-greet [{:keys [active role]}]
  (and active (= role :admin)))

(def ada (->User "Ada" true :admin))
(def answer (should-greet ada))
```

Clojure gets power from readable data and a small core. Code-as-data macros can
remove repetition, but local macro languages weaken global prediction. Its
threading family also shows the cost of several flow operators. See
[Clojure data structures](https://clojure.org/reference/data_structures) and
[threading macros](https://clojure.org/guides/threading_macros).

### Racket

```racket
(struct user (name active role) #:transparent)

(define (should-greet u)
  (and (user-active u)
       (eq? (user-role u) 'admin)))

(define ada (user "Ada" #t 'admin))
(define answer (should-greet ada))
```

Racket demonstrates how a tiny core can support whole languages. That power is
valuable for tooling and teaching, but unrestricted language layers can make
two files obey different rules. See the
[Racket language-creation guide](https://docs.racket-lang.org/guide/languages.html).

### Smalltalk

```smalltalk
Object subclass: User [
    | name active role |

    name: value [ name := value ]
    active: value [ active := value ]
    role: value [ role := value ]

    shouldGreet [
        ^ active and: [role = #admin]
    ]
]

ada := User new
    name: 'Ada';
    active: true;
    role: @admin;
    yourself.

answer := ada shouldGreet.
```

Smalltalk uses one message model. Keyword messages name argument roles, while
cascades reuse one receiver. The grammar is highly predictable, though the
receiver remains privileged. See the
[GNU Smalltalk syntax guide](https://www.gnu.org/software/smalltalk/manual/html_node/The-syntax.html).

### Zig

```zig
const Role = enum { member, admin };

const User = struct {
    name: []const u8,
    active: bool,
    role: Role,
};

fn shouldGreet(user: User) bool {
    return user.active and user.role == .admin;
}

const ada = User{ .name = "Ada", .active = true, .role = .admin };
const answer = shouldGreet(ada);
```

Zig favors explicit control but uses expected type for enum and record fields.
Ordinary code can also run at compile time. Its large builtin `@name` family is
a warning against making one sigil a catalogue of unrelated operations. See the
[Zig language reference](https://ziglang.org/documentation/master/).

### Ada and SPARK

```ada
type Role is (Member, Admin);

type User is record
   Name   : Unbounded_String;
   Active : Boolean;
   Kind   : Role;
end record;

function Should_Greet (Value : User) return Boolean is
  (Value.Active and then Value.Kind = Admin);

Ada_User : constant User :=
  (Name => To_Unbounded_String ("Ada"), Active => True, Kind => Admin);
Answer : constant Boolean := Should_Greet (Ada_User);
```

Ada is verbose but exceptionally explicit. SPARK adds checked contracts and
information-flow proof. The lesson for Jet is to expose proof without forcing
users to maintain a second copy of the program. See the
[SPARK course](https://learn.adacore.com/pdf_books/courses/intro-to-spark.pdf).

## Reusable flow, resolved

D-SHAPE-PIPE1=C rejected a general flow operator. Single `|` remains limited
to alternatives in patterns and choices. Ordinary names preserve execution
order and expose the secondary argument roles without another call model:

```jet
signup :: parse_signup(raw)
invite :: fetch_invite(signup, api)
save(invite, db)
```

When a flow must be reusable, Jet uses an ordinary named function or a named
library composition helper. Resolution, failure, ownership, dispatch, and
effects remain the ordinary function rules.

## Ideas worth taking

| Source | Idea | Jet use | Failure to avoid |
| --- | --- | --- | --- |
| ML, Roc, Swift, Zig | Expected-type shorthand | Keep `Type.{...}` beside `.{...}` and `Type.Variant` beside `.Variant`. | Do not infer a nominal type from matching fields. |
| Koka | Inferred effect rows | Let common private code omit uniquely known effects; reveal and pin them at boundaries. | Do not make dense effect notation dominate ordinary code. |
| CUE, Nickel | Order-independent refinement | Let config and policy add compatible facts; report both sources on conflict. | Never let later file order silently win. |
| Pony | Capabilities as authority | Make external power visible and narrowable. | Do not expose a large reference-mode vocabulary to beginners. |
| Unison | Semantic identity and inspection | Track definitions, dependencies, and builds by meaning behind normal files. | Do not require a database editor for source truth. |
| D | One operation, prefix and receiver call views | Prefix and receiver calls can resolve one symbol. | No fallback lookup or ambiguous extension dispatch. |
| Hazel | Typed holes and incomplete programs | Give useful expected-type feedback before code is complete. | Do not force structural editing. |
| MPS and lens research | Several views over one program | Beginner, exact, and audit views can expose the same facts. | Views must round-trip and plain text must remain complete. |
| Verse | Failure contexts with rollback | Consider explicit checked transaction regions. | Ordinary failure must not imply invisible rollback. |
| Dhall and Nix | Builds and config as values | Keep dependency graphs typed and inspectable. | Avoid several overlay and override systems. |
| Eiffel and SPARK | Contracts beside code | Reuse ordinary predicates for checked promises. | Do not make users maintain an annotation shadow-program. |
| BQN and APL | Whole-data operations | Prefer named array operations over manual index loops. | Dense symbol code cannot be required public style. |

Primary references:

- [Koka effect types](https://koka-lang.github.io/koka/doc/book.html)
- [CUE's order-independent logic](https://cuelang.org/docs/concept/the-logic-of-cue/)
- [Nickel merging](https://nickel-lang.org/user-manual/merging/)
- [Pony object capabilities](https://tutorial.ponylang.io/object-capabilities/object-capabilities.html)
- [Unison's semantic codebase](https://www.unison-lang.org/docs/the-big-idea/)
- [Hazel and typed holes](https://hazel.org/)
- [JetBrains MPS concepts](https://www.jetbrains.com/mps/concepts/)
- [Verse speculative execution](https://dev.epicgames.com/documentation/fortnite/speculative-execution)
- [Dhall language tour](https://docs.dhall-lang.org/tutorials/Language-Tour.html)
- [Eiffel contracts](https://www.eiffel.org/doc/solutions/Design_by_Contract_and_Assertions)
- [BQN documentation](https://mlochbaum.github.io/BQN/doc/index.html)

## Psychology and reasoning audit

Short code is not automatically simple code. A shape is elegant when a small
number of rules lets a reader predict many unseen examples.

Each ballot must test:

1. Can a beginner complete the common task without policy or compiler terms?
2. Can an expert reveal and pin every inferred choice?
3. Can a company forbid hidden authority and require public facts to be fixed?
4. Can a reader identify data, control, mutation, failure, and authority nearby?
5. Does one fact have one source of truth?
6. Does a rename or policy change require one semantic edit?
7. Does the compiler reject ambiguity instead of guessing?
8. Can the short and explicit forms round-trip without changing meaning?

These questions come from the Cognitive Dimensions concerns of consistency,
visibility, hidden dependencies, viscosity, and role clarity. See the
[Cognitive Dimensions tutorial](https://www.cl.cam.ac.uk/~afb21/CognitiveDimensions/CDtutorial.pdf).
Research on projectional editors also warns that a structurally valid tree can
still be awkward to type, delete, copy, or paste. Jet should keep plain text as
complete source and make structural views optional. See the
[2024 projectional editing experiment](https://drops.dagstuhl.de/entities/document/10.4230/OASIcs.SLATE.2024.5).

### Applied audit

| Viewpoint | Observed failure | Required property | Design consequence | Ready ballots |
| --- | --- | --- | --- | --- |
| Psychology | Similar marks require the reader to remember compiler categories. | Recognition from visible shape. | Marker options are separated by what users can see: attached item, scoped body, or one shared rule family. | D-SHAPE1, D-SHAPE2 |
| Logic | A short form can gain a new meaning when distant code changes. | Unique resolution and compositional meaning. | Inference is legal only when one answer is forced; ambiguity is an error, never a priority rule. | D-SHAPE-OPAQUE-INFER1, D-SHAPE-EFFECTOMIT1 |
| Beginner | Ceremony appears before the user needs control. | Safe useful defaults with no policy vocabulary. | Common code may omit uniquely known types and effects; records keep the existing `.{...}` shorthand. | D-SHAPE-OPAQUE-INFER1, D-SHAPE-EFFECTOMIT1 |
| Expert | Magic hides the type, authority, provider, or ownership choice. | Every hidden fact can be revealed and pinned. | Explicit construction, effects, copy, package, and provenance forms remain source-writable. | D-SHAPE-LIFECYCLE, D-SHAPE8, D-ECO1, D-SHAPE-MERGEPROVENANCE1 |
| Enterprise | A build succeeds while important authority or provenance stays implicit. | Policy can require facts and audit their source without changing behavior. | Public-boundary effect rules and complete merge history use the same program facts as ordinary compilation. | D-SHAPE8, D-SHAPE-MERGEPROVENANCE1, D-SHAPE-EXPOSE1 |
| Construction | Records, fresh state, conversions, views, and units look interchangeable. | Each act has one named semantic category. | Keep record construction separate; choose one spelling each for fresh state, conversion, views, runtime duration creation, whole-unit reading, and dimensional quantities. | D-SHAPE3a, D-SHAPE-CONVERT1, D-SHAPE-VIEW1, D-SHAPE-DURATION1, D-SHAPE-DURATIONCONVERT1, D-SHAPE-QUANTITY1 |
| Flow | Nested calls reverse the order people trace data. | Reading order must not create a second call model. | Use named intermediates or ordinary named composition helpers; single `|` stays alternatives-only. | D-SHAPE-PIPE1=C (ratified) |
| Internal API | An underscore may imply privacy, instability, expert support, or replacement. | One visible promise, with access and replacement decided later. | Decide only the contract status communicated by the name. | D-SHAPE-INTERNAL1 |
| Resources | Automatic cleanup hides the point where early release matters. | Scope cleanup stays guaranteed; early release visibly consumes the handle. | Compare only discovery and naming while preserving the ownership marker. | D-SHAPE-RESOURCE1 |
| Package author | Role, output, and merge forms repeat types or invent package-only mini-languages. | Reuse ordinary typed values and write each composition rule once. | Decide role shape, output representation, and repeated-field composition independently. | D-SHAPE5a, D-SHAPE5b, D-SHAPE-MERGE1 |
| Command author | Parser declarations can duplicate the application's input types. | One typed root schema drives parsing, help, completion, validation, and tests. | Decide only what owns root input types; stage field sources and subcommands afterward. | D-SHAPE-CLI1 |

Every recommended option must also pass five logical tests:

1. **Compositional meaning:** a form means the same thing inside a larger form.
2. **Unique resolution:** adding an unrelated declaration cannot silently change a short form.
3. **Local reasoning:** data flow, effects, failure, ownership, and authority are visible nearby or revealable at that point.
4. **Round-trip equivalence:** compact and explicit source compile to the same typed program, and reveal then fold restores the original meaning.
5. **Authority preservation:** a view, pipe, default, or inference rule cannot add effects, permissions, ownership, or scheduling behavior.

## Atomic ballot rule

A ballot passes only when all of these are true:

- Its result fits one enforceable sentence.
- Every option changes the same property.
- The owner can postpone every sibling and still leave coherent law.
- Reversing this decision does not force a sibling decision to reverse.
- Every option solves the same example with the same fixed assumptions.
- One focused parser, type-checker, or policy test can enforce the result.
- An option does not quietly choose another delimiter, inference rule, effect
  rule, package shape, or editor behavior.

If the owner can like an option's main idea but reject one punctuation detail
inside it, the ballot contains more than one decision and must be split.

## 2026-07-14 ballot correction

The first ballot pass used abstract product stories and treated prior art as a
feature list. The replacement ballots use this stricter standard:

- One small textbook program per ballot: `Stack`, `Task`, a text file, or a
  distance/time calculation.
- The Jet options and every language comparison perform the same task.
- The lesson teaches the concept before introducing ecosystem or policy needs.
- Community experience can veto attractive prior art. Popularity alone is not
  evidence that a feature is well designed.

Research that changed the options:

- Swift accepted explicit `copy`, but later discussion shows that copying a
  class reference may preserve identity rather than clone the object. Jet's
  `^^` candidate therefore means an independent copy followed by transfer and
  must reject types without that operation.
- Scala's `CanThrow` documentation describes the inflexibility and propagation
  burden inherited from Java checked exceptions. Jet keeps local effect
  inference and asks only where an explicitly pinned row appears.
- Python gives `_name` a weak internal-use convention and `__name` a separate
  collision-avoidance behavior. Rust instead uses `_name` to suppress unused
  warnings. Jet ballots separate bare `_`, `_name`, and `__name`.
- Elixir's first-argument pipe is popular, yet `then` and `tap` exist because
  ordinary value threading does not cover every shape. Jet's pipe ballot now
  asks for a job dot calls cannot do: building a reusable typed flow.
- Rust favors scope cleanup and an ordinary `drop(value)` for early release.
  Go's function-scoped `defer` can retain loop resources too long. Jet's
  resource ballot now separates lifetime end from fallible protocol finish.
- C++ `string_view` and `span` are useful but do not prevent dangling storage.
  Jet's view options all preserve checked owner and escape rules.
- F# units compose with no runtime cost, but Microsoft warns that units erase
  at some .NET boundaries. The quantity ballot therefore separates compile-time
  algebra from runtime and wire representation.

Primary community and design references:

- [Swift explicit-copy acceptance](https://forums.swift.org/t/accepted-se-0377-revision-make-borrowing-and-consuming-parameters-require-explicit-copying-with-the-copy-operator/65293)
- [Swift class-copy discussion](https://forums.swift.org/t/copy-operator-doesnt-clone-a-class-instance/84592)
- [Scala `CanThrow`](https://docs.scala-lang.org/scala3/reference/experimental/canthrow.html)
- [Python PEP 8 underscore guidance](https://peps.python.org/pep-0008/)
- [Python C API underscore discussion](https://discuss.python.org/t/c-api-what-should-the-leading-underscore-py-mean/18486)
- [Rust Clippy explicit-drop issue](https://github.com/rust-lang/rust-clippy/issues/6446)
- [C++ Core Guidelines view lifetime discussion](https://github.com/isocpp/CppCoreGuidelines/issues/2276)
- [F# units of measure](https://learn.microsoft.com/en-us/dotnet/fsharp/language-reference/units-of-measure)
- [F# component guidance on unit erasure](https://learn.microsoft.com/en-us/dotnet/fsharp/style-guide/component-design-guidelines)
- [mp-units points and quantities](https://mpusz.github.io/mp-units/latest/tutorials/affine_space/points_and_quantities/)
