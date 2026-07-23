// Owner scratch pad — markdown notes living beside the board, never cards.
// Files sit in <dataDir>/scratch/*.md. Preview can also open project docs under
// an allowlist (proposals/plans/design/research) as read-only.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync, unlinkSync, renameSync, statSync } from 'node:fs';
import { join, basename, resolve, relative, extname, sep } from 'node:path';
import { projectRoot as resolveProjectRoot } from './paths.mjs';
import { TowerError } from './store.mjs';

const fail = (code, msg) => { throw new TowerError(code, msg); };

export const scratchDir = (dataDir) => join(dataDir, 'scratch');

// Project-relative roots the scratch tab may preview read-only.
export const PREVIEW_ROOTS = ['docs/proposals', 'docs/plans', 'docs/design', 'docs/sidequests', 'docs/reference', '.agents/skills'];

const SLUG_RE = /^[a-z0-9][a-z0-9._-]{0,79}$/i;

function ensureDir(dataDir) {
  const dir = scratchDir(dataDir);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function slugify(title) {
  const s = String(title || '').trim().toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
  return s || `note-${Date.now().toString(36)}`;
}

function notePath(dataDir, id) {
  if (!SLUG_RE.test(id) || id.includes('..') || id.includes(sep)) fail('E_INVALID', `bad scratch id "${id}"`);
  return join(scratchDir(dataDir), `${id}.md`);
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

function serialize(title, body) {
  const t = String(title || 'Untitled').trim() || 'Untitled';
  const b = String(body ?? '').replace(/\r\n/g, '\n');
  return `---\ntitle: ${t}\n---\n${b.endsWith('\n') || !b ? b : b + '\n'}`;
}

function projectRoot(dataDir) {
  return resolveProjectRoot(dataDir);
}

export function listScratch(dataDir) {
  const dir = ensureDir(dataDir);
  return readdirSync(dir).filter(f => f.endsWith('.md')).map(f => {
    const id = basename(f, '.md');
    const p = join(dir, f);
    const raw = readFileSync(p, 'utf8');
    const { title } = parseFront(raw);
    const st = statSync(p);
    return { id, title: title || id, updated: st.mtime.toISOString(), bytes: st.size };
  }).sort((a, b) => b.updated.localeCompare(a.updated));
}

export function showScratch(dataDir, id) {
  const p = notePath(dataDir, id);
  if (!existsSync(p)) fail('E_NOT_FOUND', `no scratch note ${id}`);
  const raw = readFileSync(p, 'utf8');
  const { title, body } = parseFront(raw);
  const st = statSync(p);
  return { id, title: title || id, body, updated: st.mtime.toISOString(), bytes: st.size };
}

export function addScratch(dataDir, { title, body = '', id } = {}) {
  ensureDir(dataDir);
  let slug = id ? String(id) : slugify(title);
  if (!SLUG_RE.test(slug)) fail('E_INVALID', `bad scratch id "${slug}"`);
  let p = notePath(dataDir, slug);
  if (existsSync(p)) {
    let n = 2;
    while (existsSync(notePath(dataDir, `${slug}-${n}`))) n++;
    slug = `${slug}-${n}`;
    p = notePath(dataDir, slug);
  }
  const text = serialize(title || slug, body);
  const tmp = `${p}.tmp.${process.pid}`;
  writeFileSync(tmp, text);
  renameSync(tmp, p);
  return showScratch(dataDir, slug);
}

export function updateScratch(dataDir, id, patch = {}) {
  const cur = showScratch(dataDir, id);
  const title = patch.title !== undefined ? patch.title : cur.title;
  const body = patch.body !== undefined ? patch.body : cur.body;
  const p = notePath(dataDir, id);
  const tmp = `${p}.tmp.${process.pid}`;
  writeFileSync(tmp, serialize(title, body));
  renameSync(tmp, p);
  return showScratch(dataDir, id);
}

export function deleteScratch(dataDir, id) {
  const p = notePath(dataDir, id);
  if (!existsSync(p)) fail('E_NOT_FOUND', `no scratch note ${id}`);
  unlinkSync(p);
  return { ok: true, id };
}

// Read-only preview of a project-relative markdown path under PREVIEW_ROOTS.
export function previewDoc(dataDir, relPath) {
  const rel = String(relPath || '').replace(/\\/g, '/').replace(/^\/+/, '');
  if (!rel || rel.includes('..') || !rel.endsWith('.md')) fail('E_INVALID', 'preview path must be a .md under an allowed docs root');
  const allowed = PREVIEW_ROOTS.some(root => rel === root || rel.startsWith(root + '/'));
  if (!allowed) fail('E_INVALID', `preview path must be under: ${PREVIEW_ROOTS.join(', ')}`);
  const abs = resolve(projectRoot(dataDir), rel);
  // Defend against symlink escape: resolved path must still sit under project.
  const root = resolve(projectRoot(dataDir));
  if (abs !== root && !abs.startsWith(root + sep)) fail('E_INVALID', 'path escapes project root');
  if (!existsSync(abs)) fail('E_NOT_FOUND', `no file ${rel}`);
  const body = readFileSync(abs, 'utf8');
  const st = statSync(abs);
  return { path: rel, title: basename(rel, '.md'), body, updated: st.mtime.toISOString(), bytes: st.size, readonly: true };
}

export function listPreviewTree(dataDir) {
  const root = projectRoot(dataDir);
  const out = [];
  for (const base of PREVIEW_ROOTS) {
    const abs = join(root, base);
    if (!existsSync(abs)) continue;
    const walk = (dir, prefix) => {
      for (const name of readdirSync(dir).sort()) {
        const p = join(dir, name);
        const rel = join(prefix, name).replace(/\\/g, '/');
        try {
          const st = statSync(p);
          if (st.isDirectory()) walk(p, rel);
          else if (name.endsWith('.md')) out.push({ path: rel, title: basename(name, '.md'), updated: st.mtime.toISOString(), bytes: st.size });
        } catch { /* skip unreadable */ }
      }
    };
    walk(abs, base);
  }
  return out;
}

// Seed from a legacy single-file owner scratch if present and scratch/ empty.
export function migrateOwnerScratch(dataDir) {
  const legacy = join(dataDir, 'owner-scratch.md');
  if (!existsSync(legacy)) return null;
  if (listScratch(dataDir).length) return null;
  const body = readFileSync(legacy, 'utf8');
  return addScratch(dataDir, { id: 'owner-scratch', title: 'Owner scratch', body });
}
