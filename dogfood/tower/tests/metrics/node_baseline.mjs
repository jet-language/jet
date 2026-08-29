#!/usr/bin/env node

// Usage: node node_baseline.mjs /path/to/exported-fixture-directory
//
// This harness is read-only. It does not use openStore(): that API performs
// repair recovery, and recovery can write the supplied data directory. The
// fixture path is checked first, then the pure store normalize/project path is
// used with the fixture data.

import {
  lstatSync,
  readdirSync,
  readFileSync,
  realpathSync,
} from 'node:fs';
import { performance } from 'node:perf_hooks';
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '../../../..');
const LIVE_RELATIVE = 'plugins/tower/.tower';
const LIVE_LEXICAL = resolve(REPO_ROOT, LIVE_RELATIVE);
const LIVE_REALPATH = realpathOrNull(LIVE_LEXICAL) || LIVE_LEXICAL;
const NODE_ROOT = resolve(REPO_ROOT, 'plugins/tower/app');
const JET_ROOT = resolve(REPO_ROOT, 'dogfood/tower');

// Explicit capability-matched scopes. No globs: fixture data, generated
// output, tests, and reports cannot enter the baseline by accident. The Node
// lock/path/repair machinery is intentionally outside this comparison: the
// Jet shadow has no corresponding write or recovery machinery.
const NODE_BACKEND_FILES = Object.freeze([
  'plugins/tower/app/config.mjs',
  'plugins/tower/app/lint.mjs',
  'plugins/tower/app/server.mjs',
  'plugins/tower/app/store.mjs',
]);

const NODE_UI_FILES = Object.freeze([
  'plugins/tower/app/ui/index.html',
  'plugins/tower/app/ui/tower.js',
  'plugins/tower/app/ui/tower.css',
  'plugins/tower/app/ui/board-state.js',
  'plugins/tower/app/ui/markdown.js',
  'plugins/tower/app/ui/done-messages.js',
]);

const JET_BACKEND_FILES = Object.freeze([
  'dogfood/tower/run.jet',
]);

const JET_UI_FILES = Object.freeze([
  'dogfood/tower/src/board/index.html',
  'dogfood/tower/src/board/board.js',
  'dogfood/tower/src/board/board.css',
]);

// One language-neutral regex tokenizer for both source lists. It ignores
// line/block comments, counts each quoted string as one token, counts a
// number or ASCII identifier as one token, and counts every remaining
// non-whitespace Unicode code point as one token. Regex literals are treated
// as ordinary punctuation/text; this is intentionally lexical, not a parser.
const TOKEN_RE = /\/\/[^\r\n]*|\/\*[\s\S]*?\*\/|"(?:\\[\s\S]|[^"\\])*"|'(?:\\[\s\S]|[^'\\])*'|`(?:\\[\s\S]|[^`\\])*`|(?:0[xX][0-9a-fA-F]+n?|0[bB][01]+n?|0[oO][0-7]+n?|(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?n?)|[A-Za-z_$][A-Za-z0-9_$]*|[^\s]/gu;

const TOKENIZER = Object.freeze({
  name: 'simple-regex-v1',
  rule: 'One global regex; comments ignored, quoted strings/numbers/ASCII identifiers count once, every remaining non-whitespace code point counts once.',
  applies_to: 'Node oracle and Jet port files identically',
});

const EXCLUDED_SOURCE_SEGMENTS = new Set([
  '.jet',
  'fixtures',
  'generated',
  'reports',
  'tests',
]);

function realpathOrNull(path) {
  try {
    return realpathSync(path);
  } catch {
    return null;
  }
}

function fail(message) {
  throw new Error(`node_baseline: ${message}`);
}

function isWithin(root, candidate) {
  const child = relative(root, candidate);
  return child === '' || (!child.startsWith(`..${sep}`) && child !== '..' && !isAbsolute(child));
}

function overlaps(left, right) {
  return isWithin(left, right) || isWithin(right, left);
}

