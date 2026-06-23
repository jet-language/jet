You are implementing a major memory/access model redesign in the Jet programming language.

> **GATING — read before you build.** The *spelling* below (the `T` / `~T` / `^T` / `&T` /
> `*T` sigils) is ratified and frozen as **D-CAP7** (docs/spec/syntax-decisions.md). Three
> downstream **decisions are still open**, and you must NOT build past them — they change
> semantics, not just spelling:
> 1. **Unmarked-`T` default (D-CAP8, board card c125).** Whether `T` is fixed shared-read
>    (today's behavior; mutation is E0202/E0205) or *infer-and-elevate* (this doc's model).
>    Do not change the default-parameter semantics until this is decided.
> 2. **`&` / `*` expression grammar (D-CAP9, c127).** S58 already uses `&x`=address-of and
>    `*p`=deref. This doc's `&x`=share / `*x`=raw-pointer-of collide head-on. Do NOT ship
>    expression-position sigil parsing until the disambiguation (and `*T` vs `Ptr<T>`) is
>    settled.
> 3. **Capability overloads (D-CAP10, c128).** The overload-by-capability section may be out
>    of scope entirely under Jet's one-canonical-spelling policy (S14). Confirm scope first.
>
> Type-position sigils, parameter capability, method receivers, inference, and diagnostics
> that don't depend on the three items above can proceed. Everything else waits on the gate.

Important context:

* The public/top-level Jet repository information may be outdated.
* Do not assume README, docs, or old syntax summaries are fully accurate.
* First inspect the actual current compiler/parser/typechecker/codegen/tests in the repo.
* There is already memory-management development ongoing. Integrate with it carefully instead of replacing it blindly.
* Treat this as a trajectory-alignment task: preserve valid existing work, but steer it toward the new unified access-capability model described below.

# Goal

Implement Jet’s new beginner-friendly but expert-capable access system.

Jet should stop exposing “references vs pointers vs mutable references” as the main mental model. Instead, model value access by **capability**.

Core syntax:

```jet
T     // infer capability; starts minimal/read and elevates only when required
~T    // explicit write/edit access
^T    // explicit move/take ownership
&T    // explicit share/escape access
*T    // explicit raw unsafe pointer/address
```

Do not use `@` for this system because Jet already uses `@` for named loops (and `@=` for immutable bindings).

Do not use `!` for this system because it can be confused with boolean not / negation.

Use prefix capability sigils only.

# Surrounding Jet Syntax (verified current — use exactly this)

These five sigils are the redesign. Everything around them must use Jet’s actual current surface syntax, not Rust/Go/C placeholders. The examples in this document already follow it:

* **Functions:** `fn name(param: Type) -> Ret { … }`. The keyword is `fn`, never `proc`/`func`/`def`. A signature shown without a body is an illustration of a resolved capability signature, not literal compilable code.
* **Methods:** `impl Type { fn m(self) { … } }`, or methods written inside the `struct`/`enum` body. The receiver is named `self`; its capability is a **prefix sigil on `self`** — `~self`, `^self`, `&self`, or plain `self` (infer/read). This replaces the older `mut self` / `take self` keyword forms.
* **Bindings:** `name @= expr` is an immutable binding; `name := expr` is mutable. `=` reassigns an existing mutable binding. There is no `let`/`val`/`var`.
* **Visibility:** `pub` prefixes an exported item; everything is private by default.
* **Unsafe:** a `#Unsafe("reason") { … }` block, or a `#Unsafe("reason") fn` attribute on the line above `fn` — the safety reason is the gate's argument (D-UNSAFE2 merged the old separate `#Audit("…")` marker into it). Attributes are PascalCase with a `#` sigil (`#Unsafe`, `#Test`, `#Pure`); `@unsafe` and standalone `#Audit` are retired.
* **Optional / nullable:** `T?`.
* **Strings:** interpolation is `"… {expr} …"`; the type is `String` (never `Str`).
* **Structs:** declare `struct S { field: Type }`; construct with a brace literal `S { field: value }` — call-style `S(field: value)` is rejected.
* **Primitives:** `Int`, `Float`, `Bool`, `String`, `Char`. Fixed-width/FFI menu: `I8 I16 I32 I64 U8 U16 U32 U64 F32 F64`. Lists are `[T]`; maps `[K, V]`. `.len()` is a method call.
* **Generics:** `Type<Args>` with angle brackets.

