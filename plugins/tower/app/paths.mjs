// Paths + tiny utilities. Std-only, zero deps.
//
// Tower the tool lives in this plugin directory (TOOL_ROOT). Board DATA lives
// beside it at TOOL_ROOT/.tower by default:
//   1. TOWER_DATA env var — explicit path to a data dir or tower.json
//   2. nearest `.tower/tower.json` walking up from cwd (project-local layout)
//   3. TOOL_ROOT/.tower when that vendored board exists
//   4. nowhere → commands that need data fail with a "run `tower init`" hint
// A hit inside a linked git worktree is never the live board — it is a stale
// tracked copy — so steps 2 and 3 map it to the same path in the main working
// tree, and refuse it when the main side has no board there.
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { basename, dirname, extname, isAbsolute, join, relative, resolve } from 'node:path';
import {
  closeSync, constants, existsSync, fsyncSync, fstatSync, lstatSync,
  mkdirSync, openSync, readFileSync, readdirSync, readlinkSync, readSync, renameSync, rmdirSync,
  statSync, unlinkSync,
  writeFileSync,
} from 'node:fs';

const here = dirname(fileURLToPath(import.meta.url));
export const TOOL_ROOT = dirname(here);            // plugin root
export const UI = join(here, 'ui');
export const DEFAULT_DATA_DIR = join(TOOL_ROOT, '.tower');

