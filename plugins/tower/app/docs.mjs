// Docs tab — durable markdown under project docs/ plus a pinned owner scratchpad.
// Scratchpad: <dataDir>/scratch/owner-scratch.md
// Everything else: <project>/docs/**/*.md (no .json, no skills).
import {
  closeSync, constants, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync,
  readlinkSync,
  readdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync, renameSync,
} from 'node:fs';
import { join, basename, isAbsolute, resolve, relative, sep } from 'node:path';
import { randomBytes } from 'node:crypto';
import { projectRoot as resolveProjectRoot } from './paths.mjs';
import { TowerError } from './store.mjs';

const fail = (code, msg) => { throw new TowerError(code, msg); };

export const SCRATCH_ID = 'owner-scratch';
const SCRATCH_FILE = `${SCRATCH_ID}.md`;
export const OWNER_GUIDANCE_PATH = 'docs/agents/owner-guidance.md';
export const SECTIONS = [
  { id: 'spec', label: 'Spec', dir: 'docs/spec' },
  { id: 'proposals', label: 'Proposals', dir: 'docs/proposals' },
  { id: 'plans', label: 'Plans', dir: 'docs/plans' },
  { id: 'research', label: 'Research', dir: 'docs/research' },
  { id: 'audits', label: 'Audits', dir: 'docs/audits' },
  { id: 'references', label: 'References', dir: 'docs/reference' },
];
/** Top-level docs/ dirs that never appear in the Docs UI or counts. */
export const HIDDEN_TOP_DIRS = new Set(['archive']);
/** Fold these live dirs into another section id (no separate UI section). */
export const SECTION_ALIASES = {
  sidequests: 'plans',
};
const KNOWN_DIRS = new Set([
  ...SECTIONS.map(s => s.dir.replace(/^docs\//, '')),
  ...Object.keys(SECTION_ALIASES),
]);

const SLUG_RE = /^[a-z0-9][a-z0-9._-]{0,79}$/i;

export const scratchDir = (dataDir) => join(dataDir, 'scratch');
export const scratchPadPath = (dataDir) => join(scratchDir(dataDir), SCRATCH_FILE);

function projectRoot(dataDir) {
  return resolveProjectRoot(dataDir);
}

function slugify(title) {
  const s = String(title || '').trim().toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
  return s || `note-${Date.now().toString(36)}`;
}

function parseFront(raw) {
  if (!raw.startsWith('---\n')) return { title: null, body: raw };
  const end = raw.indexOf('\n---\n', 4);
  if (end < 0) return { title: null, body: raw };
  const head = raw.slice(4, end);
  const body = raw.slice(end + 5);
  const title = /^title:\s*(.+)$/m.exec(head)?.[1]?.trim() || null;
  return { title, body };
}

function titleFromBody(body, fallback) {
  const m = /^#\s+(.+)$/m.exec(body || '');
  return (m && m[1].trim()) || fallback;
}

function serializeScratch(title, body) {
  const t = String(title || 'Owner scratch').trim() || 'Owner scratch';
  const b = String(body ?? '').replace(/\r\n/g, '\n');
  return `---\ntitle: ${t}\n---\n${b.endsWith('\n') || !b ? b : b + '\n'}`;
}

// Node has no openat/renameat wrapper. Keep the directory descriptor open and
// address descendants through its procfs handle; every component is opened
// with O_NOFOLLOW before it is used. Unsupported platforms fail closed.
const FD_ROOT = process.platform === 'linux' ? '/proc/self/fd'
  : process.platform === 'darwin' ? '/dev/fd' : null;
const READ_FLAGS = constants.O_RDONLY | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);
const DIRECTORY_FLAGS = READ_FLAGS | (constants.O_DIRECTORY || 0);
const WRITE_FLAGS = constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL
  | (constants.O_NOFOLLOW || 0) | (constants.O_NONBLOCK || 0);
const heldDirectories = new Map();

function requireSecureFs() {
  if (!FD_ROOT || constants.O_NOFOLLOW == null || constants.O_DIRECTORY == null)
    fail('E_INVALID', 'secure docs filesystem operations are unavailable');
}

function rawFdPath(fd, name = '') {
  requireSecureFs();
  return name ? join(FD_ROOT, String(fd), name) : join(FD_ROOT, String(fd));
}

function sameIdentity(left, right) {
  return left?.dev === right?.dev && left?.ino === right?.ino;
}

function contained(root, candidate) {
  const rel = relative(root, candidate);
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}

function physicalFdPath(fd) {
  const target = readlinkSync(rawFdPath(fd));
  if (!target || target.endsWith(' (deleted)'))
    fail('E_INVALID', 'docs directory was removed during operation');
  return resolve(target);
}

function guardHeldDirectory(fd) {
  const held = heldDirectories.get(fd);
  if (!held) return;
  try {
    const current = fstatSync(fd);
    const physical = physicalFdPath(fd);
    const expected = lstatSync(held.expectedPath);
    if (!current.isDirectory() || !sameIdentity(current, held.identity)
      || !expected.isDirectory() || !sameIdentity(expected, current)
      || !contained(held.rootPhysical, physical))
      fail('E_INVALID', 'docs directory changed or moved outside the project root');
    if (held.rootFd !== fd) guardHeldDirectory(held.rootFd);
  } catch (error) {
    if (error instanceof TowerError) throw error;
    fail('E_INVALID', 'docs directory changed during operation');
  }
}

function rememberDirectory(fd, expectedPath, root = null) {
  const identity = fstatSync(fd);
  const physical = physicalFdPath(fd);
  heldDirectories.set(fd, {
    identity,
    expectedPath: resolve(expectedPath),
    rootFd: root?.rootFd ?? fd,
    rootPhysical: root?.rootPhysical ?? physical,
  });
  try { guardHeldDirectory(fd); }
  catch (error) { heldDirectories.delete(fd); throw error; }
  return fd;
}

function closeHeld(fd) {
  heldDirectories.delete(fd);
  try { closeSync(fd); } catch { /* best effort */ }
}

function fsPathError(error, message) {
  if (error instanceof TowerError) throw error;
  if (['EACCES', 'EAGAIN', 'EISDIR', 'ELOOP', 'ENODEV', 'ENOTDIR', 'ENOTSUP', 'ENXIO', 'EPERM', 'EINVAL'].includes(error.code)) fail('E_INVALID', message);
  throw error;
}

function openDirectoryAt(parentFd, name, message = 'docs path cannot be resolved') {
  guardHeldDirectory(parentFd);
  let fd;
  try { fd = openSync(rawFdPath(parentFd, name), DIRECTORY_FLAGS); }
  catch (error) { fsPathError(error, message); }
  try {
    guardHeldDirectory(parentFd);
    const parent = heldDirectories.get(parentFd);
    return rememberDirectory(fd, resolve(parent.expectedPath, name), parent);
  } catch (error) {
    closeHeld(fd);
    throw error;
  }
}

function openAbsoluteDirectory(abs) {
  requireSecureFs();
  let current;
  try {
    current = openSync(sep, DIRECTORY_FLAGS);
    rememberDirectory(current, sep);
  }
  catch (error) {
    if (current !== undefined) closeHeld(current);
    fsPathError(error, 'docs path cannot be resolved');
  }
  try {
    for (const name of abs.split(sep).filter(Boolean)) {
      const next = openDirectoryAt(current, name);
      closeHeld(current);
      current = next;
    }
    const identity = fstatSync(current);
    const physical = physicalFdPath(current);
    heldDirectories.set(current, {
      identity,
      expectedPath: resolve(abs),
      rootFd: current,
      rootPhysical: physical,
    });
    guardHeldDirectory(current);
    return current;
  } catch (error) {
    closeHeld(current);
    throw error;
  }
}

function mkdirAt(parentFd, name, message = 'docs path cannot be created safely') {
  guardHeldDirectory(parentFd);
  try { mkdirSync(rawFdPath(parentFd, name), { mode: 0o755 }); }
  catch (error) { fsPathError(error, message); }
  finally { guardHeldDirectory(parentFd); }
}

function ensureDirectoryAt(parentFd, name, message = 'docs path cannot be resolved') {
  try { return openDirectoryAt(parentFd, name, message); }
  catch (error) {
    if (error.code !== 'ENOENT') throw error;
    try { mkdirAt(parentFd, name); }
    catch (mkdirError) {
      if (mkdirError.code !== 'EEXIST') throw mkdirError;
    }
    try { return openDirectoryAt(parentFd, name, message); }
    catch (error) {
      if (error.code === 'ENOENT') fail('E_INVALID', message);
      throw error;
    }
  }
}

function closeContext(context) {
  for (const fd of [...context.fds].reverse()) {
    closeHeld(fd);
  }
}

function openDataContext(dataDir, createScratch) {
  const dataFd = openAbsoluteDirectory(resolve(dataDir));
  try {
    const scratchMessage = 'scratch cannot be resolved';
    const scratchFd = createScratch
      ? ensureDirectoryAt(dataFd, 'scratch', scratchMessage)
      : openDirectoryAt(dataFd, 'scratch', scratchMessage);
    return { dataFd, scratchFd, fds: [dataFd, scratchFd] };
  } catch (error) {
    closeHeld(dataFd);
    if (error.code === 'ENOENT' && !createScratch) return null;
    fsPathError(error, 'scratch cannot be resolved');
  }
}

function parseDocsPath(dataDir, relPath) {
  const rel = String(relPath || '').replace(/\\/g, '/').replace(/^\/+/, '');
  if (!rel || rel.includes('..') || !rel.endsWith('.md')) {
    fail('E_INVALID', 'path must be a .md file under docs/');
  }
  if (rel !== 'docs' && !rel.startsWith('docs/')) {
    fail('E_INVALID', 'path must be under docs/');
  }
  const root = resolve(projectRoot(dataDir));
  const lexicalAbs = resolve(root, rel);
  const norm = relative(root, lexicalAbs).replace(/\\/g, '/');
  if (!norm.startsWith('docs/') || norm.includes('..')) fail('E_INVALID', 'path escapes docs/');
  const parts = norm.slice('docs/'.length).split('/');
  if (!parts.length || parts.some(part => !part || part === '.' || part === '..'))
    fail('E_INVALID', 'path must be a .md file under docs/');
  return { abs: resolve(root, norm), rel: norm, parts };
}

function assertGeneralDocsWrite(rel) {
  if (rel === OWNER_GUIDANCE_PATH) {
    fail('E_OWNER_ONLY', `${OWNER_GUIDANCE_PATH} is owner-only; use the Guidance tab`);
  }
}

function openDocsContext(dataDir, createDocs) {
  const root = resolve(projectRoot(dataDir));
  const rootFd = openAbsoluteDirectory(root);
  try {
    const docsFd = createDocs ? ensureDirectoryAt(rootFd, 'docs') : openDirectoryAt(rootFd, 'docs');
    return { root, rootFd, docsFd, fds: [rootFd, docsFd] };
  } catch (error) {
    closeHeld(rootFd);
    if (error.code === 'ENOENT' && !createDocs) return null;
    fsPathError(error, 'docs path cannot be resolved');
  }
}

function docsParent(context, parts, create) {
  let parentFd = context.docsFd;
  for (const name of parts) {
    const child = create ? ensureDirectoryAt(parentFd, name) : openDirectoryAt(parentFd, name);
    context.fds.push(child);
    parentFd = child;
  }
  return parentFd;
}

function entryStatAt(parentFd, name) {
  guardHeldDirectory(parentFd);
  try { return lstatSync(rawFdPath(parentFd, name)); }
  catch (error) {
    if (error.code === 'ENOENT') return null;
    fsPathError(error, 'docs path cannot be resolved');
  } finally { guardHeldDirectory(parentFd); }
}

function isSafeRegular(stat) {
  return !!stat?.isFile() && stat.nlink === 1;
}

function requireSafeRegular(stat, message = 'docs path is not a single-link regular file') {
  if (!isSafeRegular(stat)) fail('E_INVALID', message);
}

function openFileAt(parentFd, name, message = 'docs file cannot be opened safely') {
  const before = entryStatAt(parentFd, name);
  if (before) requireSafeRegular(before, message);
  let fd;
  try { fd = openSync(rawFdPath(parentFd, name), READ_FLAGS); }
  catch (error) { fsPathError(error, message); }
  try {
    guardHeldDirectory(parentFd);
    const stat = fstatSync(fd);
    requireSafeRegular(stat, message);
    guardHeldDirectory(parentFd);
    return { fd, stat };
  } catch (error) {
    try { closeSync(fd); } catch { /* best effort */ }
    throw error;
  }
}

function readEntryAt(parentFd, name, { encoding = 'utf8', optional = false,
  message = 'docs file cannot be opened safely' } = {}) {
  let opened;
  try { opened = openFileAt(parentFd, name, message); }
  catch (error) {
    if (error.code === 'ENOENT' && optional) return null;
    throw error;
  }
  try {
    const data = readOpenedFile(parentFd, opened.fd, encoding);
    return { data, stat: opened.stat };
  } finally {
    closeSync(opened.fd);
  }
}

function readDirectoryAt(dirFd) {
  guardHeldDirectory(dirFd);
  try { return readdirSync(rawFdPath(dirFd)).sort(); }
  finally { guardHeldDirectory(dirFd); }
}

function readOpenedFile(parentFd, fd, encoding = 'utf8') {
  guardHeldDirectory(parentFd);
  try {
    const data = encoding == null ? readFileSync(fd) : readFileSync(fd, encoding);
    requireSafeRegular(fstatSync(fd));
    return data;
  } finally { guardHeldDirectory(parentFd); }
}

function unlinkAt(parentFd, name) {
  guardHeldDirectory(parentFd);
  try { unlinkSync(rawFdPath(parentFd, name)); }
  finally { guardHeldDirectory(parentFd); }
}

function unlinkIfIdentityAt(parentFd, name, expected) {
  const current = entryStatAt(parentFd, name);
  if (current && sameIdentity(current, expected)) unlinkAt(parentFd, name);
}

// If an attacker swaps the temporary name after close and before rename, the
// post-rename check must remove the unexpected destination entry as well as
// reporting the failure. Never remove a different entry later installed at the
// temporary name.
function removeUnexpectedEntryAt(parentFd, name, expected) {
  const current = entryStatAt(parentFd, name);
  if (!current || (sameIdentity(current, expected) && isSafeRegular(current))) return;
  try {
    if (current.isDirectory()) {
      guardHeldDirectory(parentFd);
      try { rmdirSync(rawFdPath(parentFd, name)); }
      finally { guardHeldDirectory(parentFd); }
    } else unlinkAt(parentFd, name);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
}

function renameAt(sourceFd, sourceName, destinationFd, destinationName) {
  guardHeldDirectory(sourceFd);
  guardHeldDirectory(destinationFd);
  try { renameSync(rawFdPath(sourceFd, sourceName), rawFdPath(destinationFd, destinationName)); }
  finally {
    guardHeldDirectory(sourceFd);
    guardHeldDirectory(destinationFd);
  }
}

function writeNewAt(parentFd, name, data) {
  const existing = entryStatAt(parentFd, name);
  if (existing) requireSafeRegular(existing);
  let fd;
  try { fd = openSync(rawFdPath(parentFd, name), WRITE_FLAGS, 0o644); }
  catch (error) {
    if (error.code === 'EEXIST') throw error;
    fsPathError(error, 'docs file cannot be created safely');
  }
  try {
    guardHeldDirectory(parentFd);
    requireSafeRegular(fstatSync(fd));
    writeFileSync(fd, data);
    guardHeldDirectory(parentFd);
    requireSafeRegular(fstatSync(fd));
    guardHeldDirectory(parentFd);
    fsyncSync(fd);
    guardHeldDirectory(parentFd);
  } finally { closeSync(fd); }
}

function atomicWriteAt(parentFd, name, data) {
  let temp;
  let fd;
  for (let attempt = 0; attempt < 64; attempt++) {
    temp = `.${name}.tmp-${randomBytes(16).toString('hex')}`;
    try {
      guardHeldDirectory(parentFd);
      fd = openSync(rawFdPath(parentFd, temp), WRITE_FLAGS, 0o600);
      guardHeldDirectory(parentFd);
      break;
    } catch (error) {
      if (fd !== undefined) {
        try { closeSync(fd); } catch { /* best effort */ }
        fd = undefined;
      }
      if (error.code === 'EEXIST') continue;
      try { unlinkAt(parentFd, temp); } catch { /* best effort */ }
      fsPathError(error, 'docs temporary file cannot be created safely');
    }
  }
  if (fd === undefined) fail('E_INVALID', 'docs temporary file name collision limit reached');
  let tempStat;
  let renamed = false;
  try {
    tempStat = fstatSync(fd);
    requireSafeRegular(tempStat);
    writeFileSync(fd, data);
    guardHeldDirectory(parentFd);
    tempStat = fstatSync(fd);
    requireSafeRegular(tempStat);
    guardHeldDirectory(parentFd);
    fsyncSync(fd);
    guardHeldDirectory(parentFd);
  } catch (error) {
    try { closeSync(fd); } catch { /* best effort */ }
    if (tempStat) try { unlinkIfIdentityAt(parentFd, temp, tempStat); } catch { /* best effort */ }
    throw error;
  }
  closeSync(fd);
  try {
    const existing = entryStatAt(parentFd, name);
    if (existing) requireSafeRegular(existing, 'docs destination is not a single-link regular file');
    guardHeldDirectory(parentFd);
    renameAt(parentFd, temp, parentFd, name);
    renamed = true;
    fsyncSync(parentFd);
    guardHeldDirectory(parentFd);
    const replaced = entryStatAt(parentFd, name);
    requireSafeRegular(replaced, 'docs destination changed to an unsafe file');
    if (!sameIdentity(replaced, tempStat)) fail('E_INVALID', 'docs destination changed during replace');
  } catch (error) {
    if (renamed) {
      try { removeUnexpectedEntryAt(parentFd, name, tempStat); } catch { /* best effort */ }
    } else {
      try { unlinkIfIdentityAt(parentFd, temp, tempStat); } catch { /* best effort */ }
    }
    fsPathError(error, 'docs file cannot be replaced safely');
  }
}

function openDocTarget(dataDir, relPath, createParents = false) {
  const parsed = parseDocsPath(dataDir, relPath);
  const context = openDocsContext(dataDir, createParents);
  if (!context) return null;
  try {
    const parentFd = docsParent(context, parsed.parts.slice(0, -1), createParents);
    const opened = openFileAt(parentFd, parsed.parts.at(-1));
    context.fds.push(opened.fd);
    return { ...context, parentFd, leaf: parsed.parts.at(-1), rel: parsed.rel, fd: opened.fd, stat: opened.stat };
  } catch (error) {
    closeContext(context);
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

/** Resolve a project-relative docs/*.md path; rejects escapes and non-md. */
export function resolveDocsPath(dataDir, relPath) {
  const { abs, rel } = parseDocsPath(dataDir, relPath);
  return { abs, rel };
}

function ensureScratchPad(context) {
  if (readEntryAt(context.scratchFd, SCRATCH_FILE, { optional: true, message: 'scratch pad cannot be opened safely' })) return;
  // Legacy single-file scratch at dataDir/owner-scratch.md.
  const legacy = readEntryAt(context.dataFd, 'owner-scratch.md', {
    optional: true, message: 'legacy scratch cannot be opened safely',
  });
  try {
    writeNewAt(context.scratchFd, SCRATCH_FILE,
      serializeScratch('Owner scratch', legacy?.data || ''));
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    // A concurrent creator won. Re-open with O_NOFOLLOW below so a raced
    // symlink is rejected instead of followed.
  }
  readEntryAt(context.scratchFd, SCRATCH_FILE, { message: 'scratch pad cannot be opened safely' });
}

export function showScratchPad(dataDir) {
  const context = openDataContext(dataDir, true);
  try {
    ensureScratchPad(context);
    const entry = readEntryAt(context.scratchFd, SCRATCH_FILE, { message: 'scratch pad cannot be opened safely' });
    const { title, body } = parseFront(entry.data);
    return {
      kind: 'scratch',
      path: `scratch/${SCRATCH_ID}.md`,
      id: SCRATCH_ID,
      title: title || 'Owner scratch',
      body,
      updated: entry.stat.mtime.toISOString(),
      bytes: entry.stat.size,
    };
  } finally {
    closeContext(context);
  }
}

export function updateScratchPad(dataDir, patch = {}) {
  const context = openDataContext(dataDir, true);
  try {
    ensureScratchPad(context);
    const current = readEntryAt(context.scratchFd, SCRATCH_FILE, { message: 'scratch pad cannot be opened safely' });
    const { title: currentTitle, body: currentBody } = parseFront(current.data);
    const title = patch.title !== undefined ? patch.title : (currentTitle || 'Owner scratch');
    const body = patch.body !== undefined ? patch.body : currentBody;
    atomicWriteAt(context.scratchFd, SCRATCH_FILE, serializeScratch(title, body));
  } finally {
    closeContext(context);
  }
  return showScratchPad(dataDir);
}

function walkMd(dirFd, prefix, out) {
  for (const name of readDirectoryAt(dirFd)) {
    if (name.startsWith('.')) continue;
    const rel = `${prefix}/${name}`.replace(/\\/g, '/');
    try {
      const st = entryStatAt(dirFd, name);
      if (!st) continue;
      if (st.isSymbolicLink()) continue;
      if (st.isDirectory()) {
        const childFd = openDirectoryAt(dirFd, name);
        try { walkMd(childFd, rel, out); }
        finally { closeHeld(childFd); }
      }
      else if (name.endsWith('.md')) {
        let title = basename(name, '.md');
        try {
          const entry = readEntryAt(dirFd, name);
          const { title: front, body } = parseFront(entry.data);
          title = front || titleFromBody(body, title);
          out.push({ path: rel, title, updated: entry.stat.mtime.toISOString(), bytes: entry.stat.size });
        } catch { /* skip raced or unreadable files */ }
      }
    } catch { /* skip */ }
  }
}

function topDirOf(rel) {
  const rest = rel.replace(/^docs\//, '');
  return rest.includes('/') ? rest.slice(0, rest.indexOf('/')) : null;
}

function sectionForRel(rel) {
  // rel like docs/proposals/foo.md or docs/first-hour.md
  const top = topDirOf(rel);
  if (!top) return 'other';
  if (HIDDEN_TOP_DIRS.has(top)) return null;
  if (SECTION_ALIASES[top]) return SECTION_ALIASES[top];
  if (KNOWN_DIRS.has(top)) {
    const sec = SECTIONS.find(s => s.dir === `docs/${top}`);
    return sec?.id || 'other';
  }
  return 'other';
}

export function listDocs(dataDir) {
  migrateScratchReports(dataDir);
  const scratch = showScratchPad(dataDir);
  const context = openDocsContext(dataDir, false);
  const files = [];
  if (context) {
    try { walkMd(context.docsFd, 'docs', files); }
    finally { closeContext(context); }
  }

  const bySection = Object.fromEntries([
    ...SECTIONS.map(s => [s.id, []]),
    ['other', []],
  ]);
  for (const f of files) {
    if (f.path === OWNER_GUIDANCE_PATH) continue; // dedicated owner-only Guidance tab
    const sec = sectionForRel(f.path);
    if (!sec) continue; // archived — hidden from UI and counts
    bySection[sec].push(f);
  }
  for (const k of Object.keys(bySection)) {
    bySection[k].sort((a, b) => b.updated.localeCompare(a.updated) || a.path.localeCompare(b.path));
  }

  const sections = [
    ...SECTIONS.map(s => ({ id: s.id, label: s.label, files: bySection[s.id] })),
    { id: 'other', label: 'Other', files: bySection.other },
  ];
  return { scratch, sections };
}

export function showDoc(dataDir, relPath) {
  const target = openDocTarget(dataDir, relPath);
  if (!target) {
    const { rel } = resolveDocsPath(dataDir, relPath);
    fail('E_NOT_FOUND', `no file ${rel}`);
  }
  try {
    const body = readOpenedFile(target.parentFd, target.fd, 'utf8');
    const { title: front } = parseFront(body);
    return {
      kind: 'doc',
      path: target.rel,
      title: front || titleFromBody(body, basename(target.rel, '.md')),
      body,
      updated: target.stat.mtime.toISOString(),
      bytes: target.stat.size,
    };
  } finally {
    closeContext(target);
  }
}

function createDoc(dataDir, rel, text) {
  const parsed = parseDocsPath(dataDir, rel);
  const context = openDocsContext(dataDir, true);
  try {
    const parentFd = docsParent(context, parsed.parts.slice(0, -1), true);
    const leaf = parsed.parts.at(-1);
    const existing = entryStatAt(parentFd, leaf);
    if (existing) {
      requireSafeRegular(existing, 'docs path is not a single-link regular file');
      return false;
    }
    try { writeNewAt(parentFd, leaf, text); }
    catch (error) {
      if (error.code !== 'EEXIST') throw error;
      const raced = entryStatAt(parentFd, leaf);
      if (raced && !isSafeRegular(raced))
        fail('E_INVALID', 'docs path changed to an unsafe file');
      return false;
    }
    return true;
  } finally {
    closeContext(context);
  }
}

export function addDoc(dataDir, { section, title, body = '', path: wantPath, id } = {}) {
  migrateScratchReports(dataDir);
  let rel;
  let generated = false;
  let generatedDir;
  let generatedSlug;
  if (wantPath) {
    ({ rel } = resolveDocsPath(dataDir, wantPath));
  } else {
    const sec = SECTIONS.find(s => s.id === section);
    if (!sec) fail('E_INVALID', `section must be one of: ${SECTIONS.map(s => s.id).join(', ')}`);
    generated = true;
    generatedDir = sec.dir;
    generatedSlug = id ? String(id) : slugify(title);
    if (!SLUG_RE.test(generatedSlug)) fail('E_INVALID', `bad file id "${generatedSlug}"`);
    rel = `${generatedDir}/${generatedSlug}.md`;
  }
  const norm = resolveDocsPath(dataDir, rel).rel;
  assertGeneralDocsWrite(norm);
  const text = String(body ?? '').replace(/\r\n/g, '\n');
  const withTitle = title && !text.startsWith('#')
    ? `# ${title}\n\n${text.endsWith('\n') || !text ? text : text + '\n'}`
    : (text.endsWith('\n') || !text ? text : text + '\n');
  const content = withTitle || (title ? `# ${title}\n` : '');
  let candidate = norm;
  let n = 2;
  for (;;) {
    if (createDoc(dataDir, candidate, content)) break;
    if (wantPath) fail('E_EXISTS', `${candidate} already exists`);
    if (!generated) fail('E_INVALID', 'docs path generation failed');
    candidate = `${generatedDir}/${generatedSlug}-${n}.md`;
    n++;
  }
  return showDoc(dataDir, candidate);
}

export function updateDoc(dataDir, relPath, patch = {}, { ownerGuidance = false } = {}) {
  const parsed = resolveDocsPath(dataDir, relPath);
  if (!ownerGuidance) assertGeneralDocsWrite(parsed.rel);
  const target = openDocTarget(dataDir, relPath);
  if (!target) {
    const { rel } = resolveDocsPath(dataDir, relPath);
    fail('E_NOT_FOUND', `no file ${rel}`);
  }
  let rel;
  try {
    const current = readOpenedFile(target.parentFd, target.fd, 'utf8');
    const body = patch.body !== undefined ? patch.body : current;
    const currentStat = entryStatAt(target.parentFd, target.leaf);
    if (!currentStat || !isSafeRegular(currentStat) || currentStat.dev !== target.stat.dev || currentStat.ino !== target.stat.ino)
      fail('E_INVALID', 'docs file changed during update');
    rel = target.rel;
    const text = String(body ?? '').replace(/\r\n/g, '\n');
    atomicWriteAt(target.parentFd, target.leaf, text.endsWith('\n') || !text ? text : text + '\n');
  } finally {
    closeContext(target);
  }
  return showDoc(dataDir, rel);
}

export function showOwnerGuidance(dataDir) {
  return showDoc(dataDir, OWNER_GUIDANCE_PATH);
}

export function updateOwnerGuidance(dataDir, patch = {}) {
  return updateDoc(dataDir, OWNER_GUIDANCE_PATH, patch, { ownerGuidance: true });
}

export function deleteDoc(dataDir, relPath) {
  const parsed = parseDocsPath(dataDir, relPath);
  assertGeneralDocsWrite(parsed.rel);
  if (topDirOf(parsed.rel) === 'archive')
    fail('E_INVALID', 'delete archived files from disk outside Tower, or restore then delete');
  const target = openDocTarget(dataDir, relPath);
  if (!target) {
    fail('E_NOT_FOUND', `no file ${parsed.rel}`);
  }
  try {
    const current = entryStatAt(target.parentFd, target.leaf);
    if (!current || !isSafeRegular(current) || current.dev !== target.stat.dev || current.ino !== target.stat.ino)
      fail('E_INVALID', 'docs file changed during delete');
    try { unlinkAt(target.parentFd, target.leaf); }
    catch (error) {
      if (error.code === 'ENOENT') fail('E_NOT_FOUND', `no file ${target.rel}`);
      throw error;
    }
    return { ok: true, path: target.rel };
  } finally {
    closeContext(target);
  }
}

/**
 * Move a live docs/*.md file into docs/archive/. Hidden from the Docs UI.
 * Refuses paths already under archive/.
 */
export function archiveDoc(dataDir, relPath) {
  const parsed = parseDocsPath(dataDir, relPath);
  const { rel } = parsed;
  assertGeneralDocsWrite(rel);
  const top = topDirOf(rel);
  if (top === 'archive') fail('E_INVALID', `${rel} is already archived`);
  if (top === 'spec') fail('E_INVALID', 'spec files are binding — do not archive; amend the spec or open a ballot');
  const context = openDocsContext(dataDir, false);
  if (!context) fail('E_NOT_FOUND', `no file ${rel}`);
  const name = basename(rel);
  try {
    const sourceParent = docsParent(context, parsed.parts.slice(0, -1), false);
    const source = openFileAt(sourceParent, parsed.parts.at(-1));
    context.fds.push(source.fd);
    const archiveFd = docsParent(context, ['archive'], true);
    const stem = name.replace(/\.md$/, '');
    let destLeaf = name;
    let n = 2;
    for (;;) {
      const existing = entryStatAt(archiveFd, destLeaf);
      if (!existing) break;
      requireSafeRegular(existing, 'archive destination is not a single-link regular file');
      destLeaf = `${stem}-${n}.md`;
      n++;
    }
    const current = entryStatAt(sourceParent, parsed.parts.at(-1));
    if (!current || !isSafeRegular(current) || current.dev !== source.stat.dev || current.ino !== source.stat.ino)
      fail('E_INVALID', 'docs file changed during archive');
    renameAt(sourceParent, parsed.parts.at(-1), archiveFd, destLeaf);
    const moved = openFileAt(archiveFd, destLeaf);
    try {
      if (moved.stat.dev !== source.stat.dev || moved.stat.ino !== source.stat.ino)
        fail('E_INVALID', 'archive destination changed during move');
    } finally {
      closeSync(moved.fd);
    }
    return { ok: true, from: rel, path: `docs/archive/${destLeaf}` };
  } catch (error) {
    if (error.code === 'ENOENT') fail('E_NOT_FOUND', `no file ${rel}`);
    throw error;
  } finally {
    closeContext(context);
  }
}

/** Classify a legacy scratch filename into audits vs research. */
export function classifyScratchReport(filename) {
  const base = basename(filename, '.md').toLowerCase();
  if (base === SCRATCH_ID || base === 'owner-scratch') return null;
  if (/research|lessons-learned/.test(base)) return 'research';
  if (/audit|persona|mission|field|surface|spec-compliance|garbage|cleanup/.test(base)) return 'audits';
  if (/^surface-research/.test(base)) return 'research';
  return 'audits'; // default leftover reports → audits
}

/**
 * One-shot: move .tower/scratch/*.md reports into docs/audits|research.
 * Leaves owner-scratch.md in place. Idempotent.
 */
export function migrateScratchReports(dataDir) {
  const scratch = openDataContext(dataDir, false);
  if (!scratch) return [];
  let context;
  const moved = [];
  try {
    context = openDocsContext(dataDir, true);
    const destinationDirs = new Map();
    for (const id of ['audits', 'research']) {
      const sec = SECTIONS.find(s => s.id === id);
      destinationDirs.set(id, docsParent(context, [sec.dir.slice('docs/'.length)], true));
    }
    for (const name of readDirectoryAt(scratch.scratchFd)) {
      if (!name.endsWith('.md')) continue;
      const section = classifyScratchReport(name);
      if (!section) continue;
      const sourceEntry = entryStatAt(scratch.scratchFd, name);
      if (!sourceEntry || !isSafeRegular(sourceEntry)) continue;
      const source = readEntryAt(scratch.scratchFd, name, {
        encoding: null, optional: true, message: 'scratch report cannot be opened safely',
      });
      if (!source) continue;
      const destinationFd = destinationDirs.get(section);
      const existing = entryStatAt(destinationFd, name);
      let copied = false;
      if (existing) {
        requireSafeRegular(existing, 'scratch migration destination is unsafe');
      } else {
        try { writeNewAt(destinationFd, name, source.data); }
        catch (error) {
          if (error.code !== 'EEXIST') throw error;
          const raced = entryStatAt(destinationFd, name);
          if (raced && !isSafeRegular(raced))
            fail('E_INVALID', 'scratch migration destination changed to an unsafe file');
        }
        copied = true;
      }
      const current = entryStatAt(scratch.scratchFd, name);
      if (!current) continue;
      if (!isSafeRegular(current) || current.dev !== source.stat.dev || current.ino !== source.stat.ino)
        fail('E_INVALID', 'scratch report changed during migration');
      unlinkAt(scratch.scratchFd, name);
      if (copied) moved.push({ from: name, to: `${SECTIONS.find(s => s.id === section).dir}/${name}` });
    }
  } finally {
    if (context) closeContext(context);
    closeContext(scratch);
  }
  return moved;
}

/** Seed scratchpad from legacy dataDir/owner-scratch.md if needed. */
export function migrateOwnerScratch(dataDir) {
  const context = openDataContext(dataDir, true);
  try { ensureScratchPad(context); }
  finally { closeContext(context); }
  return showScratchPad(dataDir);
}