These prefix sigils **supersede** Jet’s existing capability keywords. Migrate, do not run both vocabularies in parallel:

```text
mut x   / mut self    ->  ~x   / ~self          (write/edit)
take x  / take self   ->  ^x   / ^self           (move/take)
view return / shared  ->  &T                      (share/escape)
Ptr<T> + #Unsafe      ->  *T   (inside #Unsafe("reason") regions)
default (shared read) ->  T    (now infer-and-elevate, not fixed read)
```

# Design Philosophy

The user-facing model should be:

```text
blank = compiler-inferred access; starts as normal read
~     = this may be changed
^     = ownership is taken / moved out
&     = this may be shared or escape the current scope
*     = unsafe raw memory/address
```

The goal is a “magic out of the box” beginner experience with explicit expert control.

Beginners should be able to write ordinary code and let the compiler infer safe access.

Experts should be able to explicitly annotate public APIs, performance-sensitive code, FFI, allocators, and unsafe systems-level code.

# Core Syntax

Use prefix capability sigils in both type position and expression/call-site position.

## Type Position

```jet
fn print(player: Player)
fn damage(player: ~Player, amount: I32)
fn equip(player: ~Player, item: ^Item)
fn cache(texture: &Texture)

#Unsafe
fn write_bytes(ptr: *U8, len: Int)
```

## Expression / Call Position

```jet
print(player)
damage(~player, 10)
equip(~player, ^sword)
cache(&texture)

#Unsafe {
    write_bytes(*buffer, buffer.len())
}
```

# Critical Determinism Rule

Do not allow this system to produce unpredictable behavior.

Strictly separate:

```text
Capability inference = semantic permission
Performance optimization = physical representation / passing strategy
```

Capability inference decides what the program is allowed to do.

Optimization decides how to implement that capability efficiently.

Optimization must never change semantic capability.

Unmarked `T` means `Infer`, not “whatever is fastest.”

The compiler should resolve capabilities using a deterministic constraint system before performance optimization.

After capabilities are resolved, the optimizer may choose the fastest safe physical representation.

Examples:

```text
Small read-only scalar    -> pass by value/register
Large read-only struct    -> pass by readonly reference/view
Writable value            -> pass by exclusive mutable reference/view
Moved value               -> transfer ownership without unnecessary copies
Shared/escaping value     -> use valid safe escaping handle/region/refcount/task-safe mechanism
Raw value                 -> lower to unsafe pointer only inside unsafe boundaries
```

All of these are implementation choices after semantic capability resolution.

# Explicit Markers Are Hard Constraints

If an expert writes a capability marker, that marker is a semantic contract.

Examples:

```jet
fn damage(player: ~Player, amount: I32)
fn upload(image: ^Image)
fn spawn(config: &Config)

#Unsafe
fn memcpy(dst: *U8, src: *U8, len: Int) -> *U8
```

The compiler must not silently downgrade, reinterpret, or optimize these into different semantic capabilities.

If the implementation can optimize representation while preserving semantics, it may do so.

If the explicit capability conflicts with usage, emit an error.

Example:

```jet
fn inspect(player: ~Player) {
    print(player.name)
}
```

This is allowed if Jet permits overgranting explicit capability. It means the public/API contract requires writable access even if the body currently only reads.

Alternative policy may warn on overgranting:

```text
warning: player is marked writable but only read
```

Do not silently rewrite it to `Player`.

# Mixed Explicit And Inferred Capabilities

Experts may mark only some parameters.

Example:

```jet
fn equip(player: ~Player, item: Item) {
    player.inventory.add(item)
}
```

