#!/usr/bin/env node

import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { main as observeStage } from "../lowering/observe.mjs";

export async function main(argv = process.argv.slice(2)) {
  return observeStage(["--stage", "optimization", ...argv]);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = await main();
