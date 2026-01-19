#!/bin/bash
set -e

# 1. Compile
cargo build --target x86_64-unknown-none --release

# 2. Prepare ISO folder
mkdir -p iso_root
cp target/x86_64-unknown-none/release/chronos iso_root/
cp limine.cfg iso_root/

# NEW: Copy the text file to the ISO
cp welcome.txt iso_root/

# 3. Copy Limine binaries
cp ../limine/limine-bios.sys ../limine/limine-bios-cd.bin ../limine/limine-uefi-cd.bin iso_root/

# 4. Create ISO
xorriso -as mkisofs -b limine-bios-cd.bin \
        -no-emul-boot -boot-load-size 4 -boot-info-table \
        --efi-boot limine-uefi-cd.bin \
        -efi-boot-part --efi-boot-image --protective-msdos-label \
        iso_root -o chronos.iso

# 5. Deploy Limine
../limine/limine deploy chronos.iso

echo "Build complete."