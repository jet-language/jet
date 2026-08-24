# Task: repository-marker-scan

Domain: repository-search-and-edit
Case: success
Required outcome: exit=0;stdout=exact

## Input

Your program is given the path to an input directory as its first argument.
Its working directory is a scratch directory you may write to freely; the
input directory itself must be left unchanged.

The input directory contains exactly this, with small files shown inline:

    notes/
    notes/plan.md  (112 bytes)
      | # Repair plan
      | 
      | - agent_workload: inspect the failed check before the edit.
      | - Keep the successful output stable.
      | 
    README.md  (87 bytes)
      | # Sample repository
      | 
      | This anonymized tree represents a small agent-maintained project.
      | 
    src/
    src/main.txt  (134 bytes)
      | fn prepare() {
      |     // agent_workload: locate the stale behavior.
      |     fix_root_cause()
      |     // agent_workload: run the focused proof.
      | }
      | 

## Required output

Write to standard output EXACTLY the following bytes, and exit with status 0.
Trailing newline matters. Do not print anything else -- no logging, no
progress, no banner.

----- BEGIN EXPECTED STDOUT -----
/notes/plan.md|1
/src/main.txt|2
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
