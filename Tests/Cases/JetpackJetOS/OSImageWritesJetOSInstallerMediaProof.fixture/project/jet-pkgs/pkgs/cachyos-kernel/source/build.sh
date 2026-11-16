#!/bin/sh
set -eu
if [ -n "${JETOS_CACHYOS_KERNEL:-}" ] && [ -n "${JETOS_CACHYOS_INITRD:-}" ]; then
    modules="${JETOS_CACHYOS_MODULES:-}"
    if [ -n "$modules" ] && [ ! -d "$modules/kernel" ] && [ -d "$modules/lib/modules" ]; then
        for candidate in "$modules"/lib/modules/*; do
            if [ -d "$candidate/kernel" ]; then
                modules="$candidate"
                break
            fi
        done
    fi
    cp "$JETOS_CACHYOS_KERNEL" "$JETOS_KERNEL_OUT/vmlinuz-cachyos"
    cp "$JETOS_CACHYOS_INITRD" "$JETOS_KERNEL_OUT/initrd-cachyos"
    if [ -n "$modules" ] && [ -e "$modules/kernel/fs/isofs/isofs.ko.xz" ]; then
        mkdir -p "$JETOS_KERNEL_OUT/modules"
        cp "$modules/kernel/fs/isofs/isofs.ko.xz" "$JETOS_KERNEL_OUT/modules/isofs.ko.xz"
    fi
    if [ -n "$modules" ] && [ -e "$modules/kernel/drivers/gpu/drm/tiny/bochs.ko.xz" ]; then
        mkdir -p "$JETOS_KERNEL_OUT/modules"
        cp "$modules/kernel/drivers/gpu/drm/tiny/bochs.ko.xz" "$JETOS_KERNEL_OUT/modules/bochs.ko.xz"
    fi
    for module in kernel/fs/fat/fat.ko.xz kernel/fs/fat/vfat.ko.xz kernel/fs/nls/nls_ascii.ko.xz kernel/fs/nls/nls_cp437.ko.xz kernel/drivers/input/serio/serio.ko.xz kernel/drivers/input/serio/i8042.ko.xz kernel/drivers/input/serio/libps2.ko.xz kernel/drivers/input/keyboard/atkbd.ko.xz kernel/drivers/hid/hid-generic.ko.xz kernel/drivers/hid/usbhid/usbhid.ko.xz kernel/drivers/usb/host/uhci-hcd.ko.xz kernel/drivers/usb/host/ehci-hcd.ko.xz kernel/drivers/usb/host/xhci-hcd.ko.xz; do
        if [ -n "$modules" ] && [ -e "$modules/$module" ]; then
            mkdir -p "$JETOS_KERNEL_OUT/modules"
            cp "$modules/$module" "$JETOS_KERNEL_OUT/modules/$(basename "$module")"
        fi
    done
    exit 0
fi
printf 'MZ fixture-built cachyos kernel\nHdrS\n' > "$JETOS_KERNEL_OUT/vmlinuz-cachyos"
printf '070701 fixture-built cachyos initrd\n' > "$JETOS_KERNEL_OUT/initrd-cachyos"
