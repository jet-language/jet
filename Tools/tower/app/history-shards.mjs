// #6564 c7: the archive in size-bounded shards. history.json passed 60 MB,
// and GitHub refuses a file over 100 MB, so a sharded board keeps
// history.json as a small manifest and its records in history/<kind>-NNN.json,
// each at most SHARD_BYTES. Tower reads the shards back as one history.
//
// Every shard is the JSON document {"<kind>": [...]} that
// JSON.stringify(..., null, 2) writes, so its item lines are byte for byte
// the lines the single-file archive holds. Assembling the shards therefore
// gives exactly the text the single file would have, which the pair backups,
// the repair journal and `tower repair` keep using unchanged.
//
// The manifest names each shard with its item count, size and sha256. A
// reader that finds a shard which does not match (a write in flight, or a
// checkout that mixed two board versions) gets `torn` and never a partial
// history. A board moves to this layout once, by `tower archive shard
// --apply`; until then history.json stays one file. Shard files are never
// removed: a kind keeps at least as many shards as before, emptied ones
// included, so a git commit of the board never deletes a file.
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { MAX_JSON_BYTES, withDirectoryAuthority } from './paths.mjs';

export const HISTORY_LAYOUT = 'tower.history-shards/v1';
export const SHARD_DIR = 'history';
// The default bound. A sharded archive records its own in history.json
// (shardBytes), and every later write packs to that.
export const SHARD_BYTES = 8 * 1024 * 1024;
export const HISTORY_KINDS = ['decisions', 'cards', 'events'];
const MANIFEST_KEYS = new Set(['version', 'layout', 'shardBytes', ...HISTORY_KINDS, 'extra']);
// The keys a history holds itself; any other key travels in manifest.extra.
const HISTORY_KEYS = new Set(['version', ...HISTORY_KINDS]);
const SHARD_NAME = /^(decisions|cards|events)-(\d{3,})\.json$/;
const SHA256 = /^[0-9a-f]{64}$/;
// A manifest is a few KB; anything larger is a single-file archive.
const MANIFEST_BYTES = 1024 * 1024;

export const fileKey = (st) => st ? `${st.dev}:${st.ino}:${st.size}:${st.mtimeMs}:${st.ctimeMs}` : null;
const sha256 = (data) => createHash('sha256').update(data).digest('hex');
const shardFile = (kind, i) => `${kind}-${String(i + 1).padStart(3, '0')}.json`;
const shardHead = (kind) => `{\n  ${JSON.stringify(kind)}: [\n`;
const SHARD_TAIL = '\n  ]\n}\n';
const emptyShard = (kind) => `{\n  ${JSON.stringify(kind)}: []\n}\n`;

function invalid(message) {
  const error = new Error(message);
  error.code = 'E_INVALID';
  throw error;
}

const plainObject = (value) => !!value && typeof value === 'object' && !Array.isArray(value);

// The manifest a history.json holds, or null when it holds a single-file
// archive (or text that is not JSON, which the single-file reader reports).
function manifestOf(bytes) {
  if (bytes.length > MANIFEST_BYTES) return null;
  let doc;
  try { doc = JSON.parse(bytes.toString('utf8')); } catch { return null; }
  if (!plainObject(doc) || doc.layout === undefined) return null;
  return checkedManifest(doc);
}

function checkedManifest(doc) {
  const where = 'history.json (sharded archive manifest)';
  if (doc.layout !== HISTORY_LAYOUT) invalid(`${where}: unknown layout ${JSON.stringify(doc.layout)}`);
  for (const key of Object.keys(doc)) if (!MANIFEST_KEYS.has(key)) invalid(`${where}: unknown key ${JSON.stringify(key)}`);
  if (doc.extra !== undefined && !plainObject(doc.extra)) invalid(`${where}: extra must be an object`);
  for (const key of Object.keys(doc.extra || {}))
    if (HISTORY_KEYS.has(key)) invalid(`${where}: extra may not hold ${JSON.stringify(key)}`);
  if (!Number.isSafeInteger(doc.shardBytes) || doc.shardBytes < 1) invalid(`${where}: shardBytes must be a positive whole number`);
  const seen = new Set();
  for (const kind of HISTORY_KINDS) {
    if (!plainObject(doc[kind]) || !Array.isArray(doc[kind].shards) || Object.keys(doc[kind]).length !== 1)
      invalid(`${where}: ${kind} must be {"shards": [...]}`);
    for (const shard of doc[kind].shards) {
      const name = SHARD_NAME.exec(shard?.file ?? '');
      if (!plainObject(shard) || !name || name[1] !== kind || seen.has(shard.file))
        invalid(`${where}: ${kind} names an invalid shard ${JSON.stringify(shard?.file)}`);
      seen.add(shard.file);
      for (const key of ['items', 'bytes'])
        if (!Number.isSafeInteger(shard[key]) || shard[key] < 0) invalid(`${where}: ${shard.file}.${key} must be a whole number`);
      if (shard.bytes > MAX_JSON_BYTES) invalid(`${where}: ${shard.file} is larger than Tower reads`);
      if (typeof shard.sha256 !== 'string' || !SHA256.test(shard.sha256)) invalid(`${where}: ${shard.file}.sha256 must be a sha256 hex digest`);
    }
  }
  return doc;
}

