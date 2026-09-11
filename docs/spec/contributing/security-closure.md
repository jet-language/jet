# Security closure

The executable scan contract is
[`scripts/agent/security-scan.mjs`](../../../scripts/agent/security-scan.mjs).
It owns scope exclusions, schemas, inventory limits, and acceptance checks.
Do not maintain copies of those tables here. Tower owns remediation plans and
closure status; retained scan bundles are dated evidence.

## Prepare and scan

Use a clean commit after the relevant remediation has its independent proof.
Prepare a new request directory on disk:

```sh
scripts/agent/jet-env node scripts/agent/security-scan.mjs \
  prepare --repo "$repo" --out "$work"
```

Read the generated `request.json` and `scope-files.txt`. The request binds the
revision, inventory, exclusions, and required outputs. Do not truncate, widen,
or silently skip part of the scope to make a scan pass.

Run one fresh Standard prompt-only Codex Security scan against that request,
then complete that same scan. Previous reports are evidence to retain, not
input findings to recycle. If the host cannot honor the request, stop the scan
slice and record the missing prerequisite in Tower.

## Validate and retain evidence

With the completed scan directory and installed Codex Security plugin:

```sh
scripts/agent/jet-env full node scripts/agent/security-scan.mjs \
  finalize --repo "$repo" --request "$work/request.json" \
  --scan-dir "$scan_dir" --plugin-dir "$CODEX_SECURITY_PLUGIN_DIR" \
  --publish-dir "$publish"
```

The command validates the official artifacts, checks completeness and identity,
and publishes the sealed evidence only if its gates pass. Never hand-edit a
canonical scan document or finalize a sealed bundle twice. A missing report,
non-zero finding, changed identity, or incomplete coverage is not closure.

Keep the request, inventory, canonical outputs, report, and receipt together
under the approved audit location. Record acceptance evidence on the owning
Tower card; a scan does not automatically close remediation cards.

The procedure's rejecting fixtures are in
[`test-security-scan.sh`](../../../scripts/agent/test-security-scan.sh).
Historical reconciliation reports can be checked with the script's `validate`
operation. A valid reconciliation alone does not prove a fresh external scan.
