# CLI: archive filter

Build a command-line archive inspector. Read tar input, filter entries by
glob and size, print deterministic metadata, and reject traversal, duplicate
names, malformed headers, and oversized claims. Beginner mode takes one input
and writes stdout. Expert mode selects format, filter, limits, and output
file.
