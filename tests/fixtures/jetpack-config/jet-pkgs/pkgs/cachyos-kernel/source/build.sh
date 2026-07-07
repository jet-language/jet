#!/bin/sh
set -eu
if [ -n "${JETOS_CACHYOS_KERNEL:-}" ] && [ -n "${JETOS_CACHYOS_INITRD:-}" ]; then
    cp "$JETOS_CACHYOS_KERNEL" "$JETOS_KERNEL_OUT/vmlinuz-cachyos"
    cp "$JETOS_CACHYOS_INITRD" "$JETOS_KERNEL_OUT/initrd-cachyos"
    if [ -n "${JETOS_CACHYOS_MODULES:-}" ] && [ -e "$JETOS_CACHYOS_MODULES/kernel/fs/isofs/isofs.ko.xz" ]; then
        mkdir -p "$JETOS_KERNEL_OUT/modules"
        cp "$JETOS_CACHYOS_MODULES/kernel/fs/isofs/isofs.ko.xz" "$JETOS_KERNEL_OUT/modules/isofs.ko.xz"
    fi
    if [ -n "${JETOS_CACHYOS_MODULES:-}" ] && [ -e "$JETOS_CACHYOS_MODULES/kernel/drivers/gpu/drm/tiny/bochs.ko.xz" ]; then
        mkdir -p "$JETOS_KERNEL_OUT/modules"
        cp "$JETOS_CACHYOS_MODULES/kernel/drivers/gpu/drm/tiny/bochs.ko.xz" "$JETOS_KERNEL_OUT/modules/bochs.ko.xz"
    fi
    for module in kernel/fs/fat/fat.ko.xz kernel/fs/fat/vfat.ko.xz kernel/fs/nls/nls_ascii.ko.xz kernel/fs/nls/nls_cp437.ko.xz; do
        if [ -n "${JETOS_CACHYOS_MODULES:-}" ] && [ -e "$JETOS_CACHYOS_MODULES/$module" ]; then
            mkdir -p "$JETOS_KERNEL_OUT/modules"
            cp "$JETOS_CACHYOS_MODULES/$module" "$JETOS_KERNEL_OUT/modules/$(basename "$module")"
        fi
    done
    exit 0
fi
printf 'MZ fixture-built cachyos kernel\nHdrS\n' > "$JETOS_KERNEL_OUT/vmlinuz-cachyos"
printf '070701 fixture-built cachyos initrd\n' > "$JETOS_KERNEL_OUT/initrd-cachyos"
