# Systems: file index

Build a native file indexer. Read a generated tree with regular files,
symlinks, unreadable entries, Unicode names, and a 1 GiB sparse file. Emit a
sorted path/hash/size report. Hostile input includes a broken symlink and a
path that attempts to escape the declared root. Beginner mode uses safe
defaults. Expert mode selects worker count, hash, and follow-link policy.
