#!/bin/bash
set -e

# 1. Compile the kernel for bare metal
cargo build --target x86_64-unknown-none --release

# 2. Prepare the ISO directory
mkdir -p iso_root

# 3. Copy the compiled kernel
cp target/x86_64-unknown-none/release/chronos iso_root/

# 4. Copy the config
cp limine.cfg iso_root/

# 5. Copy the Limine bootloader binaries (from the folder you cloned earlier)
# ADJUST THIS PATH to where you cloned limine in Step 1
cp ../limine/limine-bios.sys ../limine/limine-bios-cd.bin ../limine/limine-uefi-cd.bin iso_root/

# 6. Create the ISO
xorriso -as mkisofs -b limine-bios-cd.bin \
        -no-emul-boot -boot-load-size 4 -boot-info-table \
        --efi-boot limine-uefi-cd.bin \
        -efi-boot-part --efi-boot-image --protective-msdos-label \
        iso_root -o chronos.iso

# 7. Install Limine to the ISO (BIOS boot support)
../limine/limine deploy chronos.iso

echo "Build complete: chronos.iso"