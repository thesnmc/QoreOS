# QoreOS

> A sovereign, legacy-free, hardware-proximal Unikernel written entirely in Rust.

QoreOS is a modern, bare-metal operating system built from the ground up to ruthlessly eliminate 30 years of legacy PC architecture. It operates under a strict stateless mandate, prioritizing raw hardware acceleration, symmetric multiprocessing, and secure, zero-trust execution.

There is no 16-bit real mode. There is no legacy BIOS. There is no 8259 PIC. There is no VGA text mode. 

QoreOS speaks directly to modern silicon.

## Core Architecture

* **Pure Rust Environment:** Engineered using `#![no_std]` and `#![no_main]` for absolute memory safety without the overhead of garbage collection.
* **Modern Bootstrapping:** Bypasses legacy BIOS entirely, utilizing a custom UEFI bootloader to execute natively in 64-bit Long Mode.
* **Hardware-Accelerated UI:** Discards VGA text mode for a native, dynamic ring-buffer console rendered directly onto the UEFI Graphics Output Protocol (GOP) physical framebuffer.
* **Symmetric Multiprocessing (SMP):** Deprecates legacy Programmable Interrupt Controllers (PIC) in favor of the modern Local APIC, executing the `INIT-SIPI` sequence to awaken dormant motherboard cores.
* **PCIe ECAM Discovery:** Abandons legacy I/O ports. QoreOS maps the entire PCIe topology into standard physical memory (ECAM) to discover NVMe, xHCI, and Network silicon via bitwise BDF shifting.

## Monorepo Structure

QoreOS utilizes a monorepo design, containing both the Ring 0 kernel and the UEFI ignition switch.

```text
QoreOS/
├── bootloader/            # Custom UEFI Bootloader and QEMU hardware configuration
│   ├── src/               # UEFI entry point (efi_main)
│   └── run.sh             # Virtual motherboard configuration (NVMe, xHCI, OVMF)
├── src/                   # QoreOS Ring 0 Kernel
│   ├── compositor.rs      # Native GUI and text rendering engine
│   ├── interrupts.rs      # Local APIC and hardware interrupt routing
│   ├── pcie.rs            # ECAM high-resolution hardware discovery
│   └── main.rs            # Kernel entry point and SMP initialization
└── x86_64-edgecore.json   # Custom bare-metal target specification
Compilation & Execution
QoreOS requires the Rust Nightly toolchain to compile the custom bare-metal target.

1. Install Prerequisites (Linux)

Bash
rustup default nightly
rustup component add rust-src llvm-tools-preview
sudo apt install qemu-system-x86 ovmf
2. Build and Launch the Virtual Hardware

Bash
# Navigate to the bootloader directory
cd bootloader

# Run the boot sequence (compiles kernel, packages EFI, and boots QEMU)
cargo run
Development Roadmap
[x] Phase 1: UEFI Bootstrapping & GOP Framebuffer Mapping

[x] Phase 2: Local APIC Routing & SMP Core Awakening

[x] Phase 3: High-Resolution PCIe ECAM Discovery

[ ] Phase 4: NVMe Storage Engine (ASQ/ACQ DMA Rings)

[ ] Phase 5: Stateful TCP/IPv4 Network Stack & ARP Caching

[ ] Phase 6: Shared-Memory Compositor IPC (Off-Screen Canvases)