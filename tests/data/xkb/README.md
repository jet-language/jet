# XKB keysym and Compose data

`core.ui.xkb` pins libxkbcommon 1.13.2 keysyms and the libX11 1.8.13
en_US.UTF-8 Compose table.

- `xkbcommon-keysyms.h` is
  `https://raw.githubusercontent.com/xkbcommon/libxkbcommon/xkbcommon-1.13.2/include/xkbcommon/xkbcommon-keysyms.h`.
  It carries its own X11/MIT-style license notices.
- `Compose` is the en_US.UTF-8 Compose table that libX11 1.8.13 installs as
  `share/X11/locale/en_US.UTF-8/Compose`, which it generates from
  `nls/en_US.UTF-8/Compose.pre`.
- `COPYING.libX11` is libX11 1.8.13's `COPYING`, from
  `https://gitlab.freedesktop.org/xorg/lib/libx11/-/raw/libX11-1.8.13/COPYING`.
  It is the license of `Compose`.

`SHA256SUMS` pins every vendored file. `Tools/agent/XkbTables.jet` checks the
header and the Compose table against it before it generates
`Core/Ui/XkbTables.jet` and its data files in `Core/Ui/XkbTables/`, the
keysym names, keysym texts and Compose trie behind `core.ui.xkb`, which
`Core/Ui/XkbTables.jet` embeds at build time:

```sh
jet Tools/agent/XkbTables.jet          # rewrite the tables
jet Tools/agent/XkbTables.jet --check  # fail when a table is stale
```

To move to a new release, replace the files, update `SHA256SUMS` and the
release numbers in this file and in `Tools/agent/XkbTables.jet`, then
regenerate. The generated tables are checked in. Jet programs never read
these files or use the network at build time or runtime.
