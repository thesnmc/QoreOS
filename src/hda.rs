use core::ptr::{read_volatile, write_volatile};
use alloc::alloc::{alloc_zeroed, Layout};

pub struct IntelHda {
    bar0: u64,
    pub corb_ptr: u64,
    pub rirb_ptr: u64,
}

impl IntelHda {
    pub fn new(bar0: u64) -> Self {
        IntelHda { bar0, corb_ptr: 0, rirb_ptr: 0 }
    }

    unsafe fn read_reg(&self, offset: u64) -> u32 { 
        read_volatile((self.bar0 + offset) as *const u32) 
    }
    
    unsafe fn read_reg16(&self, offset: u64) -> u16 {
        read_volatile((self.bar0 + offset) as *const u16)
    }

    unsafe fn write_reg(&self, offset: u64, val: u32) { 
        write_volatile((self.bar0 + offset) as *mut u32, val); 
    }
    
    unsafe fn write_reg16(&self, offset: u64, val: u16) {
        write_volatile((self.bar0 + offset) as *mut u16, val);
    }
    
    unsafe fn write_reg8(&self, offset: u64, val: u8) {
        write_volatile((self.bar0 + offset) as *mut u8, val);
    }

    pub fn init(&mut self) {
        // Read the Global Capabilities (GCAP) register
        // Prefixed with an underscore to silence the unused variable warning
        let _gcap = unsafe { self.read_reg(0x00) };
        
        unsafe { 
            crate::compositor::terminal_print("\n> HDA: AUDIO SILICON DETECTED AND MAPPED.\n", 0x3B82F6); 
        }
        
        // 1. Reset the Controller
        // Write 1 to the CRST bit in the Global Control (GCTL) register (offset 0x08)
        unsafe { self.write_reg(0x08, 1); }
        
        // Wait for the hardware to acknowledge the reset
        let mut timeout = 0;
        while unsafe { self.read_reg(0x08) & 1 } == 0 {
            unsafe { core::arch::asm!("nop"); }
            timeout += 1;
            if timeout > 10_000_000 {
                unsafe { crate::compositor::terminal_print("> HDA ERROR: CONTROLLER RESET TIMEOUT!\n", 0xEF4444); }
                return;
            }
        }
        
        // Wait a few cycles to allow the external audio codecs to power up on the link
        for _ in 0..10_000 { unsafe { core::arch::asm!("nop"); } }
        
        unsafe { 
            crate::compositor::terminal_print("> HDA: CONTROLLER AWAKE. ALLOCATING DMA RINGS...\n", 0x3B82F6); 
            
            // 2. Allocate CORB and RIRB Mailboxes in physical memory (4KB each)
            let layout = Layout::from_size_align(4096, 4096).unwrap();
            self.corb_ptr = alloc_zeroed(layout) as u64;
            self.rirb_ptr = alloc_zeroed(layout) as u64;

            // Stop the rings before modifying them
            self.write_reg8(0x4C, 0); // CORBCTL
            self.write_reg8(0x5C, 0); // RIRBCTL

            // Program CORB Base Address
            self.write_reg(0x40, (self.corb_ptr & 0xFFFFFFFF) as u32);
            self.write_reg(0x44, (self.corb_ptr >> 32) as u32);
            
            // Program RIRB Base Address
            self.write_reg(0x50, (self.rirb_ptr & 0xFFFFFFFF) as u32);
            self.write_reg(0x54, (self.rirb_ptr >> 32) as u32);

            // Set Ring Sizes to 256 entries (size code 0x02)
            self.write_reg8(0x4E, 0x02);
            self.write_reg8(0x5E, 0x02);

            // Reset the Read/Write pointers
            self.write_reg16(0x4A, 1 << 15); // Reset CORB Read Pointer
            for _ in 0..100 { core::arch::asm!("nop"); }
            self.write_reg16(0x4A, 0); // Clear reset bit
            self.write_reg16(0x48, 0); // Clear CORB Write Pointer
            self.write_reg16(0x58, 1 << 15); // Reset RIRB Write Pointer
            
            // Start the Mailboxes! (Set Run bit)
            self.write_reg8(0x4C, 1 << 1); // CORBCTL run
            self.write_reg8(0x5C, 1 << 1); // RIRBCTL run

            crate::compositor::terminal_print("> HDA: CORB & RIRB RINGS DMA MAPPED AND RUNNING.\n", 0x10B981);
            
            // 3. Scan the link to see if any Audio Codecs (Speakers/DACs) are plugged in
            let statests = self.read_reg16(0x0E); // STATESTS register
            if statests != 0 {
                crate::compositor::terminal_print("> HDA: EXTERNAL AUDIO CODEC DETECTED ON LINK!\n", 0x10B981);
            } else {
                crate::compositor::terminal_print("> HDA: NO CODEC FOUND.\n", 0xF59E0B);
            }
        }
    }
}