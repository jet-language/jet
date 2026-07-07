# jetgrep

Recursive-regex CLI slice. Deterministic mode scans the committed fixture
corpus recursively and prints byte-stable hits for `tests/slices.rs`.

Supported flags:

- `--count` prints per-file match counts.
- `--files` prints matching file paths.
- `--context N` prints `N` surrounding lines.
- `--ignore name` skips a file or directory name while walking.

When run through `jet run`, pass app flags after `--`, for example:
`jet run examples/apps/jetgrep/main.jet -- --count warning examples/apps/jetgrep/fixtures`.

Color, perf budgets, and packaging gates remain out of this slice.
