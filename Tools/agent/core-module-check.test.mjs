import test from "node:test";
import assert from "node:assert/strict";
import { checkSummary, coreModules, loweredCeilings, probeSource, ratchetProblems } from "./core-module-check.mjs";

test("core modules come from the module export rows, once each", () => {
  const text = [
    "module core exports { print } types { }",
    "source_module core.text path Core/Text/Text.jet alias core_text owns { trim }",
    "module core.text exports { trim }",
    "module core.text exports { trim }",
    "// module core.ghost exports { x }",
  ].join("\n");
  assert.deepEqual(coreModules(text), ["core", "core.text"]);
});

test("the probe imports exactly one module", () => {
  assert.equal(probeSource("core.web.virtual"), 'use core.web.virtual\n\nfn main() {\n    print("ok")\n}\n');
});

test("a check counts every error header and names the first", () => {
  const output = [
    "Error [E0204]: `tree` is being changed in this call, so it can't be used again here",
    "  --> <corelib>/Core/Service/Service.jet:920:98",
    "    |",
    "Warning [L0103] (unused_import): Import `service` is never used.",
    "Error [E0204]: `tree` is being changed in this call, so it can't be used again here",
    "  --> <corelib>/Core/Service/Service.jet:964:98",
  ].join("\n");
  assert.deepEqual(checkSummary(output, 1), {
    errors: 2,
    first: "Error [E0204]: `tree` is being changed in this call, so it can't be used again here --> <corelib>/Core/Service/Service.jet:920:98",
  });
  assert.deepEqual(checkSummary("ok: `x.jet` has no errors\n", 0), { errors: 0, first: "" });
  assert.equal(checkSummary("thread panicked\n", 134).errors, 1);
});

test("the ratchet fails on new or worse modules and on stale ceilings", () => {
  const results = new Map([
    ["core.a", { errors: 0, first: "" }],
    ["core.b", { errors: 3, first: "Error [E1] b" }],
    ["core.c", { errors: 2, first: "Error [E2] c" }],
    ["core.d", { errors: 1, first: "Error [E3] d" }],
  ]);
  const ceilings = { "core.b": 3, "core.c": 1, "core.d": 4 };
  assert.deepEqual(ratchetProblems(ceilings, results), [
    "core.c: 2 errors (ceiling 1); Error [E2] c",
    "core.d: 1 errors, below its ceiling 4; lower it with --write",
  ]);
  assert.deepEqual(ratchetProblems({ "core.b": 3 }, new Map([["core.b", { errors: 3, first: "" }]])), []);
  assert.deepEqual(ratchetProblems({}, new Map([["core.a", { errors: 1, first: "Error [E9] a" }]])), ["core.a: 1 errors (ceiling 0); Error [E9] a"]);
});

test("--write lowers and drops ceilings but never raises one", () => {
  const ceilings = { "core.b": 3, "core.d": 4, "core.e": 2 };
  const lowered = loweredCeilings(ceilings, new Map([["core.b", { errors: 0 }], ["core.d", { errors: 1 }]]));
  assert.deepEqual(lowered, { next: { "core.d": 1, "core.e": 2 }, rises: [] });
  const raised = loweredCeilings(ceilings, new Map([["core.f", { errors: 1 }]]));
  assert.deepEqual(raised.rises, ["core.f: 1 errors exceed ceiling 0"]);
});

test("--write with no committed ratchet records today's failing counts", () => {
  const first = loweredCeilings({}, new Map([["core.a", { errors: 0 }], ["core.b", { errors: 5 }]]), true);
  assert.deepEqual(first, { next: { "core.b": 5 }, rises: [] });
});