The compiler should resolve this deterministically:

```jet
fn equip(player: ~Player, item: ^Item)
```

Reasoning:

```text
player is explicitly ~Player -> fixed write access
item is unmarked -> infer from body
inventory.add consumes item -> item becomes ^Item
```

Another example:

```jet
fn register(world: ~World, asset: Asset) {
    world.assets.add(&asset)
}
```

Resolved:

```jet
fn register(world: ~World, asset: &Asset)
```

If inference conflicts with explicit markers, emit a diagnostic.

Example:

```jet
fn consume(item: Item) {
    destroy(^item)
    print(item.name)
}
```

Diagnostic should say that `item` was moved and then used afterward.

# Capability Inference

Represent unmarked capability as `Infer`.

Suggested enum:

```text
AccessCapability:
- Infer
- Read
- Write
- Move
- Share
- Raw
```

This extends the existing internal `AccessConvention` (today `Read` / `Mutate` / `Move`) — grow it into this vocabulary rather than introducing a parallel enum.

Do not parse unmarked `T` as permanently read-only.

Instead:

```text
Infer starts at the weakest semantic assumption, normally read/view.
The compiler elevates only when the program semantically requires stronger access.
```

Capability elevation:

```text
Infer -> Read   when only observation is needed
Infer -> Write  when mutation is required
Infer -> Move   when ownership is consumed/transferred
Infer -> Share  when the value escapes, is retained, cached, spawned, or stored beyond the immediate call
Infer -> Raw    never silently; raw requires explicit `*` and unsafe context
```

Raw access is not inferred in safe code.

# Capability vs Passing Strategy

The compiler should optimize for performance after capability resolution.

This means two functions may both be semantically read access:

```jet
fn length(v: Vec3) -> F32
fn render(scene: Scene)
```

But compile differently:

```text
Vec3  -> pass by value/register if cheapest
Scene -> pass by readonly reference/view if cheapest
```

Both remain semantically read-only.

Do not expose these physical passing choices as semantic differences.

Do not let optimization change whether a caller loses ownership, whether mutation is allowed, whether sharing is allowed, or whether unsafe raw access occurs.

# Stable Optimization Rules

Optimization should be deterministic for a given compiler version, target, optimization level, and explicit optimization inputs.

Allowed deterministic inputs include:

```text
type size
copy/move traits
destructor/drop behavior
escape analysis
alias analysis
target ABI
optimization level
explicit PGO/profile artifact if provided
```

Disallowed:

```text
nondeterministic heuristic changes during compilation
runtime-dependent semantic capability changes
profile-dependent capability changes
optimizer deciding to move/share/write solely for speed
```

Profile-guided optimization may affect physical representation or layout only if explicitly enabled and supplied as an input artifact. It must not change semantic capability.

# Overload Resolution

Avoid unpredictable overload resolution.

If overloads differ only by capability, unmarked calls must not guess based on performance.

Example:

```jet
fn process(data: Data)
fn process(data: ~Data)
fn process(data: ^Data)
```

For:

```jet
process(data)
```

The compiler must follow deterministic rules.

Recommended policy:

1. Prefer exact unmarked/read-compatible overload when available.
2. Do not select write/move/share overloads from an unmarked call unless required by explicit call-site capability or there is only one valid candidate.
3. If multiple capability-only overloads are viable and none is clearly selected by explicit sigil, emit an ambiguity error.
4. Require explicit call-site sigil for stronger capability:

```jet
process(~data)
process(^data)
process(&data)
```

Do not choose stronger capabilities merely because they might be faster.

(Note: Jet currently has no general function-overloading mechanism — its alias policy is one canonical spelling per construct, and polymorphism is traits-only. Confirm whether capability-only overloads are in scope before building this section; if Jet stays single-definition, this becomes a call-site-sigil disambiguation rule on a single function rather than overload selection.)

# Public API Rules

Jet has or plans to have package types like libraries and executables. Integrate capability inference accordingly.

