// The gauntlet driver of the publication export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): publicationState.
// Tools/gauntlet/entries/run-publication/driver/run.jet is the twin on the
// kit's Publication module.
//
//   node run-publication.mjs publication DIR
//   node run-publication.mjs manifest DIR
//
// DIR holds JSON files, each the object a run hands publicationState (its
// `fullScope`, `loaded` and `skipped` entries, `matrix`, `manifest`,
// `sourceMeasurements`, `results`, `scoreboard`, `axes`, `validationIssues`
// and, for a run of one axis, `axisId`); each is judged in name order and
// shown as the file name and the publication state as JSON. `manifest`
// judges each run against the canonical measurement manifest,
// Tools/gauntlet/measurement-manifest.json, in place of its own.
import path from 'node:path';
import fs from 'node:fs';
import { publicationState } from '../run.mjs';

const usage = 'usage: node run-publication.mjs publication|manifest DIR';
const [command, dir] = process.argv.slice(2);
const manifestUrl = new URL('../../measurement-manifest.json', import.meta.url);

if ((command === 'publication' || command === 'manifest') && dir) {
  const manifest = command === 'manifest' ? JSON.parse(fs.readFileSync(manifestUrl, 'utf8')) : null;
  const names = fs.readdirSync(dir).filter((name) => name.endsWith('.json')).sort();
  for (const name of names) {
    const run = JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'));
    if (manifest) run.manifest = manifest;
    console.log(`${name}\t${JSON.stringify(publicationState(run))}`);
  }
} else {
  console.error(usage);
  process.exit(64);
}
