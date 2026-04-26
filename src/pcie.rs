use crate::println;

pub fn scan_bus_zero(mcfg_address: u64) -> Option<u64> {
    let pcie_base_addr = unsafe { 
        core::ptr::read_unaligned((mcfg_address as usize + 44) as *const u64) 
    };
    
    println!("PCIe Configuration Space Base Address: {:#X}", pcie_base_addr);
    println!("Scanning Motherboard PCIe Bus 0...");

    let mut devices_found = 0;
    let mut e1000_bar0 = None; // Catch the BAR0 address!
    
    for device in 0..32 {
        let device_offset = (device as u64) << 15; 
        let device_addr = pcie_base_addr + device_offset;

        let vendor_and_device = unsafe { 
            core::ptr::read_volatile(device_addr as *const u32) 
        };
        
        let vendor_id = (vendor_and_device & 0xFFFF) as u16;
        let device_id = (vendor_and_device >> 16) as u16;

        if vendor_id != 0xFFFF {
            let class_info = unsafe { 
                core::ptr::read_volatile((device_addr + 0x08) as *const u32) 
            };
            
            let class_code = ((class_info >> 24) & 0xFF) as u8;
            let subclass = ((class_info >> 16) & 0xFF) as u8;

            let device_type = get_device_type(class_code, subclass);

            println!(" -> SLOT {:02}: [{}]", device, device_type);
            println!("          Vendor: {:#06X} | Device: {:#06X}", vendor_id, device_id);
            
            if vendor_id == 0x8086 && device_id == 0x10D3 {
                let bar0 = unsafe { 
                    core::ptr::read_volatile((device_addr + 0x10) as *const u32) 
                };
                
                let control_panel_addr = bar0 & 0xFFFFFFF0;
                println!("          >>> ETHERNET CONTROL PANEL (BAR0) FOUND AT: {:#X} <<<", control_panel_addr);

                let ral = unsafe { core::ptr::read_volatile((control_panel_addr as u64 + 0x5400) as *const u32) };
                let rah = unsafe { core::ptr::read_volatile((control_panel_addr as u64 + 0x5404) as *const u32) };

                let mac = [
                    (ral & 0xFF) as u8, ((ral >> 8) & 0xFF) as u8, ((ral >> 16) & 0xFF) as u8, ((ral >> 24) & 0xFF) as u8,
                    (rah & 0xFF) as u8, ((rah >> 8) & 0xFF) as u8,
                ];

                println!("          >>> HARDWARE MAC ADDRESS: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} <<<", 
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
                
                // Save the address so we can return it!
                e1000_bar0 = Some(control_panel_addr as u64);
            }

            devices_found += 1;
        }
    }
    
    println!("Bus 0 Scan Complete. Found {} attached hardware devices.", devices_found);
    
    // Hand the address back to main.rs
    e1000_bar0
}

fn get_device_type(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x01, 0x01) => "Storage: IDE Controller",
        (0x01, 0x06) => "Storage: SATA Controller",
        (0x01, 0x08) => "Storage: NVMe Controller",
        (0x02, 0x00) => "Network: Ethernet Controller",
        (0x03, 0x00) => "Display: VGA Compatible Controller",
        (0x04, 0x03) => "Multimedia: High Definition Audio",
        (0x06, 0x00) => "Bridge: Host Bridge",
        (0x06, 0x01) => "Bridge: ISA Bridge",
        (0x06, 0x04) => "Bridge: PCI-to-PCI Bridge",
        (0x0C, 0x03) => "Serial Bus: USB Controller",
        _ => "Unknown Device Class",
    }
}