This aligns with the existing ratified package-API metadata work: a manifest `api = stable | explicit` mode (`explicit` is opt-in, not the default). Wire the rules below through that mechanism rather than inventing a new one.

## Executables / App Code

* Allow broad inference for unmarked parameters.
* Private/internal functions may infer read/write/move/share where safe.
* Favor beginner ergonomics.

## Libraries / Public APIs

* Capability is part of the public contract.
* Public functions should require explicit capability annotations or have compiler-inferred capabilities frozen and emitted into generated interface metadata/docs.
* A change from read/inferred to `~T`, `^T`, or `&T` should be treated as a breaking API change.
* Package pinning/hash mechanisms should include resolved public capability signatures.
* Consumers should compile against emitted interface metadata, not re-infer public dependency APIs from source bodies unless explicitly building the package from source.

Example:

Source:

```jet
pub fn equip(player: Player, item: Item) {
    player.inventory.add(item)
}
```

Generated public interface:

```jet
pub fn equip(player: ~Player, item: ^Item)
```

Changing the implementation later so that a parameter changes capability should affect the public interface and be detected as an API change.

# Capability Semantics

## Unmarked `T`

Unmarked `T` means compiler inference.

It starts with minimal semantic assumptions and elevates only as required.

Examples:

```jet
fn status(player: Player) -> String {
    return "{player.name}: {player.hp}"
}
```

Resolved internally as read/view access.

```jet
fn heal(player: Player, amount: I32) {
    player.hp += amount
}
```

Resolved internally as:

```jet
fn heal(player: ~Player, amount: I32)
```

```jet
fn close(file: File) {
    os.close(^file)
}
```

Resolved internally as:

```jet
fn close(file: ^File)
```

```jet
fn spawn(config: Config) -> Worker {
    return task.spawn(&config)
}
```

Resolved internally as:

```jet
fn spawn(config: &Config) -> Worker
```

## `~T` Write / Edit

`~T` means exclusive write/edit access.

Rules:

* The callee may mutate the original value.
* Access must be exclusive for the mutation window.
* It should conflict with active reads/views that would make mutation unsafe.
* It should not imply ownership transfer.
* It should not imply heap allocation.
* It should not imply raw pointer access.

Example:

```jet
fn damage(player: ~Player, amount: I32) {
    player.hp -= amount
}

damage(~player, 25)
```

## `^T` Move / Take

`^T` means ownership is consumed by the callee.

Rules:

* The caller cannot use the moved value afterward unless reassigned.
* This is Jet’s explicit ownership-transfer marker.
* It should be used for consuming APIs, finalization, deallocation, upload/submit APIs, and transformations that destroy/rehome the original value.
* Internal optimizer moves/copies are not the same as semantic `^T`.
* `^T` is only semantic move/take when the caller loses ownership.

Example:

```jet
fn close(file: ^File) {
    os.close(file)
}

close(^file)

// file is no longer usable here
```

## `&T` Share / Escape

`&T` means the value may escape the current local scope or be retained elsewhere.

Rules:

* The callee may store, retain, spawn, cache, or otherwise allow the value to live beyond the immediate call.
* This should be stricter than ordinary read access.
* It should integrate with any existing/planned lifetime, region, arena, atomic, task, thread, or rollback systems. (Jet already has `region { … }` blocks, arena allocators, and scope-bound arena `view`s — `&T` is the capability that composes with those.)
* It should not automatically mean raw pointer.
* It should not automatically mean heap allocation.
* It means “safe escaping/shared handle” according to whatever storage/lifetime policy is valid for the type.
* The compiler must not infer `&T` solely for performance. It should infer `&T` only when the value semantically escapes or is retained.

Example:

```jet
fn spawn(config: &Config) -> Worker {
    return task.spawn(config)
}

worker @= spawn(&config)
```

## `*T` Raw / Unsafe

`*T` means raw unsafe pointer/address.

Rules:

