# User-facing case law

This is the implementation rule for human-readable UI text. It records
D-CASE-CHROME1=C and D-CASE-PROSE1=A, ratified 2026-08-25 on card #2203.
Case follows the shape of the text, not the file or the surface that prints it.

## The two cases

- **Title Case** is for short chrome that labels a thing or an action. Capitalize
  the first letter of each ordinary major word. Keep these minor words
  lowercase unless they are first or last: `a`, `an`, `the`, `and`, `but`,
  `or`, `nor`, `for`, `as`, `at`, `by`, `in`, `of`, `on`, `to`, and `via`.
  Capitalize each part of an ordinary hyphenated word by the same rule.
- **sentence case** is for prose that describes an action, state, reason, or
  next step. Capitalize the first ordinary word, plus proper nouns and exact
  code fragments. Do not change the case of a leading code fragment to make
  the sentence start with an uppercase letter.

## Surface matrix

| Surface class | Case | Rule |
| --- | --- | --- |
| Command and flag summaries in help | sentence case | A summary describes what a command or flag does. Example: `Enter the default project environment`. |
| Section headers in help | Title Case | A header labels the section. Example: `Environments:`. |
| Diagnostic What line | sentence case | The problem line is a sentence, even when it is the first line of the report. |
| Diagnostic Why and Fix lines | sentence case | Both lines are prose sentences. |
| Status and progress lines | Title Case for label-shaped rows; sentence case for explanatory sentences | Short status labels and runtime fragments use Title Case, including `Resolving`, `Realizing`, and `Installed`. A full explanation stays sentence case. |
| Table headers and column labels | Title Case | They name columns, not facts about a row. |
| TUI, Canvas, and Studio buttons, menus, tabs, and panel titles | Title Case | Each is a short interactive label. |
| Prompts and confirmations | sentence case | A question or instruction is prose. A separate button or menu item that answers it is Title Case. |
| Machine output | unchanged | JSON field names and values, and every other machine-parsed string, keep their exact bytes. |

Help usage lines use Title Case for the `Usage:` label. Command names, flags,
paths, and refs inside the line keep their own case. Surrounding usage prose
and did-you-mean text use sentence case.

Diagnostic frame labels are fixed protocol labels: `Error`, `Why:`, and `Fix:`
keep that spelling. The `More: jet-lang.dev/e/<CODE>` line required by
D-DIAG-URL1 is also a fixed label plus an exact URL and code. The three
diagnostic payload lines are the What, Why, and Fix lines listed above.

When a human-readable line begins with an identifier, flag, path, package ref,
keyword, type name, or quoted code fragment, that fragment keeps its own case
wherever it appears. Never force-capitalize it. For example:

```text
`--offline` forbids network access       # correct
`--offline` Forbids Network Access       # wrong
```

## Exclusions

This law does not restyle code identifiers, PascalCase type names, file paths,
package refs, environment variable names, JSON keys, JSON values, or any
string a machine parses. These values are data contracts. Human-readable text
inside a machine output value is still unchanged.

The application cards in milestone `e10-m08-ui-case-law` cite
D-CASE-CHROME1=C and D-CASE-PROSE1=A. They must apply this matrix and must not
create a second case rule.
