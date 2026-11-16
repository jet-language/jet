# CLI: archive filter

Build a command-line inspector for one binary USTAR archive.
The inspector reads the archive bytes, checks each 512-byte header, and reports matching entries.

The beginner command is `tool INPUT`.
It uses `format=tar`, `filter=*.md`, `max-size=64`, and `max-entries=10000`.
It writes the report to stdout.

The expert command accepts these named options:

- `--format tar|ustar`
- `--filter GLOB`
- `--max-size N`
- `--max-entries N`
- `--output FILE`

`tar` accepts empty or `ustar` magic.
`ustar` requires `ustar` magic.
The filter supports `*` and `?` over the complete entry path.
The size limit applies to each claimed entry size.
The entry limit applies to headers in archive order.

The inspector rejects empty names, absolute paths, `..` path components, duplicate names, bad checksums, bad sizes, unknown types, truncated payloads, and non-zero trailer bytes.
It counts each rejected header once and continues when the next 512-byte boundary is known.
It sorts matching rows by path.
Each row has the form `path=PATH|type=TYPE|size=BYTES`.
The report includes `format`, `entries`, `accepted`, `bytes`, `rejected`, `traversal`, `duplicates`, `malformed`, `oversize`, and `limited`.
