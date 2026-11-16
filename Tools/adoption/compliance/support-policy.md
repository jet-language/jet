# Support-policy handoff

The canonical compatibility and edition rules are in the
[`release policy`](../../../Docs/spec/release-policy.md) and
[`release/calendar.json`](../release/calendar.json). The calendar records the
ratified `D-ADOPT-LTS1` policy values—annual cadence, 12 active months, 24
maintenance months, 36 total months, and no more than three overlapping
lines—while its first-LTS dates, replacement line, and supported editions and
hosts remain explicit owner-token inputs.

The release pipeline renders `support-policy.json` from that calendar, copies
ratified values without reinterpretation, and binds `release_version` to the
artifact manifest. A preview artifact uses
`status: "preview-no-lts-claim"` and contains no LTS values. A published
artifact uses `status: "published"` only after the calendar schedule and
support matrix have been resolved and ratified.

The validator rejects a publishable bundle with unresolved schedule tokens, a
support artifact that disagrees with the calendar, or a published artifact that
lacks the complete cadence, dates, edition, host, notice, and backport fields.
It also rejects an owner token in a published support artifact.
