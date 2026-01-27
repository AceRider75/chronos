#!/bin/bash
set -e

# 0. Ensure disk.img exists and is formatted
if [ ! -f disk.img ]; then
    echo "Creating and formatting disk.img..."
    qemu-img create -f raw disk.img 64M
    mkfs.fat -F 32 disk.img
    echo "This is a persistent disk file." > disk_readme.txt
    mcopy -i disk.img disk_readme.txt ::README.TXT
    rm disk_readme.txt
    
    # NEW: Compile and add testapp1.elf to disk
    nasm -f elf64 testapp1.s -o testapp1.o
    ld -N -e 0x400080 -Ttext 0x400080 testapp1.o -o testapp1.elf
    mcopy -i disk.img testapp1.elf ::TESTAPP1.ELF
fi

# 1. Compile
cargo build --target x86_64-unknown-none --release

# 2. Prepare ISO folder
mkdir -p iso_root
cp target/x86_64-unknown-none/release/chronos iso_root/
cp limine.cfg iso_root/

# NEW: Copy the text file and test app to the ISO
cp welcome.txt iso_root/
cp testapp.elf iso_root/

# 3. Copy Limine binaries
cp ../limine/limine-bios.sys ../limine/limine-bios-cd.bin ../limine/limine-uefi-cd.bin iso_root/

# 4. Create ISO
xorriso -as mkisofs -b limine-bios-cd.bin \
        -no-emul-boot -boot-load-size 4 -boot-info-table \
        --efi-boot limine-uefi-cd.bin \
        -efi-boot-part --efi-boot-image --protective-msdos-label \
        iso_root -o chronos.iso

# 5. Deploy Limine
../limine/limine bios-install chronos.iso

echo "Build complete."