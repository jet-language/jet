#!/usr/bin/env bash
# Self-check for security-scan.mjs. Run: bash scripts/agent/test-security-scan.sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
script="$script_dir/security-scan.mjs"
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

repo="$root/repo"
mkdir -p \
  "$repo/src" \
  "$repo/.agents" \
  "$repo/.claude/worktrees/wt" \
  "$repo/.claude/bdlog" \
  "$repo/.claude/recovery" \
  "$repo/.claude/probe" \
  "$repo/.agent-worktrees/wt" \
  "$repo/.agent-scratch-1" \
  "$repo/plugins/tower/.tower" \
  "$repo/dogfood/tower/tests/parity/fixtures" \
  "$repo/site/dist" \
  "$repo/docs/reference" \
  "$repo/docs/audits/security-final-prior" \
  "$repo/target/generated" \
  "$repo/build/generated" \
  "$repo/result" \
  "$repo/.tmp/generated" \
  "$repo/node_modules/pkg"
git -C "$repo" init -q
git -C "$repo" config user.name test
git -C "$repo" config user.email test@example.invalid
printf '%s\n' '/target/' '/build/' '/result/' '/.tmp/' '/unlisted-cache/' >"$repo/.gitignore"
printf '%s\n' 'main' >"$repo/src/main.js"
printf '%s\n' 'policy' >"$repo/.agents/policy.md"
printf '%s\n' 'settings' >"$repo/.claude/settings.json"
printf '%s\n' 'session log' >"$repo/.claude/run.log"
printf '%s\n' 'worktree output' >"$repo/.claude/worktrees/wt/marker"
printf '%s\n' 'bookkeeping' >"$repo/.claude/bdlog/log"
printf '%s\n' 'recovery' >"$repo/.claude/recovery/state"
printf '%s\n' 'probe' >"$repo/.claude/probe/output"
printf '%s\n' 'worktree output' >"$repo/.agent-worktrees/wt/marker"
printf '%s\n' 'scratch' >"$repo/.agent-scratch-1/data"
printf '%s\n' 'tower state' >"$repo/plugins/tower/.tower/state.json"
printf '%s\n' 'fixture' >"$repo/dogfood/tower/tests/parity/fixtures/state.json"
printf '%s\n' 'site output' >"$repo/site/dist/index.html"
printf '%s\n' 'ledger' >"$repo/docs/reference/core-surface-ledger.json"
printf '%s\n' 'prior discovery' >"$repo/docs/audits/security-deep-scan-2026-08-03.md"
printf '%s\n' 'prior discovery' >"$repo/docs/audits/security-deep-scan-2026-08-03-full.md"
printf '%s\n' 'prior final' >"$repo/docs/audits/security-final-prior/report.md"
printf '%s\n' 'cargo output' >"$repo/target/generated/file"
printf '%s\n' 'build output' >"$repo/build/generated/file"
printf '%s\n' 'result output' >"$repo/result/package"
printf '%s\n' 'scratch output' >"$repo/.tmp/generated/file"
printf '%s\n' 'dependency' >"$repo/node_modules/pkg/index.js"

test_index="$repo/.git/test-index"
export GIT_INDEX_FILE="$test_index"
git -C "$repo" read-tree --empty
indexed_paths=(
  .gitignore
  src/main.js
  .agents/policy.md
  .claude/settings.json
  .claude/run.log
  .claude/worktrees/wt/marker
  .claude/bdlog/log
  .claude/recovery/state
  .claude/probe/output
  .agent-worktrees/wt/marker
  .agent-scratch-1/data
  plugins/tower/.tower/state.json
  dogfood/tower/tests/parity/fixtures/state.json
  site/dist/index.html
  docs/reference/core-surface-ledger.json
  docs/audits/security-deep-scan-2026-08-03.md
  docs/audits/security-deep-scan-2026-08-03-full.md
  docs/audits/security-final-prior/report.md
  target/generated/file
  build/generated/file
  result/package
  .tmp/generated/file
  node_modules/pkg/index.js
)
for path in "${indexed_paths[@]}"; do
  blob=$(git -C "$repo" hash-object -w "$repo/$path")
  git -C "$repo" update-index --add --cacheinfo "100644,$blob,$path"
done
tree=$(git -C "$repo" write-tree)
commit=$(printf 'tree %s\n\ninitial\n' "$tree" | git -C "$repo" commit-tree "$tree")
git -C "$repo" update-ref refs/heads/main "$commit"
git -C "$repo" symbolic-ref HEAD refs/heads/main
unset GIT_INDEX_FILE
rm -f "$test_index" "$test_index.lock"
git -C "$repo" read-tree "$commit"

