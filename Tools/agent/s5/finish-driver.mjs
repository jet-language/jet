#!/usr/bin/env node
import {finalizeDriver} from './receipts.mjs';
const [directory, head, phase, rawCode] = process.argv.slice(2);
const code = Number(rawCode);
if (!directory || !phase || !Number.isInteger(code) || code < 0 || code > 255) throw Error('usage: finish-driver.mjs OUT SHA PHASE REAL_DRIVER_RC');
finalizeDriver(directory, {head, phase, runId: process.env.S5_RUN_ID, code});
