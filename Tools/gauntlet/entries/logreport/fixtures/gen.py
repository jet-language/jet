#!/usr/bin/env python3
import random
import sys
from datetime import datetime, timedelta, timezone


LEVELS = ("DEBUG", "INFO", "WARN", "ERROR")
LEVEL_WEIGHTS = (40, 45, 10, 5)
COMPONENTS = (
    "api",
    "auth",
    "cache",
    "db",
    "jobs",
    "mailer",
    "payments",
    "queue",
    "search",
    "storage",
    "worker",
    "web",
)
MESSAGES = (
    "request accepted",
    "request completed",
    "connection opened",
    "connection closed",
    "cache hit",
    "cache miss",
    "retry scheduled",
    "job started",
    "job finished",
    "worker ready",
    "worker stopped",
    "query prepared",
    "query completed",
    "queue drained",
    "queue backpressure",
    "token refreshed",
    "token expired",
    "file written",
    "file rotated",
    "health check passed",
    "health check failed",
    "slow operation",
    "payload rejected",
    "payload accepted",
    "session created",
    "session closed",
    "upstream healthy",
    "upstream unavailable",
    "configuration loaded",
    "shutdown requested",
)


def main() -> None:
    output = sys.argv[1]
    rng = random.Random(13)
    timestamp = datetime(2026, 1, 1, tzinfo=timezone.utc)
    with open(output, "w", encoding="utf-8") as stream:
        for _ in range(300_000):
            timestamp += timedelta(seconds=rng.randrange(1, 60))
            level = rng.choices(LEVELS, weights=LEVEL_WEIGHTS, k=1)[0]
            component = rng.choice(COMPONENTS)
            message = rng.choice(MESSAGES)
            stream.write(f"{timestamp.isoformat().replace('+00:00', 'Z')} {level} {component} {message}\n")


if __name__ == "__main__":
    main()