mkdir "$repo/unlisted-cache"
printf '%s\n' 'unlisted ignored output' >"$repo/unlisted-cache/file"
set +e
ignored_output=$(node "$script" prepare \
  --repo "$repo" \
  --out "$root/prepared/ignored-request" 2>&1)
ignored_rc=$?
set -e
if [ "$ignored_rc" -eq 0 ] || [[ "$ignored_output" != *"outside the declared exclusions"* ]]; then
  printf 'unlisted ignored path: expected a fail-closed scope error\n%s\n' "$ignored_output" >&2
  exit 1
fi
rm -rf "$repo/unlisted-cache"

prepared_parent="$root/prepared"
mkdir "$prepared_parent"
request_dir="$prepared_parent/request"
node "$script" prepare --repo "$repo" --out "$request_dir" >"$root/prepare.out"
request="$request_dir/request.json"

node - "$request" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const requestPath = process.argv[2];
const request = JSON.parse(fs.readFileSync(requestPath, "utf8"));
const paths = fs.readFileSync(
  path.join(path.dirname(requestPath), "scope-files.txt"),
  "utf8",
).trim().split("\n");
const sorted = [...paths].sort((left, right) =>
  Buffer.from(left).compare(Buffer.from(right)),
);
if (JSON.stringify(paths) !== JSON.stringify(sorted)) {
  throw new Error("scope inventory is not byte-sorted");
}
for (const excluded of [
  ".claude/run.log",
  ".claude/worktrees/wt/marker",
  ".claude/bdlog/log",
  ".claude/recovery/state",
  ".claude/probe/output",
  ".agent-worktrees/wt/marker",
  ".agent-scratch-1/data",
  "plugins/tower/.tower/state.json",
  "dogfood/tower/tests/parity/fixtures/state.json",
  "site/dist/index.html",
  "docs/reference/core-surface-ledger.json",
  "docs/audits/security-deep-scan-2026-08-03.md",
  "docs/audits/security-deep-scan-2026-08-03-full.md",
  "docs/audits/security-final-prior/report.md",
  "target/generated/file",
  "build/generated/file",
  "result/package",
  ".tmp/generated/file",
  "node_modules/pkg/index.js",
]) {
  if (paths.includes(excluded)) {
    throw new Error("excluded path remained in inventory: " + excluded);
  }
}
for (const included of ["src/main.js", ".agents/policy.md", ".claude/settings.json"]) {
  if (!paths.includes(included)) {
    throw new Error("included path was removed: " + included);
  }
}
if (
  request.scan.mode !== "standard" ||
  request.scan.coverageMode !== "repository" ||
  request.scan.target.kind !== "git_revision" ||
  !/^[0-9a-f]{40}$/.test(request.scan.target.revision) ||
  !/^[0-9a-f]{40}$/.test(request.scan.target.tree)
) {
  throw new Error("request identity or mode is incomplete");
}
NODE

make_fixture() {
  local scan_dir="$1"
  mkdir "$scan_dir"
  node - "$request" "$scan_dir" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const request = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const scanDir = process.argv[3];
const scanId = "scan-test-1";
const startedAt = new Date(Date.parse(request.scan.preparedAt) + 1000).toISOString();
const completedAt = new Date(Date.parse(startedAt) + 1000).toISOString();
const scope = {
  includePaths: request.scan.scope.includePaths,
  excludePaths: request.scan.scope.excludePaths,
};
const findings = {
  documentType: "codex-security.findings",
  schemaVersion: "1.0",
  scanId,
  findings: [],
};
const coverage = {
  documentType: "codex-security.coverage",
  schemaVersion: "1.0",
  scanId,
  mode: "repository",
  completeness: "complete",
  inventoryStrategy: "repository",
  includePaths: request.scan.scope.includePaths,
  excludePaths: request.scan.scope.excludePaths,
  surfaces: [
    {
      id: "source",
      label: "source review",
      disposition: "no_issue_found",
      receiptRefs: [],
    },
  ],
  explicitExclusions: request.scan.scope.explicitExclusions,
  deferred: [],
  openQuestions: [],
};
const manifest = {
  documentType: "codex-security.scan-manifest",
  schemaVersion: "1.0",
  scan: {
    id: scanId,
    producer: { name: "test", version: "1" },
    status: "completed",
    startedAt,
    completedAt,
    sealedAt: completedAt,
    target: {
      kind: "git_revision",
      targetId: "test-target",
      displayName: "test",
      revision: request.scan.target.revision,
    },
    scope,
    coverageRef: "coverage.json",
    findingsRef: "findings.json",
    artifacts: [
      { path: "scan-manifest.json" },
      { path: "findings.json" },
      { path: "coverage.json" },
    ],
  },
};
const write = (name, value) =>
  fs.writeFileSync(path.join(scanDir, name), JSON.stringify(value, null, 2) + "\n");
write("scan-manifest.json", manifest);
write("findings.json", findings);
write("coverage.json", coverage);
fs.writeFileSync(
  path.join(scanDir, "report.md"),
  "# Security scan\n\n| Reportable findings | 0 |\n\n### No findings\n",
);
NODE
}