function shardBody(kind, text, file) {
  if (text === emptyShard(kind)) return '';
  const head = shardHead(kind);
  if (!text.startsWith(head) || !text.endsWith(SHARD_TAIL) || text.length <= head.length + SHARD_TAIL.length)
    invalid(`history shard ${SHARD_DIR}/${file} is not in Tower's shard format`);
  return text.slice(head.length, -SHARD_TAIL.length);
}

// The single-file text of a sharded archive, or { torn: file } when a shard
// is missing or differs from the manifest.
function assembleAt(root, manifest) {
  if (!root.tryStat(SHARD_DIR)) return { torn: `${SHARD_DIR}/` };
  const dir = root.child(SHARD_DIR);
  try {
    const parts = {};
    for (const kind of HISTORY_KINDS) {
      const bodies = [];
      for (const shard of manifest[kind].shards) {
        let data;
        try { data = dir.read(shard.file, MAX_JSON_BYTES, `cannot read unsafe history shard ${dir.expectedPath}/${shard.file}`); }
        catch (error) { if (error.code === 'ENOENT') return { torn: `${SHARD_DIR}/${shard.file}` }; throw error; }
        if (data.length !== shard.bytes || sha256(data) !== shard.sha256) return { torn: `${SHARD_DIR}/${shard.file}` };
        const body = shardBody(kind, data.toString('utf8'), shard.file);
        if (body) bodies.push(body);
      }
      parts[kind] = bodies.length ? `[\n${bodies.join(',\n')}\n  ]` : '[]';
    }
    let text = `{\n  "version": ${JSON.stringify(manifest.version ?? 1)}`;
    for (const kind of HISTORY_KINDS) text += `,\n  ${JSON.stringify(kind)}: ${parts[kind]}`;
    for (const [key, value] of Object.entries(manifest.extra || {}))
      text += `,\n  ${JSON.stringify(key)}: ${JSON.stringify(value, null, 2).replaceAll('\n', '\n  ')}`;
    return { text: `${text}\n}\n` };
  } finally {
    dir.close();
  }
}

// Read the archive through a held data-directory authority.
//   layout: 'file' (one history.json) or 'shards'
//   bytes:  the single-file text (assembled for shards), null when absent
//   value:  the parsed history when `parse`, else null
//   torn:   set instead of bytes/value when a shard does not match
export function readHistoryAt(root, { parse = true } = {}) {
  let bytes;
  try { bytes = root.read('history.json', MAX_JSON_BYTES, `cannot read unsafe history store: ${root.expectedPath}/history.json`); }
  catch (error) {
    if (error.code === 'ENOENT') return { layout: 'file', bytes: null, value: null };
    throw error;
  }
  const manifest = manifestOf(bytes);
  if (!manifest) return { layout: 'file', bytes, value: parse ? JSON.parse(bytes.toString('utf8')) : null };
  const assembled = assembleAt(root, manifest);
  if (assembled.torn) return { layout: 'shards', manifest, torn: assembled.torn, bytes: null, value: null };
  const value = parse ? JSON.parse(assembled.text) : null;
  if (value) {
    for (const kind of HISTORY_KINDS) {
      const items = manifest[kind].shards.reduce((n, shard) => n + shard.items, 0);
      if (value[kind].length !== items) invalid(`history shards hold ${value[kind].length} ${kind}, but history.json lists ${items}`);
    }
  }
  return { layout: 'shards', manifest, bytes: Buffer.from(assembled.text), value };
}

export const tornMessage = (torn) =>
  `history shard ${torn} is missing or does not match history.json; check out history.json and history/ from one board commit`;

// Identity of the archive: history.json plus every entry under history/.
// Every write is an atomic rename, so an unchanged key means unchanged bytes.
export function historyKeyAt(root) {
  const head = fileKey(root.tryStat('history.json'));
  if (!root.tryStat(SHARD_DIR)?.isDirectory()) return head;
  const dir = root.child(SHARD_DIR);
  try {
    return [head, ...dir.list().sort().map(name => `${name}=${fileKey(dir.tryStat(name))}`)].join('|');
  } finally {
    dir.close();
  }
}

