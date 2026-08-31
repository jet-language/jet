# Security closure procedure

This is the procedure for Tower card #1387. It produces one fresh, bounded,
full-repository Codex Security result. It does not close remediation cards and
it does not change Tower state.

## Scan contract

The scan is one Standard prompt-only repository scan. Do not run a deep scan,
parallel scans, repeated scans, or a scan of an earlier report.

The target is the clean Git commit recorded by the preparation command.

- Include path: .
- Start from every tracked blob at the commit, then apply the exact exclusions
  below.
- Sort the inventory by UTF-8 path bytes.
- Record the full 40-character commit ID and its tree ID.
- Record the inventory file count, byte count, and SHA-256 digest.
- Refuse a dirty worktree, an empty scope, a changed revision, or a changed
  inventory.
- Refuse any ignored worktree path that is not covered by the exclusion table.
- Refuse more than 20,000 included files or 1 GiB of included blob content.
  Never truncate the scope.

The exact exclusion patterns and reasons are part of the request and must be
copied unchanged into the Codex Security manifest and coverage document.

| Pattern | Reason |
| --- | --- |
| .git/** | Git control metadata is not committed source. |
| target/** | Cargo build output is generated. |
| target-*/** | Alternate Cargo build output is generated. |
| build/** | Build output is generated. |
| result/** | Packaged build output is generated. |
| .tmp/** | Agent scratch output is generated outside the source inventory. |
| .tmp-*/** | Agent scratch output is generated outside the source inventory. |
| node_modules/** | Installed third-party dependencies are not repository source. |
| .claude/worktrees/** | Nested agent worktrees are separate working trees. |
| .agent-worktrees/** | Nested agent worktrees are separate working trees. |
| .agent-scratch-*/** | Agent scratch output is generated. |
| .claude/*.log | Agent session logs are generated records, not repository source. |
| .claude/*.patch | Agent patch artifacts are generated records, not repository source. |
| .claude/bdlog/** | Agent bookkeeping logs are generated records. |
| .claude/recovery/** | Agent recovery records are generated records. |
| .claude/probe/** | Agent probe output is generated. |
| plugins/tower/.tower/** | Tower live state is operational data, not repository source. |
| dogfood/tower/tests/parity/fixtures/** | Generated Tower parity fixtures are test output. |
| site/dist/** | Site distribution output is generated. |
| docs/reference/core-surface-ledger.json | The generated core-surface ledger is derived data. |
| docs/audits/security-deep-scan-2026-08-03.md | The canceled discovery report is prior scan evidence, not scan input. |
| docs/audits/security-deep-scan-2026-08-03-full.md | The canceled discovery report is prior scan evidence, not scan input. |
| docs/audits/security-final-*/** | Prior final reports are outputs and must not become scan input. |

This excludes generated state and prior evidence. It keeps source, tests,
examples, compiler code, scripts, agent policy, and non-log configuration in
scope. The live candidate tables in the canceled discovery report are not
edited by this procedure.

## Preparation

Run this only after the remediation cards have their independent evidence. Use
the repository checkout and a new ignored scratch directory on repository
storage. Do not use /tmp for the request or scan bundle.

~~~
repo=$(git rev-parse --show-toplevel)
revision=$(git rev-parse HEAD)
work="$repo/.tmp/security-closure-$revision"
mkdir -p "$(dirname "$work")"
scripts/agent/jet-env node scripts/agent/security-scan.mjs \
  prepare --repo "$repo" --out "$work"
request="$work/request.json"
~~~

The command creates request.json and scope-files.txt. The directory must be
new. Keep both files with the final evidence.

## Output schema

Preparation writes this fixed request shape:

The two exclusion arrays below are abbreviated for readability in this
example. The script writes all 23 patterns and all 23 pattern-and-reason
objects, in table order, with no other entries.

~~~
{
  "documentType": "jet.security-scan.request",
  "schemaVersion": "1.0",
  "card": "#1387",
  "scan": {
    "mode": "standard",
    "coverageMode": "repository",
    "fresh": true,
    "preparedAt": "<ISO-8601 time>",
    "target": {
      "kind": "git_revision",
      "revision": "<40-hex commit>",
      "tree": "<40-hex tree>",
      "displayName": "<repository name>"
    },
    "scope": {
      "includePaths": ["."],
      "excludePaths": ["<the 23 exact patterns above, in that order>"],
      "explicitExclusions": [
        {
          "pattern": "<one exact pattern above>",
          "reason": "<that pattern's exact reason above>"
        }
      ],
      "inventory": {
        "path": "scope-files.txt",
        "fileCount": "<integer>",
        "byteCount": "<integer>",
        "sha256": "<64-hex digest>"
      },
      "limits": {
        "maxFiles": 20000,
        "maxBytes": 1073741824
      }
    },
    "outputs": {
      "canonical": [
        "scan-manifest.json",
        "findings.json",
        "coverage.json"
      ],
      "report": "report.md",
      "receipt": "security-scan.json"
    }
  }
}
~~~

