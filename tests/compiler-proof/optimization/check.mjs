#!/usr/bin/env node

import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import {
  canonicalJsonOutput,
  errorPayload,
  runContractCheck,
} from "../lowering/common.mjs";

export async function main(argv = process.argv.slice(2)) {
  const json = argv.includes("--json");
  if (argv.some((arg) => arg !== "--json")) {
    const result = errorPayload("optimization", new Error("usage: check.mjs [--json]"));
    if (json) process.stdout.write(canonicalJsonOutput(result));
    return 64;
  }
  try {
    const result = runContractCheck("optimization");
    if (json) process.stdout.write(canonicalJsonOutput(result));
    else process.stdout.write(`optimization contract: ${result.status}\n`);
    return 0;
  } catch (error) {
    const result = errorPayload("optimization", error);
    if (json) process.stdout.write(canonicalJsonOutput(result));
    else process.stderr.write(`${result.error.code}: ${result.error.message}\n`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await main();
}
