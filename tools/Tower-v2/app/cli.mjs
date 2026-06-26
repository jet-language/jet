// Text snapshot of the board for the terminal.
import { load, project, PHASES } from './store.mjs';

export function status() {
  const s = project(load());
  const bar = (n) => '█'.repeat(Math.min(12, n)) + '░'.repeat(Math.max(0, 12 - n));
  console.log(`\n  TOWER · epoch ${s.meta.currentEpoch || '—'}\n`);
  for (const ph of PHASES) {
    const cs = s.cards.filter(c => c.phase === ph.id);
    if (!cs.length) continue;
    console.log(`  ${ph.label.padEnd(9)} ${bar(cs.length)} ${cs.length}`);
  }
  console.log(`\n  BLOCKED ON YOU  ${s.counts.decide} decisions · ${s.counts.activate} to activate`);
  console.log(`  AGENT-READY     ${s.counts.agentReady}  (plan / implement / build / verify)`);
  console.log(`  open questions  ${s.counts.openQuestions}   sidequests ${s.counts.sidequests}   binder ${s.counts.binder}\n`);

  const show = (label, lane) => {
    const cs = s.cards.filter(c => c.lane.lane === lane);
    if (!cs.length) return;
    console.log(`  ${label}:`);
    for (const c of cs.slice(0, 12)) console.log(`   · #${c.num}  ${c.priority}  ${c.title.slice(0, 52)}  — ${c.lane.label}`);
  };
  show('BLOCKED ON YOU — decide', 'decide');
  show('BLOCKED ON YOU — activate', 'activate');
  show('AGENT — plan', 'plan');
  show('AGENT — implement', 'implement');
  show('AGENT — building', 'building');
  show('AGENT — verify', 'verify');
  console.log('');
}