The scan directory uses the Codex Security schemas for
scan-manifest.json, findings.json, and coverage.json. The generated report
is report.md. The publish directory also contains scan-request.json, the
byte-sorted scope-files.txt, and this fixed receipt shape:

~~~
{
  "documentType": "jet.security-scan.result",
  "schemaVersion": "1.0",
  "card": "#1387",
  "status": "passed",
  "scan": {
    "id": "<scan id>",
    "mode": "standard",
    "coverageMode": "repository",
    "target": {
      "kind": "git_revision",
      "revision": "<40-hex commit>",
      "tree": "<40-hex tree>"
    },
    "scope": {
      "includePaths": ["."],
      "excludePaths": ["<the 23 exact patterns above, in that order>"],
      "inventory": {
        "fileCount": 0,
        "byteCount": 0,
        "sha256": "<64-hex digest>"
      }
    }
  },
  "gate": {
    "reportableFindings": 0,
    "deferred": 0,
    "needsFollowUp": 0,
    "openQuestions": 0,
    "report": "report.md"
  },
  "outputs": {
    "canonical": [
      "scan-manifest.json",
      "findings.json",
      "coverage.json"
    ],
    "report": "report.md",
    "receipt": "security-scan.json",
    "request": "scan-request.json",
    "inventory": "scope-files.txt"
  },
  "sha256": {
    "scan-manifest.json": "<64-hex digest>",
    "findings.json": "<64-hex digest>",
    "coverage.json": "<64-hex digest>",
    "report.md": "<64-hex digest>",
    "scan-request.json": "<64-hex digest>",
    "scope-files.txt": "<64-hex digest>",
    "artifacts/<each copied artifact>": "<64-hex digest>"
  }
}
~~~

## Codex Security scan

Invoke the Codex Security prompt-only scan exactly once with:

~~~
{
  "mode": "standard",
  "targetPath": "<repo>",
  "scope": ".",
  "userContext": "Tower #1387 closure. Read <request>/request.json and <request>/scope-files.txt. Review every included tracked path at revision <revision>, apply the exact exclusion and coverage contract in the request, and produce complete canonical scan artifacts. Do not use prior reports as findings."
}
~~~

Replace the angle-bracket values with the repository, request directory, and
full revision recorded by preparation. Use the returned scan ID and scan
directory. Do not substitute a path from a previous run. Complete that same
scan exactly once with the Codex Security completion operation. The completion
output must contain these canonical files:

If the host cannot honor the request inventory and exclusion contract, stop
before completion and report the mismatch. Do not widen the scope to make the
scan run.

- scan-manifest.json
- findings.json
- coverage.json
- report.md

The scan manifest must identify the target as git_revision with the exact
commit ID in request.json. Its scope must contain the exact include and
exclude arrays above. Coverage must use repository mode, repository inventory,
and complete completeness. Do not hand-edit any canonical document.

## Finalization and gate

Set CODEX_SECURITY_PLUGIN_DIR to the installed Codex Security plugin
directory. Run this exact Main proof command with the returned scan directory:

~~~
publish="$repo/docs/audits/security-final-$(date -u +%F)"
scripts/agent/jet-env full node scripts/agent/security-scan.mjs \
  finalize \
  --repo "$repo" \
  --request "$request" \
  --scan-dir "$scan_dir" \
  --plugin-dir "$CODEX_SECURITY_PLUGIN_DIR" \
  --publish-dir "$publish"
~~~

The command calls the official
finalize_scan_contract.py only when the host completion returned an
unsealed draft. It always calls the read-only
validate_scan_contract.py. It then applies the independent procedure gate
and writes the report and evidence to the new publish directory. A sealed
bundle is never finalized a second time.

The command fails unless all of these conditions hold:

- the manifest status is completed;
- the target kind, commit ID, and scope match the request;
- scan start time is at or after request preparation;
- the canonical documents reference one scan ID;
- coverage is complete repository coverage;
- coverage has no deferred item, open question, or needs_follow_up surface;
- explicit exclusions match the exact exclusion table;
- findings.json contains zero reportable findings;
- generated report.md contains the zero-finding result;
- canonical files and the report are regular, non-symlink files;
- the published evidence passes the official validator again.

The published security-scan.json is the procedure receipt. It records card
#1387, the scan ID, commit and tree identity, exact scope inventory, zero gate
counts, output names, and SHA-256 digests for the other committed evidence
files.

The removal-sensitive procedure self-test is:

~~~
bash scripts/agent/test-security-scan.sh
~~~

Commit the new procedure receipt and canonical evidence only after the command
passes. Do not mark remediation cards done as part of this procedure. A
non-zero finding, incomplete coverage, changed identity, or missing report is
a failed closure and requires returning to the affected remediation work.
