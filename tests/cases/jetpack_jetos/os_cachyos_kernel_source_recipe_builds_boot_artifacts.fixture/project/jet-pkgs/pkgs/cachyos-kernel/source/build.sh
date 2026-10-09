#!/bin/sh
set -eu
printf 'MZ built cachyos kernel\nHdrS\n' > "$JETOS_KERNEL_OUT/vmlinuz-cachyos"
printf '070701 built cachyos initrd\n' > "$JETOS_KERNEL_OUT/initrd-cachyos"
