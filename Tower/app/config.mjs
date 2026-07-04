// Per-project configuration: terminology, taxonomies, server defaults.
// Lives at <dataDir>/config.json in the HOST project; everything optional.
import { readJSON, configFile } from './paths.mjs';

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
  // Known agents for the Agents view roster (listeners also self-announce).
  agents: [],                         // [{ name: "claude-main", kind: "claude" }]
  // Launch bridge (OPT-IN): lets the board UI start a headless agent turn when
  // nothing is listening. Value is a shell command; the message is appended as
  // one quoted argument. Example:
  //   "commands": { "claude": "claude -p", "codex": "codex exec" }
  commands: {},
};

export function loadConfig(dataDir) {
  const user = readJSON(configFile(dataDir), {}) || {};
  return {
    ...DEFAULTS,
    ...user,
    terms: { ...DEFAULTS.terms, ...(user.terms || {}) },
  };
}
