// Pure Now report rendering; navigation stays in tower.js.
import { renderMarkdown } from './markdown.js';

const esc = value => String(value ?? '').replace(/[&<>"]/g,
  char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char]);
const stamp = value => `<time datetime="${esc(value)}">${esc(new Date(value).toLocaleString())}</time>`;
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

function briefing(record) {
  return `<article class="report__briefing">
    <h2 class="report__title">${esc(record.title)}</h2>
    <div class="report__meta">${stamp(record.created)} · ${esc(record.by)}</div>
    <div class="report__body">${renderMarkdown(record.body)}${(record.sections || []).map(section =>
      `<section><h3>${esc(section.title)}</h3>${renderMarkdown(section.body)}${links(section.links)}</section>`
    ).join('')}</div>
  </article>`;
}

export function renderNowReports({ briefings = [], statusSnapshot = null, decisions = [] } = {}) {
  const [latest, ...history] = briefings;
  const reports = `<section class="report report--briefing" aria-label="Briefings">
    <div class="report__label">Latest briefing</div>
    ${latest ? briefing(latest) : '<p class="report__empty">No briefing posted yet.</p>'}
    ${history.length ? `<details class="report__history"><summary>Briefing history · ${history.length}</summary>${history.map(record =>
      `<details class="report__past"><summary>${esc(record.title)} · ${stamp(record.created)} · ${esc(record.by)}</summary>${briefing(record)}</details>`
    ).join('')}</details>` : ''}
  </section>`;
  const s = statusSnapshot;
  const actions = buildOwnerActions({ statusSnapshot: s, decisions });
  const status = `<section class="report report--status" aria-label="Status board">
    <div class="report__head"><h2 class="report__title">Status board</h2>${s ? `<span class="report__meta">Updated ${stamp(s.updatedAt || s.created)} · ${esc(s.by)}</span>` : ''}</div>
    ${s ? `${s.summary ? `<p>${esc(s.summary)}</p>` : ''}
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
      : '<p class="report__empty">No status snapshot posted yet.</p>'}
    <div class="report__actions"><h3>Owner action needed <span class="report__meta">${actions.length}</span></h3>
      ${actions.length ? `<ul>${actions.map(action => `<li><span>${esc(action.text)}</span>${links(action.links)}</li>`).join('')}</ul>` : '<p class="report__empty">No open ballots or reported actions.</p>'}
    </div>
  </section>`;
  return reports + status;
}
