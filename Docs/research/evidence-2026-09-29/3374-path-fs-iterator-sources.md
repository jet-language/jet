# #3374 — Path and filesystem iterator source contracts (ITER-F063)

Question: do `core.files.path` components, anchors and directory listing keep
path semantics (roots, empty paths, separators, prefixes), keep ReadDir
per-item errors apart from constructor errors, and offer a borrowed
remaining-path projection?

Binary: `~/.cache/jet-luna/safe-jet.sh` → `jet-debug-snapshot14`, 2026-09-29.
Scratch: `~/.cache/jet-test-scratch/Closer04/` (`pc.jet`, `rd.jet`).

## c1 — roots, empty paths, separators, prefixes

`pc.jet` prints `fp.parts`, `fp.parents`, `fp.drive`, `fp.root`, `fp.anchor`
and `fp.is_empty` for each case using `use core.files.path as fp`. Output
was byte-identical on `jet run`, `jet run --interpret` and the AOT binary
(`jet build pc.jet` → `.jet/build/pc`):

```
/a/b parts=[, a, b] parents=[/a, /] drive= root=/ anchor=/ empty=false
a/b parts=[a, b] parents=[a] drive= root= anchor= empty=false
 parts=[] parents=[] drive= root= anchor= empty=true
. parts=[.] parents=[] drive= root= anchor= empty=true
//srv/share/x parts=[, , srv, share, x] parents=[//srv/share, //srv, /] drive= root=/ anchor=/ empty=false
C:\x\y parts=[C:, x, y] parents=[C:\x, C:/, C:] drive=C: root=C:/ anchor=C:/ empty=false
C:x parts=[C:x] parents=[C:] drive=C: root=C:/ anchor=C:/ empty=false
a//b/ parts=[a, , b, ] parents=[a/] drive= root= anchor= empty=false
```

Peer (executed, Python 3.13.13 `PureWindowsPath`; the module treats `\` as a
separator and `X:` as a drive, so the Windows flavour is the matching law):

| input | parts | parents | drive | root | anchor |
| --- | --- | --- | --- | --- | --- |
| `/a/b` | `\ a b` | `\a`, `\` | `` | `\` | `\` |
| `.` | `[]` | `[]` | | | |
| `//srv/share/x` | `\\srv\share\ x` | `\\srv\share\` | `\\srv\share` | `\` | `\\srv\share\` |
| `C:\x\y` | `C:\ x y` | `C:\x`, `C:\` | `C:` | `\` | `C:\` |
| `C:x` | `C: x` | `C:` | `C:` | `` | `C:` |
| `a//b/` | `a b` | `a`, `.` | | | |

Verdict c1: FAIL. `parts` is `files.split_slash` (Core/files/path.jet:150): the
root becomes `""`, `.` is a component, repeated and trailing separators give
empty components. UNC `//srv/share` is not a drive, and `parents` invents a
final `/`. `C:x` (drive-relative) reports root `C:/` and is absolute
(`files.is_abs_impl`, Core/files/files.jet:230-235). `C:\x\y` parents walk
past the root to `C:` and mix separators (`C:/`).

## c2 — ReadDir per-item errors and order

`rd.jet` makes a temp dir with `a.txt`, `b.txt` and a `locked/` subdir set to
mode 0, then calls `fp.iterdir`, `fp.iterdir(locked)`, `fp.walk` and
`fp.iterdir(missing)`.

`jet run` and AOT (identical apart from the temp root):

```
iterdir raw: [a.txt, b.txt, locked]
iterdir sorted: [a.txt, b.txt, locked]
locked iterdir error: permission denied during read `<root>/locked`: Permission denied (os error 13)
walk error: permission denied during read `<root>`: Permission denied (os error 13)
constructor error: not found during read `<root>/nope`: No such file or directory (os error 2)
```

`jet run --interpret`: E0956 ``core.files.set_mode()`` isn't supported by the
current evaluator yet (rd.jet:17).

Findings:
- `iterdir`/`walk` return eager `[Path] IOError!` (path.jet:399-412). A
  per-item failure has no representation: one unreadable child makes the
  whole `walk` fail, the same way the constructor fails.
- The host `jet_std_fs_list_dir` sorts by name
  (crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs:1910), so listing order
  is sorted in practice, although no doc promises it.
- The walk error names the walk root, not the unreadable child: FSWalk.rs:207-210
  maps `read_dir(&dir)` errors through `make_error(&shown, …)`, and `shown`
  is the root.

Verdict c2: FAIL. A per-item-error ReadDir producer is new public API, so it
needs its own ballot on the generic iterator protocol.

## c3 — borrowed remaining-path projection

No `Components::as_path`-style projection exists in `core.files.path` or on
the prelude `Path` (the export list is at
Compiler/JetFoundation/Source/Registry/CoreCallRows.jet:1942). Python has
none either. Verdict: unsupported; any projection needs its own ballot.

## c4 — golden `io/path_components`

Not added. The observed output above is wrong for the card's intent, so it
must not be blessed. Existing goldens on the current binary (via
`golden_diff.sh`, a diff of stdout against `Examples/features/expected/`):

- `io/path_within`: MATCH on run, interpret and AOT. The 2026-09-15
  SetConsoleCtrlHandler unsafe gate no longer blocks AOT (pc.jet and rd.jet
  also build).
- `io/path`: `jet run` stdout matches but exits 101 with `internal compiler
  error: JIT drop 'Path<>?' value is not a result`. `--interpret` fails with
  E0956 `MIR PreludeCallId … received 1 arguments; expected 0..=0` at
  path.jet:3 (`Path.home()`). The AOT build fails with `internal compiler
  error: MIR HandleMethod route MirPreludeCallId(18360485786913888618) has
  inconsistent canonical arity metadata` (MIRRust.rs:21534).
- `io/walk_files`: fails on every tier: E0302 `WalkEntry` has no field
  `relative` (walk_files.jet:14).

## Follow-ups

1. Defect (Core, lexical): fix `parts`/`drive`/`root`/`anchor`/`parents`/
   `is_absolute` to follow the component law in the table above.
2. Defect: the walk error should name the failing directory, not the root.
3. Defect (I9): the interpreter does not support `core.files.set_mode`.
4. Owner-gated: a lazy ReadDir producer with per-item `IOError` items.
5. Owner-gated: a borrowed remaining-path projection.
