#!/usr/bin/env node
// The production typed-goal checker with the Foundation package.
// Usage: node Compiler/JetSema/Tests/run-typed-goal-fixtures.mjs OUTDIR
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerFixtureUnit } from "../../../Tools/agent/compiler-packages.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-typed-goal-fixtures.mjs OUTDIR");
function item(path, head) {
  const lines = readFileSync(join(repo, path), "utf8").split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  const end = lines.findIndex((line, index) => index > start && line === "}");
  if (start < 0 || end < start) throw new Error(`${path}: missing item ${head}`);
  return lines.slice(start, end + 1).join("\n");
}
const expressionPath = "Compiler/JetSema/Source/Sema/Expressions.jet";
const builders = ["fn sema_tir_node(", "fn sema_tir_expr(", "fn sema_expression_todo("].map((head) => item(expressionPath, head));
const foundation = compilerFixtureUnit(repo, ["Compiler/JetFoundation"]);
const fixtures = readFileSync(join(repo, "Compiler/JetSema/Tests/TypedGoalFixtures.fixture/TypedGoalFixtures.jet"), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), foundation + "\n" + builders.join("\n") + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "typed_goal_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
console.log(`${out}/project/src/compiler.jet`);
