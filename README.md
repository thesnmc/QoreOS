# QoreOS / EdgeCore Kernel
> A sovereign, legacy-free, hardware-proximal Unikernel written entirely in Rust.

QoreOS is a modern, bare-metal operating system built from the ground up to ruthlessly eliminate 30 years of legacy PC architecture. Powered by the **EdgeCore** kernel, it operates under a strict stateless mandate, prioritizing raw hardware acceleration, symmetric multiprocessing, and secure, zero-trust execution.

There is no 16-bit real mode. There is no legacy BIOS. There is no 8259 PIC. There is no standard library. 

QoreOS speaks directly to modern silicon.

## Core Architecture
* **Pure Rust Environment:** Engineered using `#![no_std]` and `#![no_main]` for absolute memory safety without the overhead of garbage collection.
* **Modern Bootstrapping:** Bypasses legacy BIOS entirely, utilizing a custom UEFI bootloader to execute natively in a 2MB Ring-0 64-bit Long Mode safe zone.
* **Intel Q35 Chipset:** Employs modern virtual motherboard architecture to expose the PCIe bus and Memory-Mapped I/O (ECAM) to the OS.
* **Symmetric Multiprocessing (SMP):** Deprecates legacy Programmable Interrupt Controllers (PIC) in favor of the modern Local APIC, executing the `INIT-SIPI` sequence to awaken dormant motherboard cores.
* **Sovereign Window Manager:** Features a custom 2D compositor with double-buffering, drop-shadow rendering, and a lock-free atomic PS/2 mouse driver with real-time drag physics.
* **Direct Memory Access (DMA):** Contains a custom NVMe storage driver that allocates physical Submission/Completion queues (SQ/CQ) to rip raw hex data directly off the silicon sectors.

## Monorepo Structure

QoreOS utilizes a monorepo design, containing both the Ring 0 EdgeCore kernel and the UEFI ignition switch.

```text
QoreOS/
├── bootloader/            # Custom Sovereign UEFI Bootloader
│   ├── src/               # UEFI entry point (efi_main)
│   └── Cargo.toml         # Bootloader dependencies
├── src/                   # EdgeCore Ring 0 Kernel
│   ├── compositor.rs      # Native GUI, Window Manager, and text rendering
│   ├── main.rs            # Kernel entry point, telemetry loop, and GUI physics
│   ├── mouse.rs           # Lock-free atomic PS/2 hardware interrupt routing
│   ├── nvme.rs            # NVMe PCIe Controller and DMA sector extraction
│   ├── e1000.rs           # Gigabit network driver (ARP/UDP)
│   ├── pcie.rs            # ECAM high-resolution hardware discovery
│   └── interrupts.rs      # Local APIC initialization
└── x86_64-edgecore.json   # Custom bare-metal target specification
```

## Compilation & Execution
QoreOS requires the Rust Nightly toolchain to compile the custom bare-metal target and a virtual Q35 motherboard with an attached NVMe drive.

### 1. Install Prerequisites (Linux)
```bash
rustup default nightly
rustup component add rust-src llvm-tools-preview
sudo apt install qemu-system-x86 ovmf
```

### 2. Forge the Virtual Silicon (One-Time Setup)
Before booting, create a raw binary file to act as the physical NVMe hard drive and inject a test payload into Sector 0.
```bash
# Create a 64MB empty binary file
dd if=/dev/zero of=nvme_disk.img bs=1M count=64

# Inject a custom Hex string into Sector 0
echo -n ">>> EDGECORE SECURE SILICON VOLUME: SECTOR 0 INITIATED <<<" | dd of=nvme_disk.img bs=512 conv=notrunc
```

### 3. Build and Launch the OS
Run the master boot sequence from the root edgecore_kernel directory. This compiles the custom alloc library, packages the EFI bootloader, and mounts the Q35 motherboard with the NVMe drive.
```bash
cargo clean && \
RUSTFLAGS="-C link-arg=--image-base=0x200000" cargo build -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec --target x86_64-edgecore.json && \
cargo build --manifest-path bootloader/Cargo.toml --target x86_64-unknown-uefi && \
mkdir -p target/esp/EFI/BOOT && \
cp bootloader/target/x86_64-unknown-uefi/debug/bootloader.efi target/esp/EFI/BOOT/BOOTX64.EFI && \
echo "\EFI\BOOT\BOOTX64.EFI" > target/esp/startup.nsh && \
qemu-system-x86_64 -machine q35 -bios /usr/share/ovmf/OVMF.fd -drive format=raw,file=fat:rw:target/esp \
-drive file=nvme_disk.img,format=raw,if=none,id=nvm \
-device nvme,serial=EDGEC0RE-1,drive=nvm
```

## Development Roadmap
- [x] Phase 1: UEFI Bootstrapping & GOP Framebuffer Mapping
- [x] Phase 2: Local APIC Routing & SMP Core Awakening
- [x] Phase 3: High-Resolution PCIe ECAM Discovery (Intel Q35)
- [x] Phase 4: Sovereign 2D Window Manager (Lock-free mouse, drag physics)
- [x] Phase 5: Stateless Network Stack (E1000, ARP Caching, UDP Broadcasts)
- [x] Phase 6: NVMe Storage Engine (ASQ/ACQ DMA Rings & Sector Extraction)
- [x] Phase 7: Native File System implementation (FAT32/ext4)
- [x] Phase 8: Ring-3 User-Space Process Scheduler