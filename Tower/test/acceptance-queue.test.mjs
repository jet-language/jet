// Card #515 — Now page owner-verification queue. Covers the two layers this
// repo actually unit-tests (store business logic + HTTP API); tower.js is a
// DOM-only client with no test harness anywhere in this repo (importing it
// under plain Node throws — it wires `document.addEventListener` at module
// scope), so the section's live rendering is proven by the temp-server curl
// self-check in the card's verification notes, not here.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, acceptanceCheckInstructions, laneOf } from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';
import * as db from '../app/store.mjs';

const resolveAcceptance = db.createAcceptanceResolver();
const provenance = (outcome) => ({ kind: 'owner-ui', session: 'test-session', challenge: `test-${outcome}`, issuedFor: 'D-ACCEPT-1', outcome });

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-avq-'));
  writeJSON(join(dir, 'tower.json'), empty('Test'));
  return openStore(dir);
};

// ---- store: checkInstructions assembly --------------------------------------

// #515 pass 2 (2026-07-12): short, phone-first shape — one line per
// non-open criterion (machine evidence already on file), plus AT MOST one
// visual-check line, only when a ref points at something visual.
test('acceptanceCheckInstructions: proof is one line per non-open criterion (open criteria stay out)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', refs: ['tests/foo.rs'] }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'section renders on Now', 'builder'));
  st.mutate((s) => db.addCriterion(s, '#1', 'accept closes the card', 'builder')); // stays open — no line
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'curl showed the section', by: 'builder' }));
  const c = st.load().cards[0];
  const ci = acceptanceCheckInstructions(c);
  assert.deepEqual(ci.proof, ['section renders on Now — met (curl showed the section)']);
  assert.equal(ci.visualCheck, null, 'tests/foo.rs is not a visual ref');
});

test('acceptanceCheckInstructions: visualCheck is one line, only for a ref that touches Tower/app/ui', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', refs: ['Tower/app/ui/tower.js', 'tests/foo.rs'] }, cfg));
  const c = st.load().cards[0];
  const ci = acceptanceCheckInstructions(c);
  assert.equal(ci.visualCheck, 'Open Tower/app/ui/tower.js — glance, confirm it looks right.');
});

test('acceptanceCheckInstructions: a very long evidence string is shortened to one line', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'x'.repeat(200), by: 'builder' }));
  const c = st.load().cards[0];
  const ci = acceptanceCheckInstructions(c);
  assert.equal(ci.proof.length, 1);
  assert.ok(ci.proof[0].length <= 100, `proof line must stay one short line, got ${ci.proof[0].length} chars`);
  assert.ok(ci.proof[0].endsWith('…'), 'shortened line is marked truncated');
});

test('acceptanceCheckInstructions: no criteria and no refs yields null (client falls back)', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A' }, cfg));
  assert.equal(acceptanceCheckInstructions(st.load().cards[0]), null);
});

test('mintAcceptance stamps checkInstructions onto D-ACCEPT and re-mint refreshes it', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s) => db.addCriterion(s, '#1', 'thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 1, { evidence: 'ran it', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 1, { evidence: 'checked it', by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'verifier' }, cfg));
  let d = st.load().decisions.find(x => x.id === 'D-ACCEPT-1');
  assert.deepEqual(d.checkInstructions.proof, ['thing works — verified (checked it)']);
  assert.equal(d.checkInstructions.visualCheck, null);

  // bounce, add a second criterion, re-attempt — the ballot's instructions
  // must refresh to match, not keep serving round-1 evidence.
  st.mutate((s) => resolveAcceptance(s, 'D-ACCEPT-1', 'bounce', 'add more coverage', provenance('bounce')));
  st.mutate((s) => db.addCriterion(s, '#1', 'second thing works', 'planner'));
  st.mutate((s) => db.meetCriterion(s, '#1', 2, { evidence: 'ran it too', by: 'builder' }));
  st.mutate((s) => db.verifyCriterion(s, '#1', 2, { evidence: 'checked it too', by: 'verifier' }));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'verifier' }, cfg));
  d = st.load().decisions.find(x => x.id === 'D-ACCEPT-1');
  assert.equal(d.status, 'open');
  assert.deepEqual(d.checkInstructions.proof, [
    'thing works — verified (checked it)',
    'second thing works — verified (checked it too)',
  ]);
});

