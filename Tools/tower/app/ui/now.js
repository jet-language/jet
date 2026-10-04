// Pure Now report rendering; navigation and collapse persistence stay in
// tower.js. Each report is a <details data-report="key"> section whose
// one-line summary (title + when it was updated) stays visible when folded.
import { renderMarkdown } from './markdown.js';

const esc = value => String(value ?? '').replace(/[&<>"]/g,
  char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char]);

const MINUTE = 60_000;
// "just now" / "12 min ago" / "5 h ago" for the last two days, then the date.
export function relativeTime(value, now = Date.now()) {
  const at = Date.parse(value);
  if (!Number.isFinite(at)) return String(value ?? '');
  const minutes = Math.round(Math.max(0, now - at) / MINUTE);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return new Date(at).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}
const stamp = value => `<time datetime="${esc(value)}" title="${esc(new Date(value).toLocaleString())}" data-rel>${esc(relativeTime(value))}</time>`;

// Keep "N min ago" labels honest between redraws.
export function refreshRelativeTimes(root, now = Date.now()) {
  for (const node of root.querySelectorAll('time[data-rel]')) {
    const text = relativeTime(node.getAttribute('datetime'), now);
    if (node.textContent !== text) node.textContent = text;
  }
}

const links = (items = []) => items.length ? `<div class="report__links">${items.map(link =>
  `<button class="btn btn--ghost btn--sm" ${link.decision ? `data-report-decision="${esc(link.decision)}"` : ''} ${link.card || link.cardId ? `data-report-card="${esc(link.card || link.cardId)}"` : ''}>${esc(link.label || link.card || link.decision)} →</button>`
).join('')}</div>` : '';

// A visual check is an owner action only once its screen capture is attached
// (the same rule as board-state.js ownerVerifyQueue).
export function buildOwnerActions({ statusSnapshot, decisions = [] } = {}) {
  return [
    ...decisions.filter(d => d.status !== 'ratified' && !d.draft
      && (d.group !== 'acceptance' || (d.visualMedia || []).length)).map(d => ({
      text: `${d.group === 'acceptance' ? 'Visual check' : 'Vote'}: ${d.title}`,
      links: [{ decision: d.id, label: d.id }],
    })),
    ...(statusSnapshot?.ownerActions || []),
  ];
}

// Sections open by default; nested history folds by default.
export const REPORT_DEFAULTS = Object.freeze({ briefing: true, status: true, actions: true, 'briefing-history': false });
const defaultOpen = key => REPORT_DEFAULTS[key] ?? false;

function section({ key, cls = '', label, title = '', meta = '', body, isOpen }) {
  const open = isOpen(key, defaultOpen(key));
  return `<details class="report ${cls}" data-report="${esc(key)}"${open ? ' open' : ''}>
    <summary class="report__summary"><span class="report__chev" aria-hidden="true">▸</span><span class="report__label">${label}</span>${title ? `<span class="report__summary-title">${title}</span>` : ''}${meta ? `<span class="report__summary-meta">${meta}</span>` : ''}</summary>
    <div class="report__content">${body}</div>
  </details>`;
}

const briefingBody = record => `<div class="report__body">${renderMarkdown(record.body)}${(record.sections || []).map(part =>
  `<section><h3>${esc(part.title)}</h3>${renderMarkdown(part.body)}${links(part.links)}</section>`
).join('')}</div>`;

// isOpen(key, fallback) supplies the owner's saved open/closed choice.
export function renderNowReports({ briefings = [], statusSnapshot = null, decisions = [] } = {}, { isOpen = (_key, fallback) => fallback } = {}) {
  const [latest, ...history] = briefings;
  const historyKey = 'briefing-history';
  const past = history.length ? `<details class="report__history" data-report="${historyKey}"${isOpen(historyKey, defaultOpen(historyKey)) ? ' open' : ''}><summary>Briefing history · ${history.length}</summary>${history.map(record => {
    const key = `briefing:${record.id}`;
    return `<details class="report__past" data-report="${esc(key)}"${isOpen(key, false) ? ' open' : ''}><summary>${esc(record.title)} · ${stamp(record.created)} · ${esc(record.by)}</summary>${briefingBody(record)}</details>`;
  }).join('')}</details>` : '';
  const brief = section({
    key: 'briefing', cls: 'report--briefing', label: 'Latest briefing', isOpen,
    title: latest ? esc(latest.title) : 'None yet',
    meta: latest ? `${stamp(latest.created)} · ${esc(latest.by)}` : '',
    body: `${latest ? briefingBody(latest) : '<p class="report__empty">No briefing posted yet.</p>'}${past}`,
  });

  const s = statusSnapshot;
  const status = section({
    key: 'status', cls: 'report--status', label: 'Status board', isOpen,
    title: s?.summary ? esc(s.summary.length > 110 ? `${s.summary.slice(0, 109)}…` : s.summary) : '',
    meta: s ? `Updated ${stamp(s.updatedAt || s.created)} · ${esc(s.by)}` : 'No snapshot yet',
    body: s ? `${s.summary ? `<p class="report__lead">${esc(s.summary)}</p>` : ''}
      <div class="report__label">Milestones</div><div class="report__grid">${s.milestones.map(m =>
        `<article class="report__tile"><div class="report__head"><h3>${esc(m.title)}</h3><span class="report__state">${esc(m.state)}</span></div>
        ${Object.entries(m.progress).map(([label, percent]) => `<div class="report__metric"><label>${esc(label)} <b>${esc(percent)}%</b><progress max="100" value="${esc(percent)}" aria-label="${esc(m.title)}: ${esc(label)}">${esc(percent)}%</progress></label></div>`).join('')}${links(m.links)}</article>`
      ).join('') || '<p class="report__empty">No milestones in this snapshot.</p>'}</div>
      <div class="report__label">Workstreams / lanes</div><div class="report__grid">${s.workstreams.map(w =>
        `<article class="report__tile"><div class="report__head"><h3>${esc(w.title)}</h3><span class="report__state">${esc(w.state)}</span></div>
        <div class="report__meta">Updated ${stamp(w.updatedAt || s.updatedAt || s.created)}</div>
        <p><b>Active workers:</b> ${esc(w.workers.join(', ') || 'None')}</p>
        ${w.blockers.length ? `<ul class="report__blockers">${w.blockers.map(b => `<li>${esc(b)}</li>`).join('')}</ul>` : '<p class="report__empty">No reported blockers.</p>'}${links(w.links)}</article>`
      ).join('') || '<p class="report__empty">No workstreams in this snapshot.</p>'}</div>
      <div class="report__meta">Snapshot posted ${stamp(s.created)}. Reported progress; card gates and ballots remain authoritative.</div>`
      : '<p class="report__empty">No status snapshot posted yet.</p>',
  });

  const actions = buildOwnerActions({ statusSnapshot: s, decisions });
  const owner = section({
    key: 'actions', cls: 'report--actions', label: 'Owner action needed', isOpen,
    title: `<span class="report__count${actions.length ? ' report__count--hot' : ''}">${actions.length}</span>`,
    meta: actions.length ? '' : 'nothing waiting',
    body: actions.length
      ? `<ul class="report__actions">${actions.map(action => `<li><span>${esc(action.text)}</span>${links(action.links)}</li>`).join('')}</ul>`
      : '<p class="report__empty">No open ballots or reported actions.</p>',
  });
  return brief + status + owner;
}
