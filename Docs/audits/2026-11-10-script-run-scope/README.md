# Script/run scope census (#6281 c180), t22 snapshot

The rule: a program is either script style or `run` style, never a mix of
the two. It holds within one load graph, one package and one project.

The rule counts entries, not names. Two things are entries:

- a `run: fn` that serves as a program entry: the load's entry file, or any
  file a package or project declares as an entry or binary;
- the top-level statements of any file.

All the entries in one scope have the same style. A script-style scope has
exactly one script file. An imported file with statements stays E0620, and an
entry file with both statements and `run: fn` stays E0621.

A library function that happens to be named `run` is not an entry. Examples
are `game.run(scene, …)` and `tasks.run()`. Neither is a module alias named
`run`, such as `run: __core_db_vdberun`.

## Method

`census/Census.jet` parses every tracked Jet source with the compiler's own
lexer and parser (`jet_lexer`, `jet_parser`). It writes `census.tsv`, which
has one row for each file that has top-level statements or binds the local
name `run` through an import.

The sources are:

- every tracked `.jet` file except `package.jet` and `workspace.jet`;
- the `tests/**/*.template` and `tests/**/*.source` files that a case copies
  into a `.jet` file.

A case's `write: x.jet = "{cap:…}"` captures its text from one of those
fixture files. The other inline `write:` sources in `tests/cases` hold only
`run: fn` programs or manifests.

The nearest `package.jet` or `workspace.jet` ancestor gives each row its
scope. Each row then gets one verdict:

| verdict | meaning |
|---|---|
| `library-run-alias` | no statements; an import binds `run` in a library module, so it is not an entry |
| `parse-error` | the parser refuses the file first, so the statement count is parser recovery and no entry check runs |
| `same-file` | statements beside `run: fn` in one file: E0621 already refuses it |
| `renamed-run` | statements in a file whose import binds `run`: the new scope check refuses it |
| `imported-script` | a sibling file imports this script: E0620 already refuses it |
| `package-scope` | a script inside a package or project: checked against that scope's entries |
| `script-standalone` | the only script in its scope, with no `run` entry: legal |

Reproduce from the repo root with the t22 compiler (the `jet_foundation`
dependency embeds `toolchain:` inputs, so `JET_TOOLCHAIN_ROOT` names the tree):

```
git ls-files > /tmp/tracked.txt
JET_TOOLCHAIN_ROOT=$PWD jet run Docs/audits/2026-11-10-script-run-scope/census/Census.jet -- . /tmp/tracked.txt census.tsv
```

## Result (t22 snapshot 4a9268e62c, seed-t22)

Scanned 17152 sources, which gave 145 rows:

| verdict | rows |
|---|---|
| parse-error | 123 |
| script-standalone | 9 |
| library-run-alias | 7 |
| same-file (E0621, unchanged) | 4 |
| imported-script (E0620, unchanged) | 2 |
| renamed-run | 0 |
| package-scope | 0 |

No tracked source is a hit for the new check:

- No script-style file sits inside a package or project, so no package or
  project mixes a script entry with a `run` entry or holds two script files.
- No script binds `run` through an import or a re-export.
- No script uses a marker or template that generates a `run` entry.
- No tracked file declares a top-level `run` library function next to
  top-level statements.

The four `same-file` and two `imported-script` rows are the tests that
already prove E0621 and E0620.

The 123 `parse-error` rows are files the parser refuses before any entry
check: research probes, audit probes, fuzz inputs, proposal samples, the three
stale `Compiler/JetDevserver` sources, and shell or JSON case templates. Their
verdict does not change.

So the migration batch (c180 SUB c) is empty. The check SUB adds the UI cases
that prove each refusal path: same file, imported module, another module of
the same package, renamed import, re-export and generated code. It also adds
a script that imports `core.game` and stays legal.

## Rule reach

The rule gets stricter. It now also refuses:

- a script-style entry file in a package or project that declares a `run`
  entry or a second script file;
- a script whose own import or re-export binds `run`;
- a script whose markers or templates generate a `run` entry.

On the t22 tree it refuses none of the 17152 tracked sources.
