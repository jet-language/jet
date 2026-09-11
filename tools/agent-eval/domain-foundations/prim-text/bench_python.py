import re
import time
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).parent
corpus_path = ROOT / "fixtures/corpus_1m.txt"
log_path = ROOT / "fixtures/logs_100m.txt"

corpus = corpus_path.read_text(encoding="utf-8")
corpus_bytes = corpus_path.stat().st_size
started = time.perf_counter_ns()
words = corpus.split()
sequences = [list(word) for word in words]
merges = []
for _ in range(12):
    counts = Counter(pair for seq in sequences for pair in zip(seq, seq[1:]))
    if not counts:
        break
    best, count = max(counts.items(), key=lambda item: item[1])
    merges.append(best)
    merged = "".join(best)
    next_sequences = []
    for seq in sequences:
        out = []
        index = 0
        while index < len(seq):
            if index + 1 < len(seq) and (seq[index], seq[index + 1]) == best:
                out.append(merged)
                index += 2
            else:
                out.append(seq[index])
                index += 1
        next_sequences.append(out)
    sequences = next_sequences
token_count = sum(map(len, sequences))
bpe_ns = time.perf_counter_ns() - started

sample_tokens = [*"Hello", "é", *"café", *"नमस्ते", *"سلام"]
started = time.perf_counter_ns()
log = log_path.read_text(encoding="utf-8")
pattern = re.compile(r"ERROR .* user_id=\d+")
line_count = 0
match_count = 0
for line in log.splitlines():
    line_count += 1
    if pattern.search(line):
        match_count += 1
scan_ns = time.perf_counter_ns() - started
print(f"bpe bytes={corpus_bytes} words={len(words)} merges={len(merges)} tokens={token_count} sample_tokens={len(sample_tokens)} elapsed_ms={bpe_ns / 1_000_000:.3f}")
print(f"scan bytes={log_path.stat().st_size} lines={line_count} matches={match_count} elapsed_ms={scan_ns / 1_000_000:.3f}")
