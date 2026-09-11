// Docs tab — durable markdown and self-contained HTML reports under project docs/ plus a pinned owner scratchpad.
// Scratchpad: <dataDir>/scratch/owner-scratch.md
// Docs stay under docs/. The owner editor has one separate, fixed root AGENTS.md target.
import {
  closeSync, constants, fchmodSync, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync,
  readlinkSync,
  readdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync, renameSync,
} from 'node:fs';
import { join, basename, isAbsolute, resolve, relative, sep } from 'node:path';
import { createHash, randomBytes } from 'node:crypto';
import { projectRoot as resolveProjectRoot } from './paths.mjs';
import { TowerError } from './store.mjs';

const fail = (code, msg) => { throw new TowerError(code, msg); };

export const SCRATCH_ID = 'owner-scratch';
const SCRATCH_FILE = `${SCRATCH_ID}.md`;
export const OWNER_GUIDANCE_PATH = 'AGENTS.md';

export const SECTIONS = [
  { id: 'spec', label: 'Spec', dir: 'docs/spec' },
  { id: 'audits', label: 'Audits', dir: 'docs/audits' },
  { id: 'research', label: 'Research', dir: 'docs/research' },
  { id: 'proposals', label: 'Proposals', dir: 'docs/proposals' },
];

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

const HTML_TITLE_ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ',
  ndash: '–', mdash: '—', hellip: '…', times: '×',
};

function titleFromHtml(body, fallback) {
  const raw = /<title\b[^>]*>([\s\S]*?)<\/title\s*>/i.exec(body || '')?.[1];
  if (!raw) return fallback;
  const title = raw
    .replace(/<[^>]*>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/&(#x[0-9a-f]+|#\d+|[a-z][a-z0-9]+);/gi, (match, entity) => {
      const lowerEntity = entity.toLowerCase();
      if (Object.hasOwn(HTML_TITLE_ENTITIES, lowerEntity))
        return HTML_TITLE_ENTITIES[lowerEntity];
      const code = lowerEntity.startsWith('#x')
        ? Number.parseInt(lowerEntity.slice(2), 16)
        : Number.parseInt(lowerEntity.slice(1), 10);
      return Number.isInteger(code) && code >= 0 && code <= 0x10ffff
        ? String.fromCodePoint(code)
        : match;
    })
    .trim();
  return title || fallback;
}

function documentFormat(name) {
  const lower = String(name).toLowerCase();
  if (lower.endsWith('.md')) return 'markdown';
  if (lower.endsWith('.html') || lower.endsWith('.htm')) return 'html';
  return null;
}

function filenameTitle(name) {
  return basename(name).replace(/\.(?:md|html?)$/i, '');
}