plugin="$root/plugin"
mkdir "$plugin" "$plugin/scripts"
touch "$plugin/scripts/finalize_scan_contract.py" "$plugin/scripts/validate_scan_contract.py"
validator_marker="$root/validator-called"
fake_python="$root/fake-python"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'case "$1" in' \
  '  *validate_scan_contract.py) printf "%s\n" called >"$VALIDATOR_MARKER" ;;' \
  '  *finalize_scan_contract.py) exit 91 ;;' \
  '  *) exit 92 ;;' \
  'esac' >"$fake_python"
chmod +x "$fake_python"
export VALIDATOR_MARKER="$validator_marker"
export JET_SECURITY_PYTHON="$fake_python"

scan="$root/scan"
make_fixture "$scan"

expect_failure() {
  local label="$1"
  local needle="$2"
  local scan_dir="$3"
  shift 3
  set +e
  output=$(node "$script" finalize \
    --repo "$repo" \
    --request "$request" \
    --scan-dir "$scan_dir" \
    --plugin-dir "$plugin" "$@" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    printf '%s: expected failure\n' "$label" >&2
    exit 1
  fi
  case "$output" in
    *"$needle"*) ;;
    *)
      printf '%s: missing error %s\n%s\n' "$label" "$needle" "$output" >&2
      exit 1
      ;;
  esac
}

bad_findings="$root/bad-findings"
cp -R "$scan" "$bad_findings"
node - "$bad_findings/findings.json" <<'NODE'
const fs = require("node:fs");
const file = process.argv[2];
const value = JSON.parse(fs.readFileSync(file, "utf8"));
value.findings.push({ findingId: "candidate" });
fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n");
NODE
expect_failure "non-empty findings" "zero-unresolved gate failed" "$bad_findings"

bad_deferred="$root/bad-deferred"
cp -R "$scan" "$bad_deferred"
node - "$bad_deferred/coverage.json" <<'NODE'
const fs = require("node:fs");
const file = process.argv[2];
const value = JSON.parse(fs.readFileSync(file, "utf8"));
value.deferred = [{ id: "deferred", reason: "test" }];
fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n");
NODE
expect_failure "deferred coverage" "coverage contains deferred work" "$bad_deferred"

bad_report="$root/bad-report"
cp -R "$scan" "$bad_report"
printf '%s\n' '# incomplete report' >"$bad_report/report.md"
expect_failure "missing generated report marker" "not the generated zero-finding report" "$bad_report"

publish="$repo/docs/audits/security-final-test"
node "$script" finalize \
  --repo "$repo" \
  --request "$request" \
  --scan-dir "$scan" \
  --plugin-dir "$plugin" \
  --publish-dir "$publish" >"$root/publish.out"
[ -s "$validator_marker" ]
[ -f "$publish/scan-manifest.json" ]
[ -f "$publish/findings.json" ]
[ -f "$publish/coverage.json" ]
[ -f "$publish/report.md" ]
[ -f "$publish/scan-request.json" ]
[ -f "$publish/scope-files.txt" ]
[ -f "$publish/security-scan.json" ]
node - "$publish/security-scan.json" <<'NODE'
const fs = require("node:fs");
const result = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (
  result.documentType !== "jet.security-scan.result" ||
  result.status !== "passed" ||
  result.card !== "#1387" ||
  result.gate.reportableFindings !== 0
) {
  throw new Error("published result did not pass the zero-unresolved gate");
}
NODE
rm -rf "$publish"

printf '%s\n' 'changed' >"$repo/src/main.js"
new_index="$repo/.git/test-index-2"
export GIT_INDEX_FILE="$new_index"
git -C "$repo" read-tree HEAD
new_blob=$(git -C "$repo" hash-object -w "$repo/src/main.js")
git -C "$repo" update-index --add --cacheinfo "100644,$new_blob,src/main.js"
new_tree=$(git -C "$repo" write-tree)
old_revision=$(git -C "$repo" rev-parse HEAD)
new_commit=$(printf 'tree %s\n\nchanged\n' "$new_tree" | git -C "$repo" commit-tree "$new_tree" -p "$old_revision")
unset GIT_INDEX_FILE
rm -f "$new_index" "$new_index.lock"
git -C "$repo" update-ref refs/heads/main "$new_commit"
git -C "$repo" read-tree "$new_commit"
expect_failure "changed integration revision" "repository revision or tree changed" "$scan"

printf 'security-scan self-check: all pass\n'
