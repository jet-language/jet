#!/usr/bin/env node
// Tower entry point. `node tower.mjs help` for the full surface.
import { run } from './app/cli.mjs';
run(process.argv.slice(2));