// In a linked worktree `.git` is a file: `gitdir: <main>/.git/worktrees/<name>`.
function linkedWorktreeRoot(p) {
  let dir = resolve(p);
  for (;;) {
    const g = join(dir, '.git');
    if (existsSync(g)) {
      let text = null;
      try { if (statSync(g).isFile()) text = readFileSync(g, 'utf8'); } catch { /* unreadable → not a worktree */ }
      const m = text && text.match(/^gitdir:\s*(.+?)\s*$/m);
      const link = m && resolve(dir, m[1]);
      const wt = link && link.match(/^(.+)[\\/]\.git[\\/]worktrees[\\/][^\\/]+$/);
      return wt ? { root: dir, main: wt[1] } : null;
    }
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

const redirectNoted = new Set();

// The canonical home of a data dir: itself, its main-checkout counterpart when
// it sits inside a linked worktree, or null when that counterpart has no board
// (a worktree copy must never be read or written — its changes would orphan).
export function canonicalDataDir(dataDir) {
  const wt = linkedWorktreeRoot(dataDir);
  if (!wt) return dataDir;
  const main = join(wt.main, relative(wt.root, resolve(dataDir)));
  if (!existsSync(join(main, 'tower.json'))) return null;
  if (!redirectNoted.has(main)) {
    redirectNoted.add(main);
    console.error(`tower: worktree checkout detected — using canonical board at ${main}`);
  }
  return main;
}

export function findDataDir(from = process.cwd()) {
  if (process.env.TOWER_DATA) {
    const p = resolve(process.env.TOWER_DATA);
    return p.endsWith('.json') ? dirname(p) : p;
  }
  let dir = resolve(from);
  for (;;) {
    if (existsSync(join(dir, '.tower', 'tower.json'))) {
      const canon = canonicalDataDir(join(dir, '.tower'));
      if (canon) return canon;
      // stale worktree copy with no canonical board — keep walking, never use it
    }
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  const start = resolve(from);
  const host = projectRoot(DEFAULT_DATA_DIR);
  const withinHost = host && (() => {
    const rel = relative(host, start);
    return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
  })();
  if (withinHost) {
    const dd = canonicalDataDir(DEFAULT_DATA_DIR);
    if (dd && existsSync(join(dd, 'tower.json'))) return dd;
  }
  return null;
}

/** Host project root for doc preview (Jet repo when data is plugins/tower/.tower). */
export function projectRoot(dataDir) {
  if (!dataDir) return null;
  const data = resolve(dataDir);
  if (data === resolve(DEFAULT_DATA_DIR)) {
    // Vendored Jet layout: <repo>/plugins/tower/.tower → <repo>
    if (basename(TOOL_ROOT) === 'tower' && basename(dirname(TOOL_ROOT)) === 'plugins') {
      return dirname(dirname(TOOL_ROOT));
    }
    return dirname(TOOL_ROOT);
  }
  // Classic layout: <project>/.tower
  if (basename(data) === '.tower') return dirname(data);
  return dirname(data);
}

export function dataFile(dir = findDataDir()) {
  if (!dir) return null;
  if (process.env.TOWER_DATA && process.env.TOWER_DATA.endsWith('.json')) return resolve(process.env.TOWER_DATA);
  return join(dir, 'tower.json');
}
export const configFile = (dir) => (dir ? join(dir, 'config.json') : null);
export const secretsFile = (dir) => (dir ? join(dir, 'secrets.json') : null);
// Append-only archive (#461): retired cards/decisions/events, same dir as
// tower.json, committed to git (NOT gitignored — it's board history).
export const historyFile = (dir) => (dir ? join(dir, 'history.json') : null);

// JSON is a board-control-plane input, not an unbounded generic file read.
// The real board is currently well below this limit; the bound leaves room for
// growth while making a hostile live/history/backup file finite.
export const MAX_JSON_BYTES = 64 * 1024 * 1024;

const FD_ROOT = process.platform === 'linux' ? '/proc/self/fd' : '/dev/fd';
const DIRECTORY_FLAGS = constants.O_RDONLY | (constants.O_DIRECTORY || 0)
  | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);
const ATOMIC_FLAGS = constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL
  | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);

function atomicFailure(message) {
  const error = new Error(message);
  error.code = 'E_INVALID';
  throw error;
}

function safeRegular(stat) {
  return !!stat?.isFile() && stat.nlink === 1;
}

function safeDirectory(stat) {
  return !!stat?.isDirectory() && !stat.isSymbolicLink?.();
}

function sameIdentity(left, right) {
  return !!left && !!right && left.dev === right.dev && left.ino === right.ino;
}

function rawFdPath(fd) {
  return join(FD_ROOT, String(fd));
}

function childName(name, message = 'unsafe directory entry name') {
  if (typeof name !== 'string' || !name || name === '.' || name === '..' || basename(name) !== name || name.includes('\0'))
    atomicFailure(message);
  return name;
}

function physicalFdPath(fd) {
  try { return readlinkSync(rawFdPath(fd)); }
  catch (error) { atomicFailure(`held directory descriptor is unavailable: ${error.message}`); }
}

function closeQuietly(fd) {
  if (fd !== undefined) {
    try { closeSync(fd); } catch { /* best effort during failure cleanup */ }
  }
}

function guardedAuthority(authority, operation) {
  if (authority.closed) atomicFailure(`held directory is closed: ${authority.expectedPath}`);
  let current;
  try { current = fstatSync(authority.fd); }
  catch (error) { atomicFailure(`held directory is unavailable: ${authority.expectedPath}: ${error.message}`); }
  if (!safeDirectory(current) || !sameIdentity(current, authority.identity)
    || physicalFdPath(authority.fd) !== authority.physicalPath)
    atomicFailure(`held directory changed during ${operation}: ${authority.expectedPath}`);
  let named;
  try { named = lstatSync(authority.expectedPath); }
  catch (error) { atomicFailure(`held directory path changed during ${operation}: ${authority.expectedPath}: ${error.message}`); }
  if (!safeDirectory(named) || !sameIdentity(named, authority.identity))
    atomicFailure(`held directory path changed during ${operation}: ${authority.expectedPath}`);
}

function openAuthorityAt(parent, name) {
  childName(name);
  guardedAuthority(parent, `open ${name}`);
  const raw = join(rawFdPath(parent.fd), name);
  let fd;
  try { fd = openSync(raw, DIRECTORY_FLAGS); }
  catch (error) {
    if (['EACCES', 'EAGAIN', 'ELOOP', 'ENODEV', 'ENOTDIR', 'ENXIO', 'EPERM', 'EINVAL'].includes(error.code))
      atomicFailure(`refusing unsafe directory ${join(parent.expectedPath, name)}`);
    throw error;
  }
  let identity;
  try {
    identity = fstatSync(fd);
    const named = lstatSync(raw);
    if (!safeDirectory(identity) || !sameIdentity(identity, named))
      atomicFailure(`directory changed while opening ${join(parent.expectedPath, name)}`);
    const authority = makeAuthority(fd, join(parent.expectedPath, name), identity);
    guardedAuthority(authority, `open ${name}`);
    guardedAuthority(parent, `open ${name}`);
    return authority;
  } catch (error) {
    closeQuietly(fd);
    throw error;
  }
}

function makeAuthority(fd, expectedPath, identity = fstatSync(fd)) {
  const authority = {
    fd,
    expectedPath: resolve(expectedPath),
    identity,
    physicalPath: physicalFdPath(fd),
    closed: false,
    path(name) {
      return join(rawFdPath(authority.fd), childName(name));
    },
    stat(name) {
      childName(name);
      guardedAuthority(authority, `stat ${name}`);
      return lstatSync(authority.path(name));
    },
    tryStat(name) {
      try { return authority.stat(name); }
      catch (error) { if (error.code === 'ENOENT') return null; throw error; }
    },
    list() {
      guardedAuthority(authority, 'list');
      const names = readdirSync(rawFdPath(authority.fd));
      guardedAuthority(authority, 'list');
      return names;
    },
    child(name) {
      return openAuthorityAt(authority, name);
    },
    ensureDirectory(name) {
      childName(name);
      guardedAuthority(authority, `create directory ${name}`);
      const raw = authority.path(name);
      try { mkdirSync(raw, { mode: 0o700 }); }
      catch (error) { if (error.code !== 'EEXIST') throw error; }
      const child = authority.child(name);
      guardedAuthority(authority, `create directory ${name}`);
      return child;
    },
    read(name, maxBytes = MAX_JSON_BYTES, message = `refusing unsafe file ${join(authority.expectedPath, name)}`) {
      return readFileAt(authority, name, maxBytes, message);
    },
    writeAtomic(name, text) {
      return writeAtomicAt(authority, name, text);
    },
    rename(from, to, expected = null) {
      return renameAt(authority, from, to, expected);
    },
    remove(name, expected = null) {
      return removeAt(authority, name, expected);
    },
    removeUnexpected(name, expected) {
      return removeUnexpectedAt(authority, name, expected);
    },
    sync() {
      guardedAuthority(authority, 'sync');
      fsyncSync(authority.fd);
      guardedAuthority(authority, 'sync');
    },
    guard(operation = 'operation') {
      guardedAuthority(authority, operation);
    },
    close() {
      if (!authority.closed) {
        authority.closed = true;
        closeSync(authority.fd);
      }
    },
  };
  return authority;
}

function openAuthority(p) {
  const expectedPath = resolve(p);
  let fd;
  let authority;
  try { fd = openSync('/', DIRECTORY_FLAGS); }
  catch (error) { throw error; }
  try {
    authority = makeAuthority(fd, '/');
    const parts = relative('/', expectedPath).split('/').filter(Boolean);
    for (const part of parts) {
      const next = openAuthorityAt(authority, part);
      authority.close();
      authority = next;
    }
    guardedAuthority(authority, 'open');
    return authority;
  } catch (error) {
    if (authority) authority.close();
    else closeQuietly(fd);
    throw error;
  }
}

// Every caller keeps this descriptor for the entire operation. The absolute
// path is used only to identify the authority and to report errors; child
// operations use /proc/self/fd (or /dev/fd) paths rooted at the held fd.
export function withDirectoryAuthority(directory, operation) {
  const authority = openAuthority(directory);
  try { return operation(authority); }
  finally { authority.close(); }
}

function readFileAt(authority, name, maxBytes, message) {
  childName(name);
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 0 || maxBytes > MAX_JSON_BYTES)
    atomicFailure(`invalid read bound for ${join(authority.expectedPath, name)}`);
  guardedAuthority(authority, `read ${name}`);
  const raw = authority.path(name);
  let before;
  try { before = lstatSync(raw); }
  catch (error) { throw error; }
  if (!safeRegular(before)) atomicFailure(message);
  if (before.size > maxBytes) atomicFailure(`file exceeds read bound: ${join(authority.expectedPath, name)}`);
  let fd;
  try {
    const readFlags = constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);
    fd = openSync(raw, readFlags);
    const opened = fstatSync(fd);
    if (!safeRegular(opened) || !sameIdentity(before, opened)) atomicFailure(message);
    const chunks = [];
    const chunk = Buffer.allocUnsafe(Math.min(64 * 1024, Math.max(1, maxBytes || 1)));
    let total = 0;
    for (;;) {
      const count = readSync(fd, chunk, 0, chunk.length, null);
      if (!count) break;
      total += count;
      if (total > maxBytes) atomicFailure(`file exceeds read bound: ${join(authority.expectedPath, name)}`);
      chunks.push(Buffer.from(chunk.subarray(0, count)));
    }
    const after = fstatSync(fd);
    if (!safeRegular(after) || !sameIdentity(opened, after) || after.size > maxBytes)
      atomicFailure(message);
    guardedAuthority(authority, `read ${name}`);
    return Buffer.concat(chunks, total);
  } catch (error) {
    if (['EACCES', 'EAGAIN', 'EISDIR', 'ELOOP', 'ENODEV', 'ENOTDIR', 'ENXIO', 'EPERM', 'EINVAL'].includes(error.code))
      atomicFailure(message);
    throw error;
  } finally {
    closeQuietly(fd);
  }
}