* Only valid in unsafe contexts or unsafe APIs. (This is the surface form for what Jet currently spells `Ptr<T>` behind the `core.mem` gate; reconcile the two.)
* Used for FFI, kernels, allocators, low-level systems work, and manual memory operations.
* It has no normal lifetime safety unless explicitly wrapped by another abstraction.
* Nullability should be explicit separately — `*U8?` for a nullable raw pointer (Jet uses `?` for optional/nullable).
* Raw pointer/address access requires explicit `*T` / `*x` inside a `#Unsafe("reason")` context, whose argument is the safety justification (D-UNSAFE2).
* Do not silently infer raw access in safe code.
* Do not let `*x` ambiguously mean both “get raw pointer” and “dereference.” Jet today reads bare `*x` outside `#Unsafe` as a dereference (E0208); design and document a clear rule that separates address-of from deref before shipping the `*` capability sigil.

Example:

```jet
#Unsafe("FFI: raw memory primitive")
fn memcpy(dst: *U8, src: *U8, len: Int) -> *U8

#Unsafe("dst and src are caller-owned buffers of at least len bytes") {
    memcpy(*dst, *src, len)
}
```

# Access vs Storage

Keep access capabilities separate from storage/allocator concepts.

Access capability answers:

```text
What can this operation do with the value?
```

Storage answers:

```text
Where/how is this value stored?
```

Do not collapse these into one concept.

Examples of future or existing storage concepts that should remain separate:

```jet
Box<T>
Arena<T>
Pin<T>
Rc<T>
Arc<T>
Handle<T>
EntityId
```

The capability system should compose with these rather than replace them.

Examples:

```jet
fn mutate(buffer: ~Buffer)
fn upload(buffer: ^Buffer)
fn cache(asset: &Asset)

#Unsafe
fn ffi(ptr: *U8)

fn submit(buffer: &Pinned<Buffer>)
fn parse(temp: ~ArenaBuffer)
```

If Jet already has different names or syntax for storage policies, preserve them unless they directly conflict.

# Method Receiver Semantics

Method definitions should support capability receivers. The receiver capability is a prefix sigil on `self`:

```jet
impl Player {
    fn name(self) -> String {
        return self.name
    }

    fn damage(~self, amount: I32) {
        self.hp -= amount
    }

    fn equip(~self, item: ^Item) {
        self.inventory.add(item)
    }

    fn share(&self) {
        PlayerRegistry.add(self)
    }

    fn destroy(^self) {
        World.remove(self)
    }
}
```

Method call syntax should remain clean:

```jet
player.name()
player.damage(10)
player.equip(^sword)
player.share()
player.destroy()
```

The compiler should infer the receiver capability from the method signature.

Explicit desugared method form should also work or be representable internally:

```jet
Player.name(player)
Player.damage(~player, 10)
Player.equip(~player, ^sword)
Player.share(&player)
Player.destroy(^player)
```

# Field Access

Do not introduce C/C++-style split field access.

Avoid:

```jet
player->hp
(*player).hp
```

Use the same field access regardless of capability:

```jet
player.hp
```

Whether `player` is read, write, shared, moved, boxed, pinned, or arena-backed should not change basic field access syntax.

# Optional / Nullable Composition

Jet uses `?` for optional/nullability; keep it separate from access.

Composition:

```jet
User?     // optional ordinary/inferred User
~User?    // optional writable User
^User?    // optional moved User
&User?    // optional shared/escaping User
*U8?      // nullable raw pointer
```

Keep the separation of concepts intact: `?` is presence/absence, the sigil is capability.

# Diagnostics

Error messages are critical.

Avoid borrow-checker jargon as the primary message. Use capability language. Jet already emits ownership diagnostics (e.g. E0120/E0201/E0202/E0205/E0206/E0208 for borrow/move/mutation/deref); update their wording and triggers to the capability vocabulary rather than adding a parallel set.

## Move Error

```jet
equip(~player, ^sword)
print(sword.name)
```

Diagnostic:

