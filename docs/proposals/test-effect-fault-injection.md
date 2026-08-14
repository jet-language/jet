# Test effect fault injection

Decision: D-TESTFAULT1=A, card #1916.

## Contract

The public form is:

~~~jet
#Test(faults: [Fs.Write]) fn sqlite_style_fail_nth_effect_loop_is_deterministic() {
}
~~~

faults is a typed #Test marker argument. It is test configuration. It does
not add a keyword, a sigil, a nested test member, or a suffix to an effect
row. The existing marker grammar reads the list and the existing effect
vocabulary supplies its root names.

Each list element is a dotted effect path. Sema checks the path shape,
canonicalizes acronym roots (Fs becomes the existing FS root), and stores the
canonical path for codegen. A root that is not in the existing effect
vocabulary uses E0119. A dotted leaf stays open under the existing effect-tree
law, so package-declared leaves remain expressible.

## One deterministic loop

The test Prelude owns the scheduler. A generated test passes its canonical
selector list to one helper. The helper:

1. runs the body once with fault injection disabled;
2. records the reachable allocation and I/O ordinals exposed by the selected
   effect adapters;
3. visits selectors in source order;
4. for each selector, visits n = 1, 2, … in order;
5. reruns the body with only that selector failing at ordinal n;
6. extends the schedule when a graceful earlier failure reveals more later
   call sites;
7. accepts an injected iteration when the body returns on the ordinary error
   rail, and rejects a panic;
8. clears the scheduler state after every iteration, including a panic.

The allocation schedule and the I/O schedule use the same fail-nth law. An
allocation adapter reports a typed allocation failure value. An I/O adapter
reports the existing IOError value. Neither adapter throws a host-only
exception or makes a policy decision.

The shared fallible-allocation rail is the final internal scheduler channel for
every non-empty fault plan. It does not add a public selector or change the
ratified `Fs.Write` spelling.

For Fs.Write, the shipped adapters cover ordinary writes, append/create,
atomic replacement, write-at, links, renames, directory mutation, temporary
files, locks, and file-writer flush/write operations. IO.Read, IO.Write, and
IO.Flush use the same Prelude scheduler for the corresponding stream
adapters.

## SQLite-style recovery

SQLite-style code can express the recovery contract with ordinary Jet error
handling:

~~~jet
use core.files as fs
use core.testing as testing

#Test(faults: [Fs.Write]) fn sqlite_style_fail_nth_effect_loop_is_deterministic() {
    path :: testing.temp_dir("sqlite-fault")
    fs.write(path, "journal") ?? return
    values := List.try_with_capacity(1) ?? return
    values.try_push(1) ?? return
    require(values.len() == 1)
    fs.remove(path) ?? return
}
~~~

The clean run proves the normal path. Each reachable write site then receives
one injected failure. ?? return handles the injected IOError without a panic,
so the iteration passes. The same body exercises the shared fail-nth allocation
rail through the typed AllocError result. It also proves that later cleanup
does not turn an earlier write failure into an uncaught panic.

## Tier law

The selector is resolved in sema. The fail-nth loop and scheduler live in the
one embedded Prelude. AOT operation adapters and Cranelift operation adapters
only marshal the selected operation into that scheduler. The interpreter
uses the same checked test metadata and the same Prelude symbols where its
ambient Core operation applies. Web builds have no native filesystem surface,
so this native test configuration has no web operation to inject.

No host engine re-parses faults, chooses a failure ordinal, or rewrites an
IOError. The production runtime keeps the scheduler inactive.

## Evidence

- crates/jet-foundation/src/Syntax/math_layout.rs registers the named
  parameter under D-TESTFAULT1=A.
- crates/jet-codegen/src/Prelude/Markers.jet publishes the [Effect] marker
  contract.
- tests/ui/test_fault_unknown_effect_root.jet and its snapshot prove the sema
  teaching diagnostic.
- tests/effects.rs proves byte-stable fail-nth schedule emission.
- examples/features/tooling/testing_helpers.jet carries the executable
  SQLite-style example; its run output remains unchanged because tests are
  collected by jet test.
