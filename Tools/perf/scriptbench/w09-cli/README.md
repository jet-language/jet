# w09: cli

A typed `#CLI` entry with two commands, timed as `greet --name World --times 3 --shout`.

Origin: the program of the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from), rewritten as a typed #CLI entry. Cards: #6061.

The audit program decoded `process.argv()` by hand. #6061 is about the typed-entry path, so the Jet program is the `#CLI` struct form, and the peers keep their own argument parsers.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
