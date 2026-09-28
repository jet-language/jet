// Per-project configuration: terminology, taxonomies, server defaults.
// Lives at <dataDir>/config.json in the HOST project; everything optional.
import { readJSON, writeJSON, configFile } from './paths.mjs';

export class ConfigError extends Error {
  constructor(message) { super(message); this.code = 'E_SECRET_CONFIG'; }
}

// Tracked config must never hold these removed fields. Push itself is gone
// (D-VERDICT-460-1); rejection stays so old committed shapes fail closed.
const TRACKED_SECRET_KEYS = ['auth', 'push'];
const hasOwn = (value, key) => Object.prototype.hasOwnProperty.call(value, key);

const rejectSecretKeys = (value, file) => {
  const found = TRACKED_SECRET_KEYS.filter(key => hasOwn(value, key));
  if (!found.length) return;
  throw new ConfigError(
    `${file} contains removed field${found.length > 1 ? 's' : ''} ${found.join(', ')}. ` +
    `Remove ${found.join('/')} from tracked config. ` +
    'Tower does not load auth or push configuration.'
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

export function publicConfig(config) {
  if (!config) return config;
  const { auth: _auth, push: _push, ...publicFields } = config;
  return publicFields;
}

export function loadConfig(dataDir) {
  const user = readJSON(configFile(dataDir), {}) || {};
  rejectSecretKeys(user, '.tower/config.json');
  return {
    ...DEFAULTS,
    ...user,
    terms: { ...DEFAULTS.terms, ...(user.terms || {}) },
  };
}
