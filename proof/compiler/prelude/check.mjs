#!/usr/bin/env node

import { canonicalJson, errorPayload, runStage } from "../runtime/common.mjs";

export const CHECK_SCHEMA = "jet.prelude-contract-check.v1";

export async function main(argv = process.argv.slice(2)) {
  return runStage("prelude", argv);
}

if (process.argv[1] && import.meta.url === new URL(process.argv[1], "file:").href) {
  process.exitCode = await main();
}

export { canonicalJson, errorPayload };
