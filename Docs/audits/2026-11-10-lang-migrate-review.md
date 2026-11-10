# D-LANG-MIGRATE1 review: rewrites for breaking language changes

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-LANG-MIGRATE1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-LANG-MIGRATE1.

## The conflict, stated plainly

- **Owner rule.** Main recorded it on 2026-11-10 in card #6622's log and card #6281's log:
  "non-Jet and retired spellings get no teaching diagnostic and no jet fix rewrite". On #6281:
  "until D-LANG-MIGRATE1 is decided, the owner rule holds".
- **D-TEACH-RECONCILE1=A** (ratified 2026-11-05, owner on ballot q43o25er): "No teaching of foreign words
  or retired Jet spellings anywhere; unknown words get the generic error that lists the legal forms."
- **The board's A broke both.** It rewrote old code with jet fix, which this ballot is allowed to decide.
  It also added a failure hint: "prep was retired in 0.4; jet fix writes comptime". That hint is teaching a
  retired Jet spelling, which D-TEACH-RECONCILE1=A settled against.

What changed:

- **A** is now rewrites with no teaching. The generic error stays. When the project's lock is older than the
  running jet, a failing check adds one toolchain line, for example "this project was locked with a0.0.41;
  jet fix --dry-run shows 3 pending rewrites". The line names no spelling, old or new.
- **E** is the old A, rewrites plus the spelling hint, so the owner can pick it on purpose. It would reverse
  D-TEACH-RECONCILE1=A for Jet's own spellings.
- **C** (teaching errors, no rewrite) is marked as also reversing D-TEACH-RECONCILE1=A.
- **The title** no longer promises an error that names the retired spelling.

Honest residue: A still changes the owner rule's no-rewrite half. That is the question this ballot puts to
the owner, and the ballot says so in its situation and its third loss.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| m01_old | prints 5 |
| m02_let | E0003 |
| m03_while | E0003 |
| m04_cbor | E0766 |
| m05_prep `LIMIT: prep { 2 + 3 }` | E0119, "There's no type called `prep`" |
| m06_comptime | prints 5 (D-META-SPELL1 is not built) |
| m06_mut | prints 5 |
| m07 `#run { 2 + 3 }` | E0733, "There's no tag called `run`", **Fix: "Did you mean `PII`?"** |

Command probes on m05:

- `jet fix --dry-run m05_prep.jet`: `no changes made`, rc 0. The same for `--edition=2026 --dry-run` and for m02.
- `jet fix --edition=2027 m05_prep.jet`: E2104, "`jet fix --edition=2027` is not built: this jet migrates
  to edition 2026 only", rc 2.

`jet new lmnew` writes `.jet/lock` with `[build.stamp] toolchain = "a0.0.43"` and a `[[toolchain]]` section
(`id = "jet-a0.0.43-c38e2e08"`). The writer is `Compiler/JetCli/Source/Cli/Host.jet:2501-2502`, and the reader
is `InspectProvenance.jet:338`. So A has a recorded starting point for projects made by jet new. D-EMBED-TOOLCHAIN1
found that `jet build` on a hand-made package writes no lock; that is the gap A's `--since` covers.

Side finding for a card: E0733 suggests `PII` for `#run`, a tag that shares no letter with it.

## Census (dev c460ba91e5)

- `comptime` appears 2,816 times in 1,013 Jet files (`git grep -c -w`). That is the D-META-SPELL1 migration.
  `#run` appears 2 times in 2 files.
- 131 commits since 2026-09-10 that touch `Compiler/JetParser` or `Compiler/JetSema` mention "retire".
- 30 commits say respell, and 284 mention a codemod.
- All 22 tracked codemod plan JSON files sit under `tests/`.

## Peers (sourced)

- Rust ships edition rewrites in the toolchain, and `cargo fix --edition` applies them.
  <https://doc.rust-lang.org/edition-guide/editions/transitioning-an-existing-project-to-a-new-edition.html>
- Go's `go fix` updates packages to use new APIs. <https://pkg.go.dev/cmd/go#hdr-Update_packages_to_use_new_APIs>
- Dart's `dart fix` applies data-driven fixes that packages ship for their own API changes.
  <https://dart.dev/tools/dart-fix>
- Python 3 made print a function. The interpreter names the new form, and the rewrite tool, 2to3, was separate.
  <https://docs.python.org/3/whatsnew/3.0.html#print-is-a-function>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| A's hint is fine because it fires only on failure | It is still teaching a retired Jet spelling | **Failed**. Moved to E. A uses a version-keyed line |
| The version line is not teaching | It points the user at jet fix | It names no spelling, so it is outside D-TEACH-RECONCILE1=A's text. The owner may still read it as close to the line, and dropping it leaves A intact |
| jet fix reads the lock's toolchain | Locks may lack the record | Holds for `jet new` (probe). Hand-made packages need `--since` until builds write locks |
| The landing rule is cheap | Churn is high | 131 retire commits in two months, so cost scales with churn. Each commit already writes its script |
| Rewrites chain | The next one is not built | D-META-SPELL1 is ratified and unbuilt (m06 prints 5; m07 is E0733). The chain is a design claim, to be tested when it lands |
| B is free | | It strands users' code, and the repository's scripts are deleted (BallotFix: migrate-corpus.sh) |

## Remaining losses

- Each breaking change needs one rewrite file and one fixture.
- The error never names the retired word under A. That is the price of keeping D-TEACH-RECONCILE1=A.
- The owner rule's no-rewrite half changes.
