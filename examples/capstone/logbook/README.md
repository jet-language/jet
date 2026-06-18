# logbook

Markdown knowledge-base manager: index, lint, search, link-graph, HTTP server.

## Run

```
jet run logbook.jet version
jet run logbook.jet index   fixtures/notes
jet run logbook.jet lint    fixtures/notes
jet run logbook.jet find    fixtures/notes "design owner"
jet run logbook.jet links   fixtures/notes jet-philosophy
jet run logbook.jet graph   fixtures/notes
jet run logbook.jet graph   fixtures/notes json
jet run logbook.jet serve   fixtures/notes
jet run logbook.jet new     fixtures/notes my-note
```

## Architecture

| Module       | Purpose                                                     |
|--------------|-------------------------------------------------------------|
| logbook.jet  | Entry point; CLI dispatch                                   |
| note.jet     | Frontmatter + body parser; `Note` and `NoteType` types      |
| index.jet    | Parallel index builder; backlinks map; dead-link detection  |
| search.jet   | Query parser; tag/type/text ranking                         |
| render.jet   | Note and listing to HTML                                    |
| server.jet   | HTTP routing: list, note, graph JSON, search, health        |
| config.jet   | Load `config.toml` with env-var overrides                   |
| hashid.jet   | FNV-1a content hash — `@unsafe` expert-tier demo            |
| ffi.jet      | Minimal FFI demo (stub PID via `@unsafe`)                   |

## Features

Exercises structs, enums, `when`-matching, optionals (`?`), maps, loops, `comptime`, `@unsafe`/`@audit`, FFI, TOML config, parallel index build, and HTTP serving — all in a single real-world CLI + server program.

## Boundaries

- No regex — `note.jet`: all string work uses `split`/`contains`/`starts_with`.
- No full YAML parse — `note.jet`: `metadata.type` is hand-parsed line-by-line.
- No tuple `when` — `server.jet`: nested `if`/`when` used for multi-key routing.
- Tasks are not `async`/`await` — `index.jet`: parallel via `jet.task`, no async syntax yet.
- Index built once, shared read-only — no `Mutex` needed.
- HTTP only, no TLS — `server.jet`.
- Thread-per-connection blocking model — `server.jet`.
- Markdown-lite only — `render.jet`: headings, bold, `[[wikilinks]]`, `#tags`; no tables or footnotes.

## Note format

```
---
name: my-note
description: one-line summary
metadata:
  type: project
---
Body text. Link to [[other-note]]. Tag with #tag.
```

`type` must be one of: `user`, `feedback`, `project`, `reference`.
