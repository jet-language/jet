#!/usr/bin/env node
// Tower — the command surface for building Jet. Sequences the workflow:
//   frozen → backlog → deciding → planning → ready → building → done
// plus a decision Focus Mode (the ballot) and a file-ingest queue. No deps.
//
// Usage:
//   node tools/Tower/Tower.mjs serve [port] [--open]   # the dashboard (main UI)
//   node tools/Tower/Tower.mjs status                  # console snapshot
//   node tools/Tower/Tower.mjs new <slug> "Title"      # scaffold a sidequest plan
//
// Owner-input state (cards/notes/scratch/answers/questions/ingest) lives in
// tools/Tower/board.json — management state only; it references plan files by
// slug and never copies their content. The ballot renders straight from
// tools/Tower/docs/ballots/decision-ballots.md. Implementation lives in app/.
import { die } from "./app/paths.mjs";
import { serve } from "./app/server.mjs";
import { status, scaffold } from "./app/cli.mjs";

const [cmd, ...rest] = process.argv.slice(2);
switch (cmd) {
  case undefined:
  case "status": status(); break;
  case "serve": serve(Number(rest.find((a) => /^\d+$/.test(a))) || 4173); break;
  case "new": scaffold(rest[0], rest.slice(1).join(" ")); break;
  default: die(`unknown command "${cmd}". commands: status | serve [port] [--open] | new <slug> "Title"`);
}
