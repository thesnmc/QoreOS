#!/bin/bash
# $1 is the path to the compiled .efi bootloader passed to us by Cargo
EFI_PATH=$1

mkdir -p esp/EFI/BOOT
cp $EFI_PATH esp/EFI/BOOT/BOOTX64.EFI
cp ../target/x86_64-edgecore/debug/edgecore_kernel esp/kernel.elf
cp /usr/share/OVMF/OVMF_VARS_4M.fd ./OVMF_VARS.fd

# Create a dummy 64MB raw image file to act as our NVMe SSD
dd if=/dev/zero of=nvme_disk.img bs=1M count=64 status=none

qemu-system-x86_64 \
    -M q35 \
    -m 256M \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
    -drive if=pflash,format=raw,file=./OVMF_VARS.fd \
    -drive format=raw,file=fat:rw:esp \
    -drive file=nvme_disk.img,if=none,id=nvm \
    -device nvme,serial=deadbeef,drive=nvm \
    -device qemu-xhci,id=xhci