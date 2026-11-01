# Philosophy

Vocabulary: [Jet vocabulary](vocabulary.md).

Jet is a pragmatic language. It should let beginners build useful programs
safely and give experts explicit control without making everyone learn the
low-level machinery first. It is a memory-safe compiled language for human
engineers, and it chooses what works for real programs and real teams over
purity or novelty. Agent usability follows from clear rules, useful
diagnostics, and predictable repairs.

This is Jet's one strategic guidance document. It sets direction, not a feature
list or release promise. Specific goals, plans, priorities between work items,
and development status belong in Tower. Code and executable evidence establish
what works. [AGENTS.md](../../AGENTS.md) defines authority and decision rights.

## Philosophy Pillars

1. **Implicit Beginner Magic, Explicit Expert Control, Enterprise Auditability:** Beginner magic is implicit: useful defaults just work without being asked for. Expert control is explicit: every override is written where it applies, so a reader can see it. We strive for useful defaults, little boilerplate/ceremony, genuinely useful, descriptive errors that guide to user-intended resolution with the what, the why, and the how to fix. Experts should never have anything short of full control available to them, HOWEVER, instead of the more common opt-in to useful defaults or "magic" for beginners (who wouldn't know what to opt into anyway), experts should generally opt-out, because they have the appropriate domain knowledge & experience to know when either approach is best. Enterprises need auditability, traceability, & enforceability for large codebases - that is what Jet should excel at.
2. **Easy To Reason About, Read, & Write:** While we maintain a strong emphasis on UX/DX/ergonomics, most of the time spent with code is reading & the most critical time spent with code is reasoning about it. We strive to prioritize the ability of a developer to reason about the code they are working with to the greatest exent possible for a programming language. Jet is also targeting high readability & excellent ergonomics.
3. **Friction Inversely Proportional To Commonality:** The most common operations, tasks, functions, methods, should have the least amount of friction. The more rare an operation, the more leeway for additional friction, while targeting minimal everywhere. 
4. **Ladders, Not Cliffs:** Each capability is one mechanism that a user climbs in rungs, from the smallest scope to the largest. A code block can become a lambda, then a function. Effects and markers apply to a block, a function, a module, or a package. A build can live in one CLI command, a custom `fn build`, or a full `build.jet`; tests climb the same way. Every rung keeps the same meaning and the same names. The nearest rung wins, and unmentioned settings inherit. Prototypes start on the lowest rung, often with no file at all, and grow to modular, scalable code without rewrites. A ladder must stay easy to reason about: add a rung only where real programs need it, and make it obvious which rung is in force.
5. **The Language Is A Library:** The compiler is a small kernel of primitives, and the compiler itself is a public, versioned library. Every capability that can be a library is one. Core is ordinary Jet built only from mechanisms any package can use: no private compiler hooks, no names only Core may call, and no diagnostics or optimizations that only Core can earn. Users can extend, replace, or build a peer to any Core feature with the same power, safety, diagnostics, and performance. Domain stacks such as GPU providers, AI, terminal UI, and web ship as libraries, not as language special cases. The CLI, the language server, `fn build`, compile-time code, and outside tools all call the same compiler APIs for parsing, checking, facts, IR, and code generation, so no tool keeps a private copy of a language rule. The kernel grows only when a library cannot express a capability safely or fast enough, and the reason is recorded.
6. **More Power, More Done, Faster:** Jet should be more powerful and expressive than the languages it replaces while letting people get more done, faster. Expressiveness serves shipping, not cleverness: a feature earns its place when it makes real programs shorter, clearer, and quicker to get right.
7. **Fewer Layers, Simpler Wins:** Fewer layers are better, and simpler is better. Every layer of indirection, configuration, tooling, or abstraction must pay for itself. When two designs do the same job, choose the one with fewer moving parts and fewer concepts to learn.
8. **Explicit Where Quality Depends On It:** Explicitness is important for quality. Implicit magic is reserved for safe defaults whose meaning is predictable. A choice that changes behavior, cost, safety, or authority is visible in the code where it is made, so authors, reviewers, and tools can see and check it.

## Design priorities

When priorities conflict, the earlier one wins:

1. **Memory and type safety.** Safe by default. Expert escape requires explicit,
   audited opt-in; unsafe power must not leak into ordinary code.
2. **Beginner experience.** Prefer useful defaults, little ceremony, and errors
   that explain what happened, why, and how to fix it. Reveal complexity when
   the program needs it, not before the first useful result.
3. **Runtime performance.** Do not buy convenient syntax with hidden runtime
   overhead. Judge performance on correct, matched workloads under the
   [strict performance gate](../../AGENTS.md#strict-performance-gate).
4. **One mechanism.** Keep one semantic path for each operation. Allow flexible
   code organization when it expresses the same meaning; reject parallel
   mechanisms that make users choose without gaining control.
5. **Implementation simplicity and compile speed.** Prefer clear boundaries,
   local reasoning, and fast feedback. These do not excuse sacrificing the
   higher priorities.
6. **Ecosystem breadth.** Aim to make Jet the best tool across workloads, with
   coherent first-party libraries. Do not foreclose systems or application
   domains through a convenient short-term design choice.

Implementation difficulty is not a reason to lower the bar. Choose for the
result and its long-term maintenance, not for an easier incomplete delivery.

## Boundaries that keep the language coherent

- **Beginner defaults, expert control.** A simple program should need little
  setup. Advanced control is explicit and pays only for what it uses.
- **Jet owns its diagnostics.** The front end checks language rules. A backend
  rejection of generated code is a compiler defect, never a user error.
- **One meaning across execution tiers.** Shared semantics belong in the
  Prelude and CoreLib; execution engines adapt them rather than inventing
  their own defaults, validation, or error behavior.
- **Facts move safely.** Tighten facts silently; loosening needs a written,
  accountable gate. A compiler fact is not a second runtime policy mechanism.
- **Say it once.** Put each spelling, semantic rule, and message in one
  executable home. Render other views from it. Keep rationale in small
  explanations, not in duplicated catalogs or assertions of current status.

Ratified decisions and their alternatives live in Tower. Specs explain the
contracts worth explaining. Neither a decision nor this document substitutes
for running the program.