function checkedChildAuthority(parent, child, name, operation) {
  if (!child || child.expectedPath !== resolve(join(parent.expectedPath, name)))
    atomicFailure(`held directory authority does not contain ${join(parent.expectedPath, name)}`);
  parent.guard(operation);
  child.guard(operation);
  return child;
}

function destinationAt(authority, name, message) {
  const current = authority.tryStat(name);
  if (current && !safeRegular(current)) atomicFailure(message);
  return current;
}

function removeAt(authority, name, expected = null) {
  childName(name);
  guardedAuthority(authority, `remove ${name}`);
  const current = authority.tryStat(name);
  if (!current) return false;
  if (expected && !sameIdentity(current, expected)) return false;
  if (!safeRegular(current)) atomicFailure(`refusing unsafe file removal ${join(authority.expectedPath, name)}`);
  unlinkSync(authority.path(name));
  guardedAuthority(authority, `remove ${name}`);
  return true;
}

// A close-to-rename attacker may replace a temporary name after our fd is
// closed. Remove only the unexpected entry now at the destination, through the
// same held parent authority. Never follow a symlink or remove another entry.
function removeUnexpectedAt(authority, name, expected) {
  childName(name);
  let current;
  try { current = authority.stat(name); }
  catch (error) {
    if (error.code === 'ENOENT') return;
    throw error;
  }
  if (sameIdentity(current, expected) && safeRegular(current)) return;
  if (current.isDirectory() && !current.isSymbolicLink?.()) rmdirSync(authority.path(name));
  else unlinkSync(authority.path(name));
  guardedAuthority(authority, `remove unexpected ${name}`);
}