```text
sword was moved here:

    equip(~player, ^sword)
                  ^^^^^^

It cannot be used afterward.

Fix:
- remove the later use
- create another value before moving
- pass sword without ^ if equip should only read it
```

## Write Conflict

```jet
name @= player.name
damage(~player, 10)
print(name)
```

Diagnostic:

```text
Cannot write to player while part of it is still being read.

    name @= player.name
            ----------- read starts here

    damage(~player, 10)
           ^^^^^^^ write requested here

    print(name)
          ---- read used here
```

## Share/Escape Error

```jet
temp @= TempBuffer {}
cache(&temp)
```

Diagnostic:

```text
temp cannot be shared because it does not live long enough.

    cache(&temp)
          ^^^^^

Fix:
- move the value into an owning container
- allocate it in a longer-lived region
- clone/copy the data into a shareable value
```

## Raw Error

```jet
write_bytes(*buffer, len)
```

Outside `#Unsafe`:

```text
Raw memory access requires unsafe.

    write_bytes(*buffer, len)
                ^^^^^^^

Wrap this operation in a #Unsafe block or use a safe API.
```

## Ambiguous Capability Overload

```jet
fn process(data: Data)
fn process(data: ~Data)

process(data)
```

Diagnostic:

```text
This call is ambiguous because multiple capability variants are available.

    process(data)
            ^^^^

Choose the intended access explicitly:

    process(data)    // read variant, if available
    process(~data)   // write variant
```

If the read variant exists and deterministic overload rules select it, no diagnostic is needed. But the compiler must not choose a stronger capability merely for performance.

# Implementation Requirements

Do this carefully and incrementally.

## Phase 1: Repository Audit

Before changing code:

1. Inspect parser, lexer, AST, type checker, lowering, IR, codegen, diagnostics, formatter, tests, docs.
2. Identify existing memory-management work (the `AccessConvention` enum, the `mut`/`take`/`view`/`ref` keywords, arena/region support, ownership diagnostics).
3. Identify existing syntax that conflicts with `~T`, `^T`, `&T`, `*T`.
4. Identify existing use of `@` (named loops, `@=` immutable bindings, `#` attributes) and ensure no conflict.
5. Identify existing use of `!` (logical not) and avoid introducing it for this system.
6. Identify existing optional/nullability syntax (`T?`).
7. Identify existing pointer/reference/borrow/storage syntax (`Ptr<T>`, `region`, arenas).
8. Identify existing overload resolution rules (or confirm Jet has none).
9. Write a short implementation note describing where capability concepts already exist and where new ones must be added.

## Phase 2: Syntax / Parsing

Implement prefix capability parsing in type positions:

```jet
~T
^T
&T
*T
```

Implement prefix capability parsing in expression positions:

```jet
~x
^x
&x
*x
```

Make sure parsing handles generics and nested types:

```jet
~List<Item>
^Map<String, Asset>
&Config
*U8
```

Resolve ambiguity with existing operators deliberately and document the rule: `~` (vs `~~` trait-attach), `^` (vs `^`/`^=` bitwise xor), `&` (vs `&&`/`&=`), `*` (vs `*` multiply and the current `*` deref). Prefix vs infix is position-disambiguated; pin down the deref/address-of distinction explicitly.

## Phase 3: AST / HIR Representation

Represent access capabilities explicitly.

Suggested enum (extend the existing `AccessConvention`):

```text
AccessCapability:
- Infer
- Read
- Write
- Move
- Share
- Raw
```

Unmarked should parse as `Infer`, not `Read`.

Later compiler stages resolve `Infer` into the needed capability.

## Phase 4: Deterministic Constraint Solving

Implement capability inference as deterministic constraint solving.

Required properties:

* Same source + same compiler version + same target/options = same resolved capabilities.
* Explicit markers are hard constraints.
* Unmarked values start as `Infer`.
* `Infer` resolves by semantic requirements, not performance preference.
* Resolution should reach a stable fixed point.
* If constraints conflict, emit an error.
* Do not use nondeterministic iteration orders where they could affect output. Sort symbols/constraints where necessary.
* Do not allow optimization passes to feed back into semantic capability resolution.

