// Per-project configuration: terminology, taxonomies, server defaults.
// Lives at <dataDir>/config.json in the HOST project; everything optional.
import { randomBytes } from 'node:crypto';
import {
  closeSync, constants, fstatSync, fsyncSync, lstatSync, openSync,
  readFileSync, renameSync, unlinkSync, writeFileSync,
} from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { readJSON, writeJSON, configFile, secretsFile } from './paths.mjs';

export class ConfigError extends Error {
  constructor(message) { super(message); this.code = 'E_SECRET_CONFIG'; }
}

const SECRET_KEYS = ['auth'];
// Tracked config must never hold these (legacy push + auth). Push itself is
// gone (D-VERDICT-460-1); rejection stays so old committed shapes fail closed.
const TRACKED_SECRET_KEYS = ['auth', 'push'];
const hasOwn = (value, key) => Object.prototype.hasOwnProperty.call(value, key);
const noFollow = constants.O_NOFOLLOW || 0;

function validateSecretFile(stat, file) {
  if (!stat.isFile()) throw new ConfigError(`${file} must be a regular file, not a link or special file`);
  if ((stat.mode & 0o077) !== 0) throw new ConfigError(`${file} permissions are too broad; require mode 0600`);
  if (typeof process.getuid === 'function' && Number.isInteger(stat.uid) && stat.uid !== process.getuid())
    throw new ConfigError(`${file} must be owned by the current user`);
}

function loadSecrets(file) {
  let before;
  try { before = lstatSync(file); }
  catch (error) {
    if (error?.code === 'ENOENT') return {};
    throw new ConfigError(`cannot inspect ${file} safely`);
  }
  validateSecretFile(before, file);

  let fd;
  try {
    fd = openSync(file, constants.O_RDONLY | noFollow);
    const opened = fstatSync(fd);
    validateSecretFile(opened, file);
    if (before.dev !== opened.dev || before.ino !== opened.ino)
      throw new ConfigError(`${file} changed while Tower was opening it; refusing to read`);
    const text = readFileSync(fd, 'utf8');
    try { return JSON.parse(text); }
    catch { throw new ConfigError(`${file} is not valid JSON`); }
  } catch (error) {
    if (error instanceof ConfigError) throw error;
    throw new ConfigError(`cannot open ${file} safely`);
  } finally {
    if (fd !== undefined) try { closeSync(fd); } catch { /* best effort after read failure */ }
  }
}

function syncParent(file) {
  let fd;
  try {
    fd = openSync(dirname(file), constants.O_RDONLY | (constants.O_DIRECTORY || 0));
    fsyncSync(fd);
  } catch { /* directory fsync is unavailable on some platforms/filesystems */ }
  finally { if (fd !== undefined) try { closeSync(fd); } catch { /* best effort */ } }
}

function writeSecrets(file, value) {
  const payload = JSON.stringify(value, null, 2) + '\n';
  let fd;
  let temp;
  try {
    for (let attempt = 0; attempt < 64; attempt++) {
      temp = join(dirname(file), `.${basename(file)}.tmp-${randomBytes(16).toString('hex')}`);
      try {
        fd = openSync(temp, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | noFollow, 0o600);
        break;
      } catch (error) {
        temp = undefined;
        if (error?.code !== 'EEXIST' && error?.code !== 'ELOOP') throw error;
      }
    }
    if (fd === undefined) throw new Error('temporary-file collision limit reached');
    writeFileSync(fd, payload, 'utf8');
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    renameSync(temp, file);
    temp = undefined;
    syncParent(file);
  } catch {
    if (fd !== undefined) try { closeSync(fd); } catch { /* best effort */ }
    if (temp !== undefined) try { unlinkSync(temp); } catch { /* best effort */ }
    throw new ConfigError(`cannot write ${file} safely`);
  }
}

const rejectSecretKeys = (value, file) => {
  const found = TRACKED_SECRET_KEYS.filter(key => hasOwn(value, key));
  if (!found.length) return;
  throw new ConfigError(
    `${file} contains secret field${found.length > 1 ? 's' : ''} ${found.join(', ')}. ` +
    `Remove ${found.join('/')} from tracked config` +
    (found.includes('auth') ? ', rotate any credentials that were committed, and put auth.token in .tower/secrets.json' : '') +
    (found.includes('push') ? '. Web push/VAPID was removed — delete push entirely' : '') +
    `. Tower will not load tracked secrets.`
  );
};

export const DEFAULTS = {
  project: 'Project',                 // shown in the UI topbar + <title>
  // What the big grouping and its inner goals are called in the UI.
  terms: { epoch: 'Epoch', epochs: 'Epochs', milestone: 'Milestone', milestones: 'Milestones', sidequest: 'Sidequests', ideas: 'Ideas', owner: 'Owner', agent: 'Agent' },
  tracks: ['epoch', 'sidequest'],
  kinds: ['task', 'feature', 'idea', 'bug'],
  priorities: ['P0', 'P1', 'P2', 'P3'],
  decisionGroups: ['design', 'architecture', 'api', 'ui', 'tooling', 'process', 'research'],
  codeLanguage: '',                   // hint for ballot code blocks (highlighting)
  port: 7878,
  backups: 20,
  // #461: days a done card / ratified decision stays live before the retire
  // pass moves it to history.json. Buffer, not a deadline — lets the owner
  // walk back a fresh ratification before it's out of easy reach.
  retireAfterDays: 3,
};

// Persist a partial update into the user's config.json (creates it if absent).
export function saveConfig(dataDir, patch) {
  rejectSecretKeys(patch || {}, 'config update');
  const file = configFile(dataDir);
  const cur = readJSON(file, {}) || {};
  rejectSecretKeys(cur, '.tower/config.json');
  const next = { ...cur, ...patch };
  writeJSON(file, next);
  return next;
}

// Secrets have one persistence path. The whole file is untracked and mode
// 0600; callers may update auth without touching public config. Push/VAPID
// is retired — saveSecrets rejects `push` patches.
export function saveSecrets(dataDir, patch) {
  const unknown = Object.keys(patch || {}).filter(key => !SECRET_KEYS.includes(key));
  if (unknown.length) {
    const msg = unknown.includes('push')
      ? `.tower/secrets.json no longer accepts push (web push/VAPID removed); got ${unknown.join(', ')}`
      : `.tower/secrets.json accepts only auth; got ${unknown.join(', ')}`;
    throw new ConfigError(msg);
  }
  const file = secretsFile(dataDir);
  const cur = loadSecrets(file) || {};
  const { push: _dropPush, ...kept } = cur;
  const next = { ...kept, ...patch };
  delete next.push;
  writeSecrets(file, next);
  return next;
}

export function publicConfig(config) {
  if (!config) return config;
  const { auth: _auth, push: _push, ...publicFields } = config;
  return publicFields;
}

export function loadConfig(dataDir) {
  const user = readJSON(configFile(dataDir), {}) || {};
  rejectSecretKeys(user, '.tower/config.json');
  const secrets = loadSecrets(secretsFile(dataDir)) || {};
  // Ignore legacy push blobs left in secrets.json after feature removal.
  return {
    ...DEFAULTS,
    ...user,
    terms: { ...DEFAULTS.terms, ...(user.terms || {}) },
    auth: secrets.auth || null,
    push: null,
  };
}