function documentPathError(allowHtml) {
  return allowHtml
    ? 'path must be a .md, .html, or .htm file under docs/'
    : 'path must be a .md file under docs/';
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

function parseDocsPath(dataDir, relPath, allowHtml = false) {
  const rel = String(relPath || '').replace(/\\/g, '/').replace(/^\/+/, '');
  const format = documentFormat(rel);
  if (!rel || rel.includes('..') || format !== 'markdown' && !(allowHtml && format === 'html')) {
    fail('E_INVALID', documentPathError(allowHtml));
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
    fail('E_INVALID', documentPathError(allowHtml));
  return { abs: resolve(root, norm), rel: norm, parts, format };
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

function fileTimes(stat) {
  const updated = stat.mtime.toISOString();
  const birthMs = stat.birthtimeMs;
  const created = Number.isFinite(birthMs) && birthMs > 0
    ? new Date(birthMs).toISOString()
    : updated;
  return { created, updated };
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

function atomicWriteAt(parentFd, name, data, { beforeReplace, mode = 0o600 } = {}) {
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
    fchmodSync(fd, mode);
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
    beforeReplace?.();
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

function openDocTarget(dataDir, relPath, createParents = false, allowHtml = false) {
  const parsed = parseDocsPath(dataDir, relPath, allowHtml);
  const context = openDocsContext(dataDir, createParents);
  if (!context) return null;
  try {
    const parentFd = docsParent(context, parsed.parts.slice(0, -1), createParents);
    const opened = openFileAt(parentFd, parsed.parts.at(-1));
    context.fds.push(opened.fd);
    return {
      ...context,
      parentFd,
      leaf: parsed.parts.at(-1),
      rel: parsed.rel,
      format: parsed.format,
      fd: opened.fd,
      stat: opened.stat,
    };
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
  try {
    writeNewAt(context.scratchFd, SCRATCH_FILE,
      serializeScratch('Owner scratch', ''));
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    // A concurrent creator won. Re-open with O_NOFOLLOW below so a raced
    // symlink is rejected instead of followed.
  }
  readEntryAt(context.scratchFd, SCRATCH_FILE, { message: 'scratch pad cannot be opened safely' });
}

function emptyScratchPad() {
  return {
    kind: 'scratch',
    path: `scratch/${SCRATCH_ID}.md`,
    id: SCRATCH_ID,
    title: 'Owner scratch',
    body: '',
    created: null,
    updated: null,
    bytes: 0,
  };
}


export function showScratchPad(dataDir) {
  const context = openDataContext(dataDir, false);
  if (!context) return emptyScratchPad();
  try {
    const entry = readEntryAt(context.scratchFd, SCRATCH_FILE, {
      optional: true, message: 'scratch pad cannot be opened safely',
    });
    if (!entry) return emptyScratchPad();
    const { title, body } = parseFront(entry.data);
    return {
      kind: 'scratch',
      path: `scratch/${SCRATCH_ID}.md`,
      id: SCRATCH_ID,
      title: title || 'Owner scratch',
      body,
      ...fileTimes(entry.stat),
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
      else {
        const format = documentFormat(name);
        if (!format) continue;
        let title = filenameTitle(name);
        try {
          const entry = readEntryAt(dirFd, name);
          if (format === 'html') {
            title = titleFromHtml(entry.data, title);
          } else {
            const { title: front, body } = parseFront(entry.data);
            title = front || titleFromBody(body, title);
          }
          out.push({ path: rel, title, format, ...fileTimes(entry.stat), bytes: entry.stat.size });
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
  const top = topDirOf(rel);
  const section = SECTIONS.find(s => s.dir === `docs/${top}`);
  return section?.id || null;
}

export function listDocs(dataDir) {
  const scratch = showScratchPad(dataDir);
  const context = openDocsContext(dataDir, false);
  const files = [];
  if (context) {
    try { walkMd(context.docsFd, 'docs', files); }
    finally { closeContext(context); }
  }

  const bySection = Object.fromEntries(SECTIONS.map(s => [s.id, []]));
  for (const f of files) {
    const sec = sectionForRel(f.path);
    if (!sec) continue;
    bySection[sec].push(f);
  }
  for (const k of Object.keys(bySection)) {
    bySection[k].sort((a, b) => b.created.localeCompare(a.created)
      || b.updated.localeCompare(a.updated)
      || a.path.localeCompare(b.path));
  }

  const sections = SECTIONS.map(s => ({ id: s.id, label: s.label, files: bySection[s.id] }));
  return { scratch, sections };
}

export function showDoc(dataDir, relPath) {
  const target = openDocTarget(dataDir, relPath, false, true);
  if (!target) {
    const { rel } = parseDocsPath(dataDir, relPath, true);
    fail('E_NOT_FOUND', `no file ${rel}`);
  }
  try {
    const body = readOpenedFile(target.parentFd, target.fd, 'utf8');
    const { title: front, body: markdownBody } = parseFront(body);
    const fallback = filenameTitle(target.rel);
    const title = target.format === 'html'
      ? titleFromHtml(body, fallback)
      : front || titleFromBody(markdownBody, fallback);
    return {
      kind: 'doc',
      format: target.format,
      path: target.rel,
      title,
      body,
      ...fileTimes(target.stat),
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
  let rel;
  let generated = false;
  let generatedDir;
  let generatedSlug;
  if (wantPath) {
    ({ rel } = resolveDocsPath(dataDir, wantPath));
    if (!sectionForRel(rel))
      fail('E_INVALID', `path must be under ${SECTIONS.map(s => s.dir).join(', ')}`);
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

export function updateDoc(dataDir, relPath, patch = {}) {
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

const guidanceRevision = body => createHash('sha256').update(body).digest('hex');

function readGuidanceAt(rootFd) {
  const entry = readEntryAt(rootFd, OWNER_GUIDANCE_PATH, {
    optional: true, message: 'AGENTS.md must be a single-link regular file',
  });
  if (!entry) fail('E_NOT_FOUND', 'no root AGENTS.md');
  return entry;
}

export function showOwnerGuidance(dataDir) {
  const rootFd = openAbsoluteDirectory(resolve(projectRoot(dataDir)));
  try {
    const { data: body, stat } = readGuidanceAt(rootFd);
    return {
      kind: 'doc',
      format: 'markdown',
      path: OWNER_GUIDANCE_PATH,
      title: titleFromBody(body, OWNER_GUIDANCE_PATH),
      body,
      revision: guidanceRevision(body),
      ...fileTimes(stat),
      bytes: stat.size,
    };
  } finally {
    closeHeld(rootFd);
  }
}

export function updateOwnerGuidance(dataDir, patch = {}) {
  if (typeof patch.body !== 'string') fail('E_INVALID', 'AGENTS.md body must be text');
  const rootFd = openAbsoluteDirectory(resolve(projectRoot(dataDir)));
  try {
    const checkRevision = () => {
      const current = readGuidanceAt(rootFd);
      if (patch.expectRev !== guidanceRevision(current.data))
        fail('E_CONFLICT', 'AGENTS.md changed or its loaded revision is missing; reload before saving');
      return current.stat;
    };
    const stat = checkRevision();
    atomicWriteAt(rootFd, OWNER_GUIDANCE_PATH, patch.body, {
      beforeReplace: checkRevision,
      mode: stat.mode & 0o777,
    });
  } finally {
    closeHeld(rootFd);
  }
  return showOwnerGuidance(dataDir);
}

export function deleteDoc(dataDir, relPath) {
  const parsed = parseDocsPath(dataDir, relPath);
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