export const historyKey = (dataDir) => withDirectoryAuthority(dataDir, historyKeyAt);

function currentManifestAt(root) {
  const st = root.tryStat('history.json');
  if (!st || st.size > MANIFEST_BYTES) return null;
  return manifestOf(root.read('history.json', MANIFEST_BYTES));
}

// Pack each kind's items, in order, into shards of at most `bound` bytes
// (one item larger than the bound gets a shard of its own). A kind keeps at
// least the shard count `previous` lists, so a shard is emptied, never removed.
export function planShards(history, { bound = SHARD_BYTES, previous = null } = {}) {
  const manifest = { version: history.version ?? 1, layout: HISTORY_LAYOUT, shardBytes: bound };
  const files = [];
  for (const kind of HISTORY_KINDS) {
    const budget = bound - shardHead(kind).length - SHARD_TAIL.length;
    const groups = [];
    let group = [];
    let size = 0;
    for (const item of history[kind] || []) {
      const text = `    ${JSON.stringify(item, null, 2).replaceAll('\n', '\n    ')}`;
      const bytes = Buffer.byteLength(text) + 2;
      if (group.length && size + bytes > budget) { groups.push(group); group = []; size = 0; }
      group.push(text);
      size += bytes;
    }
    if (group.length) groups.push(group);
    while (groups.length < Math.max(1, previous?.[kind]?.shards.length ?? 0)) groups.push([]);
    manifest[kind] = {
      shards: groups.map((items, i) => {
        const data = Buffer.from(items.length ? `${shardHead(kind)}${items.join(',\n')}${SHARD_TAIL}` : emptyShard(kind));
        const shard = { file: shardFile(kind, i), items: items.length, bytes: data.length, sha256: sha256(data) };
        files.push({ kind, ...shard, data });
        return shard;
      }),
    };
  }
  const extra = Object.fromEntries(Object.entries(history).filter(([key]) => !HISTORY_KEYS.has(key)));
  if (Object.keys(extra).length) manifest.extra = extra;
  return { manifest, files, text: `${JSON.stringify(manifest, null, 2)}\n` };
}

// Write the archive in `layout` through a held data-directory authority.
// Shards first, then history.json: a reader that sees the new manifest finds
// every shard it names. A shard whose bytes the current manifest already
// lists is left alone unless `rewriteAll` (recovery, where a crash may have
// replaced a shard after the manifest was written). `bound` applies only when
// the archive is not sharded yet; after that the manifest's shardBytes rules.
export function writeHistoryAt(root, history, layout, { bound = SHARD_BYTES, rewriteAll = false } = {}) {
  if (layout !== 'shards') {
    root.writeAtomic('history.json', `${JSON.stringify(history, null, 2)}\n`);
    return null;
  }
  const previous = currentManifestAt(root);
  const plan = planShards(history, { bound: previous?.shardBytes ?? bound, previous });
  const listed = new Map(HISTORY_KINDS.flatMap(kind => previous?.[kind].shards ?? []).map(shard => [shard.file, shard]));
  const dir = root.ensureDirectory(SHARD_DIR);
  try {
    for (const shard of plan.files) {
      const before = listed.get(shard.file);
      if (!rewriteAll && before?.sha256 === shard.sha256 && dir.tryStat(shard.file)?.size === shard.bytes) continue;
      dir.writeAtomic(shard.file, shard.data);
    }
    dir.sync();
  } finally {
    dir.close();
  }
  root.writeAtomic('history.json', plan.text);
  return plan;
}

// The shard files history.json names, as paths under dataDir (none for a
// single-file archive). Board commits add them beside history.json.
export function historyShardPaths(dataDir) {
  const manifest = withDirectoryAuthority(dataDir, currentManifestAt);
  if (!manifest) return [];
  return HISTORY_KINDS.flatMap(kind => manifest[kind].shards.map(shard => join(dataDir, SHARD_DIR, shard.file)));
}

// What `tower archive status` reports about the files.
export function historyFiles(dataDir) {
  return withDirectoryAuthority(dataDir, root => {
    const st = root.tryStat('history.json');
    const manifest = currentManifestAt(root);
    if (!manifest) return { layout: st ? 'file' : 'none', bytes: st?.size ?? 0, shards: [] };
    const shards = HISTORY_KINDS.flatMap(kind => manifest[kind].shards);
    return { layout: 'shards', bytes: st.size + shards.reduce((n, shard) => n + shard.bytes, 0), shards };
  });
}
