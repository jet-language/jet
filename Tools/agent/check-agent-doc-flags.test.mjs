import assert from "node:assert/strict";
import test from "node:test";
import { documentationPolicyErrors, nodeSourceFlags } from "./check-agent-doc-flags.mjs";

test("operational headings and task state are rejected with source locations", () => {
  const errors = documentationPolicyErrors("Docs/spec/example.md", [
    "# Example",
    "## 15. Implementation plan",
    "- [ ] Ship the change",
    "## Shipped status",
  ].join("\n"));
  assert.deepEqual(errors.map((error) => error.split(": ")[0]), [
    "Docs/spec/example.md:2", "Docs/spec/example.md:3", "Docs/spec/example.md:4",
  ]);
});

test("domain plans and fenced sample documents are not work ledgers", () => {
  assert.deepEqual(documentationPolicyErrors("Docs/spec/example.md", [
    "## Query plans", "A query plan describes execution order.",
    "````markdown", "## Status", "```", "- [x] sample", "````",
    "~~~markdown", "## Milestones", "~~~",
    "## Contracts", "Each request has a status value.",
  ].join("\n")), []);
});

test("an unrelated fence cannot hide subsequent operational content", () => {
  const errors = documentationPolicyErrors("Docs/spec/example.md", [
    "```markdown", "~~~", "## Status", "```", "## Remaining work",
  ].join("\n"));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^Docs\/spec\/example\.md:5:/);
});

test("argv dispatch maps and flag comparisons are accepted, unrelated strings are not", () => {
  const source = `
    const names = { '--subject': 'subject', '--root': 'root' };
    const help = { '--retired': 'old' };
    for (const arg of argv) {
      if (!(arg in names) && !['--format'].includes(arg)) throw Error('unknown');
    }
    const flag = process.argv[i];
    if (flag === '--dry-run') continue;
    const key = { '--jet': 'jet', '--runtime-dir': 'runtime', '--out': 'out' }[flag];
    const command = ['--spawn-only'];
    // if (arg === '--comment-only') {}
  `;
  assert.deepEqual([...nodeSourceFlags(source)].sort(),
    ['--dry-run', '--format', '--jet', '--out', '--root', '--runtime-dir', '--subject']);
});
