// The stand-in for Tools/gauntlet/harness/build-system.mjs: records its
// command line in argv.txt, copies $FAKE_RECEIPT (when set) to its --receipt
// path and exits $FAKE_EXIT (0 when unset).
import fs from 'node:fs';

const args = process.argv.slice(2);
fs.writeFileSync('argv.txt', `${args.join(' ')}\n`);
const receipt = args[args.indexOf('--receipt') + 1];
if (process.env.FAKE_RECEIPT) fs.copyFileSync(process.env.FAKE_RECEIPT, receipt);
process.exit(Number(process.env.FAKE_EXIT ?? 0));
