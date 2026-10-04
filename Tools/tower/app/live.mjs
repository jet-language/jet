// In-memory live board for `tower serve`.
//
// The board files are large (tens of MB) and agents rewrite them through the
// CLI many times an hour. Re-reading and re-parsing them per request made
// every page load and card open pay for the whole board. This module keeps
// the parsed live board, its compact projected index, and (lazily) the
// archive in memory, and reloads only when a file actually changed: a cheap
// stat (inode, size, mtime) per access plus an fs.watch nudge. Writers always
// replace the files by atomic rename, so a new inode or mtime is a reliable
// change signal.
//
// Each new index is diffed against the previous one; the patch goes out to
// live clients over SSE and is kept in a short ring so a reconnecting client
// can catch up from its revision without refetching the whole board.
import { statSync, watch } from 'node:fs';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import * as db from './store.mjs';
import { historyFile } from './paths.mjs';
import { diffIndex } from './ui/live-delta.js';

const RING = 64;
const POLL_MS = 1000;
const WATCH_DEBOUNCE_MS = 40;

function fileKey(file) {
  try {
    const st = statSync(file, { bigint: true });
    return `${st.ino}:${st.size}:${st.mtimeNs}`;
  } catch (error) {
    if (error.code === 'ENOENT') return 'missing';
    throw error;
  }
}

// One JSON body, encoded once, served many times.
export function encodeBody(value, key = 'body') {
  const raw = Buffer.from(JSON.stringify(value));
  const etag = `"tower-${key}-${createHash('sha1').update(raw).digest('hex').slice(0, 16)}"`;
  let gzip = null;
  return { raw, etag, gzip: () => (gzip ??= gzipSync(raw)) };
}

export function createLiveBoard(store, { decorate = (index) => index } = {}) {
  const liveFile = join(store.dataDir, 'tower.json');
  const archiveFile = historyFile(store.dataDir);
  let liveKey = null;
  let state = null;
  let index = null;          // { rev, value, body }
  let archiveKey = null;
  let archive = null;
  let closed = null;         // { rev, archiveKey, body }
  const ring = [];           // { base, rev, frame }
  const subscribers = new Set();

  const take = (next) => {
    if (state === next) return false;
    state = next;
    closed = null;
    return true;
  };

  // Make the in-memory board current with disk. Stat first, then load: if a
  // writer lands in between, the next check sees a newer key and reloads.
  function sync() {
    const key = fileKey(liveFile);
    if (key === liveKey && state) return false;
    const next = store.loadLive();
    liveKey = key;
    return take(next);
  }

  // A write through this process already holds the new state object; adopt
  // it instead of re-reading what was just written. The store lock is
  // released synchronously before this runs and any other writer needs far
  // longer than this tick to take it, read, and replace the file.
  function adopt(next) {
    liveKey = fileKey(liveFile);
    return take(next);
  }

  function project() {
    if (index && index.rev === state.meta.rev && index.state === state) return index;
    const value = decorate(db.projectBoard(state, store.config));
    const prev = index;
    index = { rev: state.meta.rev, state, value, body: encodeBody(value, 'state') };
    if (prev && subscribers.size) {
      const frame = { boot: value.boot, base: prev.rev, rev: index.rev, patch: diffIndex(prev.value, value) };
      const entry = { base: prev.rev, rev: index.rev, text: JSON.stringify(frame) };
      ring.push(entry);
      if (ring.length > RING) ring.shift();
      for (const fn of [...subscribers]) fn(entry);
    } else {
      ring.length = 0;
    }
    return index;
  }

  // The archive changes rarely (retire passes, restores); read it alone
  // when its file changes.
  function loadArchive() {
    const key = fileKey(archiveFile);
    if (archive && key === archiveKey) return archive;
    const next = db.loadHistoryRaw(store.dataDir);
    archiveKey = key;
    archive = next;
    closed = null;
    return archive;
  }

  // Deltas a client at `rev` still needs, or null when the ring cannot
  // bridge the gap (the client then gets the full index).
  function since(rev) {
    if (rev === index?.rev) return [];
    const start = ring.findIndex(entry => entry.base === rev);
    if (start < 0) return null;
    const chain = ring.slice(start);
    for (let i = 1; i < chain.length; i++) if (chain[i].base !== chain[i - 1].rev) return null;
    return chain.at(-1).rev === index?.rev ? chain : null;
  }

  let timer = null;
  const check = () => {
    timer = null;
    if (!subscribers.size) return;
    try {
      if (sync()) project();
    } catch (error) { console.error('tower: cannot refresh live state', error); }
  };
  const nudge = () => { if (!timer) timer = setTimeout(check, WATCH_DEBOUNCE_MS); };
  let watcher = null;
  try {
    watcher = watch(store.dataDir, { persistent: false }, (_event, name) => {
      if (name === 'tower.json' || name === 'history.json') nudge();
    });
    watcher.on('error', () => {});
  } catch { /* the stat poll below still catches every change */ }
  const poll = setInterval(check, POLL_MS);
  poll.unref();

  sync();

  return {
    // Current live state (read-only: never mutate the returned object).
    state() { sync(); return state; },
    // The state already in memory, without waiting for a reload. A pending
    // change is picked up right after this request; subscribed clients then
    // get its delta. Used where an answer now beats one ~150 ms fresher.
    peek() {
      if (fileKey(liveFile) !== liveKey) {
        setImmediate(() => {
          try { if (sync() && subscribers.size) project(); } catch (error) { console.error('tower: cannot refresh live state', error); }
        });
      }
      return state;
    },
    index() { sync(); return project(); },
    archive: loadArchive,
    adopt(next) { adopt(next); return project(); },
    closed() {
      sync();
      const h = loadArchive();
      if (closed && closed.rev === state.meta.rev && closed.archiveKey === archiveKey) return closed;
      closed = { rev: state.meta.rev, archiveKey, body: encodeBody(db.projectClosed(state, store.config, h), 'closed') };
      return closed;
    },
    since,
    subscribe(fn) { subscribers.add(fn); return () => subscribers.delete(fn); },
    close() {
      clearInterval(poll);
      clearTimeout(timer);
      watcher?.close();
      subscribers.clear();
    },
  };
}
