use crate::println;
use x86_64::registers::control::Cr3;

pub fn init() {
    println!("Initializing Memory Management Unit (MMU)...");
    let (level_4_table_frame, flags) = Cr3::read();
    let phys_addr = level_4_table_frame.start_address().as_u64();
    println!(">>> PAGING HIERARCHY SECURED <<<");
    println!("Active PML4 Table located at Physical Address: {:#X}", phys_addr);
    println!("CR3 Hardware Flags: {:?}", flags);
    println!("-----------------------------------");
}

pub fn walk_page_table(virt_addr: u64) -> Option<u64> {
    let (level_4_table_frame, _) = Cr3::read();
    let mut current_table_phys_addr = level_4_table_frame.start_address().as_u64();

    let indices = [
        (virt_addr >> 39) & 0x1FF,
        (virt_addr >> 30) & 0x1FF,
        (virt_addr >> 21) & 0x1FF,
        (virt_addr >> 12) & 0x1FF,
    ];
    let page_offset = virt_addr & 0xFFF; 

    for (level, &index) in indices.iter().enumerate() {
        let table_ptr = current_table_phys_addr as *const u64;
        let entry = unsafe { core::ptr::read_volatile(table_ptr.add(index as usize)) };
        
        if entry & 1 == 0 { return None; }
        current_table_phys_addr = entry & 0x000FFFFF_FFFFF000;
        
        if (level == 1 || level == 2) && (entry & (1 << 7)) != 0 {
            let offset_mask = if level == 1 { 0x3FFF_FFFF } else { 0x1F_FFFF };
            return Some(current_table_phys_addr + (virt_addr & offset_mask));
        }
    }
    Some(current_table_phys_addr + page_offset)
}

// ---------------------------------------------------------
// NEW: The Surgical PTE Injector
// ---------------------------------------------------------
pub unsafe fn map_page(virt_addr: u64, phys_frame: u64) {
    let (level_4_table_frame, _) = Cr3::read();
    let pml4_ptr = level_4_table_frame.start_address().as_u64() as *const u64;

    let pml4_index = ((virt_addr >> 39) & 0x1FF) as usize;
    let pml4_entry = core::ptr::read_volatile(pml4_ptr.add(pml4_index));
    if pml4_entry & 1 == 0 { return; } // Safety abort if intermediate table is missing

    let pdpt_phys = (pml4_entry & 0x000FFFFF_FFFFF000) as *const u64;
    let pdpt_index = ((virt_addr >> 30) & 0x1FF) as usize;
    let pdpt_entry = core::ptr::read_volatile(pdpt_phys.add(pdpt_index));
    if pdpt_entry & 1 == 0 || pdpt_entry & (1<<7) != 0 { return; }

    let pd_phys = (pdpt_entry & 0x000FFFFF_FFFFF000) as *const u64;
    let pd_index = ((virt_addr >> 21) & 0x1FF) as usize;
    let pd_entry = core::ptr::read_volatile(pd_phys.add(pd_index));
    if pd_entry & 1 == 0 || pd_entry & (1<<7) != 0 { return; }

    let pt_phys = (pd_entry & 0x000FFFFF_FFFFF000) as *mut u64;
    let pt_index = ((virt_addr >> 12) & 0x1FF) as usize;

    // Write the physical frame address into the Level 1 Table, setting Present (Bit 0) and Writable (Bit 1)
    let new_pte = phys_frame | 0b11;
    core::ptr::write_volatile(pt_phys.add(pt_index), new_pte);

    // Force the CPU to clear its cache for this specific address!
    core::arch::asm!("invlpg [{}]", in(reg) virt_addr);
}