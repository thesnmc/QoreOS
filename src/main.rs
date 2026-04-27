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
pub mod nvme; 
pub mod mouse;

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
pub static mut NVME_DRIVE: Option<nvme::Nvme> = None; 

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
    let mut nvme_bar0: u64 = 0; 
    let mut xhci_found = false;
    let mut _xhci_bar0: u64 = 0; 

    if let Ok(_sig_str) = core::str::from_utf8(&signature) { 
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
            
            let pcie_base_addr = unsafe { core::ptr::read_unaligned((mcfg_address as usize + 44) as *const u64) };

            for bus in 0..=255 {
                for device in 0..32 {
                    for function in 0..8 {
                        let pci_addr = pcie_base_addr + (bus << 20) + (device << 15) + (function << 12);
                        let vendor_id = unsafe { core::ptr::read_volatile(pci_addr as *const u16) };
                        
                        if vendor_id != 0xFFFF { 
                            let class_code = unsafe { core::ptr::read_volatile((pci_addr + 0x0B) as *const u8) };
                            let subclass = unsafe { core::ptr::read_volatile((pci_addr + 0x0A) as *const u8) };
                            let prog_if = unsafe { core::ptr::read_volatile((pci_addr + 0x09) as *const u8) };
                            
                            // ---------------------------------------------------------
                            // THE FIX: DYNAMIC 32/64 BIT BAR PARSING
                            // ---------------------------------------------------------
                            if class_code == 0x01 && subclass == 0x08 { 
                                nvme_found = true; 
                                let bar0_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x10) as *const u32) };
                                let bar_type = (bar0_raw >> 1) & 0x03;
                                
                                if bar_type == 2 { // 64-bit BAR
                                    let bar1_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x14) as *const u32) };
                                    nvme_bar0 = ((bar1_raw as u64) << 32) | ((bar0_raw & 0xFFFFFFF0) as u64);
                                } else { // 32-bit BAR
                                    nvme_bar0 = (bar0_raw & 0xFFFFFFF0) as u64;
                                }

                                let cmd_addr = (pci_addr + 0x04) as *mut u16;
                                let mut cmd = unsafe { core::ptr::read_volatile(cmd_addr) };
                                cmd |= (1 << 1) | (1 << 2); 
                                unsafe { core::ptr::write_volatile(cmd_addr, cmd); }
                            }
                            
                            if class_code == 0x0C && subclass == 0x03 && prog_if == 0x30 { 
                                xhci_found = true; 
                                let bar0_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x10) as *const u32) };
                                _xhci_bar0 = (bar0_raw & 0xFFFFFFF0) as u64; 
                            }
                        }
                    }
                }
            }
        }
        
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

        if local_apic_base != 0 {
            let icr_low = (local_apic_base + 0x300) as *mut u32;
            core::ptr::write_volatile(icr_low, 0x000C4500);
            for _ in 0..10_000 { core::arch::asm!("nop"); } 
            core::ptr::write_volatile(icr_low, 0x000C4608);
        }

        mouse::init();
        x86_64::instructions::interrupts::enable();

        compositor::init(boot_info);
        compositor::fill_rect(0, 0, compositor::SERVER.width, compositor::SERVER.height, 0xFFFFFF);
        compositor::fill_rect(0, 0, compositor::SERVER.width, 40, 0x1E293B);
        compositor::draw_string(20, 10, "QOREOS UNIKERNEL SECURE CONSOLE", 0xFFFFFF, 2);

        compositor::terminal_print("INIT: Core 0 (BSP) Online.\n", 0x10B981);
        compositor::terminal_print("INIT: APIC Interrupt Routing Established.\n", 0x10B981);
        
        if nvme_found && nvme_bar0 != 0 { 
            compositor::terminal_print("ECAM: NVMe Direct Storage Controller Found!\n", 0x3B82F6); 
            let mut drive = nvme::Nvme::new(nvme_bar0);
            drive.init();
            drive.identify_controller();
            NVME_DRIVE = Some(drive);
        }

        if xhci_found { compositor::terminal_print("ECAM: xHCI Extensible Host Controller Found!\n", 0x3B82F6); }
        
        compositor::terminal_print("SMP: INIT-SIPI IPI Broadcast Transmitted to AP Cores.\n", 0x3B82F6);
        compositor::terminal_print("\n> TYPE A MESSAGE AND HIT ENTER TO FIRE UDP\n> ", 0x1E293B);
    }

    unsafe {
        if let Some(ref mut nic) = NET_CARD {
            nic.arp_request([10, 0, 2, 2]); 
        }
    }

    // ---------------------------------------------------------
    // THE QOREOS MAIN IDLE LOOP
    // ---------------------------------------------------------
    // ---------------------------------------------------------
    // THE QOREOS MAIN IDLE LOOP
    // ---------------------------------------------------------
    let mut old_mx = -1;
    let mut old_my = -1;

    loop { 
        unsafe {
            // 1. Process Network Traffic
            if let Some(ref mut nic) = NET_CARD { nic.poll(); }

            // 2. Fetch the lock-free Mouse Coordinates
            let mut mx = mouse::MOUSE_X.load(core::sync::atomic::Ordering::Relaxed);
            let mut my = mouse::MOUSE_Y.load(core::sync::atomic::Ordering::Relaxed);

            // If the mouse moved, we need to redraw the screen!
            if mx != old_mx || my != old_my {
                
                // --- HARDWARE BOUNDARY COLLISION ---
                // Prevent the mouse from flying off the screen and crashing the GPU!
                if mx < 0 { mx = 0; mouse::MOUSE_X.store(0, core::sync::atomic::Ordering::Relaxed); }
                if my < 0 { my = 0; mouse::MOUSE_Y.store(0, core::sync::atomic::Ordering::Relaxed); }
                if mx >= compositor::SERVER.width as i32 { 
                    mx = compositor::SERVER.width as i32 - 10; 
                    mouse::MOUSE_X.store(mx, core::sync::atomic::Ordering::Relaxed); 
                }
                if my >= compositor::SERVER.height as i32 { 
                    my = compositor::SERVER.height as i32 - 10; 
                    mouse::MOUSE_Y.store(my, core::sync::atomic::Ordering::Relaxed); 
                }

                // --- RESTORE THE BACKGROUND ---
                // Wipe the old cursor by rapidly redrawing the clean UI underneath it
                compositor::fill_rect(0, 0, compositor::SERVER.width, 40, 0x1E293B); // Draw Top Bar
                compositor::draw_string(20, 10, "QOREOS UNIKERNEL SECURE CONSOLE", 0xFFFFFF, 2);
                if let Some(ref canvas) = compositor::SERVER.terminal_layer {
                    compositor::blit_canvas(canvas); // Blast the RAM Canvas to the screen
                }

                // --- DRAW THE FLOATING CURSOR ---
                // Draw a sleek Blue cursor with a Black shadow
                compositor::fill_rect(mx as usize, my as usize, 8, 8, 0x000000); 
                compositor::fill_rect(mx as usize + 1, my as usize + 1, 6, 6, 0x3B82F6); 

                old_mx = mx;
                old_my = my;
            }
        }
        // Sleep the CPU until the next hardware interrupt wakes it up
        x86_64::instructions::hlt(); 
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop { x86_64::instructions::hlt(); }
}