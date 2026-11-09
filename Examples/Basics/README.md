# Basics

Values, output, loops, branches and functions: the shortest path through Jet.

Run a lesson from the repository root, for example `jet run Examples/Basics/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [BodyRules](BodyRules.jet) — a result follows the one arrow, named bodies use braces, a body's last expression is its value, and `return` exits early.
- [Branches](Branches.jet) — Ordered decisions with Jet's pattern-arm table.
- [ChainedComparison](ChainedComparison.jet) — A same-direction relational chain reads like the range guard it is — `0 <= sev < 10` means "sev is between 0 and 10", not two separate comparisons the…
- [Compound](Compound.jet)
- [ComprehensionForms](ComprehensionForms.jet)
- [DebugTrace](DebugTrace.jet) — `debug(value)` returns its value unchanged and writes one stderr line with the file, line, expression text, and Debug form.
- [DefaultRefs](DefaultRefs.jet) — A default may reference an earlier parameter.
- [FencedNames](FencedNames.jet) — `<: … :>` repeats its whole statement once per entry without creating a list.
- [FirstHour](FirstHour.jet) — First-hour tour: one small command-line tool that also builds as a native binary.
- [FirstHourExpert](FirstHourExpert.jet) — Expert variant of first_hour.jet.
- [FizzBuzz](FizzBuzz.jet)
- [Hello](Hello.jet) — Your first program: `run: fn` is the explicit entry.
- [IfExpression](IfExpression.jet) — `if` as a value, and optional condition parens.
- [IfUniversal](IfUniversal.jet) — `if` is the one branching keyword.
- [InterpDebugLabel](InterpDebugLabel.jet) — Trailing `=` reprints the expression text, then " = ", then the value.
- [LabeledLoops](LabeledLoops.jet) — a loop can carry a compile-time name, `break(name)` and `next(name)` target it from inside a nested loop.
- [LoopForms](LoopForms.jet) — the shapes of the single `loop` keyword.
- [MethodChain](MethodChain.jet) — A broken dot-chain keeps one step per line, each freely commented.
- [MultilineString](MultilineString.jet) — `"""…"""` spans many lines.
- [Numbers](Numbers.jet)
- [Onboarding](Onboarding/) — First-hour lesson 3: the `run.jet` that `jet new` creates, edited to read a typed `#CLI` argument.
- [PatternMatching](PatternMatching.jet)
- [Pipes](Pipes.jet) — A pipe runs one step per line; inside a `|>` step, `<>` is the value of the line above.
- [PrintLines](PrintLines.jet) — Builtin `print` is variadic; each argument writes its own line.
- [ScriptRun](ScriptRun.jet) — Ordinary files run only their explicit `run: fn` body.
- [SplitJoin](SplitJoin.jet) — Splitting a text and joining the pieces again replaces every separator with the joiner, keeping empty pieces at the start, between two separators and at the…
- [StringAppend](StringAppend.jet) — `text += piece` appends to a String place: a local, a struct field, a field reached through a list element, or a `&String` parameter.
- [StructCallWord](StructCallWord.jet) — Calls in record fields preserve fixed-width bits, not exact-Int pointer tags.
- [StructConstants](StructConstants.jet) — Module-scope `:` constants can hold struct, enum, and list literal data.
- [Tuples](Tuples.jet)
- [ValueDispatch](ValueDispatch.jet) — Choosing one branch from many: multi-arm `if`.
- [ValueLoopSearch](ValueLoopSearch.jet) — A search loop states what exhaustion means.
- [Values](Values.jet) — Values and types.
- [ValueTailLoops](ValueTailLoops.jet) — The final line of a value-expected function is in value position, so a finite source loop with `->` there is a collecting loop too, exactly as in `names:…

## Functions

- [CallablePolicies](Functions/CallablePolicies.jet) — Declaration defaults and apply(...) replacements keep the same checked callable signature.
- [Callbacks](Functions/Callbacks.jet)
- [ClosureCaptureWrites](Functions/ClosureCaptureWrites.jet)
- [Closures](Functions/Closures.jet)
- [FunctionValueLabels](Functions/FunctionValueLabels.jet) — A function read as a value keeps its labels and defaults.
- [Functions](Functions/Functions.jet) — Functions are defined with `fn` and called by name.
- [LambdaInference](Functions/LambdaInference.jet) — A lambda parameter's type may be omitted where the expected type already fixes it.
- [LambdaInterface](Functions/LambdaInterface.jet) — Lambdas write parameters and an optional effect row only.
- [MemberNamespaces](Functions/MemberNamespaces.jet) — Fields and methods have separate namespaces.
- [NamedArgs](Functions/NamedArgs.jet) — Argument labels and parameter contracts.
- [NeverReturn](Functions/NeverReturn.jet)
- [ReturnedFunctionCall](Functions/ReturnedFunctionCall.jet) — A returned function value uses the explicit `.call(...)` projection, whether it is first bound or invoked directly.
- [RootCalls](Functions/RootCalls.jet) — One marked receiver supports both call spellings.
- [StructAsFn](Functions/StructAsFn.jet) — Parentheses call functions and lambdas only.
- [TrailingBlock](Functions/TrailingBlock.jet) — Pass a multiline code block as a function argument with `() -> { … }`.
- [VariadicsSpread](Functions/VariadicsSpread.jet) — Variadic parameters, call spread, and list spread.
