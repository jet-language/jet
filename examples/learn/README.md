# Learn

`jet learn` is Jet's deterministic, offline curriculum. It guides a learner
through four source-and-run lessons: a loop, state and lifetime, effects, and
foreign bindings. The curriculum is data in
[`curriculum.json`](curriculum.json); the command implementation is
[`Source/CmdLearn.rs`](../../Source/CmdLearn.rs), and the
behavioral contract is covered by [`tests/learn.rs`](../../tests/learn.rs).

## Start the curriculum

Run it from the repository root through the project environment wrapper:

```sh
scripts/agent/jet-env jet learn
```

The command stages each lesson under `.jet/learn/feedback/`. A lesson follows
the same evidence loop:

1. predict the result;
2. check the prediction against the program;
3. read the explanation;
4. make a controlled edit; and
5. transfer the idea to a new example.

The staged source is feedback material, not a second copy of the curriculum.
Use the source path printed by the command when an explanation points back to a
lesson.

## Modes and feedback

The curriculum and its checks are offline and deterministic. A terminal stdin
selects interactive prompts; piped actions make the same session usable
non-interactively. These switches select an explicit interaction or output
mode:

```sh
scripts/agent/jet-env jet learn --watch=off
scripts/agent/jet-env jet learn --json
scripts/agent/jet-env jet learn --quiet
scripts/agent/jet-env jet learn --check
```

`--check` validates the packaged curriculum and exits without advancing a
lesson.
`--json` emits machine-readable lesson events; `--quiet` suppresses
presentation text while retaining command results. `--watch=off` disables
source watching when a caller needs a one-shot session. Learners may answer
`unknown`, skip, or cancel a checkpoint; the command can resume an existing
feedback directory and treats stale or cancelled sessions explicitly rather
than silently reusing their state.

The VS Code extension exposes the same workflow through its Learn Watch and
Learn Once commands. See [`editors/vscode/README.md`](../../editors/vscode/README.md)
for editor setup; the command line remains the portable contract.

## Lesson layout

Each lesson supplies source, a prediction prompt, observed evidence, an
explanation, a constrained edit, and a transfer prompt. The curriculum's
sequence and prerequisite fields determine order; this README intentionally
does not duplicate that data. To add or revise a lesson, update the curriculum
record and its executable fixture together, then exercise the focused learn
checks described in the contributor guidance.
