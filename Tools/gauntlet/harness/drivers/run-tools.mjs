// The gauntlet driver of the tool-probe export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): probeAxisTools.
// Tools/gauntlet/entries/run-tools/driver/run.jet is the twin on the kit's
// Axis module.
//
//   node run-tools.mjs probe FILE
//
// `probe` runs each case of the JSON array in FILE, the `tools` an axis
// runner declares and the `jet` binary it names, in this directory, and shows
// the case's `name` and each tool's probe, or the error of a case that
// declares no tools. A probe shows its fields in order. This directory in a
// path or reason reads `<cwd>`. A path outside it, a version of one line of at
// most 300 characters and a stderr of at most 4000 read `<path>`, `<version>`
// and `<stderr>`, so the dev shell's store paths and tool versions change no
// output.
import fs from 'node:fs';
import { probeAxisTools } from '../run.mjs';

const usage = 'usage: node run-tools.mjs probe FILE';
const [command, file] = process.argv.slice(2);
const cwd = process.cwd();

// `text` with this directory as `<cwd>`.
function local(text) {
  return text.replaceAll(cwd, '<cwd>');
}

// A probe's field as the driver shows it.
function field(key, value) {
  if (typeof value !== 'string') return value;
  if (key === 'resolved') return value.startsWith(`${cwd}/`) ? local(value) : (value.startsWith('/') ? '<path>' : value);
  if (key === 'version') return value !== '' && !value.includes('\n') && value.length <= 300 ? '<version>' : value;
  if (key === 'stderr') return value !== '' && value.length <= 4000 ? '<stderr>' : value;
  return local(value);
}

function shown(probe) {
  return Object.fromEntries(Object.entries(probe).map(([key, value]) => [key, field(key, value)]));
}

if (command === 'probe' && file) {
  for (const { name, tools, jet } of JSON.parse(fs.readFileSync(file, 'utf8'))) {
    try {
      for (const probe of await probeAxisTools(cwd, tools, jet)) console.log(`${name}\t${JSON.stringify(shown(probe))}`);
    } catch (error) {
      console.log(`${name}\terror: ${error.message}`);
    }
  }
} else {
  console.error(usage);
  process.exit(64);
}
