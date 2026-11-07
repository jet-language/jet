#!/usr/bin/env node
// Production task routing with Jet sema's carrier helpers and the Foundation package.
// Usage: node Compiler/JetIR/Tests/run-task-carrier-fixtures.mjs OUTDIR
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { seedDir } from "../../../Tools/agent/roots.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
// The fixture unit of ENTRY... (Tools/agent/CompilerPackages.jet unit, run by the seed).
const compilerFixtureUnit = (...entries) => execFileSync(join(seedDir(), "jet"), [join(repo, "Tools/agent/CompilerPackages.jet"), "--root", repo, "unit", ...entries], { maxBuffer: 1 << 30 }).toString("utf8");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-task-carrier-fixtures.mjs OUTDIR");
function item(path, head) {
  const lines = readFileSync(join(repo, path), "utf8").split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  const end = lines.findIndex((line, index) => index > start && line === "}");
  if (start < 0 || end < start) throw new Error(`${path}: missing item ${head}`);
  return lines.slice(start, end + 1).join("\n");
}
const handles = "Compiler/JetIR/Source/RouteHandle.jet";
const taskgroups = "Compiler/JetSema/Source/Sema/Calls/TaskGroups.jet";
const definitions = [
  item(handles, "IRHandleRoute: struct {"),
  ...["ir_handle_route: fn(", "ir_handle_core: fn(", "ir_handle_type_without_tags: fn(", "ir_handle_task_route: fn("].map((head) => item(handles, head)),
  item("Compiler/JetIR/Source/BuildHelpers.jet", "ir_failure_carrier_for_type: fn("),
  item(taskgroups, "sema_taskgroup_failure_result: fn("),
  item(taskgroups, "sema_taskgroup_task_element: fn("),
];
const foundation = compilerFixtureUnit("Compiler/JetFoundation");
const fixtures = readFileSync(join(repo, "Compiler/JetIR/Tests/TaskCarrierFixtures.fixture/TaskCarrierFixtures.jet"), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), foundation + "\n" + definitions.join("\n") + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "task_carrier_fixtures"\nversion: "0.1.0"\nauthority: { allow: [IO, Mem.Alloc] }\n');
console.log(`${out}/project/src/compiler.jet`);
