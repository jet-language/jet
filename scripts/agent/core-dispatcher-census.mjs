#!/usr/bin/env node

import { pathToFileURL } from "node:url";

import {
  coreSourceFacts,
  publicDispatcherRows,
  validatePublicDispatcher,
} from "./check-core-surface-ledger.mjs";

const SCHEMA = "jet-core-dispatcher-census-v1";
const CORE_SOURCE = "crates/jet-codegen/src/Prelude/Core.jet";
const CORE_CALLS = "crates/jet-foundation/src/Syntax/core_calls.rs";

function compareRows(left, right) {
  const leftKey = `${left.module}.${left.member}`;
  const rightKey = `${right.module}.${right.member}`;
  if (leftKey < rightKey) return -1;
  if (leftKey > rightKey) return 1;
  return left.sourceLine - right.sourceLine;
}

function classifyRows(declarations, rows) {
  validatePublicDispatcher(declarations, rows, { allowUndeclared: true });
  const membersByModule = new Map(
    declarations.modules.map((entry) => [entry.module, new Set(entry.members)]),
  );
  const adapterKeys = new Set(
    declarations.dispatcherRows
      .filter((row) => row.kind === "adapter")
      .map((row) => `${row.module}.${row.member}`),
  );
  return rows
    .map((row) => {
      const key = `${row.module}.${row.member}`;
      const members = membersByModule.get(row.module);
      const derivability = members?.has(row.member)
        ? "declaration-derived"
        : adapterKeys.has(key)
          ? "adapter-only"
          : "undeclared";
      return {
        module: row.module,
        member: row.member,
        derivability,
        source_line: row.sourceLine,
      };
    })
    .sort(compareRows);
}

export function census() {
  const declarations = coreSourceFacts();
  const rows = classifyRows(declarations, publicDispatcherRows());
  const summary = {
    total: rows.length,
    declaration_derived: rows.filter((row) => row.derivability === "declaration-derived").length,
    adapter_only: rows.filter((row) => row.derivability === "adapter-only").length,
    undeclared: rows.filter((row) => row.derivability === "undeclared").length,
  };
  return {
    schema: SCHEMA,
    sources: {
      declaration: CORE_SOURCE,
      dispatcher: CORE_CALLS,
    },
    summary,
    rows,
  };
}

function usage() {
  return [
    "usage: node scripts/agent/core-dispatcher-census.mjs [--json]",
    "",
    "Prints the stable Core dispatcher derivability census as machine JSON.",
  ].join("\n");
}

export function main(argv = process.argv.slice(2)) {
  try {
    if (argv.includes("--help") || argv.includes("-h")) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    if (argv.some((argument) => argument !== "--json")) {
      throw new Error(`unknown option ${argv.find((argument) => argument !== "--json")}`);
    }
    process.stdout.write(`${JSON.stringify(census(), null, 2)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
