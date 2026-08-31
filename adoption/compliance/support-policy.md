# Support-policy handoff

The release policy defines compatibility and edition behavior. D-ADOPT-LTS1 is
ratified. The first GA date, replacement line, and support matrix remain
pending, so this pack publishes no dated LTS support claim yet.

The release pipeline must render `support-policy.json` from
[`release/calendar.json`](../release/calendar.json), copy the ratified values
without reinterpretation, and bind its `release_version` to the artifact
manifest. The validator rejects a publishable bundle with unresolved schedule
tokens or a support artifact that disagrees with the calendar.
