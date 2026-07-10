// Per-project configuration: terminology, taxonomies, server defaults.
// Lives at <dataDir>/config.json in the HOST project; everything optional.
import { readJSON, writeJSON, configFile } from './paths.mjs';

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
  // Auth token for non-localhost requests (LAN/tailnet). Auto-generated on
  // first `tower serve`; localhost is always exempt so local CLIs just work.
  auth: null,                         // { token: "…" }
  // Web push (auto-generated VAPID keys) + subscriptions live here too.
  push: null,                         // { publicKey, privateKey, subscriptions: [] }
};

// Persist a partial update into the user's config.json (creates it if absent).
export function saveConfig(dataDir, patch) {
  const file = configFile(dataDir);
  const cur = readJSON(file, {}) || {};
  const next = { ...cur, ...patch };
  writeJSON(file, next);
  return next;
}

export function loadConfig(dataDir) {
  const user = readJSON(configFile(dataDir), {}) || {};
  return {
    ...DEFAULTS,
    ...user,
    terms: { ...DEFAULTS.terms, ...(user.terms || {}) },
  };
}
