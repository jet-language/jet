import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
const root = dirname(new URL(import.meta.url).pathname);
const fixtures = join(root, "fixtures");
mkdirSync(fixtures, { recursive: true });
const seed = "Hello café κόσμε नमस्ते سلام — Jet tokenizer search. The quick brown fox jumps over 42 logs; user_id=31415 level=INFO.\n";
const seedBytes = Buffer.from(seed, "utf8");
const corpus = Buffer.alloc(1_048_576);
for (let at = 0; at < corpus.length; at += seedBytes.length) {
  seedBytes.copy(corpus, at, 0, Math.min(seedBytes.length, corpus.length - at));
}
writeFileSync(join(fixtures, "corpus_1m.txt"), corpus);
const logSeed = Buffer.from(
  "2026-09-02T00:00:00Z INFO api request user_id=31415 path=/orders status=200\n" +
  "2026-09-02T00:00:01Z WARN api retry user_id=27182 path=/orders status=503\n" +
  "2026-09-02T00:00:02Z ERROR db timeout user_id=16180 path=/orders status=500\n",
  "ascii",
);
const logs = Buffer.alloc(100_000_000);
for (let at = 0; at < logs.length; at += logSeed.length) {
  logSeed.copy(logs, at, 0, Math.min(logSeed.length, logs.length - at));
}
writeFileSync(join(fixtures, "logs_100m.txt"), logs);
console.log(`corpus_1m_bytes=${corpus.length}`);
console.log(`logs_100m_bytes=${logs.length}`);
