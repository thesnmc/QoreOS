#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;
use core::panic::PanicInfo;

mod gdt;
mod interrupts;
pub mod vga; 
pub mod pcie;
pub mod allocator;
pub mod e1000;
pub mod memory;
pub mod frame_allocator;
pub mod compositor; 

#[repr(C)]
pub struct BootInfo {
    pub framebuffer_base: u64, pub framebuffer_size: usize,
    pub width: usize, pub height: usize, pub stride: usize,
    pub memory_map_size: usize, pub acpi2_rsdp_addr: u64,
    pub memory_map_addr: u64, pub memory_map_desc_size: usize,
}

#[repr(C)]
pub struct UefiMemoryDescriptor {
    pub ty: u32, pub pad: u32, pub physical_start: u64,
    pub virtual_start: u64, pub number_of_pages: u64, pub attribute: u64,
}

pub static mut NET_CARD: Option<e1000::E1000> = None;

#[unsafe(no_mangle)]
pub extern "sysv64" fn _start(boot_info: *const BootInfo) -> ! {
    x86_64::instructions::interrupts::disable();
    let info = unsafe { &*boot_info };

    let fb_ptr = info.framebuffer_base as *mut u8;
    unsafe { core::ptr::write_bytes(fb_ptr, 0x00, info.framebuffer_size); }
    let writer = unsafe { vga::VgaWriter::new(boot_info) };
    *vga::WRITER.lock() = Some(writer);

    let rsdp_ptr = info.acpi2_rsdp_addr as *const u8;
    let mut signature = [0u8; 8];
    unsafe { core::ptr::copy_nonoverlapping(rsdp_ptr, signature.as_mut_ptr(), 8); }
    
    let mut e1000_bar0 = None; 
    let mut local_apic_base: u64 = 0;
    let mut ioapic_base: u64 = 0;
    let mut nvme_found = false;
    let mut xhci_found = false;
    let mut xhci_bar0: u64 = 0; // NEW: Storing the physical memory address of the USB Silicon

    if let Ok(sig_str) = core::str::from_utf8(&signature) {
        let xsdt_ptr_location = (rsdp_ptr as usize + 24) as *const u64;
        let xsdt_address = unsafe { core::ptr::read_unaligned(xsdt_ptr_location) };
        let xsdt_length = unsafe { core::ptr::read_unaligned((xsdt_address as usize + 4) as *const u32) };
        let entry_count = (xsdt_length - 36) / 8;
        let entries_start = (xsdt_address as usize + 36) as *const u64;
        
        let mut mcfg_address: u64 = 0;
        let mut madt_address: u64 = 0;

        for i in 0..entry_count {
            let entry_address = unsafe { core::ptr::read_unaligned(entries_start.add(i as usize)) };
            let mut table_sig = [0u8; 4];
            unsafe { core::ptr::copy_nonoverlapping(entry_address as *const u8, table_sig.as_mut_ptr(), 4); }
            if let Ok(sig_str) = core::str::from_utf8(&table_sig) {
                if sig_str == "MCFG" { mcfg_address = entry_address; }
                if sig_str == "APIC" { madt_address = entry_address; }
            }
        }
        
        if mcfg_address != 0 { 
            e1000_bar0 = pcie::scan_bus_zero(mcfg_address); 
            
            // ---------------------------------------------------------
            // TARGET 2: HIGH-RESOLUTION PCIe ECAM DISCOVERY
            // ---------------------------------------------------------
            for bus in 0..=255 {
                for device in 0..32 {
                    for function in 0..8 {
                        let pci_addr = mcfg_address + (bus << 20) + (device << 15) + (function << 12);
                        let vendor_id = unsafe { core::ptr::read_volatile(pci_addr as *const u16) };
                        
                        if vendor_id != 0xFFFF { 
                            let class_code = unsafe { core::ptr::read_volatile((pci_addr + 0x0B) as *const u8) };
                            let subclass = unsafe { core::ptr::read_volatile((pci_addr + 0x0A) as *const u8) };
                            let prog_if = unsafe { core::ptr::read_volatile((pci_addr + 0x09) as *const u8) };
                            
                            // Identify NVMe Storage
                            if class_code == 0x01 && subclass == 0x08 { nvme_found = true; }
                            
                            // Identify modern xHCI USB 3.0 Controller
                            if class_code == 0x0C && subclass == 0x03 && prog_if == 0x30 { 
                                xhci_found = true; 
                                // Extract BAR0 (Offset 0x10). Mask out the lower 4 bits (flags).
                                let bar0_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x10) as *const u32) };
                                xhci_bar0 = (bar0_raw & 0xFFFFFFF0) as u64;
                            }
                        }
                    }
                }
            }
        } // <-- THIS BRACE WAS MISSING! IT IS FIXED NOW.
        
        if madt_address != 0 {
            local_apic_base = unsafe { core::ptr::read_unaligned((madt_address as usize + 0x24) as *const u32) } as u64;
            let madt_length = unsafe { core::ptr::read_unaligned((madt_address as usize + 4) as *const u32) };
            let mut offset = 0x2C;
            while offset < madt_length {
                let entry_type = unsafe { core::ptr::read_unaligned((madt_address as usize + offset as usize) as *const u8) };
                let entry_len = unsafe { core::ptr::read_unaligned((madt_address as usize + offset as usize + 1) as *const u8) };
                if entry_type == 1 { 
                    ioapic_base = unsafe { core::ptr::read_unaligned((madt_address as usize + offset as usize + 4) as *const u32) } as u64;
                }
                offset += entry_len as u32;
            }
        }
    }

    unsafe {
        allocator::init_heap();
        frame_allocator::init(boot_info);
        memory::init();
        gdt::init();  
        interrupts::init_idt();

        if let Some(bar0) = e1000_bar0 {
            let mut nic = e1000::E1000::new(bar0);
            nic.init_rx_ring();
            nic.init_tx_ring();
            NET_CARD = Some(nic);
        }

        interrupts::init_apic(local_apic_base, ioapic_base); 
        
        let mut ps2_cmd = x86_64::instructions::port::Port::<u8>::new(0x64);
        ps2_cmd.write(0xAE); 

        // ---------------------------------------------------------
        // TARGET 1: SMP AWAKENING (INIT-SIPI)
        // ---------------------------------------------------------
        if local_apic_base != 0 {
            let icr_low = (local_apic_base + 0x300) as *mut u32;
            core::ptr::write_volatile(icr_low, 0x000C4500);
            for _ in 0..10_000 { core::arch::asm!("nop"); } 
            core::ptr::write_volatile(icr_low, 0x000C4608);
        }

        x86_64::instructions::interrupts::enable();

        // ---------------------------------------------------------
        // TARGET 3: DYNAMIC UI CONSOLE
        // ---------------------------------------------------------
        compositor::init(boot_info);
        compositor::fill_rect(0, 0, compositor::SERVER.width, compositor::SERVER.height, 0xFFFFFF);
        compositor::fill_rect(0, 0, compositor::SERVER.width, 40, 0x1E293B);
        compositor::draw_string(20, 10, "EDGECORE UNIKERNEL SECURE CONSOLE", 0xFFFFFF, 2);

        compositor::terminal_print("INIT: Core 0 (BSP) Online.\n", 0x10B981);
        compositor::terminal_print("INIT: APIC Interrupt Routing Established.\n", 0x10B981);
        
        if nvme_found { compositor::terminal_print("ECAM: NVMe Direct Storage Controller Found!\n", 0x3B82F6); }
        if xhci_found { compositor::terminal_print("ECAM: xHCI Extensible Host Controller Found!\n", 0x3B82F6); }
        
        compositor::terminal_print("SMP: INIT-SIPI IPI Broadcast Transmitted to AP Cores.\n", 0x3B82F6);
        compositor::terminal_print("\n> TYPE A MESSAGE AND HIT ENTER TO FIRE UDP\n> ", 0x1E293B);
    }

    loop { x86_64::instructions::hlt(); }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop { x86_64::instructions::hlt(); }
}