// #515/#516 lane bug: an acceptance ballot is a DIFFERENT owner duty (the
// dedicated verify queue), not a generic decide-deck entry. Before this fix
// laneOf() counted the open D-ACCEPT-* decision as a blocking generic
// decision, so a card sitting in verify with an unresolved acceptance
// ballot mislabeled as lane 'decide' — exactly what card #516 showed live
// (board tile said "1 decision to make" and clicking it sent the owner into
// focusAll() hunting for a ballot deliberately excluded from that deck: a
// dead end that reads as "does nothing").
test('laneOf: a verify-phase card with an open acceptance ballot stays lane verify, not decide', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'A', needsAcceptance: true }, cfg));
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'done', by: 'builder' }, cfg)); // no criteria — mints straight away
  const s = st.load();
  const c = s.cards.find(x => x.num === 1);
  assert.equal(c.phase, 'verify');
  assert.equal(s.decisions.find(d => d.id === 'D-ACCEPT-1').status, 'open');
  const lane = laneOf(c, s.decisions, s.cards);
  assert.equal(lane.lane, 'verify', `expected verify lane, got ${JSON.stringify(lane)}`);
});

test('a verify-phase card with no needsAcceptance flag carries no D-ACCEPT ballot — the client renders it as a bare row', () => {
  const st = fresh();
  st.mutate((s, cfg) => db.addCard(s, { title: 'Manually parked in verify' }, cfg));
  // no criteria, no flag — an agent (or a stray CLI call) parked it in verify
  // directly, never going through the done-gate at all.
  st.mutate((s, cfg) => db.updateCard(s, '#1', { phase: 'verify', by: 'some-agent' }, cfg));
  const c = st.load().cards[0];
  assert.equal(c.phase, 'verify');
  assert.equal(c.needsAcceptance, false);
  assert.equal(st.load().decisions.find(x => x.id === 'D-ACCEPT-1'), undefined);
});

// ---- server: the Accept / Bounce actions the Now page's buttons drive -------

const dir = mkdtempSync(join(tmpdir(), 'tower-avq-srv-'));
writeJSON(join(dir, 'tower.json'), empty('Srv'));
writeJSON(configFile(dir), { project: 'Srv' });
const store = openStore(dir);
const PORT = 17959;
const server = serve(store, PORT, false);
after(() => server.close());
const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (route, body) => {
  const r = await fetch(url('/api/' + route), { method: 'POST', body: JSON.stringify(body) });
  return { status: r.status, json: await r.json() };
};

const ownerSession = async () => {
  const page = await fetch(url('/'), { headers: { accept: 'text/html' } });
  const cookie = page.headers.get('set-cookie')?.split(';', 1)[0];
  assert.ok(cookie, 'owner page must establish an HttpOnly in-memory session');
  return cookie;
};

const ownerResolve = async (cookie, decisionId, outcome, comment) => {
  const issued = await fetch(url('/api/acceptance/challenge'), {
    method: 'POST',
    headers: { 'content-type': 'application/json', cookie, 'x-tower-owner-action': 'verify' },
    body: JSON.stringify({ decisionId, outcome }),
  });
  const issuedBody = await issued.json();
  assert.equal(issued.status, 200, issuedBody.message);
  const challenge = issuedBody.result;
  const resolved = await fetch(url('/api/acceptance/resolve'), {
    method: 'POST',
    headers: { 'content-type': 'application/json', cookie, 'x-tower-owner-action': 'verify' },
    body: JSON.stringify({ challenge: challenge.challenge, decisionId, outcome, comment }),
  });
  return { status: resolved.status, json: await resolved.json(), challenge: challenge.challenge };
};

