# Task: repository-semantic-inspection

Domain: repository-search-and-edit
Case: semantic-index
Required outcome: exit=0;stdout=exact

## Input

Your program is given the path to an input directory as its first argument.
Its working directory is a scratch directory you may write to freely; the
input directory itself must be left unchanged.

The input directory contains exactly this, with small files shown inline:

    migration.codemod.json  (366 bytes)
      | {
      |   "version": 2,
      |   "name": "RenamePrepare",
      |   "project": "project",
      |   "roots": [
      |     {"path": "examples", "validate": "clean"}
      |   ],
      |   "rules": [
      |     {
      |       "id": "rename-prepare",
      |       "kind": "symbol_rename",
      |       "from": {"name": "prepare", "symbol_kind": "function", "defined_in": "examples/main.jet"},
      |       "to": "configure",
      |       "matches": 2
      |     }
      |   ]
      | }
      | 
    project/
    project/examples/
    project/examples/main.jet  (130 bytes)
      | fn prepare() {
      |     print("ready")
      | }
      | 
      | fn run() {
      |     prepare()
      |     print("prepare stays in text")
      | }
      | 
      | // prepare remains a comment.
      | 

## Required output

Write to standard output EXACTLY the following bytes, and exit with status 0.
Trailing newline matters. Do not print anything else -- no logging, no
progress, no banner.

----- BEGIN EXPECTED STDOUT -----
definitions=2
references=3
calls=3
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

Write your solution in Jet, as a single file named `candidate.jet`.

Check that it compiles or parses with:

    scripts/agent/jet-env jet check candidate.jet

Fix every error the checker reports and check again. When the checker is
clean, stop and report. Do not run the program against the expected output --
you are not given a way to compare, and guessing from the output is not part
of this task.

## Report format

Your final message must be exactly these lines and nothing else:

ROUNDS: <number of edits you made after the first version; 0 if the first version checked clean>
CLEAN: <yes|no>
DIAGNOSTICS: <total count of distinct checker errors you saw across all rounds>
