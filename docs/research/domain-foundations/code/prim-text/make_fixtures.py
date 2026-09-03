from pathlib import Path

root = Path(__file__).parent
fixtures = root / "fixtures"
fixtures.mkdir(parents=True, exist_ok=True)

# Repeated multilingual text makes both Unicode segmentation and BPE pairs observable.
seed = (
    "Hello café κόσμε नमस्ते سلام — Jet tokenizer search. "
    "The quick brown fox jumps over 42 logs; user_id=31415 level=INFO.\n"
)
# Exactly 1 MiB of valid UTF-8 corpus for tokenizer training.
corpus = (seed * ((1_048_576 // len(seed.encode("utf-8"))) + 1)).encode("utf-8")[:1_048_576]
(fixtures / "corpus_1m.txt").write_bytes(corpus)

# Exactly 100,000,000 bytes of newline-delimited logs. The final line is complete.
log_seed = (
    "2026-09-02T00:00:00Z INFO api request user_id=31415 path=/orders status=200\n"
    "2026-09-02T00:00:01Z WARN api retry user_id=27182 path=/orders status=503\n"
    "2026-09-02T00:00:02Z ERROR db timeout user_id=16180 path=/orders status=500\n"
)
need = 100_000_000
out = bytearray()
while len(out) + len(log_seed.encode("ascii")) <= need:
    out.extend(log_seed.encode("ascii"))
if len(out) < need:
    out.extend(b"X" * (need - len(out)))
(fixtures / "logs_100m.txt").write_bytes(out)
print(f"corpus_1m_bytes={len(corpus)}")
print(f"logs_100m_bytes={len(out)}")
