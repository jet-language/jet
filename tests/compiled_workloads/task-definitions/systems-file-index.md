# Systems: deterministic file index

Build a native file indexer. The sole argument is a directory root. Walk the
real tree recursively with `lstat` semantics: never follow a symlink, including
broken, absolute, escaping, or cyclic links. For every readable regular file,
hash its exact bytes with SHA-256 and emit `file|relative-path|byte-count|hash`.
Normalize relative separators to `/`, sort file rows by relative path, and
hash the sorted rows joined with a final newline for `index_sha256`.

The fixture is a large deterministic tree with nested directories, Unicode
names, a repeated-content file, symlink cycles, and a permission-hostile file.
Unreadable or non-regular entries increment `rejects`; symlinks increment
`links` and are skipped without resolving their targets. The hostile fixture
adds broken, absolute, and escaping links. Safe no-follow behavior is the only
mode; there is no caller-controlled follow-link escape hatch.

The hostile root also carries the harness-only `.fixture-modes.tsv` file.
Each line names a root-relative path and an octal mode separated by a tab.
The runner applies these modes before invoking the adapters, then removes the
control file from the staged tree; adapters must not index it.
