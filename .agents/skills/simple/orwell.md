# Orwell’s rules

Apply on every natural-language sentence with `simple*`.

1. **No stale figures of speech.** Do not use a metaphor, simile, or other figure
   of speech that you are used to seeing in print.
2. **Short word over long word.** Never use a long word where a short one will do.
3. **Cut needless words.** If you can cut a word out, cut it out.
4. **Active over passive.** Never use the passive where you can use the active.
5. **Everyday word over jargon.** Never use a foreign phrase, a scientific word,
   or a jargon word if an everyday English equivalent will do.
6. **Do not sound barbarous.** Break any of these rules sooner than say anything
   outright barbarous.

Source: George Orwell, “Politics and the English Language” (1946).

## With STE / Jet

| Tension | Resolution |
|---------|------------|
| #3 vs STE “keep articles” | Cut filler. Keep required grammar words. |
| #5 vs Jet technical nouns | Keep exact Jet/API/code terms. Cut decorative jargon only. |
| #4 vs unknown-agent passive | Prefer active. Unknown agent in a description may stay passive. |
| #6 vs stacked style | Clarity and truth win. See [priority.md](priority.md). |
| Code / identifiers / errors | Never rewrite into synonyms. |

## Quick rejects

| Reject | Prefer |
|--------|--------|
| “at the end of the day”, “move the needle”, “low-hanging fruit” | State the fact |
| “utilize”, “facilitate”, “leverage” | `use`, `help` |
| “in order to”, “due to the fact that” | `to`, `because` |
| “it should be noted that” | Delete; state the fact |
| “a number of” | `some` / the real count |
| “going forward”, “robust solution” | Say what changes |
