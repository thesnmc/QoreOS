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
pub mod fat32;
pub mod usermode;
pub mod hda;

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
pub static mut AUDIO_DRIVE: Option<hda::IntelHda> = None;

// --- THE RING-3 SANDBOX APP ---
// This runs entirely in isolated User Mode. 
extern "C" fn ring3_user_task() -> ! {
    loop {
        // Spinning safely in the Ring-3 Sandbox...
    }
}

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
    
    // Audio hardware tracking
    let mut hda_found = false;
    let mut hda_bar0: u64 = 0;

    let mut sector_data_str = alloc::string::String::new();

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
                            
                            // --- NVMe Check ---
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
                            
                            // --- Intel HDA Check ---
                            if class_code == 0x04 && subclass == 0x03 {
                                hda_found = true;
                                let bar0_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x10) as *const u32) };
                                let bar_type = (bar0_raw >> 1) & 0x03;
                                
                                if bar_type == 2 { // 64-bit BAR
                                    let bar1_raw = unsafe { core::ptr::read_volatile((pci_addr + 0x14) as *const u32) };
                                    hda_bar0 = ((bar1_raw as u64) << 32) | ((bar0_raw & 0xFFFFFFF0) as u64);
                                } else { // 32-bit BAR
                                    hda_bar0 = (bar0_raw & 0xFFFFFFF0) as u64;
                                }

                                let cmd_addr = (pci_addr + 0x04) as *mut u16;
                                let mut cmd = unsafe { core::ptr::read_volatile(cmd_addr) };
                                cmd |= (1 << 1) | (1 << 2); 
                                unsafe { core::ptr::write_volatile(cmd_addr, cmd); }
                            }
                            
                            // --- xHCI USB Check ---
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

        // COMPOSITOR INIT
        compositor::init(boot_info);
        compositor::fill_rect(0, 0, compositor::SERVER.width, compositor::SERVER.height, 0xFFFFFF);
        compositor::fill_rect(0, 0, compositor::SERVER.width, 40, 0x1E293B);
        compositor::draw_string(20, 10, "QOREOS UNIKERNEL SECURE CONSOLE", 0xFFFFFF, 2);

        compositor::terminal_print("INIT: Core 0 (BSP) Online.\n", 0x10B981);
        compositor::terminal_print("INIT: APIC Interrupt Routing Established.\n", 0x10B981);
        
        // NVME INIT
        if nvme_found && nvme_bar0 != 0 { 
            compositor::terminal_print("ECAM: NVMe Direct Storage Controller Found!\n", 0x3B82F6); 
            let mut drive = nvme::Nvme::new(nvme_bar0);
            drive.init();
            drive.identify_controller();
            
            let sector_0 = drive.read_sector(0);
            let bpb = unsafe { &*(sector_0.as_ptr() as *const fat32::Fat32BootSector) };
            
            let volume = fat32::Fat32Volume::new(bpb);
            
            let root_lba = volume.cluster_to_lba(volume.root_cluster);
            if root_lba > 0 {
                let root_sector = drive.read_sector(root_lba);
                let entries = unsafe { core::slice::from_raw_parts(root_sector.as_ptr() as *const fat32::Fat32DirEntry, 512 / 32) };
                
                let mut payload_cluster = 0;
                for entry in entries {
                    if entry.name == *b"PAYLOAD TXT" {
                        payload_cluster = ((entry.fst_clus_hi as u32) << 16) | (entry.fst_clus_lo as u32);
                        break;
                    }
                }
                
                if payload_cluster >= 2 {
                    let payload_lba = volume.cluster_to_lba(payload_cluster);
                    let payload_sector = drive.read_sector(payload_lba);
                    
                    let mut str_len = 0;
                    while str_len < 512 && payload_sector[str_len] != 0 && payload_sector[str_len] != 0x0A { str_len += 1; }
                    
                    if let Ok(secret_str) = core::str::from_utf8(&payload_sector[..str_len]) {
                        sector_data_str = alloc::format!("FILE CONTENT: {}", secret_str);
                        compositor::terminal_print("\n> FAT32 DECRYPTED: PAYLOAD.TXT EXTRACTED\n", 0x10B981);
                    }
                } else {
                    sector_data_str = alloc::string::String::from("ERROR: PAYLOAD.TXT NOT FOUND!");
                }
            } else {
                sector_data_str = alloc::string::String::from("ERROR: INVALID FAT32 ROOT CLUSTER");
            }

            NVME_DRIVE = Some(drive);
        }

        // AUDIO INIT (Perfectly timed after GUI is active)
        if hda_found && hda_bar0 != 0 {
            let mut hda = hda::IntelHda::new(hda_bar0);
            hda.init();
            AUDIO_DRIVE = Some(hda);
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
    let mut old_mbtn = 0;

    // UPGRADE: Height increased to 300px to fit Audio Status telemetry
    let mut desktop_win = compositor::Window::new(100, 200, 600, 300, "EDGECORE DIAGNOSTICS", 0x334155);
    
    let mut is_dragging = false;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;

    loop { 
        unsafe {
            if let Some(ref mut nic) = NET_CARD { nic.poll(); }

            let mut mx = mouse::MOUSE_X.load(core::sync::atomic::Ordering::Relaxed);
            let mut my = mouse::MOUSE_Y.load(core::sync::atomic::Ordering::Relaxed);
            let mbtn = mouse::MOUSE_BTN.load(core::sync::atomic::Ordering::Relaxed);
            
            let left_click = (mbtn & 0x01) != 0; 
            let click_just_pressed = left_click && (old_mbtn & 0x01) == 0; 

            if mx < 0 { mx = 0; mouse::MOUSE_X.store(0, core::sync::atomic::Ordering::Relaxed); }
            if my < 0 { my = 0; mouse::MOUSE_Y.store(0, core::sync::atomic::Ordering::Relaxed); }
            if mx >= compositor::SERVER.width as i32 { mx = compositor::SERVER.width as i32 - 10; mouse::MOUSE_X.store(mx, core::sync::atomic::Ordering::Relaxed); }
            if my >= compositor::SERVER.height as i32 { my = compositor::SERVER.height as i32 - 10; mouse::MOUSE_Y.store(my, core::sync::atomic::Ordering::Relaxed); }

            let drop_btn_x = desktop_win.x as usize + 20;
            let drop_btn_y = desktop_win.y as usize + 240; // Shifted button down

            if desktop_win.is_open {
                let close_x = desktop_win.x + desktop_win.width as i32 - 20;
                let close_y = desktop_win.y + 4;

                if click_just_pressed && mx >= close_x && mx <= close_x + 16 && my >= close_y && my <= close_y + 16 {
                    desktop_win.is_open = false;
                    is_dragging = false;
                } 
                else if click_just_pressed && mx >= drop_btn_x as i32 && mx <= (drop_btn_x + 380) as i32 && my >= drop_btn_y as i32 && my <= (drop_btn_y + 24) as i32 {
                    compositor::terminal_print("\n> [WARNING] EJECTING FROM KERNEL MODE. DROPPING TO RING-3...\n", 0xEF4444);
                    if let Some(ref canvas) = compositor::SERVER.terminal_layer { compositor::blit_canvas(canvas); }
                    
                    static mut USER_STACK: [u8; 4096 * 4] = [0; 4096 * 4];
                    let stack_ptr = USER_STACK.as_ptr() as u64 + (4096 * 4);
                    
                    let code_selector = gdt::GDT.1.user_code.0;
                    let data_selector = gdt::GDT.1.user_data.0;

                    usermode::drop_to_usermode(code_selector, data_selector, ring3_user_task as *const () as u64, stack_ptr);
                }
                else if left_click {
                    if !is_dragging {
                        if mx >= desktop_win.x && mx <= desktop_win.x + desktop_win.width as i32 && 
                           my >= desktop_win.y && my <= desktop_win.y + 24 {
                            is_dragging = true;
                            drag_offset_x = mx - desktop_win.x;
                            drag_offset_y = my - desktop_win.y;
                        }
                    } else {
                        desktop_win.x = mx - drag_offset_x;
                        desktop_win.y = my - drag_offset_y;
                        if desktop_win.x < 0 { desktop_win.x = 0; }
                        if desktop_win.y < 40 { desktop_win.y = 40; } 
                        if desktop_win.x + desktop_win.width as i32 > compositor::SERVER.width as i32 { desktop_win.x = compositor::SERVER.width as i32 - desktop_win.width as i32; }
                        if desktop_win.y + desktop_win.height as i32 > compositor::SERVER.height as i32 { desktop_win.y = compositor::SERVER.height as i32 - desktop_win.height as i32; }
                    }
                } else {
                    is_dragging = false;
                }
            }

            // --- CONTINUOUS RENDERING PIPELINE ---
            compositor::fill_rect(0, 0, compositor::SERVER.width, 40, 0x1E293B); 
            compositor::draw_string(20, 10, "QOREOS UNIKERNEL SECURE CONSOLE", 0xFFFFFF, 2);
            if let Some(ref canvas) = compositor::SERVER.terminal_layer { compositor::blit_canvas(canvas); }

            if desktop_win.is_open {
                compositor::draw_window(&desktop_win);
                
                let text_x = desktop_win.x as usize + 20;
                let text_y = desktop_win.y as usize + 50;
                
                let diag_text = alloc::format!("MOUSE X: {}   MOUSE Y: {}", mx, my);
                compositor::draw_string(text_x, text_y, &diag_text, 0x10B981, 2); 
                compositor::draw_string(text_x, text_y + 40, "SYS: 2MB RING-0 SAFE ZONE ACTIVE", 0xFFFFFF, 1);
                compositor::draw_string(text_x, text_y + 60, "SYS: E1000 NETWORK CARD ONLINE", 0xFFFFFF, 1);
                
                let nvme_status = if nvme_found { "DATA SECURED" } else { "NOT DETECTED" };
                let nvme_text = alloc::format!("STORAGE: NVME CONTROLLER {}", nvme_status);
                compositor::draw_string(text_x, text_y + 80, &nvme_text, 0x3B82F6, 1);

                // --- UPGRADE: LIVE HDA AUDIO STATUS GUI ---
                let hda_status = if hda_found { "CORB/RIRB DMA ONLINE" } else { "OFFLINE" };
                let hda_text = alloc::format!("AUDIO: INTEL HDA {}", hda_status);
                compositor::draw_string(text_x, text_y + 100, &hda_text, 0x3B82F6, 1);

                if sector_data_str.len() > 0 {
                    compositor::draw_string(text_x, text_y + 130, "FAT32 PAYLOAD DECRYPTED:", 0x10B981, 1);
                    compositor::draw_string(text_x, text_y + 150, &sector_data_str, 0xF59E0B, 1);
                }

                // DRAW THE RED BUTTON
                compositor::fill_rect(drop_btn_x, drop_btn_y, 380, 24, 0xEF4444); 
                compositor::draw_string(drop_btn_x + 10, drop_btn_y + 6, "INITIATE SECURE RING-3 DROP (LOCKS GUI)", 0xFFFFFF, 1);
            } 

            let cursor_color = if is_dragging { 0x10B981 } else { 0x3B82F6 };
            compositor::fill_rect(mx as usize, my as usize, 8, 8, 0x000000); 
            compositor::fill_rect(mx as usize + 1, my as usize + 1, 6, 6, cursor_color); 

            old_mbtn = mbtn;
        }
        x86_64::instructions::hlt(); 
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop { x86_64::instructions::hlt(); }
}