function writeAtomicAt(authority, name, text) {
  childName(name);
  const target = join(authority.expectedPath, name);
  let temporary;
  let fd;
  for (let attempt = 0; attempt < 64; attempt++) {
    temporary = `.${name}.tmp-${randomBytes(16).toString('hex')}`;
    try {
      fd = openSync(authority.path(temporary), ATOMIC_FLAGS, 0o600);
      break;
    } catch (error) {
      if (error.code === 'EEXIST' || error.code === 'ELOOP') continue;
      throw error;
    }
  }
  if (fd === undefined) atomicFailure(`temporary-file collision limit reached for ${target}`);
  let temporaryStat;
  let committed = false;
  let renamed = false;
  try {
    writeFileSync(fd, text);
    temporaryStat = fstatSync(fd);
    if (!safeRegular(temporaryStat)) atomicFailure(`temporary atomic-write file is unsafe for ${target}`);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    destinationAt(authority, name, `refusing unsafe atomic-write destination ${target}`);
    guardedAuthority(authority, `replace ${name}`);
    renameSync(authority.path(temporary), authority.path(name));
    renamed = true;
    const destination = authority.tryStat(name);
    if (!destination || !sameIdentity(destination, temporaryStat) || !safeRegular(destination)) {
      try { removeUnexpectedAt(authority, name, temporaryStat); } catch { /* preserve the security failure */ }
      atomicFailure(`atomic-write destination changed during replace ${target}`);
    }
    authority.sync();
    committed = true;
  } finally {
    if (!committed && !renamed && !temporaryStat && fd !== undefined) {
      try { temporaryStat = fstatSync(fd); } catch { /* best effort */ }
    }
    closeQuietly(fd);
    if (!committed) {
      if (renamed) {
        try { removeUnexpectedAt(authority, name, temporaryStat); } catch { /* best effort */ }
      } else if (temporary !== undefined && temporaryStat) {
        try { removeAt(authority, temporary, temporaryStat); } catch { /* best effort */ }
      }
    }
  }
}

