// Shared, deliberately predictable duplicate signals for card add + lint.

const TITLE_WORD_RE = /[a-z0-9]+/g;
const PATH_RE = /\b((?:tests?|examples?|fixtures?|specs?)\/[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*)/gi;
const NAME_RE = /\b[A-Za-z][A-Za-z0-9]*(?:_[A-Za-z0-9]+)+\b/g;
const TITLE_STOP_WORDS = new Set([
  'a', 'an', 'and', 'as', 'at', 'by', 'for', 'from', 'in', 'into', 'is', 'it',
  'of', 'on', 'or', 'the', 'to', 'with', 'add', 'allow', 'build', 'change',
  'check', 'create', 'debug', 'fix', 'handle', 'make', 'remove', 'support',
  'update', 'use', 'using', 'card', 'cards', 'issue', 'problem',
]);
const REFERENCE_NAME_RE = /(?:^|_)(?:test|fixture|example|spec)(?:_|$)|^(?:test|fixture|example|spec)[A-Z]/i;
// Two meaningful words are too little signal for a title-only duplicate.
const MIN_TITLE_SIGNAL = 3;

const clean = (value) => String(value ?? '').toLowerCase();

export const isOpenCard = (card) => !!card && card.phase !== 'done' && card.phase !== 'frozen';

export function titleTokens(title) {
  return new Set((clean(title).match(TITLE_WORD_RE) || [])
    .filter(token => token.length > 1 && !TITLE_STOP_WORDS.has(token)));
}

export function referenceTokens(body) {
  const text = String(body ?? '');
  const tokens = new Set();
  for (const match of text.matchAll(PATH_RE)) tokens.add(`path:${clean(match[1])}`);
  for (const match of text.matchAll(NAME_RE)) {
    const name = match[0];
    if (REFERENCE_NAME_RE.test(name)) tokens.add(`name:${clean(name)}`);
  }
  return tokens;
}

export const referenceLabel = (token) => String(token).replace(/^[^:]+:/, '');

function shared(left, right) {
  return [...left].filter(token => right.has(token));
}

function strongTitleOverlap(left, right) {
  const a = titleTokens(left);
  const b = titleTokens(right);
  const common = shared(a, b);
  if (Math.min(a.size, b.size) < MIN_TITLE_SIGNAL) return false;
  return common.length >= 2 && common.length / Math.min(a.size, b.size) >= 0.75;
}

export function findDuplicateCandidates(cards, incoming = {}) {
  const input = incoming || {};
  const title = input.title;
  const references = referenceTokens(input.body);
  return (cards || [])
    .filter(isOpenCard)
    .map(card => {
      const reasons = [];
      if (strongTitleOverlap(title, card.title)) reasons.push('strong title overlap');
      const common = shared(references, referenceTokens(card.body));
      if (common.length) reasons.push(...common.map(token => `shared ${referenceLabel(token)}`));
      if (!reasons.length) return null;
      return {
        id: card.id ?? null,
        num: card.num ?? null,
        title: String(card.title ?? '(untitled)'),
        phase: String(card.phase ?? 'unknown'),
        matches: reasons,
      };
    })
    .filter(Boolean)
    .sort((a, b) => (a.num ?? Number.MAX_SAFE_INTEGER) - (b.num ?? Number.MAX_SAFE_INTEGER));
}