Constraint examples:

```text
field read                 -> requires at least Read
field assignment           -> requires Write
call to ~T parameter        -> requires Write
call to ^T parameter        -> requires Move
call to &T parameter        -> requires Share
explicit *x                -> requires Raw + unsafe context
use after ^ move            -> error
write while active read     -> error
share of short-lived value  -> error
```

## Phase 5: Type Checking / Capability Inference

Implement or integrate with existing inference:

* Reads require read access.
* Mutation requires write access.
* Ownership consumption requires move access.
* Escaping/storing/spawning/caching requires share access.
* Unsafe raw operations require raw access and unsafe context.

If existing work already has borrow/lifetime/ownership checking, adapt it to use this capability vocabulary instead of duplicating logic.

## Phase 6: Lowering / IR

Ensure capabilities survive long enough for checking and diagnostics.

After checking, lower to existing reference/pointer/ownership mechanisms as appropriate.

Do not require runtime overhead for capability markers unless existing semantics require it.

Resolved capability should be represented separately from physical passing strategy.

## Phase 7: Codegen

Generate efficient code equivalent to the lower-level model.

Expected mapping may be roughly:

```text
Read  -> immutable borrow/view/reference or by-value for cheap scalar types
Write -> exclusive mutable borrow/reference
Move  -> ownership transfer / move
Share -> safe escaping handle / ref-count / region-checked handle / task-safe reference
Raw   -> raw pointer/address
```

Do not hard-code this mapping prematurely if Jet already has or plans allocator/region abstractions. Keep the capability model independent.

## Phase 8: Optimization

After semantic capabilities are resolved, optimize physical passing strategy.

The optimizer may choose:

```text
by value
by register
readonly reference/view
exclusive reference/view
move elision
copy elision
safe shared handle
region-backed handle
raw pointer inside unsafe lowering
```

But it must not change:

```text
whether caller loses ownership
whether mutation is allowed
whether value can escape
whether raw unsafe access occurs
public capability signature
diagnostic behavior
```

Optimization must be deterministic for the same compiler version, target, flags, and explicit inputs.

## Phase 9: Tests

Add tests for:

* Parsing prefix type capabilities.
* Parsing prefix expression capabilities.
* Function parameters.
* Method receivers.
* Method call desugaring.
* Mutation inference.
* Move inference.
* Share/escape inference.
* Mixed explicit and inferred parameters.
* Explicit markers acting as hard constraints.
* Move-after-use errors.
* Read/write conflicts.
* Share/escape lifetime errors.
* Raw pointer requiring unsafe.
* Capability-only overload ambiguity (if overloading is in scope).
* Deterministic overload resolution.
* Deterministic inferred public API metadata.
* Public API capability metadata/freeze behavior.
* Interaction with generics.
* Interaction with optionals.
* Interaction with transactions/rollback if currently implemented.
* Interaction with atomics/tasks/networking if currently implemented.
* Conflict avoidance with `@` named loops.
* No new use of `!` for capability markers.
* Optimization does not change semantic capability.
* Same source/options produce same resolved capability signatures.

Every diagnostic needs a `tests/ui` snapshot; every user-visible feature needs a golden-tested `examples/` entry.

# Required Example Coverage

Make sure these examples or equivalent tests work.

## Basic

```jet
fn print(player: Player)
fn damage(player: ~Player, amount: I32)
fn equip(player: ~Player, item: ^Item)
fn cache(texture: &Texture)

#Unsafe
fn write_bytes(ptr: *U8, len: Int)

print(player)
damage(~player, 10)
equip(~player, ^sword)
cache(&texture)

#Unsafe {
    write_bytes(*buffer, buffer.len())
}
```

## Inferred Beginner Code

```jet
fn status(player: Player) -> String {
    return "{player.name}: {player.hp}"
}

fn heal(player: Player, amount: I32) {
    player.hp += amount
}

fn equip(player: Player, item: Item) {
    player.inventory.add(item)
}
```

