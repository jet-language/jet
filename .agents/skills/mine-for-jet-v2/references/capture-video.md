# Video or talk: transcript first

Read metadata/linked primary sources with static readers; browser only when
needed. Retrieve subtitles/comments without downloading video by default:

```sh
Tools/agent/jet-env nix shell nixpkgs#yt-dlp --command yt-dlp \
  --skip-download --write-subs --write-auto-subs --sub-langs 'en.*' \
  --sub-format json3 --write-comments --write-info-json \
  -o "$CAPTURE_ROOT/%(id)s.%(ext)s" 'VIDEO_URL'
Tools/agent/jet-env node .agents/skills/mine-for-jet-v2/scripts/transcript.mjs \
  --input "$CAPTURE_ROOT/VIDEO_ID.en.json3" --output "$CAPTURE_ROOT/transcript.txt"
```

Prefer creator captions, record caption source/language; auto/unknown captions
are not high-confidence technical evidence without corroboration. Cleanup is
not verification. Missing captions need an available authorized transcription
path or explicit gap; never infer thesis from title/description/popularity.

Read **all transcript chunks**, one ≤ 5-minute segment at a time, writing that
segment's ledger while reading. Subtitle retrieval alone does not establish body review. Keep
caption-only versus visually inspected claims distinct. Retrieve localized
frames only when on-screen spelling, code, diagrams or results matter; record
frame timestamp/OCR uncertainty, and verify exact code against linked primary
sources where available. Do not download/sweep frames for caption-only claims.

When audience review is relevant, preserve root/reply counts and boundaries,
substantive corrections and linked-source identities. Report API warnings and
incomplete threads; deduplicated caption lines/comment counts are not complete
technical coverage. A linked paper/artifact remains a distinct reviewed source.
