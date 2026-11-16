import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: structured_data INPUT_FILE");
}

try {
  const payload = JSON.parse(readFileSync(process.argv[2], "utf8"));
  if (!Array.isArray(payload.events)) throw new Error("events");
  for (const event of payload.events) {
    if (
      event === null ||
      typeof event.service !== "string" ||
      !Number.isInteger(event.duration_ms)
    ) throw new Error("event");
  }
  const summaries = [...new Set(payload.events.map((event) => event.service))]
    .sort()
    .map((service) => {
      const rows = payload.events.filter((event) => event.service === service);
      return {
        service,
        count: rows.length,
        total_ms: rows.reduce((total, event) => total + event.duration_ms, 0),
      };
    });
  console.log(JSON.stringify({ total_events: payload.events.length, summaries }));
} catch {
  console.log("invalid-json");
}
