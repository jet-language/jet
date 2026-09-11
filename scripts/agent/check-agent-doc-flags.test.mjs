import assert from "node:assert/strict";
import test from "node:test";
import { documentationPolicyErrors } from "./check-agent-doc-flags.mjs";

test("operational headings and task state are rejected with source locations", () => {
  const errors = documentationPolicyErrors("docs/spec/example.md", [
    "# Example",
    "## 15. Implementation plan",
    "- [ ] Ship the change",
    "## Shipped status",
  ].join("\n"));
  assert.deepEqual(errors.map((error) => error.split(": ")[0]), [
    "docs/spec/example.md:2", "docs/spec/example.md:3", "docs/spec/example.md:4",
  ]);
});

test("domain plans and fenced sample documents are not work ledgers", () => {
  assert.deepEqual(documentationPolicyErrors("docs/spec/example.md", [
    "## Query plans", "A query plan describes execution order.",
    "````markdown", "## Status", "```", "- [x] sample", "````",
    "~~~markdown", "## Milestones", "~~~",
    "## Contracts", "Each request has a status value.",
  ].join("\n")), []);
});

test("an unrelated fence cannot hide subsequent operational content", () => {
  const errors = documentationPolicyErrors("docs/spec/example.md", [
    "```markdown", "~~~", "## Status", "```", "## Remaining work",
  ].join("\n"));
  assert.equal(errors.length, 1);
  assert.match(errors[0], /^docs\/spec\/example\.md:5:/);
});
