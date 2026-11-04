# #5910 held patches (SP05b and SP05c)

These patches are the SP05 phase-arena work for #5910 c17. They were found on
2026-11-04 in the rescued scratch copy. The proof logs are from the 2026-10-04
proof queue. The `$HOME` in `SP05b-proof.sh` stands for the user's home
directory. The log text is not changed.

`sp05c.patch` contains everything in `sp05b.patch`, plus SP05c items 1-3.
Both cover the same 16 files. Land `sp05c.patch` alone; `sp05b.patch` is
only a record.

| Item | File | Base | Applies to a96f0867f1 | State | Next step |
|---|---|---|---|---|---|
| SP05c | `sp05c.patch` | 512356f682 (2026-10-04) | No: `git apply --3way` conflicts in all 15 Jet/Markdown files, and `crates/jet-driver/src/Trace.rs` no longer exists | Items 1-3 on top of SP05b. `SP05c-item1-iso.log` is a whole-compiler isocheck of the items-1-3 snapshot: `rc=0 errors=0`, `SP05c-item1-iso.rc` 0 | Rebase onto dev and drop the `crates/` hunk. Then run chain full and lane-check on JetSema and JetDriver, measure register + sema.check on L5, and land (#5910 c17) |
| SP05b | `sp05b.patch` | 512356f682 | No (same files as SP05c) | Fully contained in `sp05c.patch`. `SP05b-proof.log`: isocheck `rc=0`. The cargo Trace tests could not run (`cargo: not found`, rc 127), and the L5 profile was killed (rc 137). `SP05b-proof.rc` 1 | None: it lands as part of SP05c |
| SP05b delta | `sp05b-delta-vs-595d1ef0d.patch` | 595d1ef0d4 (savepoint sp05-lookups) | No | SP05b as a change from the sp05-lookups savepoint: 14 of the 16 files | None: it is a review aid for the SP05b half of SP05c |

`sp05c.patch` and `sp05b.patch` apply with no errors to 512356f682.
`sp05b-delta-vs-595d1ef0d.patch` applies with no errors to 595d1ef0d4.