function slashPath(path) {
  return path.split('\\').join('/');
}

function sameOrBelowText(path, root) {
  const candidate = slashPath(path);
  const base = slashPath(root).replace(/\/$/, '');
  return candidate === base || candidate.startsWith(`${base}/`);
}

function rejectLivePath(path, label, rawPath = path) {
  const lexical = resolve(path);
  if (overlaps(lexical, LIVE_LEXICAL) || sameOrBelowText(rawPath, LIVE_LEXICAL))
    fail(`${label} lexically overlaps ${LIVE_RELATIVE}`);

  const resolved = realpathOrNull(lexical);
  if (resolved && overlaps(resolved, LIVE_REALPATH))
    fail(`${label} realpath overlaps ${LIVE_RELATIVE}`);
}

function fixtureArgument() {
  const args = process.argv.slice(2);
  if (args.length !== 1 || args[0] === '' || args[0].startsWith('-'))
    fail('usage: node node_baseline.mjs EXPORTED_FIXTURE_DIRECTORY');
  return args[0];
}

function assertSafeTree(root) {
  const entries = readdirSync(root, { withFileTypes: true });
  for (const entry of entries) {
    const child = join(root, entry.name);
    const stat = lstatSync(child);
    rejectLivePath(child, `fixture entry ${child}`);
    if (stat.isSymbolicLink()) fail(`fixture contains a symlink: ${child}`);
    if (stat.isDirectory()) {
      assertSafeTree(child);
    } else if (stat.isFile()) {
      if (stat.nlink !== 1) fail(`fixture contains a hardlinked regular file: ${child}`);
    } else {
      fail(`fixture contains a non-regular entry: ${child}`);
    }
  }
}

function safeFixtureDirectory(input) {
  const raw = isAbsolute(input) ? input : join(process.cwd(), input);
  const lexical = resolve(raw);
  rejectLivePath(lexical, 'fixture directory', raw);

  const stat = lstatSync(lexical);
  if (stat.isSymbolicLink()) fail('fixture directory is a symlink');
  if (!stat.isDirectory()) fail(`fixture directory is not a directory: ${lexical}`);

  const resolved = realpathSync(lexical);
  rejectLivePath(resolved, 'fixture directory');
  assertSafeTree(resolved);
  return { input, lexical, resolved };
}

function fixtureFile(root, name) {
  const path = join(root, name);
  const stat = lstatSync(path);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`fixture file is not regular: ${path}`);
  if (stat.nlink !== 1) fail(`fixture file is hardlinked: ${path}`);
  rejectLivePath(path, `fixture file ${name}`);
  return path;
}

function readFixtureJson(root, name) {
  const path = fixtureFile(root, name);
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    fail(`${name} is not valid JSON: ${error.message}`);
  }
}

function sourceFile(relativePath) {
  if (isAbsolute(relativePath)) fail(`source path must be relative: ${relativePath}`);
  const segments = relativePath.split(/[\\/]/);
  if (segments.some(segment => EXCLUDED_SOURCE_SEGMENTS.has(segment)))
    fail(`source path is excluded from metrics: ${relativePath}`);
  if (/(^|[._-])generated([._-]|$)/i.test(basename(relativePath)))
    fail(`generated source path is excluded from metrics: ${relativePath}`);

  const path = resolve(REPO_ROOT, relativePath);
  if (!isWithin(REPO_ROOT, path)) fail(`source path escapes repository: ${relativePath}`);
  const stat = lstatSync(path);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`source file is not regular: ${relativePath}`);
  return path;
}

function physicalNonblankLoc(source) {
  return source.split(/\r\n|\r|\n/).filter(line => line.trim() !== '').length;
}

function lexicalTokens(source) {
  let count = 0;
  for (const match of source.matchAll(TOKEN_RE)) {
    if (match[0].startsWith('//') || match[0].startsWith('/*')) continue;
    count += 1;
  }
  return count;
}

