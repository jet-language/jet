# Capture a video or talk

Use browser tools for metadata and linked primary sources. If YouTube does not
expose transcript or comments through browser tools, use `yt-dlp`:

```sh
nix shell nixpkgs#yt-dlp --command yt-dlp \
  --skip-download \
  --write-subs --write-auto-subs --sub-langs 'en.*' --sub-format json3 \
  --write-comments --write-info-json \
  -o 'target-mine-ID/%(id)s.%(ext)s' \
  'VIDEO_URL'
```

Prefer creator subtitles over auto-captions. Label auto-caption uncertainty.
Creator captions (`subtitles` in the info JSON) are high quality; auto or
unknown captions are not high-confidence evidence without corroboration.

If captions are absent, use an available transcription path or report the gap.
Never infer the argument from the title or description alone.

Parse json3 into `target-mine-ID/transcript.txt` as `[mm:ss] text` lines. Drop
empty events, deduplicate, and merge progressive auto-caption updates while
retaining start times. Read the transcript in timestamp chunks.

Retrieve comments broadly when audience evidence is in scope. Keep root and
reply counts. Record incomplete threads and API warnings.