Expected resolved capabilities:

```jet
fn status(player: Player)
fn heal(player: ~Player, amount: I32)
fn equip(player: ~Player, item: ^Item)
```

## Mixed Expert / Inferred Code

```jet
fn equip(player: ~Player, item: Item) {
    player.inventory.add(item)
}
```

Expected resolved capabilities:

```jet
fn equip(player: ~Player, item: ^Item)
```

## File IO

```jet
fn read_all(file: File) -> [U8]
fn seek(file: ~File, offset: Int)
fn close(file: ^File)

#Unsafe
fn read_raw(dst: *U8, len: Int)

bytes @= read_all(file)
seek(~file, 128)
close(^file)

#Unsafe {
    read_raw(*buffer, buffer.len())
}
```

## Networking

```jet
fn peer(socket: Socket) -> Address
fn send(socket: ~Socket, packet: ^Packet)
fn register(socket: &Socket)

#Unsafe
fn send_raw(socket: ~Socket, ptr: *U8, len: Int)

addr @= peer(socket)

packet @= Packet { bytes: encode(message) }
send(~socket, ^packet)

register(&socket)

#Unsafe {
    send_raw(~socket, *bytes, bytes.len())
}
```

## Asset Pipeline

```jet
fn dimensions(image: Image) -> Vec2
fn normalize(image: ~Image)
fn upload(image: ^Image) -> GpuTexture
fn cache(texture: &GpuTexture)

#Unsafe
fn upload_raw(ptr: *U8, len: Int) -> GpuTexture

size @= dimensions(image)
normalize(~image)

texture @= upload(^image)
cache(&texture)

#Unsafe {
    raw_texture @= upload_raw(*bytes, bytes.len())
}
```

## Methods

```jet
impl Player {
    fn name(self) -> String {
        return self.name
    }

    fn damage(~self, amount: I32) {
        self.hp -= amount
    }

    fn equip(~self, item: ^Item) {
        self.inventory.add(item)
    }

    fn share(&self) {
        PlayerRegistry.add(self)
    }

    fn destroy(^self) {
        World.remove(self)
    }
}

player.name()
player.damage(10)
player.equip(^sword)
player.share()
player.destroy()
```

## Ambiguous Overload

```jet
fn process(data: Data)
fn process(data: ~Data)

process(data)
process(~data)
```

Expected behavior:

* `process(data)` selects read/inferred-compatible overload if unambiguous.
* `process(~data)` selects write overload.
* If no deterministic rule can select one, emit an ambiguity error.
* A compiler flag to enable stronger overloads for performance vs keeping weaker overloads as default, must still be deterministic.

# Deliverables

Produce:

1. A repository audit summary.
2. An implementation plan based on the actual current compiler architecture.
3. The code changes.
4. Tests.
5. Updated docs or design notes.
6. Migration notes for old memory/reference/pointer syntax (`mut`/`take`/`view`/`ref` keywords, the D-CAP1 word vocabulary, `Ptr<T>`).
7. A list of unresolved design conflicts or places where current Jet architecture blocks the full model.
8. A note explaining how deterministic capability inference is guaranteed.
9. A note explaining how capability inference is separated from performance optimization.

# Non-Negotiables

* Do not assume outdated top-level docs are correct.
* Do not erase existing memory-management work without understanding it.
* Do not use `@` for capability syntax.
* Do not use `!` for capability syntax.
* Use prefix capability sigils only.
* Keep unmarked values as `Infer`.
* Start inference from minimal/read assumptions.
* Elevate capabilities only when semantically required.
* Keep capability inference deterministic.
* Keep performance optimization separate from semantic capability.
* Never let optimization change ownership, mutability, sharing, or unsafe semantics.
* Keep access separate from storage.
* Keep raw pointer behavior unsafe.
* Preserve clean field access with `.`.
* Favor beginner ergonomics while giving experts explicit control.
