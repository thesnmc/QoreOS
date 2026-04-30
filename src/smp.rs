use core::sync::atomic::{AtomicUsize, Ordering};

pub static AP_COUNT: AtomicUsize = AtomicUsize::new(0);

// We keep our static page tables just in case we hit a Null pointer in the hardware!
#[repr(align(4096))]
struct PageTable { entries: [u64; 512] }
static mut SMP_PDPT: PageTable = PageTable { entries: [0; 512] };
static mut SMP_PD: PageTable = PageTable { entries: [0; 512] };

// Allocate perfectly safe static stacks in virtual memory!
#[repr(align(4096))]
struct ApStack([u8; 16384]);
static mut AP_STACKS: [ApStack; 4] = [
    ApStack([0; 16384]), ApStack([0; 16384]), ApStack([0; 16384]), ApStack([0; 16384])
];

#[unsafe(no_mangle)]
pub extern "sysv64" fn ap_main() -> ! {
    AP_COUNT.fetch_add(1, Ordering::SeqCst);
    loop { unsafe { core::arch::asm!("cli; hlt"); } }
}

pub unsafe fn copy_trampoline() {
    crate::compositor::terminal_print("   -> [DIAG] Disabling CR0.WP...\n", 0x3B82F6);
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0);
    let old_cr0 = cr0;
    
    // ACTUAL FIX: ONLY disable Write-Protect (bit 16). Leave Paging (bit 31) ALONE.
    core::arch::asm!("mov cr0, {}", in(reg) cr0 & !(1 << 16));

    crate::compositor::terminal_print("   -> [DIAG] Resolving Hardware CR3...\n", 0x3B82F6);
    let mut cr3: u64;
    core::arch::asm!("mov {}, cr3", out(reg) cr3);
    
    let pml4 = (cr3 & 0x000FFFFFFFFFF000) as *mut u64;
    let mut pml4e = core::ptr::read_volatile(pml4.add(0));
    
    if pml4e & 1 == 0 {
        crate::compositor::terminal_print("   -> [DIAG] PML4[0] Missing! Injecting Static PDPT...\n", 0xF59E0B);
        let pdpt_phys = SMP_PDPT.entries.as_ptr() as u64;
        core::ptr::write_volatile(pml4.add(0), pdpt_phys | 0b11);
        pml4e = core::ptr::read_volatile(pml4.add(0));
    }

    let pdpt = (pml4e & 0x000FFFFFFFFFF000) as *mut u64;
    let mut pdpte = core::ptr::read_volatile(pdpt.add(0));
    
    if pdpte & 1 == 0 {
        crate::compositor::terminal_print("   -> [DIAG] PDPT[0] Missing! Injecting Static PD...\n", 0xF59E0B);
        let pd_phys = SMP_PD.entries.as_ptr() as u64;
        core::ptr::write_volatile(pdpt.add(0), pd_phys | 0b11);
        pdpte = core::ptr::read_volatile(pdpt.add(0));
    }

    if pdpte & (1 << 7) == 0 {
        crate::compositor::terminal_print("   -> [DIAG] Forcing Surgical 2MB Map for AP Trampoline...\n", 0x3B82F6);
        let pd = (pdpte & 0x000FFFFFFFFFF000) as *mut u64;
        
        // THE FIX: We map ONLY PD[0] (0 to 2MB).
        // This makes the 0x8000 trampoline safe, but leaves PD[1] alone so the kernel doesn't unmap!
        core::ptr::write_volatile(pd.add(0), 0x000000 | 0x83);
    } 
    
    core::arch::asm!("mov cr3, {}", in(reg) cr3); // Flush TLB

    crate::compositor::terminal_print("   -> [DIAG] Copying 161-Byte Infallible Trampoline...\n", 0x3B82F6);
    let trampoline: [u8; 161] = [
        // --- 16-BIT REAL MODE --- 
        0xFA, 0x0F, 0x09, 0x31, 0xC0, 0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0, 
        0x0F, 0x01, 0x16, 0x00, 0x81, // lgdt [0x8100] (Fixed Hex!)
        0x0F, 0x20, 0xC0, 0x66, 0x83, 0xC8, 0x01, 
        0x0F, 0x22, 0xC0, 0x66, 0xEA, 0x22, 0x80, 0x00, 0x00, 0x08, 0x00,
        
        // --- 32-BIT PROTECTED MODE --- 
        0xB8, 0x10, 0x00, 0x00, 0x00, 0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0, 
        0x8E, 0xE0, 0x8E, 0xE8, 0x0F, 0x20, 0xE0, 0x83, 0xC8, 0x20, 0x0F, 
        0x22, 0xE0, 0xA1, 0x00, 0x85, 0x00, 0x00, 0x0F, 0x22, 0xD8, 0xB9, 
        0x80, 0x00, 0x00, 0xC0, 0x0F, 0x32, 0x0D, 0x00, 0x01, 0x00, 0x00, 
        0x0F, 0x30, 0x0F, 0x20, 0xC0, 0x0D, 0x00, 0x00, 0x00, 0x80, 0x0F, 
        0x22, 0xC0, 0xEA, 0x62, 0x80, 0x00, 0x00, 0x18, 0x00,
        
        // --- 64-BIT LONG MODE --- 
        0xB8, 0x10, 0x00, 0x00, 0x00, 0x8E, 0xD8, 0x8E, 0xC0, 0x8E, 0xD0, 0x8E, 0xE0, 0x8E, 0xE8, 
        0xB8, 0x00, 0x00, 0x00, 0x00,                               
        0xF0, 0x0F, 0xC1, 0x04, 0x25, 0x00, 0x86, 0x00, 0x00,       
        0xC1, 0xE0, 0x0E,                                           
        0x48, 0x8B, 0x1C, 0x25, 0x08, 0x86, 0x00, 0x00,             
        0x48, 0x29, 0xC3,                                           
        0x48, 0x89, 0xDC,                                           
        0x48, 0x31, 0xED,                                           
        0x48, 0x8B, 0x04, 0x25, 0x10, 0x86, 0x00, 0x00,             
        0xFF, 0xD0,                                                 
        0xFA, 0xF4, 0xEB, 0xFD                                      
    ];

    core::ptr::copy_nonoverlapping(trampoline.as_ptr(), 0x8000 as *mut u8, trampoline.len());

    crate::compositor::terminal_print("   -> [DIAG] Injecting Safe Configurations...\n", 0x3B82F6);
    let gdt_base = 0x8120 as *mut u64;
    core::ptr::write_volatile(gdt_base.add(0), 0x0000000000000000); 
    core::ptr::write_volatile(gdt_base.add(1), 0x00CF9A000000FFFF); 
    core::ptr::write_volatile(gdt_base.add(2), 0x00CF92000000FFFF); 
    core::ptr::write_volatile(gdt_base.add(3), 0x00AF9A000000FFFF); 
    
    core::ptr::write_unaligned(0x8100 as *mut u16, 31);
    core::ptr::write_unaligned(0x8102 as *mut u32, 0x8120);

    core::ptr::write_volatile(0x8500 as *mut u32, cr3 as u32);
    core::ptr::write_volatile(0x8600 as *mut u32, 0); 

    let stack_top = AP_STACKS.as_ptr() as u64 + (16384 * 4);
    core::ptr::write_volatile(0x8608 as *mut u64, stack_top);
    core::ptr::write_volatile(0x8610 as *mut u64, ap_main as u64);

    crate::compositor::terminal_print("   -> [DIAG] Restoring Protections...\n", 0x3B82F6);
    core::arch::asm!("mov cr0, {}", in(reg) old_cr0);
    
    crate::compositor::terminal_print("   -> [DIAG] DONE. Proceeding to INIT-SIPI.\n", 0x10B981);
}