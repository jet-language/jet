# #3125 — MIME type parsing and extension mapping (SCRIPT-F38, D-URL1=A)

Question: does `core.net.mime` keep type/subtype plus parameters with RFC 9110
quoting, give explicit outcomes for malformed types and unknown extensions,
and serve as the one typed representation that HTTP and email use?

Binary: `jet-debug-snapshot14` (safe-jet.sh), 2026-09-29. Probe:
`~/.cache/jet-test-scratch/Closer04/mime_probe.jet`.

## Evidence

`mime_probe.jet` matches `mime.parse(text)` against `.Ok(m)` / `.Err(_)`.
`jet run` and `jet run --interpret` printed identical output:

```
upper: ok top=text sub=html                 # "Text/HTML; CharSet=UTF-8"
quoted-semi: MIMEError                      # text/plain; name="a;b"; charset=utf-8  (valid per RFC 9110)
escaped-quote: ok top=text sub=plain        # text/plain; name="a\"b"
duplicate: ok top=text sub=plain            # charset=utf-8; charset=latin1  (accepted silently)
unterminated: ok top=text sub=plain         # text/plain; name="abc  (accepted)
empty-name: MIMEError                       # text/plain; =x
no-slash: MIMEError                         # textplain
unknown ext: none
unknown mime: none
```

AOT (`jet build mime_probe.jet`) does not build: rustc E0063 `missing field
'params' in initializer of 'JetMIME'` (generated mime_probe.rs:142945). The
Jet `MIME{top, sub}` value (Core/net/mime.jet:12-15) is lowered into the host
`JetMIME`, which has a third field.

The receiver methods are unreachable from the Core type. Calling
`m.essence()` or `m.param("charset")` on an `.Ok(m)` binding gives E0102
"`…mime.jet::MIME` has no method `param`". A direct `h :: mime.parse(...)`
inside a `-> String MIMEError!` function gives the same E0102. In `run()` it
gives E2402 (no `MIMEError -> Err` conversion). `MIMEError` has no `Display`
and cannot be shown with `:Debug` (E0112).

The existing golden `net/url_mime` fails on all three tiers
(`golden_diff.sh net/url_mime`):
- run and interpret: exit 1 with no stdout;
- AOT: E2402 at url_mime.jet:6/20/25 and E0102 `URL` has no method
  `to_string` (url_mime.jet:26).

Source read for c3: media types are still spelled as strings in
- Core/http/client.jet:262,268 (`session_header(…, "Content-Type", "application/json")`);
- Core/http/http.jet:866-867 (`"application/json; charset=utf-8"`, `"text/plain; charset=utf-8"`);
- Core/email/email.jet:264, 1433, 1450 (`"{mime}; charset=utf-8"`, multipart boundary splicing).

## Verdicts

- c1 (normalization and quoting): FAIL. Parameters are parsed and then
  discarded (mime.jet:44-59), a quoted `;` is rejected, an unterminated
  quote is accepted, a duplicate `charset` is accepted, and the parameter
  values cannot be observed.
- c2 (explicit outcomes): PARTIAL. An unknown extension or type gives
  `None` on run and interpret. Malformed input gives the payload-less
  `MIMEError.Syntax`, which cannot be displayed, and the unterminated case
  is wrongly accepted.
- c3 (canonical typed use by HTTP/web/email): FAIL. Those modules splice
  strings (see the list above).
- c4 (url_mime golden with the new cases): FAIL. The current golden is red
  on every tier, and the new cases would record wrong behaviour.

## Follow-ups

The implementation in the card plan (params in `MIME`, RFC 9110 parser,
removal of the host `JetMIME` second meaning, consumers via `mime.parse`)
remains to be done. The defects are listed in the closer JSON.
