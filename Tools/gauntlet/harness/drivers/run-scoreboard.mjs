// The gauntlet driver of the scoreboard export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): buildScoreboard.
// Tools/gauntlet/entries/run-scoreboard/driver/run.jet is the twin on the
// kit's Scoreboard module.
//
//   node run-scoreboard.mjs scoreboard DIR
//
// DIR holds JSON files, each an object with the `matrix`, the measured
// `results`, the measurement `manifest` and, when given, a Tower store
// (`tower`: its `rev` and `cards`) and the machine `ceiling` (its
// `references` and `measurements`); each is judged in name order and shown
// as the file name and the scoreboard as JSON.
import path from 'node:path';
import fs from 'node:fs';
import { buildScoreboard } from '../run.mjs';

const usage = 'usage: node run-scoreboard.mjs scoreboard DIR';
const [command, dir] = process.argv.slice(2);

// The Tower state of a store, as run.mjs reads the live one.
function towerState(store) {
  if (store === undefined) return undefined;
  if (!Array.isArray(store?.cards)) return { status: 'unavailable', reason: 'Tower store has no cards array' };
  return { status: 'available', revision: store.rev ?? null, cards: new Map(store.cards.map((card) => [card.num, card])) };
}

if (command === 'scoreboard' && dir) {
  const names = fs.readdirSync(dir).filter((name) => name.endsWith('.json')).sort();
  for (const name of names) {
    const input = JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'));
    const scoreboard = buildScoreboard(input.matrix, input.results, input.manifest, towerState(input.tower), input.ceiling);
    console.log(`${name}\t${JSON.stringify(scoreboard)}`);
  }
} else {
  console.error(usage);
  process.exit(64);
}
