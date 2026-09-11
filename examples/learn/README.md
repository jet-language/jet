# Jet Learn

`jet learn` is an offline, deterministic set of four source-and-run lessons.
The lessons cover loop control flow, state/lifetime transitions, effect
boundaries, and foreign bindings.

Run it from a checkout:

```text
scripts/agent/jet-env jet learn
```

The runner places one lesson source at a time under `.jet/learn/feedback/`.
Each lesson follows the same path:

1. Read the source and predict what it will do.
2. Check the source and compare the compiler or program output.
3. Explain the result, then make one controlled edit.
4. Solve a different example with a different source shape.

Type `unknown` if you do not know the answer. Learn shows the written
explanation immediately, without counting the prediction as correct or
completing the lesson.

The terminal view shows the source, the exact file to edit, expected and actual
output, compiler errors, hints, and the current action. Save a change to let
the default watcher check it again. `--watch=off` performs one check.
`--json` emits machine status, and `--quiet` suppresses lesson prose.

While Learn waits for a file edit, use Ctrl+C to exit and `jet learn` to resume.
Typed commands such as `cancel` work at `learn>` prompts.

```text
scripts/agent/jet-env jet learn --watch=off
scripts/agent/jet-env jet learn --watch=off --json
scripts/agent/jet-env jet learn --check
```

Progress records bind to the curriculum, source, and checked-answer revisions.
If the source changes outside the lesson flow, the saved step becomes `stale`
and the lesson starts again from the changed source. `skip` is recorded as
`unmeasured` and cannot complete a lesson. `cancel` saves the current step;
running `jet learn` again resumes it. Ordinary editing is never interrupted
by an automatic quiz. The VS Code actions **Jet: Learn (Watch)** and
**Jet: Learn Once** invoke this same CLI path.
