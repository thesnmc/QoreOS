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
* **Sovereign Window Manager:** Features a custom 2D compositor with double-buffering, lock-free atomic PS/2 mouse routing, and an interactive desktop with drag physics.
* **Direct Storage Access:** Contains a custom NVMe storage driver and native FAT32 parser. Allocates physical Submission/Completion queues (SQ/CQ) to rip raw hex data directly off silicon sectors and decrypt the filesystem natively.
* **High-Frequency Networking:** Features a bare-metal E1000 Gigabit driver. Utilizes PCI Bus Mastering to achieve DMA packet interception, overriding standard hypervisor limits. Includes a custom Ring-0 Protocol Analyzer capable of automatic ARP handshakes and completing full 3-way TCP/IP handshakes via a microsecond port-yield loop.
* **Hardware Audio:** Integrates Intel High Definition Audio (HDA), enabling direct DMA CORB/RIRB ring allocation to stream raw frequency data to the audio codec.

## Monorepo Structure

QoreOS utilizes a monorepo design, containing both the Ring-0 EdgeCore kernel and the UEFI ignition switch.

```text
QoreOS/
├── bootloader/            # Custom Sovereign UEFI Bootloader
│   ├── src/               # UEFI entry point (efi_main)
│   └── Cargo.toml         # Bootloader dependencies
├── src/                   # EdgeCore Ring-0 Kernel
│   ├── compositor.rs      # Native GUI, Window Manager, and text rendering
│   ├── main.rs            # Kernel entry point, high-frequency yield loop, and GUI physics
│   ├── mouse.rs           # Lock-free atomic PS/2 hardware interrupt routing
│   ├── nvme.rs            # NVMe PCIe Controller and DMA sector extraction
│   ├── fat32.rs           # Native File Allocation Table parser and payload decrypter
│   ├── e1000.rs           # Gigabit network driver (Hardware DMA, ARP cache, Ring Buffers)
│   ├── net.rs             # Protocol Analyzer and TCP 3-Way Handshake Engine
│   ├── hda.rs             # Intel HD Audio Controller and CORB/RIRB streaming
│   ├── pcie.rs            # ECAM high-resolution hardware discovery
│   ├── interrupts.rs      # Local APIC initialization and IDT routing
│   └── usermode.rs        # Ring-0 to Ring-3 Context Switching Engine
└── x86_64-edgecore.json   # Custom bare-metal target specification
```

## Compilation & Execution
QoreOS requires the Rust Nightly toolchain to compile the custom bare-metal target, and a virtual Q35 motherboard equipped with NVMe, Intel HDA, and E1000 networking.

### 1. Install Prerequisites (Linux)
```bash
rustup default nightly
rustup component add rust-src llvm-tools-preview
sudo apt install qemu-system-x86 ovmf netcat-openbsd
```

### 2. Forge the Virtual Silicon (One-Time Setup)
Create a raw binary file formatted as a FAT32 filesystem to test the Unikernel's native decryption capabilities.
```bash
# Create a 64MB empty binary file
dd if=/dev/zero of=nvme_disk.img bs=1M count=64

# Format the drive as FAT32
mkfs.fat -F 32 nvme_disk.img

# Mount the drive, write the payload, and unmount
sudo mount -o loop nvme_disk.img /mnt
sudo bash -c 'echo "EDGECORE FAT32 FILESYSTEM DECRYPTION SUCCESSFUL. RING-0 I/O ONLINE." > /mnt/PAYLOAD.TXT'
sudo umount /mnt
```

### 3. Build and Launch the OS
Run the master boot sequence from the root edgecore_kernel directory. This compiles the custom alloc library, packages the EFI bootloader, and mounts the complete virtual hardware stack (including network port forwarding).
```bash
cargo clean && \
RUSTFLAGS="-C link-arg=--image-base=0x200000" cargo build -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec --target x86_64-edgecore.json && \
cargo build --manifest-path bootloader/Cargo.toml --target x86_64-unknown-uefi && \
mkdir -p target/esp/EFI/BOOT && \
cp bootloader/target/x86_64-unknown-uefi/debug/bootloader.efi target/esp/EFI/BOOT/BOOTX64.EFI && \
qemu-system-x86_64 -machine q35 -bios /usr/share/ovmf/OVMF.fd -drive format=raw,file=fat:rw:target/esp \
-drive file=nvme_disk.img,format=raw,if=none,id=nvm \
-device nvme,serial=EDGEC0RE-1,drive=nvm \
-audiodev none,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 \
-netdev user,id=net0,hostfwd=tcp::8080-:80 -device e1000,netdev=net0
```

### 4. Trigger the TCP Handshake
Once the Unikernel boots and QEMU's router auto-authenticates via ARP, open a second host terminal and strike the forwarded port to test the hardware's bi-directional TCP engine:
```bash
nc -vz 127.0.0.1 8080
```
*Note: Because QoreOS utilizes an ultra-low-power `hlt()` state, the physical mouse must be moved within the QEMU window the moment the `nc` command is executed to awaken the CPU and trigger the hardware packet poll.*

## Development Roadmap
- [x] Phase 1: UEFI Bootstrapping & GOP Framebuffer Mapping
- [x] Phase 2: Local APIC Routing & SMP Core Awakening
- [x] Phase 3: High-Resolution PCIe ECAM Discovery (Intel Q35)
- [x] Phase 4: Sovereign 2D Window Manager (Lock-free mouse, drag physics)
- [x] Phase 5: NVMe Storage Engine (ASQ/ACQ DMA Rings & Sector Extraction)
- [x] Phase 6: Native File System implementation (FAT32 Parser & Decryption)
- [x] Phase 7: Hardware Audio Integration (Intel HDA, CORB/RIRB Ring Streaming)
- [x] Phase 8: Stateless Network Stack (E1000 DMA Unlock, 3-Way TCP Handshake, Port Yielding)
- [x] Phase 9: Ring-3 User-Space Context Switching
- [ ] Phase 10: Micro-HTTP Server (Raw FAT32 Payload Transmission)