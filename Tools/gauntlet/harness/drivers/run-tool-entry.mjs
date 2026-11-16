// The gauntlet driver of the tool-entry export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): runToolEntry.
// Tools/gauntlet/entries/run-tool-entry/driver/run.jet is the twin on the
// kit's ToolCases module.
//
//   node run-tool-entry.mjs run DIR
//
// `run` loads the corpus entries of DIR and runs the parity cases of each
// tool entry with the `jet` on PATH, its tool paths under this directory and
// its runs under work/<entry>, and shows the entry's directory name and its
// parity run as JSON, or the error that stopped it. This directory reads
// `<cwd>`, and the `note:` line a Jet program (Tools/agent/JetEnv.jet among
// them) prints on stderr is left out of a reason, so where the checkout
// lives changes no output.
import path from 'node:path';
import { loadEntries, runToolEntry } from '../run.mjs';

const usage = 'usage: node run-tool-entry.mjs run DIR';
const [command, dir] = process.argv.slice(2);
const cwd = process.cwd();

// `text` without the `note:` lines of the reasons it shows as JSON.
function withoutNotes(text) {
  const [first, ...notes] = text.split('note: ');
  return first + notes.map((part) => {
    const end = part.indexOf('\\n');
    return end < 0 ? `note: ${part}` : part.slice(end + 2);
  }).join('');
}

if (command === 'run' && dir) {
  const { tools } = await loadEntries(path.resolve(dir), null);
  for (const { dir: entryDir, entry, directoryName } of tools) {
    try {
      const result = await runToolEntry(entryDir, entry, 'jet', path.join(cwd, 'work', directoryName), cwd);
      console.log(`${directoryName}\t${withoutNotes(JSON.stringify(result).replaceAll(cwd, '<cwd>'))}`);
    } catch (error) {
      console.log(`${directoryName}\terror: ${error.message.replaceAll(cwd, '<cwd>')}`);
    }
  }
} else {
  console.error(usage);
  process.exit(64);
}