test('needsAcceptance card in verify: state carries the D-ACCEPT ballot until owner acts', async () => {
  await post('card/add', { title: 'Flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/criteria-add', { id: '#1', text: 'does the thing', by: 'planner' });
  await post('card/criteria-meet', { id: '#1', n: 1, evidence: 'built', by: 'builder' });
  await post('card/criteria-verify', { id: '#1', n: 1, evidence: 'checked', by: 'verifier' });
  const done = await post('card/update', { id: '#1', phase: 'done', by: 'verifier' });
  assert.equal(done.status, 200);
  assert.equal(done.json.result.phase, 'verify');
  const state = await (await fetch(url('/api/state'))).json();
  const ballot = state.decisions.find(d => d.id === 'D-ACCEPT-1');
  assert.ok(ballot, 'ballot must be in projected state for the Now page to read');
  assert.equal(ballot.status, 'open');
  assert.deepEqual(ballot.checkInstructions.proof, ['does the thing — verified (checked)']);
  assert.equal(ballot.checkInstructions.visualCheck, null);
  // #516-style regression: the card's own lane must read verify, never
  // decide, while this ballot sits open (see laneOf test above for why).
  assert.equal(state.cards.find(c => c.num === 1).lane.lane, 'verify');
});

test('forged #515/#516 path: generic clearance rejects caller-supplied owner and agent quote, then audits both', async () => {
  for (const payload of [
    { decisionId: 'D-ACCEPT-1', outcome: 'accept', by: 'owner' },
    { decisionId: 'D-ACCEPT-1', outcome: 'accept', by: 'agent', quote: 'owner said accept' },
  ]) {
    const r = await post('clearance', payload);
    assert.equal(r.status, 403);
    assert.equal(r.json.error, 'E_ACCEPTANCE_OWNER_UI');
  }
  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards.find(c => c.num === 1).phase, 'verify');
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-1').status, 'open');
  assert.equal(state.events.filter(e => e.action === 'acceptance.reject' && e.ref === 'D-ACCEPT-1').length, 2);
});

test('generic clearance batch rejects acceptance atomically and audits it', async () => {
  const r = await post('clearance/batch', { by: 'owner', decisions: [{ decisionId: 'D-ACCEPT-1', outcome: 'accept' }] });
  assert.equal(r.status, 403);
  assert.equal(r.json.error, 'E_ACCEPTANCE_OWNER_UI');
  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards.find(c => c.num === 1).phase, 'verify');
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-1').status, 'open');
  assert.ok(state.events.some(e => e.action === 'acceptance.reject' && e.note.includes('clearance/batch')));
});

test('caller-supplied owner cannot close the card or clear needsAcceptance around the ballot', async () => {
  for (const payload of [
    { id: '#1', phase: 'done', by: 'owner' },
    { id: '#1', needsAcceptance: false, by: 'owner' },
  ]) {
    const r = await post('card/update', payload);
    assert.equal(r.status, 403);
    assert.equal(r.json.error, 'E_ACCEPTANCE_OWNER_UI');
  }
  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards.find(c => c.num === 1).phase, 'verify');
  assert.equal(state.cards.find(c => c.num === 1).needsAcceptance, true);
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-1').status, 'open');
  assert.equal(state.events.filter(e => e.action === 'acceptance.reject' && e.ref === 'D-ACCEPT-1').length, 5);
});

test('dedicated owner UI action accepts atomically with immutable provenance', async () => {
  const cookie = await ownerSession();
  const r = await ownerResolve(cookie, 'D-ACCEPT-1', 'accept');
  assert.equal(r.status, 200);
  assert.equal(r.json.result.status, 'ratified');
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 1);
  assert.equal(card.phase, 'done');
  const ratifyEvent = state.events.find(e => e.action === 'acceptance.resolve' && e.ref === 'D-ACCEPT-1');
  assert.ok(ratifyEvent, 'ratification must be in the event log');
  assert.equal(ratifyEvent.by, 'owner');
  assert.match(ratifyEvent.note, /owner-ui session=.* challenge=.* outcome=accept/);
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-1').provenance.kind, 'owner-ui');
});

test('dedicated owner UI action bounces with the comment logged', async () => {
  await post('card/add', { title: 'Second flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/update', { id: '#2', phase: 'done', by: 'builder' }); // no criteria — mints straight away
  const cookie = await ownerSession();
  const r = await ownerResolve(cookie, 'D-ACCEPT-2', 'bounce', 'missing the edge case');
  assert.equal(r.status, 200);
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 2);
  assert.equal(card.phase, 'building');
  assert.match(card.log[0].text, /Bounced back to building: missing the edge case/);
  const ratifyEvent = state.events.find(e => e.action === 'acceptance.resolve' && e.ref === 'D-ACCEPT-2');
  assert.equal(ratifyEvent.by, 'owner');
});

