# Designing Jet programs

This guide is for the programmer about to write a large Jet program, or to
change a hard part of one. It gives a method for hard problems, the SOLID
principles as they read in Jet, and the escape-hatch ladder: the order in
which to reach for more power when the safe default is not enough.
[Philosophy](../philosophy.md) is the strategy this guide applies; the
[language spec](../spec.md) and the [Core library reference](../Reference/core-library.md)
are the contracts it relies on, and [Jet vocabulary](../vocabulary.md) defines
the words they share.

## A method for hard problems

Work through these steps in order. Each one is cheap next to the rework it
saves.

1. **Say what it means in one sentence.** Write the behavior a caller sees,
   not the mechanism: "a saved draft survives a crash", not "the editor
   fsyncs a journal". If the sentence needs "and", there are two problems.
2. **Find the mechanism that already owns that meaning.** Search Core and
   your own modules before adding anything. When a mechanism exists, extend
   it; a second mechanism beside it makes every reader choose between two
   ways to say one thing. "One mechanism" is a design priority in
   [philosophy](../philosophy.md#design-priorities).
3. **Write the smallest program that shows the behavior.** A `#Test` or an
   example that fails now and passes when you are done is the definition of
   done. Write the failure cases too: what the caller sees when the disk is
   full, the input is hostile, or the peer goes away.
4. **Pick the lowest rung of the ladder that does the job.** A block before a
   lambda, a lambda before a function, a module before a package
   ([philosophy](../philosophy.md#philosophy-pillars), "Ladders, Not Cliffs").
   Climb only when the program needs the next rung.
5. **Make authority visible.** Write the effects a function needs in its
   arrow, `-[FS.Read]>`, as narrow as the effect tree allows
   ([D-EFFTREE1](../syntax-decisions.md#effects--safety)). Every effect is
   allowed by default and code denies what it must not do
   ([D-AUTH-DEFAULT1](../syntax-decisions.md#effects--safety)), so a written
   effect is a statement to the reader, and `-[!Net]>` is a promise the
   compiler holds.
6. **Make failure a type.** A caller should be able to match every way the
   operation fails: return a typed error (`-> Draft SaveError!`), not a
   sentinel string or a panic. A panic is for a broken invariant, never for an
   input the caller could send.
7. **Measure before you optimize.** Write the clear version first, measure
   it on the real workload, and keep the measurement beside the change. A
   faster version that changes behavior is a different program.
8. **Leave one home for each rule.** When the work is done, each rule,
   spelling and message lives in one place, and every other view is rendered
   from it.

## SOLID, translated for Jet

The SOLID principles come from class-based languages. Jet has no classes or
inheritance; it has structs, enums, traits implemented inside a type, effects
and modules. The principles still hold, in these forms.

### Single responsibility: one meaning, one home

A module or function owns one meaning. A struct holds the data of one idea,
and the functions that keep its rules sit beside it. When two places compute
the same thing, one of them is wrong today or will be after the next change;
make one call the other.

### Open for extension: traits and providers

Extend a design by implementing its trait, not by editing a closed list. Core
does this itself: a package adds a compute device by implementing the
`Accelerator` trait, and Core admits it without any edit to Core
([D-ACCEL-LIB1](../Reference/core-library.md#corecompute--checked-tensors-and-explicit-devices)).
Design your own extension points the same way: a trait the extension
implements, and one function that admits an implementation.

```jet
Shape: trait {
    area: fn(self) -> Float Never!,
}

Square: struct {
    side: Float,

    impl Shape {
        area: fn(self) -> Float Never! { self.side * self.side }
    }
}
```

### Liskov substitution: an implementation keeps the contract

Code written against a trait must work with every implementation of it. Jet
checks the shape of that promise: an implementation's signature must match
the trait's, and a trait method may declare an effect upper bound that every
implementation is held to ([D-EFF3](../syntax-decisions.md#effects--safety)).
The behavior is yours to keep. State it in the trait's doc comment, and test
it against every implementation, as Core admits an accelerator only after it
gives the CPU reference answers.

### Interface segregation: small traits, narrow effects

A trait asks for the fewest methods its callers need. A function asks for
the narrowest effects it uses: `-[FS.Read]>`, not `-[FS]>`, when it only
reads. Narrow effects are interfaces too: they tell the reader, and the
compiler, everything the function can touch.

### Dependency inversion: depend on the seam, not the provider

High-level code names a trait or a Core seam; the concrete provider is chosen
at the edge of the program. Core's UI and its target machines work this way:
a UI program draws through one backend interface whether the pixels land in a
terminal, a native window or a browser page, and a target machine supplies
its clock, allocator and output providers to code that never names them.
Pass the provider in, or choose it in `run`, so the rest of the program can
run under a test double.

## The escape-hatch ladder

Safe Jet is the default, and most programs never leave it. When a program
needs power the safe default does not give, climb one rung at a time, and stop
at the first rung that does the job. Each rung is explicit in the source, so a
reviewer can find every place a program left the rung below.

1. **Core, as documented.** Use the Core API with the effects it declares.
   If Core lacks something another language's standard library has, that is
   a gap in Core to report, not a reason to climb
   ([philosophy](../philosophy.md#philosophy-pillars), "More Power, More Done,
   Faster").
2. **A trait implementation or provider package.** Add the capability through
   the mechanism Core already exposes (an `Accelerator`, a UI provider, a
   target machine's providers). The result has the same checks and
   diagnostics as Core's own code.
3. **A sandboxed plugin.** Run code you do not trust as a WebAssembly
   component. A guest gets only the rights the host grants it
   ([D-PLUGIN1](../architecture.md#application-plugin-host-boundary-d-plugin1--d-plugin-export1--4717)),
   so a bug or a hostile guest cannot reach the rest of the process.
4. **Another program, under `Exec`.** Run a separate executable with
   `core.process`. The call carries the `Exec` effect, so every caller up the
   chain shows it, and a module that denies `Exec` cannot reach it. `Exec`
   and `FFI` are the two escape-hatch effects
   ([D-AUTH-DEFAULT1](../syntax-decisions.md#effects--safety)).
5. **A foreign function, under `FFI`.** Bind a C function with `#Import(c)`.
   The binding records the `FFI.C` effect, and a scalar binding needs nothing
   more:

   ```jet
   #Import(c) host_reenter: fn(value: Int) -> Int = "host_reenter"
   ```

6. **An audited unsafe block.** Raw memory, pointer formation, inline
   `#FFI(c)` bodies and foreign calls that transfer ownership need an
   enclosing `#Unsafe("reason")` block. The reason is required and is the
   audit record ([D-UNSAFE2](../safety.md#the-audited-escape)):

   ```jet
   cell: 1
   #Unsafe("`cell` is live on this stack frame and the pointer never escapes") {
       ptr: *cell
       value: ptr.*
   }
   ```

7. **An audited unsafe function.** `#Unsafe("reason") name: fn …` makes the
   whole body an audited region. Calling it still needs an enclosing
   `#Unsafe` region, so the obligation reaches every caller.

Keep each rung small. Wrap the code on a high rung in a safe function with
narrow effects and a typed error, and let the rest of the program call that.
A package or an organization may forbid the upper rungs outright; a lexical
gate cannot raise that floor ([safety](../safety.md#the-audited-escape)).