function renameAt(authority, from, to, expected = null) {
  childName(from); childName(to);
  guardedAuthority(authority, `rename ${from}`);
  const source = authority.stat(from);
  if (expected && !sameIdentity(source, expected)) atomicFailure(`rename source changed: ${join(authority.expectedPath, from)}`);
  if (!safeRegular(source)) atomicFailure(`refusing unsafe rename source: ${join(authority.expectedPath, from)}`);
  destinationAt(authority, to, `refusing unsafe rename destination: ${join(authority.expectedPath, to)}`);
  renameSync(authority.path(from), authority.path(to));
  const destination = authority.tryStat(to);
  if (!destination || !sameIdentity(destination, source) || !safeRegular(destination)) {
    try { removeUnexpectedAt(authority, to, source); } catch { /* preserve the security failure */ }
    atomicFailure(`rename destination changed: ${join(authority.expectedPath, to)}`);
  }
  guardedAuthority(authority, `rename ${from}`);
}

export function readJSONAt(authority, name, fallback, maxBytes = MAX_JSON_BYTES) {
  try { return JSON.parse(authority.read(name, maxBytes).toString('utf8')); }
  catch (error) { if (error.code === 'ENOENT') return fallback; throw error; }
}

export const readJSON = (p, fallback, maxBytes = MAX_JSON_BYTES) => {
  if (!p) return fallback;
  try {
    return withDirectoryAuthority(dirname(p), authority => readJSONAt(authority, basename(p), fallback, maxBytes));
  } catch (error) {
    if (error.code === 'ENOENT') return fallback;
    throw error;
  }
};

export function syncFile(p) {
  return withDirectoryAuthority(dirname(p), authority => {
    const stat = authority.stat(basename(p));
    if (!safeRegular(stat)) atomicFailure(`refusing unsafe file sync ${p}`);
    const fd = openSync(authority.path(basename(p)), constants.O_RDONLY | (constants.O_NOFOLLOW || 0));
    try { fsyncSync(fd); } finally { closeQuietly(fd); }
  });
}

export function syncDir(p) {
  return withDirectoryAuthority(p, authority => authority.sync());
}

export function writeTextAtomic(p, text) {
  return withDirectoryAuthority(dirname(p), authority => authority.writeAtomic(basename(p), text));
}

export function writeJSON(p, v) {
  writeTextAtomic(p, JSON.stringify(v, null, 2) + '\n');
}

