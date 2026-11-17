---
title: Owner scratch
---

"Error messages are the user interface of your compiler." Jonathan Blow https://youtu.be/e6crOMC9WCE?si=AFgqGl_HUBQ_0cRO

Have a customizable setting for defining style guides for auto linting in jet?

Have opus create a briefing/pitch deck for jet to consider getting anthropic to sponsor/back the project

Update philosophy to be a PRAGMATIC language as well
Implicit beginner magic, explicit expert control.
Be more powerful & expressive, while getting more done faster.
Fewer layers are better, simpler is better
Explicitness is important for quality

Cache hits are critical -> how can we give control over L1,2,3 cache, visualize when we are no longer in cache & go to RAM or HDD/SSD?

Do we have a type based switch system?

Only hard problems require guidance -> how does jet set users up to tackle hard problems with a consistent approach and clarity?

Need composition for our APIs - wrappers can be nice & solve some problems but often times make things outside that case harder -> have a very strong set of primitives-level APIs that allow expression of everything needed, then provide pareto level high quality wrappers for most common work as quality of life improvement & friction reduction.

What is SOLID, how can we learn from it & apply it to jet & what are its weaknesses that we should avoid?

Need to meet/beat zig -> in particular sor arrays, SOA with ability to reach in & touch pointers in arrays.

Overly safe/overly encapsulated/over prescription can be harmful & limit what a programmer needs to do, especially for engineers it removes the ability to do a cost benefit analysis. How can we structurally avoid this in jet, traditional methods and innovative approaches?

How do we reduce layers of abstraction between the language & the generated ASM?

Need to include primitive types that Odin does -> quaternions, different endian types, complex numbers, floating point numbers w endian specialization, etc.

Zero undefined behavior in odin -> need to ensure the same in jet.

Set compilation/build settings in fn build() for HW optimization, like if you know the HW word size, can set a value per target in build settings, build for CPU/GPU or multiple targets, etc.

Odin does not have tuples but can return multiple values -> I like this idea but how do we keep output names?

Relook at string types -> string should be constant, not flexible, and string builders (dont love the name). String builders are the mutable version that can grow shrink, change, etc -> strings should generally be fixed. Discuss.

Pointer classes: null, pointer to T, Maybe(Pointer to T)
~ as not, complement with ~=, &~

Odin doesnt like move semantics, why?

Type conversions generally need casts, how can we make this clean? Maybe use <>?

Immutables should be comptime consts. Verify.

Implicit type inference in some cases, where, when, is it supported, when should it not be allowed?

Odin allows choosing an enum's int type that backs it & assigns them to numbers -> do we have this functionality? Can we add it? seems very useful even if it can only be used on non complex/non nested enum types.

Always error on incomplete enum matching/handling -> ensure our pattern matching always requires exhausting enum cases.

Remove alias keyword, just alias by assigning as a const/immutable declaration. myNewType :: myOldType

Odin has variadic min, max, clamp built in directly, no lib import needed, part of prelude -> ensure jet has this.

Odin has excellent built in array types/support, especially for things like 3d/graphics/etc. They have built in array operators that just work out of the box with * or + etc, and have SWIZZLING -> ALL MUST BE ADDED TO/VERIFIED for jet.

How can jet support custom allocators? What does this feature intefere with? Does this overlap with anything? Can we just support it natively? what about soa data types? We should support those.

LLVM has regressions, it is slow and buggy and not ideal, definitely want to consider our own backend replacing LLVM.

Mojo supports split compilation on CPU/GPU, how can jet do that? Mojo has a portable GPU kernel library, can jet use/support something similar to unlock maximum performance & allow users to more easily take advantage of the GPU? right now barrier to entry is massive for utilizing GPU.

Work backwards from the speed of light as engineering limitation to determine what the language should support/generate for machine code/binary. Want to be faster than rust with tail optimizations etc. 

