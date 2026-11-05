// #6564 c5: personal and machine information never enters the board or a
// committed file. One rule table serves three callers: `tower redact` rewrites
// the stored board, store.mutate refuses a write that adds a match, and
// Tools/agent/personal-info-guard.sh greps a checkout with the same patterns
// (`node personal-info.mjs patterns`).
//
// The rules come from the machine running Tower, so this file holds no
// personal data: the home directory, the login name, the host name, the
// scratch root and other mounts, and git's user.email and user.name. Each rule
// names its neutral replacement, so a redaction keeps the text's meaning.
// Two more rules hold for every machine: the dev tool roots. Tools read their
// state from $JET_STATE_ROOT and their queues and locks from $JET_RUN_ROOT
// (Tools/agent/roots.sh), so no committed file names one machine's copy.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { homedir, hostname, userInfo } from 'node:os';
import { fileURLToPath } from 'node:url';

export const RULE_TABLE = 'personal-info/v2';

// Names too common to be personal; a machine using one is not flagged by it.
const GENERIC_NAMES = new Set([
  'admin', 'arch', 'build', 'builder', 'ci', 'debian', 'dev', 'devhost', 'fedora', 'localhost',
  'nixos', 'owner', 'root', 'runner', 'test', 'tester', 'ubuntu', 'user', 'users',
]);

const escapeRegExp = (text) => text.replace(/[\\^$.*+?()[\]{}|/]/g, '\\$&');
const within = (path, root) => path === root || path.startsWith(`${root}/`);