function measureSources(files, root) {
  const perFile = files.map(relativePath => {
    const source = readFileSync(sourceFile(relativePath), 'utf8');
    return {
      path: relativePath,
      physical_nonblank_loc: physicalNonblankLoc(source),
      lexical_tokens: lexicalTokens(source),
    };
  });

  return {
    root,
    files: [...files],
    physical_nonblank_loc: perFile.reduce((sum, file) => sum + file.physical_nonblank_loc, 0),
    lexical_tokens: perFile.reduce((sum, file) => sum + file.lexical_tokens, 0),
    per_file: perFile,
  };
}

function sumSourceScopes(root, scopes) {
  const files = scopes.flatMap(scope => scope.files);
  if (new Set(files).size !== files.length) fail('source scopes contain duplicate files');
  return {
    root,
    files,
    physical_nonblank_loc: scopes.reduce((sum, scope) => sum + scope.physical_nonblank_loc, 0),
    lexical_tokens: scopes.reduce((sum, scope) => sum + scope.lexical_tokens, 0),
    per_file: scopes.flatMap(scope => scope.per_file),
  };
}

function measureSourceScopes(root, backendFiles, uiFiles) {
  const backend = measureSources(backendFiles, root);
  const ui = measureSources(uiFiles, root);
  return {
    backend,
    ui,
    total: sumSourceScopes(root, [backend, ui]),
  };
}

function roundMillis(value) {
  return Math.round(value * 1000) / 1000;
}

async function main() {
  const fixture = safeFixtureDirectory(fixtureArgument());
  const storeUrl = pathToFileURL(resolve(NODE_ROOT, 'store.mjs')).href;
  const configUrl = pathToFileURL(resolve(NODE_ROOT, 'config.mjs')).href;
  const [{ emptyHistory, normalize, project }, { loadConfig }] = await Promise.all([
    import(storeUrl),
    import(configUrl),
  ]);

  const rssBefore = process.memoryUsage().rss;
  const operationStarted = performance.now();

  const tower = readFixtureJson(fixture.resolved, 'tower.json');
  const history = { ...emptyHistory(), ...(readFixtureJson(fixture.resolved, 'history.json') || {}) };
  const config = loadConfig(fixture.resolved);
  const fixtureLoaded = performance.now();

  // normalize() is the load step performed by openStore().project(); include
  // it in the full projection-call timing while keeping openStore() unused.
  const projectStarted = performance.now();
  const projected = project(normalize(tower, history.cards), config, history);
  const projectFinished = performance.now();
  const serialized = JSON.stringify(projected);
  const operationFinished = performance.now();
  const rssAfter = process.memoryUsage().rss;

  const result = {
    schema: 'jet.tower.metrics.node-baseline.v1',
    paths: {
      fixture_input: fixture.input,
      fixture_lexical: fixture.lexical,
      fixture_realpath: fixture.resolved,
      repository: REPO_ROOT,
      node_oracle_root: NODE_ROOT,
      jet_port_root: JET_ROOT,
    },
    timing_ms: {
      fixture_load: roundMillis(fixtureLoaded - operationStarted),
      project: roundMillis(projectFinished - projectStarted),
      load_and_project: roundMillis(projectFinished - operationStarted),
      load_project_and_serialize: roundMillis(operationFinished - operationStarted),
    },
    rss_bytes: {
      before: rssBefore,
      after: rssAfter,
      delta: rssAfter - rssBefore,
    },
    serialized_projected_state_bytes: Buffer.byteLength(serialized, 'utf8'),
    comparison_scope: 'Backend read-model/projection/service plus UI; excludes Node write/recovery machinery absent from the Jet shadow.',
    tokenizer: TOKENIZER,
    sources: {
      node: measureSourceScopes(NODE_ROOT, NODE_BACKEND_FILES, NODE_UI_FILES),
      jet: measureSourceScopes(JET_ROOT, JET_BACKEND_FILES, JET_UI_FILES),
    },
  };

  console.log(JSON.stringify(result));
}

main().catch(error => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
