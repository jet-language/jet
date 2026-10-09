#!/usr/bin/env node
// Assemble production task routing with Jet sema's carrier helpers and Foundation.
// Usage: node Compiler/JetCodegen/Tests/run-task-carrier-fixtures.mjs OUTDIR
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-task-carrier-fixtures.mjs OUTDIR");
const rungs = join(out, "rungs");
const assembled = spawnSync(process.execPath, [join(repo, "Tools/agent/stage1/ladder.mjs"), "rungs", repo, rungs], { encoding: "utf8" });
if (assembled.status !== 0) throw new Error(assembled.stderr || assembled.stdout);
function item(path, head) {
  const lines = readFileSync(join(repo, path), "utf8").split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  const end = lines.findIndex((line, index) => index > start && line === "}");
  if (start < 0 || end < start) throw new Error(`${path}: missing item ${head}`);
  return lines.slice(start, end + 1).join("\n");
}
const handles = "Compiler/JetCodegen/Source/Codegen/HandleMethods.jet";
const taskgroups = "Compiler/JetSema/Source/Sema/Calls/TaskGroups.jet";
const definitions = [
  item(handles, "struct JetCodegenHandleRoute {"),
  ...["fn jet_codegen_handle_route(", "fn jet_codegen_handle_core(", "fn jet_codegen_handle_type_without_tags(", "fn jet_codegen_handle_task_route("].map((head) => item(handles, head)),
  item("Compiler/JetCodegen/Source/Codegen/Expressions.jet", "fn jet_codegen_failure_carrier_for_type("),
  item(taskgroups, "fn sema_taskgroup_failure_result("),
  item(taskgroups, "fn sema_taskgroup_task_element("),
];
const foundation = readFileSync(join(rungs, "L1/project/src/compiler.jet"), "utf8");
const fixtures = readFileSync(join(repo, "Compiler/JetCodegen/Tests/TaskCarrierFixtures.jet"), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), foundation + "\n" + definitions.join("\n") + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "task_carrier_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, Panic] } }\n');
console.log(`${out}/project/src/compiler.jet`);
