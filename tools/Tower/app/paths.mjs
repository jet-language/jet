// Tower v2 — paths + tiny shared utilities. No dependencies.
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
export const APP = resolve(dirname(fileURLToPath(import.meta.url)));
export const UI = join(APP, "ui");

export const P = {
  sidequests: join(ROOT, "tools/Tower/docs/sidequests"),
  proposals: join(ROOT, "tools/Tower/docs/proposals"),
  plansDir: join(ROOT, "tools/Tower/docs/plans"),
  ideasDir: join(ROOT, "tools/Tower/docs/ideas"),
  ballotMd: join(ROOT, "tools/Tower/docs/ballots/decision-ballots.md"),
  ratified: join(ROOT, "docs/spec/syntax-decisions.md"),
  results: join(ROOT, "tools/Tower/docs/ballots/ballot-results.md"),
  board: join(ROOT, "tools/Tower/board.json"),
  regenQueue: join(ROOT, "tools/Tower/regen-queue.md"),
  askQueue: join(ROOT, "tools/Tower/questions-queue.md"),
  ingestQueue: join(ROOT, "tools/Tower/ingest-queue.md"),
};

export const read = (p) => (existsSync(p) ? readFileSync(p, "utf8") : "");
export const rel = (p) => p.replace(ROOT + "/", "");

export const now = () => new Date().toISOString();
export const stamp = () => now().replace("T", " ").slice(0, 16);
export const newId = () => "c" + Date.now().toString(36) + Math.floor(Math.random() * 1e4).toString(36);

// console color + io
export const C = { dim: "\x1b[2m", b: "\x1b[1m", grn: "\x1b[32m", yel: "\x1b[33m", cyn: "\x1b[36m", rst: "\x1b[0m" };
export const out = (s = "") => process.stdout.write(s + "\n");
export const die = (s) => { process.stderr.write(s + "\n"); process.exit(1); };
export const truncate = (s, n) => (s.length > n ? s.slice(0, n - 1) + "…" : s);