Need cohesive experience to push code to GPU, how can we do that in jet? Can we push code to run on the GPU?
Sine is defined in mojo so that it is portable -> i want to have custom allocations/declarations where we can to make jet code more portable and accurate.

Mojo has a bytecode/vm effectively which supports portability -> one of jet's goals is to effectively replace WASM as a portable target, so we need to deeply investigate. One issue with WASM is that it doesnt expose the HW, like tensor cores, so it is a lowest common denominator system. We need to improve on that, have some kind of metadata API that allows portable code to leverage the HW capabilities exposed by the host running the jet sandbox.

RAII not great, cleanup memory earlier, there can be tail recursion issues where memory is actually NOT freed because it is overwritten before being cleaned up/destructed. 

Need to optimize out memcopies.

Why would you need classes when you have structs?

Where does jet stand on unity build vs multiple compilation units? Seems like unity is the overall preference -> what are the end effects of this?

Does {} notation work in multiline strings in jet as well? like 
```jet
var_long :: """
Lorem Ipsum {myVar}
"""
```

Research zig @... what do they mean? 

Have an agent analyze/harvest/mine release notes for all languages over the last 5 years to see what we should consider avoiding, fixing, adding, removing, etc. with their reasoning for why things may not have worked in practice.

Look into Zig Build Server Protocol -> what to include, what to exclude, how to loop into comptime metaprogramming (jai style)

:: should basically be an implicit request for compiler comptime eval as an optimization, prep/comptime keyword forces comptime eval

Need to do a "challenge audit" -> are each of the features, tools, methodologies, conventions, etc. good, beneficial, as simplified as possible? Need to analyze the taste of the language, we did a kitchen sink build, now we need to apply taste and cull, curate the language to be cohesive and not just allow everything all the time.

Need excellent builtin support for things like state machines

Need to preserve compiler memory optimizations. In rust, structs are reorganized for maximum memory efficiency, unless #repr(c) is used, in which case the compiler does not optimize -> Always want the optimization and STILL need to confirm interop/FFI with C WITH optimizations in jet

Create an "onboarding" file both to educate and bring new maintainers/contributors up to speed & to assist any agent development

Ideas for the ~ and ^ symbols? What should they do?

Power should be **

Terminal "commands" like "execute(term.clear_buffer, term.enable_raw_mode, term.enable_alternate_screen, term.hide_cursor)

Need our version of jai implicit context passed as an invisible arg to every function. Allows effectively garbage collected style implicitly -> how can we implement? https://youtu.be/sEax8CQmgwo?si=KbuxOTtnm087KI6H 

TLA+ /\ as trailing disjunction?

Thoughts on every value in the language being basically a custom wrapped pointer, so whenever you pass something it is always "by pointer" unless you "unpack it" to copy to another memory address. That way you can access both the value or pointer without having to think about it, the compiler picks the most efficient one, and things are clean. Just rough brainstorming, need discussion

Port doom to jet to test

Mine: https://smarimccarthy.is/posts/2024-12-02-four-years-of-jai/
https://www.forrestthewoods.com/blog/learning-jai-via-advent-of-code/

Make := that is never reassigned an error

Lambda as unnamed function and anonymous union as unnamed enum

Mine the jet playlist to gather as much intel on jai as possible, put into a "reverse engineering" analysis to learn from it. Also, ensure we have an exhaustive list/info on the compiler and build options and metaprograms specifically so we can ensure the same level of support features customization etc in jet

dispatch by type in jai just works -> if param.value.tag == {
  Int -> 
  Float -> 
  else ->
}

Research relative pointers and benefits -> maybe use at comptime and erase for zero cost runtime?
```c
typedef struct Node Node;

struct Node {
    char text[32];
    uint16_t left;
    uint16_t right;
};

#define NODE_POOL_CAP 1024
Node node_pool[NODE_POOL_CAP];

int main()
{
    Node root;
    node_pool[root->left];
    return 0;
}
```

language bindings could just be a single jet file that is a metaprogram for rewriting in jet or some other binding method
