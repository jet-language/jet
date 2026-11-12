// The gauntlet driver of the scaling gate's corpus libraries (#5072 c7):
// Tools/perf/scaling/corpora.mjs, domains.mjs, mixed.mjs and package.mjs,
// one command per export, printing its result.
// Tools/gauntlet/entries/scaling-corpora/driver/run.jet is the twin on the
// jet_scaling_corpora package.
//
//   node scaling-corpora.mjs shapes | layouts | domains | modulus
//                            | corpus SHAPE N SEED LAYOUT | mixed CODE_LINES SEED
//                            | lines TEXT | digest NAME TEXT ... | manifest NAME
//                            | random SEED COUNT | tree COUNT PREFIX
//                            | finish NAME ROOTS [MAIN] | pack PREFIX SIZES [PER_FILE]
//
// ROOTS and SIZES are comma-separated; N, SEED, COUNT and PER_FILE are
// integers.
import { LAYOUTS, SHAPES, corpus } from '../../../perf/scaling/corpora.mjs';
import { DOMAINS } from '../../../perf/scaling/domains.mjs';
import { MODULUS as MIXED_MODULUS, countLines, isCodeLine as mixedIsCodeLine, mixedSources, sourceDigest as mixedDigest } from '../../../perf/scaling/mixed.mjs';
import { MODULUS, codeLines, consumerTree, finish, isCodeLine, manifest, pack, random, sourceDigest } from '../../../perf/scaling/package.mjs';

const WORDS = ['alpha', 'bravo', 'cedar'];
const [command, ...args] = process.argv.slice(2);
// Each file's lines under its name, each after `|`, so a blank line or a final
// newline shows.
const printFiles = (files) => {
  for (const name of Object.keys(files).sort()) {
    console.log(`== ${name}`);
    for (const line of files[name].split('\n')) console.log(`|${line}`);
  }
};

try {
  if (command === 'shapes') {
    for (const [name, shape] of Object.entries(SHAPES)) console.log(`${name} ${shape.kind} ${shape.unit} ${shape.base}: ${shape.dimension}`);
  } else if (command === 'layouts') {
    console.log(LAYOUTS.join(' '));
  } else if (command === 'domains') {
    for (const [name, domain] of Object.entries(DOMAINS)) console.log(`${name}: ${domain.dimension}`);
  } else if (command === 'modulus') {
    console.log(`package ${MODULUS} mixed ${MIXED_MODULUS}`);
  } else if (command === 'corpus') {
    const built = corpus(args[0], Number(args[1]), Number(args[2]), args[3]);
    console.log(`${built.kind} diagnostics ${built.expect.diagnostics} code ${built.code_lines} sha ${built.source_sha256}`);
    console.log(`files ${Object.keys(built.files).sort().join(' ')}`);
  } else if (command === 'mixed') {
    const built = mixedSources(Number(args[0]), Number(args[1]));
    const s = built.stats;
    console.log(`sha ${built.source_sha256} expected ${built.expected.trim()}`);
    console.log(`physical ${s.physical_lines} code ${s.code_lines} bytes ${s.bytes} functions ${s.functions} types ${s.types} comptime ${s.comptime_items} sections ${s.sections}`);
  } else if (command === 'lines') {
    const counted = countLines(args[0]);
    console.log(`physical ${counted.physical} code ${counted.code}`);
    args[0].split('\n').forEach((line, index) => console.log(`${index} ${mixedIsCodeLine(line)} ${isCodeLine(line)}`));
  } else if (command === 'digest') {
    const files = {};
    for (let at = 0; at + 1 < args.length; at += 2) files[args[at]] = args[at + 1];
    console.log(`package ${sourceDigest(files)} mixed ${mixedDigest(files)} code ${codeLines(files)}`);
  } else if (command === 'manifest') {
    process.stdout.write(manifest(args[0]));
  } else if (command === 'random') {
    const count = Number(args[1]);
    const raw = random(Number(args[0]));
    const ints = random(Number(args[0]));
    const picks = random(Number(args[0]));
    console.log(Array.from({ length: count }, () => raw.next() * 4294967296).join(' '));
    console.log(Array.from({ length: count }, () => ints.int(1, 6)).join(' '));
    console.log(Array.from({ length: count }, () => picks.pick(WORDS)).join(' '));
  } else if (command === 'tree') {
    const files = {};
    const entries = Array.from({ length: Number(args[0]) }, (_, index) => `entry_${index}`);
    console.log(`roots ${consumerTree(files, entries, args[1]).join(' ')}`);
    for (const name of Object.keys(files).sort()) console.log(`${name} ${files[name].split('\n').length}`);
    console.log(`code ${codeLines(files)} sha ${sourceDigest(files)}`);
  } else if (command === 'finish') {
    const files = args.length > 2 ? { 'main.jet': args[2] } : {};
    finish(args[0], files, args[1].split(','));
    printFiles(files);
  } else if (command === 'pack') {
    const files = {};
    const items = args[1].split(',').map((size, item) => Array.from({ length: Number(size) }, (_, line) => `item_${item}_${line}`));
    if (args.length > 2) pack(files, args[0], items, Number(args[2]));
    else pack(files, args[0], items);
    printFiles(files);
  } else {
    console.log('usage: driver shapes|layouts|domains|modulus|corpus|mixed|lines|digest|manifest|random|tree|finish|pack ...');
    process.exit(2);
  }
} catch (error) {
  console.log(`error: ${error.message}`);
  process.exit(1);
}
