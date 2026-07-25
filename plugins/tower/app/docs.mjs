// Docs tab — durable markdown under project docs/ plus a pinned owner scratchpad.
// Scratchpad: <dataDir>/scratch/owner-scratch.md
// Everything else: <project>/docs/**/*.md (no .json, no skills).
import {
  existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync,
  unlinkSync, renameSync, statSync, copyFileSync,
} from 'node:fs';
import { join, basename, dirname, resolve, relative, sep } from 'node:path';
import { projectRoot as resolveProjectRoot } from './paths.mjs';
import { TowerError } from './store.mjs';

const fail = (code, msg) => { throw new TowerError(code, msg); };

export const SCRATCH_ID = 'owner-scratch';
export const SECTIONS = [
  { id: 'spec', label: 'Spec', dir: 'docs/spec' },
  { id: 'proposals', label: 'Proposals', dir: 'docs/proposals' },
  { id: 'plans', label: 'Plans', dir: 'docs/plans' },
  { id: 'research', label: 'Research', dir: 'docs/research' },
  { id: 'audits', label: 'Audits', dir: 'docs/audits' },
  { id: 'references', label: 'References', dir: 'docs/reference' },
];
/** Top-level docs/ dirs that never appear in the Docs UI or counts. */
export const HIDDEN_TOP_DIRS = new Set(['archive', 'ballots']);
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
export const scratchPadPath = (dataDir) => join(scratchDir(dataDir), `${SCRATCH_ID}.md`);

function projectRoot(dataDir) {
  return resolveProjectRoot(dataDir);
}