test('owner provenance fails closed for missing session, replay, wrong decision/outcome, and non-loopback marker', async () => {
  await post('card/add', { title: 'Third flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/update', { id: '#3', phase: 'done', by: 'builder' });
  await post('card/add', { title: 'Fourth flagged card', needsAcceptance: true, by: 'owner' });
  await post('card/update', { id: '#4', phase: 'done', by: 'builder' });
  let r = await fetch(url('/api/acceptance/challenge'), { method: 'POST', body: JSON.stringify({ decisionId: 'D-ACCEPT-3', outcome: 'accept' }) });
  assert.equal(r.status, 403);
  const cookie = await ownerSession();
  r = await fetch(url('/api/acceptance/challenge'), { method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify', 'x-forwarded-for': '203.0.113.1' }, body: JSON.stringify({ decisionId: 'D-ACCEPT-3', outcome: 'accept' }) });
  assert.equal(r.status, 403);
  const issue = async () => {
    const issued = await fetch(url('/api/acceptance/challenge'), { method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' }, body: JSON.stringify({ decisionId: 'D-ACCEPT-3', outcome: 'accept' }) });
    return (await issued.json()).result.challenge;
  };
  let challenge = await issue();
  r = await fetch(url('/api/acceptance/resolve'), { method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' }, body: JSON.stringify({ challenge, decisionId: 'D-ACCEPT-4', outcome: 'accept' }) });
  assert.equal(r.status, 403);
  challenge = await issue();
  r = await fetch(url('/api/acceptance/resolve'), { method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' }, body: JSON.stringify({ challenge, decisionId: 'D-ACCEPT-3', outcome: 'bounce' }) });
  assert.equal(r.status, 403);
  const ok = await ownerResolve(cookie, 'D-ACCEPT-3', 'accept');
  const replay = await fetch(url('/api/acceptance/resolve'), { method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' }, body: JSON.stringify({ challenge: ok.challenge, decisionId: 'D-ACCEPT-3', outcome: 'accept' }) });
  assert.equal(replay.status, 403);
});

// Owner directive: "clicking Accept ... throws an error and does nothing"
// must never mean a crash — a double-click (two independent challenges,
// both resolved) always leaves exactly one winner and one sane, structured
// loser, never a 500 or a torn write.
test('double-click: two independently-challenged accept attempts on the same ballot — one wins, one fails closed and sane', async () => {
  const cookie = await ownerSession();
  const issue = async () => {
    const issued = await fetch(url('/api/acceptance/challenge'), {
      method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' },
      body: JSON.stringify({ decisionId: 'D-ACCEPT-4', outcome: 'accept' }),
    });
    return (await issued.json()).result.challenge;
  };
  const [chA, chB] = await Promise.all([issue(), issue()]);
  const resolve = (challenge) => fetch(url('/api/acceptance/resolve'), {
    method: 'POST', headers: { cookie, 'x-tower-owner-action': 'verify' },
    body: JSON.stringify({ challenge, decisionId: 'D-ACCEPT-4', outcome: 'accept' }),
  });
  const [rA, rB] = await Promise.all([resolve(chA), resolve(chB)]);
  const winners = [rA, rB].filter(r => r.status === 200);
  const losers = [rA, rB].filter(r => r.status !== 200);
  assert.equal(winners.length, 1, 'exactly one click closes the card');
  assert.equal(losers.length, 1, 'the other click fails, not silently no-ops or double-applies');
  const loserBody = await losers[0].json();
  assert.ok(loserBody.error && loserBody.message, 'loser gets a structured error, not a crash/empty body');
  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards.find(c => c.num === 4).phase, 'done', 'card closes exactly once');
});

// Root cause of the P0 report: acceptance previously hard-required literal
// loopback with no path for an authenticated remote/phone device, even
// though the rest of the API already treats a device presenting auth.token
// as the owner (README "Live + remote": LAN/tailnet + PWA + push). A device
// with no token still can't prove it's the owner and stays blocked.
test('remote device: rejected with no auth.token configured, accepted once it presents the configured token', async () => {
  const rdir = mkdtempSync(join(tmpdir(), 'tower-avq-remote-'));
  writeJSON(join(rdir, 'tower.json'), empty('Remote'));
  writeJSON(configFile(rdir), { project: 'Remote' });
  const rstore = openStore(rdir);
  rstore.config.auth = { token: 'owner-secret-123' };
  const rport = 17960;
  const rserver = serve(rstore, rport, false);
  const rurl = (p) => `http://localhost:${rport}${p}`;
  try {
    await fetch(rurl('/api/card/add'), { method: 'POST', body: JSON.stringify({ title: 'Remote card', needsAcceptance: true, by: 'owner' }) });
    await fetch(rurl('/api/card/update'), { method: 'POST', body: JSON.stringify({ id: '#1', phase: 'done', by: 'builder' }) });

    // simulated remote device: loopback socket, but marked forwarded (same
    // trick the existing non-loopback test above uses) — no token presented.
    const noToken = await fetch(rurl('/api/acceptance/challenge'), {
      method: 'POST', headers: { 'x-tower-owner-action': 'verify', 'x-forwarded-for': '203.0.113.9' },
      body: JSON.stringify({ decisionId: 'D-ACCEPT-1', outcome: 'accept' }),
    });
    assert.equal(noToken.status, 403);
    const noTokenBody = await noToken.json();
    assert.match(noTokenBody.message, /auth\.token/, 'error must point at the fix, not just say no');

    // same remote device, now presenting the correct bearer token — this is
    // exactly what a phone away from the loopback machine can do. A real
    // owner-session cookie is required too (same as loopback), so mint one
    // first the same way a page load would, forwarded+authed the whole way.
    const remoteHeaders = { 'x-forwarded-for': '203.0.113.9', authorization: 'Bearer owner-secret-123' };
    const page = await fetch(rurl('/'), { headers: remoteHeaders });
    const remoteCookie = page.headers.get('set-cookie')?.split(';', 1)[0];
    assert.ok(remoteCookie, 'a token-authenticated remote GET / must also mint an owner session');
    const withToken = await fetch(rurl('/api/acceptance/challenge'), {
      method: 'POST',
      headers: { ...remoteHeaders, cookie: remoteCookie, 'x-tower-owner-action': 'verify' },
      body: JSON.stringify({ decisionId: 'D-ACCEPT-1', outcome: 'accept' }),
    });
    assert.equal(withToken.status, 200, JSON.stringify(await withToken.clone().json()));
    const challenge = (await withToken.json()).result.challenge;
    const resolved = await fetch(rurl('/api/acceptance/resolve'), {
      method: 'POST',
      headers: { ...remoteHeaders, cookie: remoteCookie, 'x-tower-owner-action': 'verify' },
      body: JSON.stringify({ challenge, decisionId: 'D-ACCEPT-1', outcome: 'accept' }),
    });
    assert.equal(resolved.status, 200);
    const state = await (await fetch(rurl('/api/state'))).json();
    assert.equal(state.cards.find(c => c.num === 1).phase, 'done', 'remote-but-authenticated accept round-trips to a closed card');

    // wrong token still fails closed.
    const badToken = await fetch(rurl('/api/acceptance/challenge'), {
      method: 'POST',
      headers: { 'x-tower-owner-action': 'verify', 'x-forwarded-for': '203.0.113.9', authorization: 'Bearer not-the-token' },
      body: JSON.stringify({ decisionId: 'D-ACCEPT-1', outcome: 'accept' }),
    });
    assert.equal(badToken.status, 403);
  } finally {
    rserver.close();
  }
});

test('a card parked in verify without the flag has no D-ACCEPT ballot but is still in state.cards for the Now page to list', async () => {
  await post('card/add', { title: 'Unflagged in verify', by: 'owner' });
  const upd = await post('card/update', { id: '#5', phase: 'verify', by: 'some-agent' });
  assert.equal(upd.status, 200);
  const state = await (await fetch(url('/api/state'))).json();
  const card = state.cards.find(c => c.num === 5);
  assert.equal(card.phase, 'verify');
  assert.equal(card.needsAcceptance, false);
  assert.equal(state.decisions.find(d => d.id === 'D-ACCEPT-5'), undefined);
});
