# Complete ballot exemplar

This is a submit-ready shape, not a copyable decision. Replace every fact with
current card evidence. The recommended option is `A` and appears first in both
option lists; `whyNot` covers the losing option `B`.

```json
{
  "cardId": "#12",
  "id": "D-CACHE1",
  "title": "Cache invalidation strategy",
  "group": "architecture",
  "ballotMode": "full",
  "surface": {
    "gist": "How should cached prices expire?",
    "lesson": "A cache keeps a reusable copy of expensive work. This choice sets how old a price may become and how quickly a new price reaches customers.",
    "trio": {
      "current": {
        "note": "The current cache has no expiry rule, so a changed price can stay stale.",
        "code": "price = cache.get(\"price\")"
      },
      "wild": {
        "lang": "Rails",
        "note": "Rails documents an explicit expiry time for each cached value. See https://guides.rubyonrails.org/caching_with_rails.html.",
        "code": "Rails.cache.write(\"price\", value, expires_in: 5.minutes)"
      }
    },
    "options": [
      {
        "key": "A",
        "name": "TTL per entry",
        "gist": "Expire each price after a bounded time.",
        "gains": ["Works without changing every writer."],
        "losses": ["A price can stay stale until its timer expires."],
        "proposed": {
          "code": "price = cache.get(\"price\", ttl: 300)"
        }
      },
      {
        "key": "B",
        "name": "Event-driven purge",
        "gist": "Purge a price whenever its source changes.",
        "gains": ["A changed price reaches readers quickly."],
        "losses": ["Every writer must publish a correct purge event."],
        "proposed": {
          "code": "price = cache.get(\"price\")\nsource.on_change { cache.purge(\"price\") }"
        }
      }
    ],
    "recommendation": {
      "rec": "A",
      "why": "A bounded timer works for every writer without requiring a new event path.",
      "gains": ["It bounds staleness with one local rule."],
      "losses": [
        {
          "loss": "A price can stay stale until its timer expires.",
          "whyUnavoidable": "Without a source event, elapsed time is the only change signal."
        }
      ],
      "whyNot": [
        {
          "key": "B",
          "reason": "B refreshes sooner but relies on every writer publishing correctly."
        }
      ],
      "tradeoff": "The service accepts bounded staleness to avoid a required event path."
    }
  },
  "gist": "How should cached prices expire?",
  "lesson": "A cache keeps a reusable copy of expensive work. This choice sets how old a price may become and how quickly a new price reaches customers.",
  "story": "Dana ships a pricing page. A vendor changes a rate at 9am, and customers must see a safe result without a hidden invalidation path.",
  "inWild": "Rails.cache.write(\"price\", value, expires_in: 5.minutes)\n# https://guides.rubyonrails.org/caching_with_rails.html",
  "options": [
    {
      "key": "A",
      "name": "TTL per entry",
      "detail": "Expire each price after a bounded time. It works without changing every writer, but a price may stay stale until the timer expires.",
      "technical": "The cache stores an expiry timestamp with each entry and rejects entries past that timestamp.",
      "code": "price = cache.get(\"price\", ttl: 300)"
    },
    {
      "key": "B",
      "name": "Event-driven purge",
      "detail": "Purge a price whenever its source changes. It reaches readers quickly, but every writer must publish a correct purge event.",
      "technical": "Each source mutation emits a named event that invalidates matching cache keys.",
      "code": "price = cache.get(\"price\")\nsource.on_change { cache.purge(\"price\") }"
    }
  ],
  "comparisons": [
    {
      "lang": "Rails",
      "note": "Rails uses an explicit per-entry expiry time; official guide: https://guides.rubyonrails.org/caching_with_rails.html.",
      "code": "Rails.cache.write(\"price\", value, expires_in: 5.minutes)"
    }
  ],
  "rec": "A",
  "recommendation": {
    "why": "A bounded timer works for every writer without requiring a new event path.",
    "whyNot": [
      {
        "key": "B",
        "reason": "B refreshes sooner but relies on every writer publishing correctly."
      }
    ],
    "tradeoff": "The service accepts bounded staleness to avoid a required event path."
  },
  "reviewPasses": {
    "beginner": "Fresh agent: reader-17. Skill: rli5. The beginner pass tested the complete ballot and found one undefined term; the lesson was repaired.",
    "adversarial": "Author model family: family-a. Adversarial model family: family-a. Fresh agent: reader-18. The adversarial review attacked the recommendation and repaired one failure mode."
  }
}
```

Use the scaffold as the local authoring file:

```sh
mkdir -p ~/.cache/jet-luna
tower decision scaffold '#12' --id D-CACHE1 --out ~/.cache/jet-luna/ballot.json
```

Complete the fields and required reader evidence before submission, including
for a full draft. Follow the [canonical submission and ready transition](../SKILL.md#mechanics).
Adding the scaffold alone preserves its draft state; read back the non-draft
decision and owner lane after `--ready`. The owner alone ratifies it.