// Rolling backups: hold the data directory and the backup directory for the
// entire create/list/delete/retention operation. No path is re-resolved after
// an attacker swaps an ancestor.
export function backupRequiredAt(parent, p, keep = 20, heldBackup = null) {
  const parentPath = resolve(dirname(p));
  if (parentPath !== parent.expectedPath)
    atomicFailure(`backup parent authority does not contain ${p}`);
  const sourceName = basename(p);
  parent.guard(`back up ${sourceName}`);
  if (heldBackup) checkedChildAuthority(parent, heldBackup, 'backups', `back up ${sourceName}`);
  let source;
  try { source = readFileAt(parent, sourceName, MAX_JSON_BYTES, `cannot back up unsafe file ${p}`); }
  catch (error) {
    if (error.code === 'ENOENT') throw new Error(`cannot back up missing file ${p}`);
    throw error;
  }
  let backup;
  let ownsBackup = false;
  try {
    backup = heldBackup || parent.ensureDirectory('backups');
    ownsBackup = !heldBackup;
    checkedChildAuthority(parent, backup, 'backups', `back up ${sourceName}`);
    const prefix = `${basename(p, extname(p))}-`;
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    let name;
    let fd;
    for (let attempt = 0; attempt < 64; attempt++) {
      name = `${prefix}${stamp}-${randomBytes(16).toString('hex')}.json`;
      try {
        fd = openSync(backup.path(name), ATOMIC_FLAGS, 0o600);
        break;
      } catch (error) {
        if (error.code === 'EEXIST' || error.code === 'ELOOP') continue;
        throw error;
      }
    }
    if (fd === undefined) atomicFailure(`backup-file collision limit reached for ${p}`);
    let destination;
    let committed = false;
    try {
      destination = fstatSync(fd);
      if (!safeRegular(destination)) atomicFailure(`backup destination is unsafe for ${p}`);
      writeFileSync(fd, source);
      const written = fstatSync(fd);
      if (!sameIdentity(written, destination) || !safeRegular(written))
        atomicFailure(`backup destination changed while writing ${p}`);
      fsyncSync(fd);
      closeSync(fd);
      fd = undefined;
      const installed = backup.stat(name);
      if (!sameIdentity(installed, destination) || !safeRegular(installed)) {
        try { backup.removeUnexpected(name, destination); } catch { /* preserve the security failure */ }
        atomicFailure(`backup destination changed during write ${p}`);
      }
      committed = true;
    } finally {
      closeQuietly(fd);
      if (!committed && destination) {
        try { backup.remove(name, destination); } catch { /* best effort */ }
      }
    }
    backup.sync();
    parent.sync();
    const old = backup.list().filter(file => file.startsWith(prefix)).sort();
    const keepCount = Number.isFinite(Number(keep)) ? Math.max(0, Number(keep)) : 20;
    for (const file of old.slice(0, Math.max(0, old.length - keepCount))) backup.remove(file);
    backup.sync();
    parent.guard(`finish backup ${sourceName}`);
    return join(parent.expectedPath, 'backups', name);
  } finally {
    if (ownsBackup) backup.close();
  }
}

export function backupRequired(p, keep = 20) {
  return withDirectoryAuthority(dirname(p), parent => backupRequiredAt(parent, p, keep));
}

export function backup(p, keep = 20) {
  try { return backupRequired(p, keep); }
  catch { /* routine writes keep backups best-effort */ }
}

export function readFileNoFollow(p, message = `refusing unsafe file ${p}`, maxBytes = MAX_JSON_BYTES) {
  return withDirectoryAuthority(dirname(p), authority => readFileAt(authority, basename(p), maxBytes, message));
}

export function fileExistsNoFollow(p) {
  if (!p) return false;
  return withDirectoryAuthority(dirname(p), authority => {
    const stat = authority.tryStat(basename(p));
    if (!stat) return false;
    if (!safeRegular(stat)) atomicFailure(`refusing unsafe file ${p}`);
    return true;
  });
}

export function unlinkFileNoFollow(p) {
  return withDirectoryAuthority(dirname(p), authority => {
    const removed = authority.remove(basename(p));
    if (removed) authority.sync();
    return removed;
  });
}

// Read the newest matching backup while the held backup directory remains
// open. The returned value never exposes a path that outlives its authority.
export function readLatestJSON(directory, prefix, fallback = null) {
  try {
    return withDirectoryAuthority(directory, authority => {
      const files = authority.list().filter(name => name.startsWith(prefix)).sort();
      if (!files.length) return fallback;
      return readJSONAt(authority, files.at(-1), fallback, MAX_JSON_BYTES);
    });
  } catch (error) {
    if (error.code === 'ENOENT') return fallback;
    throw error;
  }
}

let seq = 0;
export const newId = (prefix) => `${prefix}${(seq++).toString(36)}${process.hrtime.bigint().toString(36).slice(-6)}`;
export const today = () => new Date().toISOString().slice(0, 10);
export const now = () => new Date().toISOString();
