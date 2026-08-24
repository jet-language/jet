# Task: build-test-failure-recovery

Domain: build-test-debug-and-git
Case: compile-check-recovery
Required outcome: exit=0;stdout=exact

## Input

Your program is given the path to an input directory as its first argument.
Its working directory is a scratch directory you may write to freely; the
input directory itself must be left unchanged.

The input directory contains exactly this, with small files shown inline:

    build_test_recovery.jet  (83 bytes)
      | fn run() {
      |     print("ready")
      | }
      | 
      | #Test("build test recovery") {
      |     assert(true)
      | }
      | 
    invalid.jet  (31 bytes)
      | fn run( {
      |     print("ready")
      | }
      | 
    invalid.mjs  (42 bytes)
      | function run( {
      |   console.log("ready");
      | }
      | 
    invalid.py  (29 bytes)
      | def run(:
      |     print("ready")
      | 
    invalid.sh  (37 bytes)
      | if then
      |     printf '%s\n' 'ready'
      | fi
      | 
    valid.mjs  (22 bytes)
      | console.log("ready");
      | 
    valid.py  (15 bytes)
      | print("ready")
      | 
    valid.sh  (42 bytes)
      | #!/usr/bin/env bash
      | printf '%s\n' 'ready'
      | 

## Required output

Write to standard output EXACTLY the following bytes, and exit with status 0.
Trailing newline matters. Do not print anything else -- no logging, no
progress, no banner.

----- BEGIN EXPECTED STDOUT -----
invalid=failed
recovery=ok
test=ready
----- END EXPECTED STDOUT -----

## Rules

- Read the input from the directory path given as the first argument.
- Do not hardcode the expected output as a literal string. Compute it from the
  input. A submission that prints a baked-in constant is a failure even though
  the bytes match.
- Do not write into the input directory. Scratch files must go in the working
  directory and must be cleaned up before exit.
- No network access.

## Language

Write your solution in JavaScript (Node ESM), as a single file named `candidate.mjs`.

Check that it compiles or parses with:

    node --check candidate.mjs

Fix every error the checker reports and check again. When the checker is
clean, stop and report. Do not run the program against the expected output --
you are not given a way to compare, and guessing from the output is not part
of this task.

## Report format

Your final message must be exactly these lines and nothing else:

ROUNDS: <number of edits you made after the first version; 0 if the first version checked clean>
CLEAN: <yes|no>
DIAGNOSTICS: <total count of distinct checker errors you saw across all rounds>