function docsRoot(dataDir) {
  return join(projectRoot(dataDir), 'docs');
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

function atomicWrite(abs, text) {
  mkdirSync(dirname(abs), { recursive: true });
  const tmp = `${abs}.tmp.${process.pid}`;
  writeFileSync(tmp, text);
  renameSync(tmp, abs);
}

/** Resolve a project-relative docs/*.md path; rejects escapes and non-md. */
export function resolveDocsPath(dataDir, relPath) {
  const rel = String(relPath || '').replace(/\\/g, '/').replace(/^\/+/, '');
  if (!rel || rel.includes('..') || !rel.endsWith('.md')) {
    fail('E_INVALID', 'path must be a .md file under docs/');
  }
  if (rel !== 'docs' && !rel.startsWith('docs/')) {
    fail('E_INVALID', 'path must be under docs/');
  }
  const root = resolve(projectRoot(dataDir));
  const abs = resolve(root, rel);
  if (abs !== root && !abs.startsWith(root + sep)) fail('E_INVALID', 'path escapes project root');
  const docsAbs = resolve(docsRoot(dataDir));
  if (abs !== docsAbs && !abs.startsWith(docsAbs + sep)) fail('E_INVALID', 'path must be under docs/');
  // Re-check after resolve (symlink / .. normalization).
  const norm = relative(root, abs).replace(/\\/g, '/');
  if (!norm.startsWith('docs/') || norm.includes('..')) fail('E_INVALID', 'path escapes docs/');
  return { abs, rel: norm };
}

function ensureScratchPad(dataDir) {
  mkdirSync(scratchDir(dataDir), { recursive: true });
  const p = scratchPadPath(dataDir);
  if (!existsSync(p)) {
    // Legacy single-file scratch at dataDir/owner-scratch.md
    const legacy = join(dataDir, 'owner-scratch.md');
    if (existsSync(legacy)) {
      atomicWrite(p, serializeScratch('Owner scratch', readFileSync(legacy, 'utf8')));
    } else {
      atomicWrite(p, serializeScratch('Owner scratch', ''));
    }
  }
  return p;
}

export function showScratchPad(dataDir) {
  const p = ensureScratchPad(dataDir);
  const raw = readFileSync(p, 'utf8');
  const { title, body } = parseFront(raw);
  const st = statSync(p);
  return {
    kind: 'scratch',
    path: `scratch/${SCRATCH_ID}.md`,
    id: SCRATCH_ID,
    title: title || 'Owner scratch',
    body,
    updated: st.mtime.toISOString(),
    bytes: st.size,
  };
}

export function updateScratchPad(dataDir, patch = {}) {
  const cur = showScratchPad(dataDir);
  const title = patch.title !== undefined ? patch.title : cur.title;
  const body = patch.body !== undefined ? patch.body : cur.body;
  atomicWrite(scratchPadPath(dataDir), serializeScratch(title, body));
  return showScratchPad(dataDir);
}

function walkMd(absDir, prefix, out) {
  if (!existsSync(absDir)) return;
  for (const name of readdirSync(absDir).sort()) {
    if (name.startsWith('.')) continue;
    const p = join(absDir, name);
    const rel = `${prefix}/${name}`.replace(/\\/g, '/');
    try {
      const st = statSync(p);
      if (st.isDirectory()) walkMd(p, rel, out);
      else if (name.endsWith('.md')) {
        let title = basename(name, '.md');
        try {
          const raw = readFileSync(p, 'utf8');
          const { title: front, body } = parseFront(raw);
          title = front || titleFromBody(body, title);
        } catch { /* keep stem */ }
        out.push({ path: rel, title, updated: st.mtime.toISOString(), bytes: st.size });
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
  ensureScratchPad(dataDir);
  const root = docsRoot(dataDir);
  const files = [];
  if (existsSync(root)) walkMd(root, 'docs', files);

  const bySection = Object.fromEntries([
    ...SECTIONS.map(s => [s.id, []]),
    ['other', []],
  ]);
  for (const f of files) {
    const sec = sectionForRel(f.path);
    if (!sec) continue; // archived / ballots — hidden from UI and counts
    bySection[sec].push(f);
  }
  for (const k of Object.keys(bySection)) {
    bySection[k].sort((a, b) => b.updated.localeCompare(a.updated) || a.path.localeCompare(b.path));
  }

  const sections = [
    ...SECTIONS.map(s => ({ id: s.id, label: s.label, files: bySection[s.id] })),
    { id: 'other', label: 'Other', files: bySection.other },
  ];
  return { scratch: showScratchPad(dataDir), sections };
}

export function showDoc(dataDir, relPath) {
  const { abs, rel } = resolveDocsPath(dataDir, relPath);
  if (!existsSync(abs)) fail('E_NOT_FOUND', `no file ${rel}`);
  const body = readFileSync(abs, 'utf8');
  const st = statSync(abs);
  const { title: front } = parseFront(body);
  return {
    kind: 'doc',
    path: rel,
    title: front || titleFromBody(body, basename(rel, '.md')),
    body,
    updated: st.mtime.toISOString(),
    bytes: st.size,
  };
}

export function addDoc(dataDir, { section, title, body = '', path: wantPath, id } = {}) {
  migrateScratchReports(dataDir);
  const root = projectRoot(dataDir);
  let rel;
  if (wantPath) {
    ({ rel } = resolveDocsPath(dataDir, wantPath));
  } else {
    const sec = SECTIONS.find(s => s.id === section);
    if (!sec) fail('E_INVALID', `section must be one of: ${SECTIONS.map(s => s.id).join(', ')}`);
    let slug = id ? String(id) : slugify(title);
    if (!SLUG_RE.test(slug)) fail('E_INVALID', `bad file id "${slug}"`);
    rel = `${sec.dir}/${slug}.md`;
    let n = 2;
    while (existsSync(join(root, rel))) {
      rel = `${sec.dir}/${slug}-${n}.md`;
      n++;
    }
  }
  const { abs, rel: norm } = resolveDocsPath(dataDir, rel);
  if (existsSync(abs)) fail('E_EXISTS', `${norm} already exists`);
  const text = String(body ?? '').replace(/\r\n/g, '\n');
  const withTitle = title && !text.startsWith('#')
    ? `# ${title}\n\n${text.endsWith('\n') || !text ? text : text + '\n'}`
    : (text.endsWith('\n') || !text ? text : text + '\n');
  atomicWrite(abs, withTitle || (title ? `# ${title}\n` : ''));
  return showDoc(dataDir, norm);
}

export function updateDoc(dataDir, relPath, patch = {}) {
  const cur = showDoc(dataDir, relPath);
  const body = patch.body !== undefined ? patch.body : cur.body;
  const { abs, rel } = resolveDocsPath(dataDir, relPath);
  const text = String(body ?? '').replace(/\r\n/g, '\n');
  atomicWrite(abs, text.endsWith('\n') || !text ? text : text + '\n');
  return showDoc(dataDir, rel);
}

export function deleteDoc(dataDir, relPath) {
  const { abs, rel } = resolveDocsPath(dataDir, relPath);
  if (!existsSync(abs)) fail('E_NOT_FOUND', `no file ${rel}`);
  if (topDirOf(rel) === 'archive') fail('E_INVALID', 'delete archived files from disk outside Tower, or restore then delete');
  unlinkSync(abs);
  return { ok: true, path: rel };
}

/**
 * Move a live docs/*.md file into docs/archive/. Hidden from the Docs UI.
 * Refuses paths already under archive/.
 */
export function archiveDoc(dataDir, relPath) {
  const { abs, rel } = resolveDocsPath(dataDir, relPath);
  if (!existsSync(abs)) fail('E_NOT_FOUND', `no file ${rel}`);
  const top = topDirOf(rel);
  if (top === 'archive') fail('E_INVALID', `${rel} is already archived`);
  if (top === 'spec') fail('E_INVALID', 'spec files are binding — do not archive; amend the spec or open a ballot');
  const name = basename(rel);
  const root = projectRoot(dataDir);
  const destRel = `docs/archive/${name}`;
  let destAbs = join(root, destRel);
  let n = 2;
  while (existsSync(destAbs)) {
    const stem = name.replace(/\.md$/, '');
    destAbs = join(root, `docs/archive/${stem}-${n}.md`);
    n++;
  }
  const destNorm = relative(root, destAbs).replace(/\\/g, '/');
  mkdirSync(dirname(destAbs), { recursive: true });
  renameSync(abs, destAbs);
  return { ok: true, from: rel, path: destNorm };
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
  const dir = scratchDir(dataDir);
  if (!existsSync(dir)) return [];
  const root = projectRoot(dataDir);
  const moved = [];
  for (const name of readdirSync(dir)) {
    if (!name.endsWith('.md')) continue;
    const section = classifyScratchReport(name);
    if (!section) continue;
    const sec = SECTIONS.find(s => s.id === section);
    const destRel = `${sec.dir}/${name}`;
    const destAbs = join(root, destRel);
    const srcAbs = join(dir, name);
    mkdirSync(dirname(destAbs), { recursive: true });
    if (!existsSync(destAbs)) {
      copyFileSync(srcAbs, destAbs);
      moved.push({ from: name, to: destRel });
    }
    unlinkSync(srcAbs);
  }
  // Ensure audits dir exists even if empty (UI section).
  mkdirSync(join(root, 'docs', 'audits'), { recursive: true });
  return moved;
}

/** Seed scratchpad from legacy dataDir/owner-scratch.md if needed. */
export function migrateOwnerScratch(dataDir) {
  ensureScratchPad(dataDir);
  return showScratchPad(dataDir);
}