function gitConfig(key, cwd) {
  try {
    return execFileSync('git', ['config', '--get', key], { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return '';
  }
}

// /proc/mounts escapes space, tab, newline and backslash as octal.
function machineMounts() {
  let text = '';
  try { text = readFileSync('/proc/mounts', 'utf8'); } catch { return []; }
  return text.split('\n').map(line => (line.split(' ')[1] || '').replace(/\\([0-7]{3})/g, (_, o) => String.fromCharCode(parseInt(o, 8))))
    .filter(mount => /^\/(?:mnt|media|run\/media)\/./.test(mount));
}

export function machineIdentity({ env = process.env, cwd = process.cwd() } = {}) {
  let user = env.USER || env.LOGNAME || '';
  if (!user) { try { user = userInfo().username; } catch { user = ''; } }
  return {
    home: env.HOME || homedir(),
    user,
    host: hostname().split('.')[0],
    scratchRoot: env.JET_SCRATCH_ROOT || '',
    mounts: machineMounts(),
    email: gitConfig('user.email', cwd),
    name: gitConfig('user.name', cwd),
  };
}

// Path rules match a literal path that does not continue into a longer name;
// word rules match a whole word in any letter case. Longer paths go first so
// /etc/profiles/per-user/<user> wins over a bare home rule.
export function personalInfoRules(identity) {
  const rules = [];
  const seen = new Set();
  const path = (find, replace, label) => {
    find = String(find || '').replace(/\/+$/, '');
    if (find.length < 2 || seen.has(find)) return;
    seen.add(find);
    rules.push({ kind: 'path', label, find, replace, source: `${escapeRegExp(find)}(?![\\w-])` });
  };
  const word = (find, replace, label) => {
    find = String(find || '').trim();
    if (find.length < 3 || GENERIC_NAMES.has(find.toLowerCase()) || seen.has(find.toLowerCase())) return;
    seen.add(find.toLowerCase());
    rules.push({ kind: 'word', label, find, replace, source: `\\b${escapeRegExp(find)}\\b` });
  };
  const { home, user, host, scratchRoot, mounts = [], email, name } = identity;
  const login = user && user !== 'root' ? user : '';
  const scratch = scratchRoot && !within(scratchRoot, home) && !within(scratchRoot, '/tmp') ? scratchRoot.replace(/\/+$/, '') : '';
  if (login) path(`/etc/profiles/per-user/${login}`, '/etc/profiles/per-user/$USER', 'per-user profile');
  if (scratch) path(scratch, '$JET_SCRATCH_ROOT', 'scratch root');
  for (const mount of mounts) path(mount, mount === scratch ? '$JET_SCRATCH_ROOT' : '/mnt/volume', 'mount');
  if (home && home !== '/') path(home, '~', 'home');
  if (login) {
    path(`/home/${login}`, '~', 'home');
    path(`/Users/${login}`, '~', 'home');
  }
  if (email) path(email, 'user@example.com', 'email');
  if (name) word(name, 'owner', 'git name');
  // A GitHub noreply address carries the account handle.
  const handle = /^\d+\+([^@]+)@users\.noreply\.github\.com$/i.exec(email || '')?.[1];
  if (handle) word(handle, 'owner', 'git name');
  if (login) word(login, 'user', 'user name');
  if (host) word(host, 'devhost', 'host name');
  // Every spelling of the state root, the machine's home first, then the bare
  // cache-relative form that code joins to homedir(). Both roots are joined
  // from parts so this file does not match its own rules.
  const toolRoot = (forms, replace, label) => {
    forms = [...new Set(forms)];
    rules.push({ kind: 'path', label, find: forms[0], replace, source: `(?:${forms.map(escapeRegExp).join('|')})(?![\\w-])` });
  };
  const stateRoot = ['.cache', 'jet-dev'].join('/');
  const homes = [home && home !== '/' ? home : '', login ? `/home/${login}` : '', login ? `/Users/${login}` : '', '${HOME}', '$HOME', '~'];
  toolRoot([...homes.filter(Boolean).map(prefix => `${prefix}/${stateRoot}`), stateRoot], '$JET_STATE_ROOT', 'state root');
  toolRoot([['', 'tmp', 'tdev'].join('/')], '$JET_RUN_ROOT', 'run root');
  const order = (rule) => (rule.kind === 'path' ? 0 : 1);
  rules.sort((a, b) => order(a) - order(b) || b.find.length - a.find.length);
  return rules;
}

// Path rules run first (exact case), then word rules (any case); within each
// pass one alternation regex finds every rule, and the capture group that
// matched names the rule.
function compilePass(rules, flags) {
  return rules.length ? { rules, regex: new RegExp(rules.map(rule => `(${rule.source})`).join('|'), flags) } : null;
}

// Keep the letter case of a matched word: Name -> User, NAME -> USER.
function caseLike(matched, replacement, find) {
  if (matched === find) return replacement;
  if (matched === matched.toUpperCase() && matched !== matched.toLowerCase()) return replacement.toUpperCase();
  if (matched[0] === matched[0].toUpperCase() && matched.slice(1) === matched.slice(1).toLowerCase())
    return replacement[0].toUpperCase() + replacement.slice(1);
  return replacement;
}

export function personalInfoMatcher(rules) {
  const passes = [
    compilePass(rules.filter(rule => rule.kind === 'path'), 'g'),
    compilePass(rules.filter(rule => rule.kind === 'word'), 'gi'),
  ].filter(Boolean);
  const ruleOf = (pass, match) => pass.rules[match.slice(1, pass.rules.length + 1).findIndex(group => group !== undefined)];
  const replace = (text, counts = null) => {
    if (typeof text !== 'string') return text;
    for (const pass of passes) {
      text = text.replace(pass.regex, (...match) => {
        const rule = ruleOf(pass, match);
        if (counts) counts[rule.label] = (counts[rule.label] || 0) + 1;
        return rule.kind === 'word' ? caseLike(match[0], rule.replace, rule.find) : rule.replace;
      });
    }
    return text;
  };
  const count = (text) => {
    if (typeof text !== 'string') return 0;
    let n = 0;
    for (const pass of passes) for (const _ of text.matchAll(pass.regex)) n++;
    return n;
  };
  // Adds each matched span (word spans folded to lower case) to `into`.
  const tally = (text, into) => {
    if (typeof text !== 'string') return into;
    for (const pass of passes) {
      for (const match of text.matchAll(pass.regex)) {
        const key = ruleOf(pass, match).kind === 'word' ? match[0].toLowerCase() : match[0];
        into.set(key, (into.get(key) || 0) + 1);
      }
    }
    return into;
  };
  // Every match with its whitespace-delimited token and that token redacted,
  // so a refusal can show the writer the exact text to write instead.
  const spans = (text) => {
    if (typeof text !== 'string') return [];
    const found = [];
    for (const pass of passes) {
      for (const match of text.matchAll(pass.regex)) {
        let start = match.index;
        while (start > 0 && !/\s/.test(text[start - 1])) start--;
        let end = match.index + match[0].length;
        while (end < text.length && !/\s/.test(text[end])) end++;
        const token = text.slice(start, end);
        const rule = ruleOf(pass, match);
        found.push({ span: rule.kind === 'word' ? match[0].toLowerCase() : match[0], token, redacted: replace(token) });
      }
    }
    return found;
  };
  return { rules, replace, spans, count, tally };
}

// Calls fn on every string under `value`, object keys included.
export function eachString(value, fn) {
  if (typeof value === 'string') return fn(value);
  if (Array.isArray(value)) {
    for (const item of value) eachString(item, fn);
  } else if (value && typeof value === 'object') {
    for (const key of Object.keys(value)) {
      fn(key);
      eachString(value[key], fn);
    }
  }
}

// Per-span match counts over every string under `value`.
export function tallyValue(value, matcher) {
  const counts = new Map();
  eachString(value, text => matcher.tally(text, counts));
  return counts;
}

let machineMatcher = null;
export function machinePersonalInfo() {
  machineMatcher ??= personalInfoMatcher(personalInfoRules(machineIdentity()));
  return machineMatcher;
}

// Rewrite every string (and object key) under `value` in place; returns the
// number of spans replaced, with per-rule counts added to `counts`.
export function redactValue(value, matcher, counts = {}) {
  const sum = () => Object.values(counts).reduce((a, b) => a + b, 0);
  const before = sum();
  const redact = (text) => matcher.replace(text, counts);
  const visit = (node) => {
    if (Array.isArray(node)) {
      for (let i = 0; i < node.length; i++) {
        if (typeof node[i] === 'string') node[i] = redact(node[i]);
        else visit(node[i]);
      }
    } else if (node && typeof node === 'object') {
      const keys = Object.keys(node);
      for (const key of keys) {
        if (typeof node[key] === 'string') node[key] = redact(node[key]);
        else visit(node[key]);
      }
      // A key naming personal information is renamed in place, keeping order.
      if (keys.some(key => matcher.count(key))) {
        const entries = keys.map(key => [redact(key), node[key]]);
        for (const key of keys) delete node[key];
        for (const [key, item] of entries) node[key] = item;
      }
    }
  };
  visit(value);
  return sum() - before;
}

// The refusal a write gets when `after` holds more of some span than the
// `before` tally did, or null. It names each offending token and the exact
// text to write instead, e.g. $JET_SCRATCH_ROOT/lanes/X/run.log.
export function personalInfoAdded(matcher, before, after, limit = 3) {
  const now = tallyValue(after, matcher);
  const added = new Set([...now].filter(([span, n]) => n > (before.get(span) || 0)).map(([span]) => span));
  if (!added.size) return null;
  const examples = new Map();
  eachString(after, text => {
    if (examples.size >= limit || !matcher.count(text)) return;
    for (const hit of matcher.spans(text))
      if (added.has(hit.span) && examples.size < limit) examples.set(hit.token, hit.redacted);
  });
  const fixes = [...examples].map(([token, redacted]) => `write ${JSON.stringify(redacted)} instead of ${JSON.stringify(token)}`);
  return `this write adds personal or machine information, which the board refuses (#6564 c5): ${fixes.join('; ')}`;
}

// `tower redact`: rewrite every matched span in the live board and the
// archive, ratified decisions, briefings and papercuts included, and count the
// spans per file, per store section and per rule. Only matched spans change.
export function redactBoard(state, history, matcher) {
  const files = {};
  const sections = {};
  const rules = {};
  let total = 0;
  const redactSection = (file, name, holder, key) => {
    let n;
    if (typeof holder[key] === 'string') {
      const before = Object.values(rules).reduce((a, b) => a + b, 0);
      holder[key] = matcher.replace(holder[key], rules);
      n = Object.values(rules).reduce((a, b) => a + b, 0) - before;
    } else {
      n = redactValue(holder[key], matcher, rules);
    }
    if (n) {
      files[file] = (files[file] || 0) + n;
      sections[name] = (sections[name] || 0) + n;
    }
    total += n;
  };
  for (const [file, doc] of [['tower.json', state], ['history.json', history]]) {
    for (const key of Object.keys(doc)) {
      if (key === 'decisions' && Array.isArray(doc.decisions)) {
        doc.decisions.forEach((decision, i) =>
          redactSection(file, `${file} decisions${decision?.status === 'ratified' ? ' (ratified)' : ''}`, doc.decisions, i));
      } else {
        redactSection(file, `${file} ${key}`, doc, key);
      }
    }
  }
  return { ruleTable: RULE_TABLE, total, files, sections, rules };
}

// `node personal-info.mjs patterns` prints one rule per line for the shell
// guard: "path<TAB>regex" (case-sensitive) or "word<TAB>regex" (any case).
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  if (process.argv[2] !== 'patterns') {
    process.stderr.write('usage: node personal-info.mjs patterns\n');
    process.exit(64);
  }
  for (const rule of personalInfoRules(machineIdentity())) process.stdout.write(`${rule.kind}\t${rule.source}\n`);
}
