#!/usr/bin/env node
// Production diagnostic builders/gates with the Foundation package.
// Usage: node Compiler/JetSema/Tests/run-return-diagnostic-fixtures.mjs OUTDIR [--web-host]
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerFixtureUnit } from "../../../Tools/agent/compiler-packages.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) throw new Error("usage: run-return-diagnostic-fixtures.mjs OUTDIR");
const webHost = process.argv[3] === "--web-host";
if (process.argv[3] && !webHost) throw new Error("unknown diagnostic fixture mode");
// The item declared by `head` (`fn name(`, public or not) through its closing brace.
function item(path, head) {
  const lines = readFileSync(join(repo, path), "utf8").split("\n");
  const start = lines.findIndex((line) => line.startsWith(head) || line.startsWith(`pub ${head}`));
  const end = lines.findIndex((line, index) => index > start && line === "}");
  if (start < 0 || end < start) throw new Error(`${path}: missing item ${head}`);
  return lines.slice(start, end + 1).join("\n");
}
const diagnosticPath = "Compiler/JetSema/Source/Sema/Diagnostics.jet";
const builders = (webHost ? [
  "fn sema_error_optional(", "fn sema_error(",
] : [
  "fn sema_error_optional(", "fn sema_error(",
  "fn sema_type_show(", "fn sema_type_int_range_text(",
  "fn sema_type_nominal_leaf(", "fn sema_type_fix_hint(",
  "fn sema_return_value_fix(", "fn sema_return_value_mismatch(",
]).map((head) => item(diagnosticPath, head));
if (webHost) {
  const checkPath = "Compiler/JetSema/Source/Sema/CheckProgram.jet";
  builders.push(item(checkPath, "fn sema_web_host_unsupported_core_apis("));
  builders.push(item(checkPath, "fn sema_web_host_check_core_usage("));
} else {
  builders.push(item("Compiler/JetSema/Source/Sema/Expressions.jet", "fn sema_expr_unwrap_tag("));
}
const foundation = compilerFixtureUnit(repo, ["Compiler/JetFoundation"]);
const fixtureName = webHost ? "WebHostDiagnosticFixtures" : "ReturnDiagnosticFixtures";
const fixtures = readFileSync(join(repo, `Compiler/JetSema/Tests/${fixtureName}.fixture/${fixtureName}.jet`), "utf8");
mkdirSync(join(out, "project/src"), { recursive: true });
writeFileSync(join(out, "project/src/compiler.jet"), foundation + "\n" + builders.join("\n") + "\n" + fixtures);
writeFileSync(join(out, "project/package.jet"), 'name: "return_diagnostic_fixtures"\nversion: "0.1.0"\nauthority: { allow: [IO, Mem.Alloc] }\n');
console.log(`${out}/project/src/compiler.jet`);
